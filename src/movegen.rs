use crate::bitboard::*;
use crate::pieces::*;
use crate::position::*;
use crate::tables::*;

/// Generate pseudo-legal moves. When `captures_only` is set, only captures
/// and promotions are produced (used by the quiescence search).
pub fn generate_pseudo(pos: &Position, captures_only: bool) -> Vec<Move> {
    let mut moves = Vec::with_capacity(48);
    let us = pos.side;
    let them = us.opposite();
    let own = pos.color_bb(us);
    let enemy = pos.color_bb(them);
    let occ = pos.occupied();

    generate_pawns(pos, us, them, own, enemy, captures_only, &mut moves);
    generate_knights(pos, us, own, enemy, captures_only, &mut moves);
    generate_sliders(pos, us, own, enemy, occ, captures_only, &mut moves);
    generate_king(pos, us, them, own, enemy, occ, captures_only, &mut moves);
    generate_castling(pos, us, them, occ, &mut moves);

    moves
}

fn add(moves: &mut Vec<Move>, from: usize, to: usize, capture: bool, promo: Option<PieceKind>, extra: u8) {
    let mut flags = extra;
    if capture {
        flags |= FLAG_CAPTURE;
    }
    moves.push(Move {
        from: from as u8,
        to: to as u8,
        promotion: promo,
        flags,
    });
}

fn generate_pawns(
    pos: &Position,
    us: Color,
    them: Color,
    _own: u64,
    _enemy: u64,
    captures_only: bool,
    moves: &mut Vec<Move>,
) {
    let pawns = pos.pieces_bb(us, PieceKind::Pawn);
    let occ = pos.occupied();
    let empty = !occ;

    // `pre_promo_rank` is the rank a pawn must stand on to be able to promote
    // on its next push/capture (white on index 6, black on index 1).
    let (push_dir, start_rank, pre_promo_rank) = match us {
        Color::White => (8isize, 1, 6),
        Color::Black => (-8isize, 6, 1),
    };

    let mut p = pawns;
    while p != 0 {
        let sq = p.trailing_zeros() as usize;
        p &= p - 1;
        let from = sq;
        let r = rank_of(from);
        let f = file_of(from) as isize;

        // Forward push(es).
        let one = (sq as isize + push_dir) as usize;
        if !captures_only && (occ & bit(one)) == 0 {
            if r == pre_promo_rank {
                for k in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight] {
                    add(moves, from, one, false, Some(k), 0);
                }
            } else {
                add(moves, from, one, false, None, 0);
                if r == start_rank {
                    let two = (one as isize + push_dir) as usize;
                    if (empty & bit(two)) != 0 {
                        add(moves, from, two, false, None, FLAG_DOUBLE_PUSH);
                    }
                }
            }
        }

        // Captures (incl. en passant) and promotions.
        for df in [-1isize, 1isize] {
            let tf = f + df;
            if tf < 0 || tf > 7 {
                continue;
            }
            let t = (sq as isize + push_dir + df) as usize;
            if (occ & bit(t)) == 0 {
                // En passant.
                if let Some(ep) = pos.ep {
                    if ep as usize == t {
                        add(moves, from, t, true, None, FLAG_EP_CAPTURE);
                    }
                }
                continue;
            }
            let target = pos.board[t];
            if target.map(|pc| pc.color == them).unwrap_or(false) {
                if r == pre_promo_rank {
                    for k in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight] {
                        add(moves, from, t, true, Some(k), 0);
                    }
                } else {
                    add(moves, from, t, true, None, 0);
                }
            }
        }
    }
    let _ = them;
}

fn generate_knights(
    pos: &Position,
    us: Color,
    own: u64,
    _enemy: u64,
    captures_only: bool,
    moves: &mut Vec<Move>,
) {
    let mut knights = pos.pieces_bb(us, PieceKind::Knight);
    while knights != 0 {
        let sq = knights.trailing_zeros() as usize;
        knights &= knights - 1;
        let mut targets = knight_attacks(sq) & !own;
        if captures_only {
            targets &= pos.occupied();
        }
        push_targets(pos, us, sq, targets, moves);
    }
}

fn generate_sliders(
    pos: &Position,
    us: Color,
    own: u64,
    _enemy: u64,
    occ: u64,
    captures_only: bool,
    moves: &mut Vec<Move>,
) {
    let mut bishops = pos.pieces_bb(us, PieceKind::Bishop);
    while bishops != 0 {
        let sq = bishops.trailing_zeros() as usize;
        bishops &= bishops - 1;
        let mut t = bishop_attacks(sq, occ) & !own;
        if captures_only {
            t &= pos.occupied();
        }
        push_targets(pos, us, sq, t, moves);
    }
    let mut rooks = pos.pieces_bb(us, PieceKind::Rook);
    while rooks != 0 {
        let sq = rooks.trailing_zeros() as usize;
        rooks &= rooks - 1;
        let mut t = rook_attacks(sq, occ) & !own;
        if captures_only {
            t &= pos.occupied();
        }
        push_targets(pos, us, sq, t, moves);
    }
    let mut queens = pos.pieces_bb(us, PieceKind::Queen);
    while queens != 0 {
        let sq = queens.trailing_zeros() as usize;
        queens &= queens - 1;
        let mut t = queen_attacks(sq, occ) & !own;
        if captures_only {
            t &= pos.occupied();
        }
        push_targets(pos, us, sq, t, moves);
    }
}

fn push_targets(pos: &Position, us: Color, from: usize, targets: u64, moves: &mut Vec<Move>) {
    let them = us.opposite();
    let enemy = pos.color_bb(them);
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as usize;
        t &= t - 1;
        let capture = (enemy & bit(to)) != 0;
        add(moves, from, to, capture, None, 0);
    }
}

fn generate_king(
    pos: &Position,
    us: Color,
    _them: Color,
    own: u64,
    _enemy: u64,
    _occ: u64,
    captures_only: bool,
    moves: &mut Vec<Move>,
) {
    let bb = pos.pieces_bb(us, PieceKind::King);
    if bb == 0 {
        return;
    }
    let sq = bb.trailing_zeros() as usize;
    let mut targets = king_attacks(sq) & !own;
    if captures_only {
        targets &= pos.occupied();
    }
    push_targets(pos, us, sq, targets, moves);
}

fn generate_castling(pos: &Position, us: Color, them: Color, occ: u64, moves: &mut Vec<Move>) {
    let rank = if us == Color::White { 0 } else { 7 };
    let ksq = sq_index(4, rank);
    if pos.board[ksq].map(|p| p.kind != PieceKind::King).unwrap_or(true) {
        return;
    }
    if pos.attacked_by(ksq, them) {
        return;
    }

    // King-side.
    let wk = if us == Color::White { CASTLE_WK } else { CASTLE_BK };
    if (pos.castling & wk) != 0 {
        let rook_sq = sq_index(7, rank);
        let between = bit(sq_index(5, rank)) | bit(sq_index(6, rank));
        let rook_ok = pos
            .board[rook_sq]
            .map(|p| p.color == us && p.kind == PieceKind::Rook)
            .unwrap_or(false);
        if rook_ok && (occ & between) == 0 && !pos.attacked_by(sq_index(5, rank), them) {
            moves.push(Move {
                from: ksq as u8,
                to: sq_index(6, rank) as u8,
                promotion: None,
                flags: FLAG_KING_CASTLE,
            });
        }
    }

    // Queen-side.
    let wq = if us == Color::White { CASTLE_WQ } else { CASTLE_BQ };
    if (pos.castling & wq) != 0 {
        let rook_sq = sq_index(0, rank);
        let between = bit(sq_index(1, rank)) | bit(sq_index(2, rank)) | bit(sq_index(3, rank));
        let rook_ok = pos
            .board[rook_sq]
            .map(|p| p.color == us && p.kind == PieceKind::Rook)
            .unwrap_or(false);
        if rook_ok && (occ & between) == 0 && !pos.attacked_by(sq_index(3, rank), them) {
            moves.push(Move {
                from: ksq as u8,
                to: sq_index(2, rank) as u8,
                promotion: None,
                flags: FLAG_QUEEN_CASTLE,
            });
        }
    }
}

/// Pseudo-legal moves that are also legal (don't leave own king in check).
pub fn generate_legal(pos: &Position) -> Vec<Move> {
    filter_legal(pos, generate_pseudo(pos, false))
}

pub fn generate_legal_captures_and_promotions(pos: &Position) -> Vec<Move> {
    filter_legal(pos, generate_pseudo(pos, true))
}

fn filter_legal(pos: &Position, moves: Vec<Move>) -> Vec<Move> {
    let us = pos.side;
    let mut legal = Vec::with_capacity(moves.len());
    for m in moves {
        let mut child = pos.clone();
        child.make_move(m);
        let king = child.king_square(us);
        let attacked = match king {
            Some(k) => child.attacked_by(k, us.opposite()),
            // No king: accept (e.g. some puzzle positions). Shouldn't happen
            // with validated FENs.
            None => true,
        };
        if !attacked {
            legal.push(m);
        }
    }
    legal
}

// -------------------------------------------
// SAN generation
// -------------------------------------------

fn material_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Knight => 320,
        PieceKind::Bishop => 330,
        PieceKind::Rook => 500,
        PieceKind::Queen => 900,
        PieceKind::King => 20000,
    }
}

pub fn san(pos: &Position, m: &Move, legal: &[Move]) -> String {
    if m.is_king_castle() {
        return check_suffix(pos, m);
    }
    if m.is_queen_castle() {
        return "O-O-O".to_string() + &check_suffix_raw(pos, m);
    }

    let moving = pos.board[m.from as usize].expect("san: no piece");
    let piece_letter = if moving.kind == PieceKind::Pawn {
        String::new()
    } else {
        moving.kind.fen_char().to_string()
    };

    // Disambiguation.
    let mut same_file = false;
    let mut same_rank = false;
    let mut ambiguous = false;
    if moving.kind != PieceKind::Pawn {
        for other in legal {
            if other.from != m.from
                && other.to == m.to
                && pos.board[other.from as usize]
                    .map(|p| p.kind == moving.kind)
                    .unwrap_or(false)
            {
                ambiguous = true;
                if file_of(other.from as usize) == file_of(m.from as usize) {
                    same_file = true;
                }
                if rank_of(other.from as usize) == rank_of(m.from as usize) {
                    same_rank = true;
                }
            }
        }
    }

    let mut disamb = String::new();
    if ambiguous {
        if !same_file {
            disamb.push((b'a' + file_of(m.from as usize) as u8) as char);
        } else if !same_rank {
            disamb.push((b'1' + rank_of(m.from as usize) as u8) as char);
        } else {
            disamb.push((b'a' + file_of(m.from as usize) as u8) as char);
            disamb.push((b'1' + rank_of(m.from as usize) as u8) as char);
        }
    }

    let mut s = piece_letter + &disamb;

    if m.is_capture() {
        if moving.kind == PieceKind::Pawn {
            s.push((b'a' + file_of(m.from as usize) as u8) as char);
        }
        s.push('x');
    }

    s.push_str(&square_name(m.to as usize));

    if let Some(promo) = m.promotion {
        s.push('=');
        s.push(promo.fen_char());
    }

    s + &check_suffix_raw(pos, m)
}

fn check_suffix_raw(pos: &Position, m: &Move) -> String {
    let mut child = pos.clone();
    child.make_move(*m);
    if !child.in_check() {
        return String::new();
    }
    // Is it mate?
    let more = generate_legal(&child);
    if more.is_empty() {
        "#".to_string()
    } else {
        "+".to_string()
    }
}

fn check_suffix(pos: &Position, m: &Move) -> String {
    "O-O".to_string() + &check_suffix_raw(pos, m)
}

// Silence unused warnings for helper.
#[allow(dead_code)]
fn _mvv_lva(victim: PieceKind, attacker: PieceKind) -> i32 {
    material_value(victim) * 10 - material_value(attacker)
}
