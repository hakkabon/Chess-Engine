use crate::eval::relative_eval;
use crate::movegen::{generate_legal, generate_legal_captures_and_promotions, san};
use crate::pieces::Color;
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

// Bucketed TT: 4 entries per bucket for better replacement
const TT_BUCKETS: usize = 1 << 18; // 262,144 buckets
const TT_ENTRIES_PER_BUCKET: usize = 4;
const TT_SIZE: usize = TT_BUCKETS * TT_ENTRIES_PER_BUCKET; // 1,048,576 entries

#[derive(Clone, Copy)]
struct TTEntry {
    key: u64,
    depth: i32,
    flag: u8,
    score: i32,
    mv: Move,
    gen: u8, // Generation for replacement (always-replace vs depth-preferred)
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
            gen: 0,
        }
    }
}

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

const FUTILITY_MARGINS: [i32; 4] = [0, 75, 150, 225];
const RFP_MARGINS: [i32; 8] = [0, 0, 0, 100, 150, 200, 250, 300];

const NMP_BASE_REDUCTION: i32 = 3;
const NMP_DEPTH_DIVISOR: i32 = 6;
const NMP_EVAL_DIVISOR: i32 = 200;
const NMP_MAX_EXTRA_REDUCTION: i32 = 3;

const LMR_BASE_DIVISOR: f32 = 2.5;
const LMR_MOVES_FULL_DEPTH: usize = 4;

/// Aspiration window delta (centipawns) for each depth.
/// Wider window at higher depths.
const ASPIRATION_DELTAS: [i32; 10] = [0, 0, 0, 30, 40, 50, 60, 70, 80, 90];

struct Search {
    tt: Vec<TTEntry>,
    tt_gen: u8, // Current generation for TT replacement
    killers: [[Option<Move>; 2]; MAX_PLY],
    counter_moves: [[Option<Move>; 64]; 12],
    followup_moves: [[Option<Move>; 64]; 64],
    history: [[i32; 64]; 64],
    start: Instant,
    time_limit: Duration,
    max_time: Duration, // Hard time limit
    nodes: u64,
    stop: bool,
    last_root_move: Option<Move>,
    found_mate: bool,
    // Time management
    time_per_move: u64,
    move_overhead: u64,
    wtime: u64,
    btime: u64,
    winc: u64,
    binc: u64,
    moves_to_go: u32,
    root_depth: i32,
    root_best_score: i32,
}

impl Search {
    fn new(time_ms: u64) -> Search {
        Search {
            tt: vec![TTEntry::empty(); TT_SIZE],
            tt_gen: 0,
            killers: [[None; 2]; MAX_PLY],
            counter_moves: [[None; 64]; 12],
            followup_moves: [[None; 64]; 64],
            history: [[0; 64]; 64],
            start: Instant::now(),
            time_limit: Duration::from_millis(time_ms.max(50)),
            max_time: Duration::from_millis(time_ms.max(50)),
            nodes: 0,
            stop: false,
            last_root_move: None,
            found_mate: false,
            time_per_move: time_ms.max(50),
            move_overhead: 50,
            wtime: 0,
            btime: 0,
            winc: 0,
            binc: 0,
            moves_to_go: 0,
            root_depth: 0,
            root_best_score: 0,
        }
    }

    fn new_with_time_control(
        wtime: u64,
        btime: u64,
        winc: u64,
        binc: u64,
        moves_to_go: u32,
        side: Color,
    ) -> Search {
        let my_time = if side == Color::White { wtime } else { btime };
        let my_inc = if side == Color::White { winc } else { binc };
        
        // Calculate time for this move
        // Use a fraction of remaining time, adjusted for increment
        let moves_left = if moves_to_go > 0 { moves_to_go as f64 } else { 30.0 };
        let time_budget = (my_time as f64 / moves_left) + (my_inc as f64 * 0.8);
        
        // Cap at 80% of remaining time, minimum 50ms
        let time_ms = (time_budget.min(my_time as f64 * 0.8)).max(50.0) as u64;
        
        let mut s = Search::new(time_ms);
        s.wtime = wtime;
        s.btime = btime;
        s.winc = winc;
        s.binc = binc;
        s.moves_to_go = moves_to_go;
        s.time_per_move = time_ms;
        s.max_time = Duration::from_millis(time_ms.saturating_add(200)); // Hard limit
        s
    }

    fn check_time(&mut self) {
        if self.nodes & 4095 == 0 {
            let elapsed = self.start.elapsed();
            if elapsed >= self.time_limit {
                self.stop = true;
            }
            // Hard stop if we exceed max_time
            if elapsed >= self.max_time {
                self.stop = true;
            }
        }
    }

    // Increment generation for new search (for TT replacement)
    fn new_search(&mut self) {
        self.tt_gen = self.tt_gen.wrapping_add(1);
        if self.tt_gen == 0 { self.tt_gen = 1; } // Avoid 0
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
            let see_result = see(pos, m);
            s += 1_000_000 + see_result.gain * 10;
        } else {
            let p = (ply as usize).min(MAX_PLY - 1);
            if self.killers[p][0] == Some(m) {
                s += 900_000;
            } else if self.killers[p][1] == Some(m) {
                s += 800_000;
            }
            
            if let Some(prev) = prev_move {
                let piece_idx = pos.board[prev.from as usize].map(|pc| pc.index()).unwrap_or(0);
                let to_sq = prev.to as usize;
                if self.counter_moves[piece_idx][to_sq] == Some(m) {
                    s += 850_000;
                }
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

        if pos.halfmove >= 100 {
            return 0;
        }

        let alpha_orig = alpha;
        let mut tt_move = None;

        if depth > 0 {
            let key = pos.hash;
            let bucket = (key as usize) & (TT_BUCKETS - 1);
            let base_idx = bucket * TT_ENTRIES_PER_BUCKET;
            
            // Probe all entries in bucket
            for i in 0..TT_ENTRIES_PER_BUCKET {
                let entry = self.tt[base_idx + i];
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
                    break; // Found matching key, no need to check other entries
                }
            }
        }

        // NULL MOVE PRUNING
        if depth >= 3 && !pos.in_check() && !self.found_mate {
            let static_eval = relative_eval(pos);
            if static_eval >= beta && depth < 12 {
                let mut child = pos.clone();
                child.side = child.side.opposite();
                child.ep = None;
                child.hash ^= ZOBRIST_SIDE;
                if let Some(ep) = pos.ep {
                    child.hash ^= crate::zobrist::ZOBRIST_EP[ep as usize];
                }
                
                let eval_diff = static_eval - beta;
                let extra_reduction = (eval_diff / NMP_EVAL_DIVISOR).min(NMP_MAX_EXTRA_REDUCTION);
                let reduction = NMP_BASE_REDUCTION + depth / NMP_DEPTH_DIVISOR + extra_reduction;
                let r = reduction.min(depth - 1);
                
                let score = -self.negamax(&child, depth - 1 - r, -beta, -beta + 1, ply + 1, None);
                if score >= beta {
                    return beta;
                }
            }
        }

        // REVERSE FUTILITY PRUNING
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

        let static_eval = relative_eval(pos);

        for (move_index, m) in moves.iter().enumerate() {
            // FUTILITY PRUNING
            let is_quiet = !m.is_capture() && !m.is_promotion();
            let is_pv_candidate = move_index < LMR_MOVES_FULL_DEPTH;
            
            if is_quiet && !is_pv_candidate && depth <= 3 {
                let margin = FUTILITY_MARGINS[depth as usize];
                if static_eval + margin <= alpha {
                    continue;
                }
            }

            // LATE MOVE REDUCTIONS
            let mut new_depth = depth - 1;
            let do_full_depth_search = is_pv_candidate || m.is_capture() || m.is_promotion();
            
            if !do_full_depth_search && depth >= 3 {
                let move_num = (move_index + 1) as f32;
                let depth_f = depth as f32;
                let reduction = ((move_num.ln() * depth_f.ln()) / LMR_BASE_DIVISOR) as i32;
                let reduction = reduction.clamp(1, depth - 1);
                new_depth = depth - 1 - reduction;
            }

            let mut child = pos.clone();
            child.make_move(*m);
            let mut score = -self.negamax(&child, new_depth, -beta, -alpha, ply + 1, Some(*m));

            // LMR RE-SEARCH
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
                if !m.is_capture() {
                    if self.killers[p][0] != Some(*m) {
                        self.killers[p][1] = self.killers[p][0];
                        self.killers[p][0] = Some(*m);
                    }
                    self.history[m.from as usize][m.to as usize] += depth * depth;
                    
                    if let Some(prev) = prev_move {
                        let piece_idx = pos.board[prev.from as usize].map(|pc| pc.index()).unwrap_or(0);
                        let to_sq = prev.to as usize;
                        self.counter_moves[piece_idx][to_sq] = Some(*m);
                        self.followup_moves[prev.from as usize][prev.to as usize] = Some(*m);
                    }
                }
                break;
            }
        }

        if depth > 0 {
            let key = pos.hash;
            let bucket = (key as usize) & (TT_BUCKETS - 1);
            let base_idx = bucket * TT_ENTRIES_PER_BUCKET;
            
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
            
            // Bucketed replacement strategy
            // Slot 0: Always-replace (for exact scores and current search)
            // Slots 1-3: Depth-preferred (keep deeper entries)
            let mut replace_idx = 0;
            let mut best_depth = -1;
            let mut best_depth_idx = 1;
            
            for i in 0..TT_ENTRIES_PER_BUCKET {
                let entry = self.tt[base_idx + i];
                if entry.key == key {
                    replace_idx = i;
                    break;
                }
                if entry.key == 0 {
                    replace_idx = i;
                    break;
                }
                // Track deepest entry for depth-preferred replacement
                if entry.depth > best_depth {
                    best_depth = entry.depth;
                    best_depth_idx = i;
                }
            }
            
            // If no empty slot and no key match, use depth-preferred (skip slot 0)
            if replace_idx == 0 && self.tt[base_idx].key != 0 && self.tt[base_idx].key != key {
                if flag == TT_EXACT || depth > best_depth {
                    replace_idx = 0; // Always-replace slot for exact scores
                } else {
                    replace_idx = best_depth_idx; // Depth-preferred
                }
            }
            
            self.tt[base_idx + replace_idx] = TTEntry {
                key,
                depth,
                flag,
                score: stored,
                mv,
                gen: self.tt_gen,
            };
        }

        best_score
    }

    /// Search one ply at the root with aspiration windows.
    fn root_search(&mut self, pos: &Position, root_moves: &[Move], depth: i32) -> (Move, i32) {
        let mut moves = root_moves.to_vec();
        if let Some(b) = self.last_root_move {
            if let Some(i) = moves.iter().position(|m| *m == b) {
                moves.swap(0, i);
            }
        }

        let mut best_move = moves[0];
        let mut best_score = -INF;
        
        // Aspiration window around previous best score
        let mut alpha = -INF;
        let mut beta = INF;
        
        if depth >= 4 && self.root_depth > 0 {
            let delta = ASPIRATION_DELTAS[depth as usize].min(100);
            alpha = self.root_best_score - delta;
            beta = self.root_best_score + delta;
            
            // Clamp to valid range
            if alpha < -INF + 100 { alpha = -INF + 100; }
            if beta > INF - 100 { beta = INF - 100; }
        }

        let mut research = false;
        
        loop {
            alpha = if research { -INF } else { alpha };
            beta = if research { INF } else { beta };
            best_score = -INF;
            
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

            // Check for aspiration window failure
            if !research && (best_score <= alpha || best_score >= beta) {
                research = true;
                continue;
            }
            break;
        }

        self.last_root_move = Some(best_move);
        self.root_best_score = best_score;
        if best_score >= MATE_BOUND || best_score <= -MATE_BOUND {
            self.found_mate = true;
        }

        // Store root in TT
        let key = pos.hash;
        let bucket = (key as usize) & (TT_BUCKETS - 1);
        let base_idx = bucket * TT_ENTRIES_PER_BUCKET;
        self.tt[base_idx] = TTEntry {
            key,
            depth,
            flag: TT_EXACT,
            score: adjust_store(best_score, 0),
            mv: best_move,
            gen: self.tt_gen,
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
        search.root_depth = depth;
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

/// Search with full time control parameters (for UCI `go wtime btime winc binc movestogo`)
pub fn search_info_time_control(
    pos: &Position,
    max_depth: u8,
    wtime: u64,
    btime: u64,
    winc: u64,
    binc: u64,
    moves_to_go: u32,
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

    let mut search = Search::new_with_time_control(wtime, btime, winc, binc, moves_to_go, pos.side);
    let mut best_move = root_moves[0];
    let mut best_score = 0;
    let mut best_pv = Vec::new();
    let mut last_nodes = 0u64;
    let mut last_depth = 0i32;

    for depth in 1..=max_depth {
        search.root_depth = depth;
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
        let bucket = (key as usize) & (TT_BUCKETS - 1);
        let base_idx = bucket * TT_ENTRIES_PER_BUCKET;
        
        // Find matching entry in bucket
        let mut found = false;
        for i in 0..TT_ENTRIES_PER_BUCKET {
            let entry = search.tt[base_idx + i];
            if entry.key == key {
                let m = entry.mv;
                let legal = generate_legal(&p);
                if !legal.contains(&m) {
                    return pv;
                }
                pv.push(m);
                p.make_move(m);
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }
    pv
}

/// Compute a SAN string for `m` in the context of `pos`.
pub fn move_san(pos: &Position, m: &Move) -> String {
    let legal = generate_legal(pos);
    san(pos, m, &legal)
}