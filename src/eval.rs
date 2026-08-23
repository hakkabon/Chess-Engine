use crate::bitboard::*;
use crate::pieces::*;
use crate::position::*;

const MATERIAL: [i32; 6] = [100, 320, 330, 500, 900, 20000];

// Piece-square tables (Michniewski "simplified evaluation"). Listed from
// rank 8 down to rank 1.
const PAWN_PST: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5, 5, 10, 25, 25, 10, 5, 5,
    0, 0, 0, 20, 20, 0, 0, 0,
    5, -5, -10, 0, 0, -10, -5, 5,
    5, 10, 10, -20, -20, 10, 10, 5,
    0, 0, 0, 0, 0, 0, 0, 0,
];

const KNIGHT_PST: [i32; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50,
    -40, -20, 0, 0, 0, 0, -20, -40,
    -30, 0, 10, 15, 15, 10, 0, -30,
    -30, 5, 15, 20, 20, 15, 5, -30,
    -30, 0, 15, 20, 20, 15, 0, -30,
    -30, 5, 10, 15, 15, 10, 5, -30,
    -40, -20, 0, 5, 5, 0, -20, -40,
    -50, -40, -30, -30, -30, -30, -40, -50,
];

const BISHOP_PST: [i32; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20,
    -10, 0, 0, 0, 0, 0, 0, -10,
    -10, 0, 5, 10, 10, 5, 0, -10,
    -10, 5, 5, 10, 10, 5, 5, -10,
    -10, 0, 10, 10, 10, 10, 0, -10,
    -10, 10, 10, 10, 10, 10, 10, -10,
    -10, 5, 0, 0, 0, 0, 5, -10,
    -20, -10, -10, -10, -10, -10, -10, -20,
];

const ROOK_PST: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0,
    5, 10, 10, 10, 10, 10, 10, 5,
    -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5,
    0, 0, 0, 5, 5, 0, 0, 0,
];

const QUEEN_PST: [i32; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20,
    -10, 0, 0, 0, 0, 0, 0, -10,
    -10, 0, 5, 5, 5, 5, 0, -10,
    -5, 0, 5, 5, 5, 5, 0, -5,
    0, 0, 5, 5, 5, 5, 0, -5,
    -10, 5, 5, 5, 5, 5, 0, -10,
    -10, 0, 5, 0, 0, 0, 0, -10,
    -20, -10, -10, -5, -5, -10, -10, -20,
];

const KING_MID_PST: [i32; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30,
    -20, -30, -30, -40, -40, -30, -30, -20,
    -10, -20, -20, -20, -20, -20, -20, -10,
    20, 20, 0, 0, 0, 0, 20, 20,
    20, 30, 10, 0, 0, 10, 30, 20,
];

const KING_END_PST: [i32; 64] = [
    -50, -40, -30, -20, -20, -30, -40, -50,
    -30, -20, -10, 0, 0, -10, -20, -30,
    -30, -10, 20, 30, 30, 20, -10, -30,
    -30, -10, 30, 40, 40, 30, -10, -30,
    -30, -10, 30, 40, 40, 30, -10, -30,
    -30, -10, 20, 30, 30, 20, -10, -30,
    -30, -30, 0, 0, 0, 0, -30, -30,
    -50, -30, -30, -30, -30, -30, -50, -50,
];

/// Is the position a known insufficient-material draw?
pub fn insufficient_material(pos: &Position) -> bool {
    let minors = |c: Color| -> usize {
        pos.pieces_bb(c, PieceKind::Knight).count_ones() as usize
            + pos.pieces_bb(c, PieceKind::Bishop).count_ones() as usize
    };
    let pawns = (pos.pieces_bb(Color::White, PieceKind::Pawn)
        | pos.pieces_bb(Color::Black, PieceKind::Pawn))
        != 0;
    let heavy = (pos.pieces_bb(Color::White, PieceKind::Rook)
        | pos.pieces_bb(Color::Black, PieceKind::Rook)
        | pos.pieces_bb(Color::White, PieceKind::Queen)
        | pos.pieces_bb(Color::Black, PieceKind::Queen))
        != 0;
    if pawns || heavy {
        return false;
    }
    minors(Color::White) <= 1 && minors(Color::Black) <= 1
}

fn pst_value(kind: PieceKind, sq: usize, color: Color, endgame: bool) -> i32 {
    let r = rank_of(sq);
    let f = file_of(sq);
    // Tables are written from White's perspective (rank 1 = bottom, index
    // (7 - r) * 8 + f). For Black, mirror the rank so a Black piece on its
    // 2nd rank reads the same table row as a White piece on its 2nd rank.
    let idx = if color == Color::White {
        (7 - r) * 8 + f
    } else {
        r * 8 + f
    };
    match kind {
        PieceKind::Pawn => PAWN_PST[idx],
        PieceKind::Knight => KNIGHT_PST[idx],
        PieceKind::Bishop => BISHOP_PST[idx],
        PieceKind::Rook => ROOK_PST[idx],
        PieceKind::Queen => QUEEN_PST[idx],
        PieceKind::King => {
            if endgame {
                KING_END_PST[idx]
            } else {
                KING_MID_PST[idx]
            }
        }
    }
}

fn detect_endgame(pos: &Position) -> bool {
    let wq = pos.pieces_bb(Color::White, PieceKind::Queen) != 0;
    let bq = pos.pieces_bb(Color::Black, PieceKind::Queen) != 0;
    let w_minors = pos.pieces_bb(Color::White, PieceKind::Knight).count_ones()
        + pos.pieces_bb(Color::White, PieceKind::Bishop).count_ones();
    let b_minors = pos.pieces_bb(Color::Black, PieceKind::Knight).count_ones()
        + pos.pieces_bb(Color::Black, PieceKind::Bishop).count_ones();
    let w_rook = pos.pieces_bb(Color::White, PieceKind::Rook) != 0;
    let b_rook = pos.pieces_bb(Color::Black, PieceKind::Rook) != 0;
    if !wq && !bq {
        return true;
    }
    wq && bq && w_minors <= 1 && b_minors <= 1 && !w_rook && !b_rook
}

/// Static evaluation. Returns a score from White's perspective (positive
/// favors White).
pub fn evaluate(pos: &Position) -> i32 {
    if insufficient_material(pos) {
        return 0;
    }
    let endgame = detect_endgame(pos);

    let mut score = 0;
    for kind in PieceKind::ALL {
        for color in Color::ALL {
            let mut bb = pos.pieces_bb(color, kind);
            let sign = if color == Color::White { 1 } else { -1 };
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let v = MATERIAL[kind.index()] + pst_value(kind, sq, color, endgame);
                score += sign * v;
            }
        }
    }
    score
}

/// Relative eval: score from the perspective of the side to move.
pub fn relative_eval(pos: &Position) -> i32 {
    let e = evaluate(pos);
    if pos.side == Color::White {
        e
    } else {
        -e
    }
}
