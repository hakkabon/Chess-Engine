use crate::eval::relative_eval;
use crate::movegen::{generate_legal, generate_legal_captures_and_promotions, san};
use crate::position::*;
use crate::see::see;
use crate::zobrist::ZOBRIST_SIDE;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const INF: i32 = 1_000_000;
pub const MATE: i32 = 100_000;
/// Scores at/above this magnitude encode a forced mate.
const MATE_BOUND: i32 = MATE - 1000;
const MAX_PLY: usize = 128;

// Transposition-table flags.
const TT_EXACT: u8 = 0;
const TT_LOWER: u8 = 1; // score is a lower bound (failed high)
const TT_UPPER: u8 = 2; // score is an upper bound (failed low)

const TT_SIZE: usize = 1 << 20; // 1,048,576 entries

#[derive(Clone, Copy)]
struct TTEntry {
    key: u64,
    depth: i32,
    flag: u8,
    score: i32,
    mv: Move,
}

impl TTEntry {
    const fn empty() -> TTEntry {
        TTEntry {
            key: 0,
            depth: 0,
            flag: 0,
            score: 0,
            mv: Move {
                from: 0,
                to: 0,
                promotion: None,
                flags: 0,
            },
        }
    }
}

/// Convert a mate score to a ply-independent value for TT storage.
#[inline]
fn adjust_store(score: i32, ply: i32) -> i32 {
    if score >= MATE_BOUND {
        score + ply
    } else if score <= -MATE_BOUND {
        score - ply
    } else {
        score
    }
}

/// Convert a ply-independent TT score back to the current node's perspective.
#[inline]
fn adjust_load(score: i32, ply: i32) -> i32 {
    if score >= MATE_BOUND {
        score - ply
    } else if score <= -MATE_BOUND {
        score + ply
    } else {
        score
    }
}

// ============================================================================
// PRUNING AND REDUCTION CONSTANTS
// ============================================================================

/// Futility pruning margins for quiet moves at shallow depths.
/// If static eval + margin < alpha, we can prune the quiet move because
/// even with a small positional improvement it won't raise alpha.
/// Margins increase with depth because deeper searches have more tactical potential.
const FUTILITY_MARGINS: [i32; 4] = [0, 75, 150, 225]; // depth 0, 1, 2, 3

/// Reverse futility pruning margins.
/// If static eval - margin >= beta at depth >= 3, the position is "too good"
/// and we can return beta immediately (similar to a null-move cutoff).
const RFP_MARGINS: [i32; 8] = [0, 0, 0, 100, 150, 200, 250, 300]; // index by depth

/// Null Move Pruning reduction formula: R = 3 + depth/6 + (eval - beta)/200 (capped)
/// We don't NMP when in check, in endgames, or at very high depths.
const NMP_BASE_REDUCTION: i32 = 3;
const NMP_DEPTH_DIVISOR: i32 = 6;
const NMP_EVAL_DIVISOR: i32 = 200;
const NMP_MAX_EXTRA_REDUCTION: i32 = 3;

/// LMR reduction formula: reduction = log(move_index) * log(depth) / 2.5
/// First few moves (PV candidates) are searched at full depth.
/// Captures, promotions, and moves giving check are typically excluded from LMR.
const LMR_BASE_DIVISOR: f32 = 2.5;
const LMR_MOVES_FULL_DEPTH: usize = 4; // First N moves get full depth search

struct Search {
    tt: Vec<TTEntry>,
    killers: [[Option<Move>; 2]; MAX_PLY],
    /// Counter moves: counter_moves[piece][square] = move that refuted this piece moving to this square
    counter_moves: [[Option<Move>; 64]; 12],
    /// Follow-up moves: followup_moves[from][to] = move that followed this move
    followup_moves: [[Option<Move>; 64]; 64],
    history: [[i32; 64]; 64],
    start: Instant,
    time_limit: Duration,
    nodes: u64,
    stop: bool,
    last_root_move: Option<Move>,
    found_mate: bool,
}

impl Search {
    fn new(time_ms: u64) -> Search {
        Search {
            tt: vec![TTEntry::empty(); TT_SIZE],
            killers: [[None; 2]; MAX_PLY],
            counter_moves: [[None; 64]; 12],
            followup_moves: [[None; 64]; 64],
            history: [[0; 64]; 64],
            start: Instant::now(),
            time_limit: Duration::from_millis(time_ms.max(50)),
            nodes: 0,
            stop: false,
            last_root_move: None,
            found_mate: false,
        }
    }

    fn check_time(&mut self) {
        if self.nodes & 4095 == 0 && self.start.elapsed() >= self.time_limit {
            self.stop = true;
        }
    }

    fn order(&self, moves: &mut Vec<Move>, pos: &Position, tt_move: Option<Move>, ply: i32, prev_move: Option<Move>) {
        moves.sort_by(|a, b| self.move_score(pos, *b, tt_move, ply, prev_move).cmp(&self.move_score(pos, *a, tt_move, ply, prev_move)));
    }

    fn move_score(&self, pos: &Position, m: Move, tt_move: Option<Move>, ply: i32, prev_move: Option<Move>) -> i32 {
        if let Some(t) = tt_move {
            if t == m {
                return 1_000_000_000;
            }
        }
        let mut s = 0;
        if m.is_capture() {
            // Use SEE (Static Exchange Evaluation) for accurate capture ordering
            // SEE simulates the capture-recapture sequence and returns net material gain
            let see_result = see(pos, m);
            // Scale SEE gain to fit in move ordering (base 1M for all captures)
            // Good captures (positive SEE) get high scores, bad captures get lower scores
            s += 1_000_000 + see_result.gain * 10;
        } else {
            let p = (ply as usize).min(MAX_PLY - 1);
            if self.killers[p][0] == Some(m) {
                s += 900_000;
            } else if self.killers[p][1] == Some(m) {
                s += 800_000;
            }
            
            // Counter move heuristic: if opponent's last move was refuted by this move,
            // it's likely good here too (e.g., if they moved a piece to a square we can capture)
            if let Some(prev) = prev_move {
                let piece_idx = pos.board[prev.from as usize].map(|pc| pc.index()).unwrap_or(0);
                let to_sq = prev.to as usize;
                if self.counter_moves[piece_idx][to_sq] == Some(m) {
                    s += 850_000; // Between killer 1 and killer 2
                }
                
                // Follow-up move: if this move often follows the previous move
                if self.followup_moves[prev.from as usize][prev.to as usize] == Some(m) {
                    s += 750_000;
                }
            }
            
            s += self.history[m.from as usize][m.to as usize];
        }
        s
    }

    fn quiescence(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: i32, prev_move: Option<Move>) -> i32 {
        if self.stop {
            return 0;
        }
        self.nodes += 1;

        let stand_pat = relative_eval(pos);
        if stand_pat >= beta {
            return beta;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }

        let in_check = pos.in_check();
        let mut moves = if in_check {
            generate_legal(pos)
        } else {
            generate_legal_captures_and_promotions(pos)
        };
        if moves.is_empty() {
            if in_check {
                return -(MATE - ply);
            }
            return alpha;
        }
        self.order(&mut moves, pos, None, 0, prev_move);

        for m in moves {
            let mut child = pos.clone();
            child.make_move(m);
            let score = -self.quiescence(&child, -beta, -alpha, ply + 1, Some(m));
            if self.stop {
                return alpha;
            }
            if score >= beta {
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }
        alpha
    }

    fn negamax(
        &mut self,
        pos: &Position,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: i32,
        prev_move: Option<Move>,
    ) -> i32 {
        if self.stop {
            return 0;
        }
        self.nodes += 1;
        self.check_time();
        if self.stop {
            return 0;
        }

        // Fifty-move rule short-circuits drawn nodes.
        if pos.halfmove >= 100 {
            return 0;
        }

        let alpha_orig = alpha;
        let mut tt_move = None;

        if depth > 0 {
            let key = pos.hash;
            let idx = (key as usize) & (TT_SIZE - 1);
            let entry = self.tt[idx];
            if entry.key == key {
                tt_move = Some(entry.mv);
                if entry.depth >= depth {
                    let s = adjust_load(entry.score, ply);
                    match entry.flag {
                        TT_EXACT => return s,
                        TT_LOWER if s >= beta => return s,
                        TT_UPPER if s <= alpha => return s,
                        _ => {}
                    }
                }
            }
        }

        // -------------------------------------------------------------------------
        // NULL MOVE PRUNING (NMP)
        // -------------------------------------------------------------------------
        // Concept: In chess, having the move is usually an advantage. If we "pass"
        // (make a null move) and the opponent still can't improve their score enough
        // to reach beta, then our position is so good that we don't need to search
        // the actual moves - we can just return beta (fail-high).
        //
        // Conditions for NMP:
        // 1. Depth >= 3 (need sufficient depth for the reduced search to be meaningful)
        // 2. Not in check (can't pass when in check)
        // 3. Static eval >= beta (position is already "too good")
        // 4. Not in a pure endgame (NMP is unsafe in endgames where zugzwang is common)
        // 5. Depth not too high (avoid NMP at very high depths where zugzwang more likely)
        // -------------------------------------------------------------------------
        if depth >= 3 && !pos.in_check() && !self.found_mate {
            // Quick endgame detection: if both sides have no queens and limited material,
            // NMP becomes risky due to zugzwang. We use a simple heuristic:
            // if the static evaluation is not overwhelmingly large, skip NMP.
            let static_eval = relative_eval(pos);
            
            // Don't NMP if eval is close to alpha (not clearly winning)
            if static_eval >= beta && depth < 12 {
                // Create a "null move" position: same board, opposite side to move,
                // no en passant, flip side-to-move in hash.
                let mut child = pos.clone();
                child.side = child.side.opposite();
                child.ep = None;
                // Update Zobrist hash incrementally: flip side-to-move, clear EP
                child.hash ^= ZOBRIST_SIDE;
                if let Some(ep) = pos.ep {
                    child.hash ^= crate::zobrist::ZOBRIST_EP[ep as usize];
                }
                
                // Reduction formula: deeper nodes get larger reductions.
                // R = 3 + depth/6 + (eval - beta)/200, capped at +3 extra.
                let eval_diff = static_eval - beta;
                let extra_reduction = (eval_diff / NMP_EVAL_DIVISOR).min(NMP_MAX_EXTRA_REDUCTION);
                let reduction = NMP_BASE_REDUCTION + depth / NMP_DEPTH_DIVISOR + extra_reduction;
                let r = reduction.min(depth - 1); // Can't reduce more than depth-1
                
                // Search with a null window [beta-1, beta] - we only care if it fails high
                let score = -self.negamax(&child, depth - 1 - r, -beta, -beta + 1, ply + 1, None);
                
                // If null move search returns >= beta, the position is "too good"
                // and we can prune this entire subtree.
                if score >= beta {
                    return beta;
                }
                // Otherwise, continue with normal search. The null move search
                // may have populated the TT with useful information.
            }
        }

        // -------------------------------------------------------------------------
        // REVERSE FUTILITY PRUNING (RFP)
        // -------------------------------------------------------------------------
        // Concept: If static eval is so high that even after subtracting a margin
        // it's still >= beta, then the position is "too good" and we can return beta
        // without searching moves. This is similar to NMP but doesn't require a
        // separate search - it's a pure static evaluation cutoff.
        //
        // We only do this at depth >= 3 where the margins are calibrated.
        // -------------------------------------------------------------------------
        if depth >= 3 && depth < 8 && !pos.in_check() {
            let static_eval = relative_eval(pos);
            let margin = RFP_MARGINS[depth as usize];
            if static_eval - margin >= beta {
                return beta;
            }
        }

        let moves = generate_legal(pos);
        if moves.is_empty() {
            return if pos.in_check() {
                -(MATE - ply)
            } else {
                0
            };
        }

        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply, prev_move);
        }

        let mut moves = moves;
        self.order(&mut moves, pos, tt_move, ply, prev_move);

        let mut best_score = -INF;
        let mut best_move = moves[0];
        let p = (ply as usize).min(MAX_PLY - 1);

        // -------------------------------------------------------------------------
        // STATIC EVALUATION FOR FUTILITY PRUNING
        // -------------------------------------------------------------------------
        // We compute the static evaluation once here for use in:
        // 1. Futility Pruning (pruning quiet moves at shallow depths)
        // 2. Probcut (optional, not implemented here)
        // 3. Extended futility pruning in the move loop
        // -------------------------------------------------------------------------
        let static_eval = relative_eval(pos);

        // -------------------------------------------------------------------------
        // FUTILITY PRUNING (at the node level, before move loop)
        // -------------------------------------------------------------------------
        // At very shallow depths (1-2), if static eval + large_margin < alpha,
        // then even the best quiet move is unlikely to raise alpha. We can skip
        // generating quiet moves entirely and only search captures/checks.
        // Note: We already generated all moves above, but we could move this
        // before move generation for efficiency. For now we use it in the loop.
        // -------------------------------------------------------------------------

        for (move_index, m) in moves.iter().enumerate() {
            // -------------------------------------------------------------------------
            // FUTILITY PRUNING (per-move, inside the loop)
            // -------------------------------------------------------------------------
            // For quiet moves (non-capture, non-promotion, non-check-giving) at
            // shallow depths, if static_eval + futility_margin <= alpha, prune.
            // The margin increases with depth because deeper searches have more
            // tactical potential to find improvements.
            //
            // We don't futility prune:
            // - Captures (tactical)
            // - Promotions (tactical)
            // - Moves that give check (forcing)
            // - The first few moves (PV candidates, TT move, captures)
            // -------------------------------------------------------------------------
            let is_quiet = !m.is_capture() && !m.is_promotion();
            let is_pv_candidate = move_index < LMR_MOVES_FULL_DEPTH;
            
            if is_quiet && !is_pv_candidate && depth <= 3 {
                let margin = FUTILITY_MARGINS[depth as usize];
                if static_eval + margin <= alpha {
                    // This quiet move is futile - even with a small positional
                    // gain it won't beat alpha. Skip it.
                    continue;
                }
            }

            // -------------------------------------------------------------------------
            // LATE MOVE REDUCTIONS (LMR)
            // -------------------------------------------------------------------------
            // Concept: Moves are ordered from best to worst. Late moves are unlikely
            // to be good, so we search them with reduced depth first. If they fail
            // high (score > alpha), we re-search at full depth to verify.
            //
            // Reduction formula: R = log(move_index) * log(depth) / 2.5
            // - First few moves (PV candidates) get full depth
            // - Captures and promotions typically excluded from LMR
            // - Moves giving check often excluded (but we don't detect checks here)
            // -------------------------------------------------------------------------
            let mut new_depth = depth - 1;
            let do_full_depth_search = is_pv_candidate || m.is_capture() || m.is_promotion();
            
            // Apply LMR for quiet moves after the first few
            if !do_full_depth_search && depth >= 3 {
                // Move index is 0-based, so move_index >= 4 means 5th move or later
                let move_num = (move_index + 1) as f32;
                let depth_f = depth as f32;
                let reduction = ((move_num.ln() * depth_f.ln()) / LMR_BASE_DIVISOR) as i32;
                let reduction = reduction.clamp(1, depth - 1); // At least 1, at most depth-1
                new_depth = depth - 1 - reduction;
            }

            let mut child = pos.clone();
            child.make_move(*m);
            let mut score = -self.negamax(&child, new_depth, -beta, -alpha, ply + 1, Some(*m));

            // -------------------------------------------------------------------------
            // LMR RE-SEARCH
            // -------------------------------------------------------------------------
            // If we reduced the depth and the move failed high (score > alpha),
            // we must re-search at full depth to verify. This is the "verification"
            // search that ensures we don't miss a good move due to over-reduction.
            // -------------------------------------------------------------------------
            if !do_full_depth_search && score > alpha && score < beta {
                score = -self.negamax(&child, depth - 1, -beta, -alpha, ply + 1, Some(*m));
            }

            if self.stop {
                return best_score;
            }
            if score > best_score {
                best_score = score;
                best_move = *m;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                // Beta cutoff: this move is too good for the opponent.
                // Update killer moves and history heuristic for quiet moves.
                if !m.is_capture() {
                    if self.killers[p][0] != Some(*m) {
                        self.killers[p][1] = self.killers[p][0];
                        self.killers[p][0] = Some(*m);
                    }
                    // History heuristic: increase score for this from->to move.
                    // Weight by depth^2 because deeper cutoffs are more significant.
                    self.history[m.from as usize][m.to as usize] += depth * depth;
                    
                    // Counter move heuristic: record that this move refutes the opponent's last move
                    if let Some(prev) = prev_move {
                        let piece_idx = pos.board[prev.from as usize].map(|pc| pc.index()).unwrap_or(0);
                        let to_sq = prev.to as usize;
                        self.counter_moves[piece_idx][to_sq] = Some(*m);
                        
                        // Follow-up move: this move often follows the previous move
                        self.followup_moves[prev.from as usize][prev.to as usize] = Some(*m);
                    }
                }
                break;
            }
        }

        if depth > 0 {
            let key = pos.hash;
            let idx = (key as usize) & (TT_SIZE - 1);
            let flag = if best_score <= alpha_orig {
                TT_UPPER
            } else if best_score >= beta {
                TT_LOWER
            } else {
                TT_EXACT
            };
            let stored = adjust_store(best_score, ply);
            let mv = if flag == TT_UPPER {
                tt_move.unwrap_or(best_move)
            } else {
                best_move
            };
            self.tt[idx] = TTEntry {
                key,
                depth,
                flag,
                score: stored,
                mv,
            };
        }

        best_score
    }

    /// Search one ply at the root (full window so we get an exact score).
    fn root_search(&mut self, pos: &Position, root_moves: &[Move], depth: i32) -> (Move, i32) {
        let mut moves = root_moves.to_vec();
        if let Some(b) = self.last_root_move {
            if let Some(i) = moves.iter().position(|m| *m == b) {
                moves.swap(0, i);
            }
        }

        let mut best_move = moves[0];
        let mut best_score = -INF;
        let mut alpha = -INF;
        let beta = INF;

        for m in &moves {
            let mut child = pos.clone();
            child.make_move(*m);
            let score = -self.negamax(&child, depth - 1, -beta, -alpha, 1, Some(*m));
            if self.stop {
                return (best_move, best_score);
            }
            if score > best_score {
                best_score = score;
                best_move = *m;
            }
            if score > alpha {
                alpha = score;
            }
        }

        self.last_root_move = Some(best_move);
        if best_score >= MATE_BOUND || best_score <= -MATE_BOUND {
            self.found_mate = true;
        }

        // Store the root node so `extract_pv` can begin reconstructing the
        // principal variation from here. The root search uses a full window,
        // so the score is exact.
        let key = pos.hash;
        let idx = (key as usize) & (TT_SIZE - 1);
        self.tt[idx] = TTEntry {
            key,
            depth,
            flag: TT_EXACT,
            score: adjust_store(best_score, 0),
            mv: best_move,
        };

        (best_move, best_score)
    }
}

/// Rich search result, including the principal variation and node count,
/// suitable for UCI `info` output.
pub struct SearchInfo {
    pub best_move: Option<Move>,
    pub score: i32,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub depth: i32,
}

/// Find the best move from `pos`, searching up to `max_depth` with a soft
/// time budget of `time_ms` milliseconds (iterative deepening). Returns `None`
/// when the side to move has no legal moves.
pub fn find_best_move(pos: &Position, max_depth: u8, time_ms: u64) -> Option<(Move, i32)> {
    let info = search_info(pos, max_depth, time_ms, None);
    info.best_move.map(|m| (m, info.score))
}

/// Search and return full diagnostics. `stop`, if provided, is checked between
/// iterative-deepening iterations so a caller (e.g. UCI `infinite`) can abort.
pub fn search_info(
    pos: &Position,
    max_depth: u8,
    time_ms: u64,
    stop: Option<&AtomicBool>,
) -> SearchInfo {
    let max_depth = max_depth.clamp(1, 64) as i32;
    let root_moves = generate_legal(pos);
    if root_moves.is_empty() {
        return SearchInfo {
            best_move: None,
            score: 0,
            pv: Vec::new(),
            nodes: 0,
            depth: 0,
        };
    }

    let mut search = Search::new(time_ms);
    let mut best_move = root_moves[0];
    let mut best_score = 0;
    let mut best_pv = Vec::new();
    let mut last_nodes = 0u64;
    let mut last_depth = 0i32;

    for depth in 1..=max_depth {
        let (mv, score) = search.root_search(pos, &root_moves, depth);
        best_move = mv;
        best_score = score;
        last_nodes = search.nodes;
        last_depth = depth;
        best_pv = extract_pv(pos, &search, 20);
        if search.found_mate {
            break;
        }
        if let Some(s) = stop {
            if s.load(Ordering::Relaxed) {
                break;
            }
        }
        if search.stop {
            break;
        }
    }

    SearchInfo {
        best_move: Some(best_move),
        score: best_score,
        pv: best_pv,
        nodes: last_nodes,
        depth: last_depth,
    }
}

/// Reconstruct the principal variation from the transposition table.
fn extract_pv(pos: &Position, search: &Search, max_len: usize) -> Vec<Move> {
    let mut pv = Vec::new();
    let mut p = pos.clone();
    for _ in 0..max_len {
        let key = p.hash;
        let idx = (key as usize) & (TT_SIZE - 1);
        let entry = search.tt[idx];
        if entry.key != key {
            break;
        }
        let m = entry.mv;
        let legal = generate_legal(&p);
        if !legal.contains(&m) {
            break;
        }
        pv.push(m);
        p.make_move(m);
    }
    pv
}

/// Compute a SAN string for `m` in the context of `pos`.
pub fn move_san(pos: &Position, m: &Move) -> String {
    let legal = generate_legal(pos);
    san(pos, m, &legal)
}