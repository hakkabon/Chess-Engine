use crate::eval::relative_eval;
use crate::movegen::{generate_legal, generate_legal_captures_and_promotions, san};
use crate::pieces::*;
use crate::position::*;
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

#[inline]
fn piece_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Knight => 320,
        PieceKind::Bishop => 330,
        PieceKind::Rook => 500,
        PieceKind::Queen => 900,
        PieceKind::King => 20000,
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

struct Search {
    tt: Vec<TTEntry>,
    killers: [[Option<Move>; 2]; MAX_PLY],
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

    fn order(&self, moves: &mut Vec<Move>, pos: &Position, tt_move: Option<Move>, ply: i32) {
        moves.sort_by(|a, b| self.move_score(pos, *b, tt_move, ply).cmp(&self.move_score(pos, *a, tt_move, ply)));
    }

    fn move_score(&self, pos: &Position, m: Move, tt_move: Option<Move>, ply: i32) -> i32 {
        if let Some(t) = tt_move {
            if t == m {
                return 1_000_000_000;
            }
        }
        let mut s = 0;
        if m.is_capture() {
            let victim_sq = if m.is_ep() {
                if pos.side == Color::White {
                    m.to as usize - 8
                } else {
                    m.to as usize + 8
                }
            } else {
                m.to as usize
            };
            let (victim, attacker) = if m.is_ep() {
                (PieceKind::Pawn, PieceKind::Pawn)
            } else {
                let v = pos.board[victim_sq].expect("capture target");
                (v.kind, pos.board[m.from as usize].unwrap().kind)
            };
            s += 1_000_000 + piece_value(victim) * 16 - piece_value(attacker);
        } else {
            let p = (ply as usize).min(MAX_PLY - 1);
            if self.killers[p][0] == Some(m) {
                s += 900_000;
            } else if self.killers[p][1] == Some(m) {
                s += 800_000;
            }
            s += self.history[m.from as usize][m.to as usize];
        }
        s
    }

    fn quiescence(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: i32) -> i32 {
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
        self.order(&mut moves, pos, None, 0);

        for m in moves {
            let mut child = pos.clone();
            child.make_move(m);
            let score = -self.quiescence(&child, -beta, -alpha, ply + 1);
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

        let moves = generate_legal(pos);
        if moves.is_empty() {
            return if pos.in_check() {
                -(MATE - ply)
            } else {
                0
            };
        }

        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply);
        }

        let mut moves = moves;
        self.order(&mut moves, pos, tt_move, ply);

        let mut best_score = -INF;
        let mut best_move = moves[0];
        let p = (ply as usize).min(MAX_PLY - 1);

        for m in moves {
            let mut child = pos.clone();
            child.make_move(m);
            let score = -self.negamax(&child, depth - 1, -beta, -alpha, ply + 1);
            if self.stop {
                return best_score;
            }
            if score > best_score {
                best_score = score;
                best_move = m;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                if !m.is_capture() {
                    if self.killers[p][0] != Some(m) {
                        self.killers[p][1] = self.killers[p][0];
                        self.killers[p][0] = Some(m);
                    }
                    self.history[m.from as usize][m.to as usize] += depth * depth;
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
            let score = -self.negamax(&child, depth - 1, -beta, -alpha, 1);
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
