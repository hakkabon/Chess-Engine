use crate::bitboard::move_to_uci;
use crate::movegen::generate_legal;
use crate::position::Position;

/// Count the total number of leaf nodes at `depth` — the classic perft metric
/// used to verify move-generation correctness against known reference values.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = generate_legal(pos);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes: u64 = 0;
    for m in moves {
        let mut child = pos.clone();
        child.make_move(m);
        nodes += perft(&child, depth - 1);
    }
    nodes
}

/// Perft divided by root move: returns `(uci_move, node_count)` for each legal
/// move at the current position, useful for locating move-gen bugs.
pub fn perft_divide(pos: &Position, depth: u32) -> Vec<(String, u64)> {
    if depth == 0 {
        return Vec::new();
    }
    let moves = generate_legal(pos);
    let mut out = Vec::with_capacity(moves.len());
    for m in moves {
        let mut child = pos.clone();
        child.make_move(m);
        let n = perft(&child, depth - 1);
        out.push((move_to_uci(&m), n));
    }
    out
}
