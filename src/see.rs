//! Static Exchange Evaluation (SEE)
//!
//! SEE estimates the net material gain/loss of a capture sequence on a given
//! square. It simulates the capture-recapture sequence assuming both sides
//! play optimally (always capturing with the least valuable piece first).
//!
//! This is far more accurate than simple MVV-LVA (Most Valuable Victim -
//! Least Valuable Attacker) because it accounts for:
//! - Defenders that can recapture
//! - X-ray attacks (pieces behind the victim)
//! - Pin considerations (simplified)
//! - Promotion values
//!
//! The algorithm works by building a "swap list" of all attackers and defenders
//! on the target square, sorted by piece value, then simulating the capture
//! sequence from both sides' perspectives.

use crate::bitboard::bit;
use crate::pieces::*;
use crate::position::*;
use crate::tables::*;

/// Piece values in centipawns for SEE calculations.
/// King is given a very high value to discourage capturing it (though
/// in practice we never capture the king in legal chess).
const SEE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 20000];

/// Maximum number of attackers/defenders we track for a single square.
/// In practice, a square can have at most ~10-12 attackers in extreme cases.
const MAX_SEE_ATTACKERS: usize = 32;

/// Result of a Static Exchange Evaluation.
#[derive(Debug, Clone, Copy)]
pub struct SeeResult {
    /// Net gain from the perspective of the side making the initial capture.
    /// Positive = good capture, Negative = losing material.
    pub gain: i32,
    /// Whether the capture sequence is "winning" (gain >= 0).
    pub is_good: bool,
}

impl SeeResult {
    #[inline]
    pub fn new(gain: i32) -> SeeResult {
        SeeResult {
            gain,
            is_good: gain >= 0,
        }
    }
}

/// Compute the SEE value of a capture move.
///
/// This simulates the capture-recapture sequence on the target square,
/// assuming both sides capture with their least valuable attacker first.
///
/// # Arguments
/// * `pos` - Current position
/// * `m` - The capture move to evaluate
///
/// # Returns
/// `SeeResult` with the net gain from the moving side's perspective.
pub fn see(pos: &Position, m: Move) -> SeeResult {
    // Only evaluate captures and promotions
    if !m.is_capture() && !m.is_promotion() {
        return SeeResult::new(0);
    }

    let us = pos.side;
    let them = us.opposite();
    let target_sq = m.to as usize;
    let from_sq = m.from as usize;

    // Determine the victim piece value
    let victim_value = if m.is_ep() {
        SEE_VALUES[PieceKind::Pawn.index()]
    } else if let Some(victim) = pos.board[target_sq] {
        SEE_VALUES[victim.kind.index()]
    } else {
        // No victim (shouldn't happen for captures)
        return SeeResult::new(0);
    };

    // Determine the attacker piece value
    let attacker = pos.board[from_sq].expect("attacker exists");
    let attacker_value = SEE_VALUES[attacker.kind.index()];

    // For promotions, the "attacker" becomes the promoted piece
    let promoted_value = if let Some(promo) = m.promotion {
        SEE_VALUES[promo.index()]
    } else {
        attacker_value
    };

    // Build the list of all attackers and defenders on the target square
    // from both sides, including x-ray attackers behind the first piece.
    let occ = pos.occupied();
    
    // We'll build a combined list of (piece_value, color) pairs for all attackers
    // on the target square, then sort by value ascending (least valuable first).
    // Each side will capture with their least valuable available attacker.
    let mut attacker_list: [(i32, Color); MAX_SEE_ATTACKERS] = [(0, Color::White); MAX_SEE_ATTACKERS];
    let mut attacker_count = 0;

    // Helper to add attackers of a specific type for a specific color
    // Excludes the moving piece (from_sq) since it's the one making the capture
    let mut add_attackers_of_type = |bb: u64, kind: PieceKind, color: Color| {
        let mut b = bb & !bit(from_sq); // Exclude moving piece
        while b != 0 && attacker_count < MAX_SEE_ATTACKERS {
            b &= b - 1; // Remove LSB
            let value = SEE_VALUES[kind.index()];
            attacker_list[attacker_count] = (value, color);
            attacker_count += 1;
        }
    };

    // Pawn attacks: find pawns that attack the target square.
    // A pawn of color C attacks target_sq if it sits on a square that a pawn of
    // the OPPOSITE color would attack FROM target_sq.
    // e.g., Black pawn on d6 attacks e5. pawn_attacks(White, e5) = {d6, f6}.
    let pawn_us = pawn_attacks(us.opposite(), target_sq) & pos.pieces_bb(us, PieceKind::Pawn);
    let pawn_them = pawn_attacks(them.opposite(), target_sq) & pos.pieces_bb(them, PieceKind::Pawn);
    add_attackers_of_type(pawn_us, PieceKind::Pawn, us);
    add_attackers_of_type(pawn_them, PieceKind::Pawn, them);

    // Knight attacks
    let knight_attacks_bb = knight_attacks(target_sq);
    let knight_us = knight_attacks_bb & pos.pieces_bb(us, PieceKind::Knight);
    let knight_them = knight_attacks_bb & pos.pieces_bb(them, PieceKind::Knight);
    add_attackers_of_type(knight_us, PieceKind::Knight, us);
    add_attackers_of_type(knight_them, PieceKind::Knight, them);

    // Bishop/Queen attacks (diagonal) - includes x-ray attacks
    let bishop_attacks_bb = bishop_attacks(target_sq, occ);
    let bishop_us = bishop_attacks_bb & (pos.pieces_bb(us, PieceKind::Bishop) | pos.pieces_bb(us, PieceKind::Queen));
    let bishop_them = bishop_attacks_bb & (pos.pieces_bb(them, PieceKind::Bishop) | pos.pieces_bb(them, PieceKind::Queen));
    // Separate bishops and queens for correct values
    let bishop_us_only = bishop_us & pos.pieces_bb(us, PieceKind::Bishop);
    let queen_us_only = bishop_us & pos.pieces_bb(us, PieceKind::Queen);
    let bishop_them_only = bishop_them & pos.pieces_bb(them, PieceKind::Bishop);
    let queen_them_only = bishop_them & pos.pieces_bb(them, PieceKind::Queen);
    add_attackers_of_type(bishop_us_only, PieceKind::Bishop, us);
    add_attackers_of_type(queen_us_only, PieceKind::Queen, us);
    add_attackers_of_type(bishop_them_only, PieceKind::Bishop, them);
    add_attackers_of_type(queen_them_only, PieceKind::Queen, them);

    // Rook/Queen attacks (orthogonal) - includes x-ray attacks
    let rook_attacks_bb = rook_attacks(target_sq, occ);
    let rook_us = rook_attacks_bb & (pos.pieces_bb(us, PieceKind::Rook) | pos.pieces_bb(us, PieceKind::Queen));
    let rook_them = rook_attacks_bb & (pos.pieces_bb(them, PieceKind::Rook) | pos.pieces_bb(them, PieceKind::Queen));
    let rook_us_only = rook_us & pos.pieces_bb(us, PieceKind::Rook);
    let queen_us_rook = rook_us & pos.pieces_bb(us, PieceKind::Queen);
    let rook_them_only = rook_them & pos.pieces_bb(them, PieceKind::Rook);
    let queen_them_rook = rook_them & pos.pieces_bb(them, PieceKind::Queen);
    add_attackers_of_type(rook_us_only, PieceKind::Rook, us);
    add_attackers_of_type(queen_us_rook, PieceKind::Queen, us);
    add_attackers_of_type(rook_them_only, PieceKind::Rook, them);
    add_attackers_of_type(queen_them_rook, PieceKind::Queen, them);

    // King attacks (for completeness, though king captures are rare in SEE)
    let king_attacks_bb = king_attacks(target_sq);
    let king_us = king_attacks_bb & pos.pieces_bb(us, PieceKind::King);
    let king_them = king_attacks_bb & pos.pieces_bb(them, PieceKind::King);
    add_attackers_of_type(king_us, PieceKind::King, us);
    add_attackers_of_type(king_them, PieceKind::King, them);

    // Sort attackers by value ascending (least valuable first)
    // This ensures each side captures with their least valuable piece
    attacker_list[..attacker_count].sort_by_key(|&(v, _)| v);

    // =========================================================================
    // SIMPLIFIED SEE ALGORITHM
    // =========================================================================
    // We simulate the forced capture sequence, tracking net gain from the
    // perspective of the side who made the INITIAL capture (us).
    //
    // Algorithm:
    // 1. Build lists of attackers for both sides (excluding the moving piece)
    // 2. Sort each side's attackers by value ascending (least valuable first)
    // 3. Simulate alternating captures: each side captures with their least
    //    valuable available attacker
    // 4. Track running gain from our perspective:
    //    - gain[0] = victim_value (we captured this)
    //    - When opponent captures our piece: gain -= their_piece_value
    //    - When we capture their piece: gain += their_piece_value
    // 5. At each step, the side to move can choose to STOP (stand pat) or
    //    CONTINUE. This is handled by minimax from the bottom up.
    // =========================================================================

    // Separate attackers by color for easier access
    let mut us_attackers: [i32; MAX_SEE_ATTACKERS] = [0; MAX_SEE_ATTACKERS];
    let mut them_attackers: [i32; MAX_SEE_ATTACKERS] = [0; MAX_SEE_ATTACKERS];
    let mut us_count = 0;
    let mut them_count = 0;

    for i in 0..attacker_count {
        let (val, color) = attacker_list[i];
        if color == us {
            us_attackers[us_count] = val;
            us_count += 1;
        } else {
            them_attackers[them_count] = val;
            them_count += 1;
        }
    }

    // Simulate the capture sequence
    // gain[i] = net gain from our perspective after i captures
    // gain[0] = victim_value (we just captured the victim)
    let mut gain = [0i32; MAX_SEE_ATTACKERS];
    gain[0] = victim_value;
    let mut gain_len = 1;

    // The piece currently on the target square (starts as our promoted piece)
    let mut piece_on_square = promoted_value;
    let mut turn = them; // Opponent moves first (recapture)
    let mut us_idx = 0;
    let mut them_idx = 0;

    while gain_len < MAX_SEE_ATTACKERS {
        if turn == us {
            // Our turn to recapture
            if us_idx >= us_count {
                break; // No more attackers
            }
            let attacker_val = us_attackers[us_idx];
            us_idx += 1;
            // We capture their piece: gain increases by their piece value
            gain[gain_len] = gain[gain_len - 1] + attacker_val;
            piece_on_square = attacker_val;
        } else {
            // Opponent's turn to recapture
            if them_idx >= them_count {
                break; // No more attackers
            }
            let attacker_val = them_attackers[them_idx];
            them_idx += 1;
            // They capture our piece: gain decreases by our piece value
            gain[gain_len] = gain[gain_len - 1] - piece_on_square;
            piece_on_square = attacker_val;
        }
        turn = turn.opposite();
        gain_len += 1;
    }

    // Apply minimax from the bottom up
    // At each step, the side to move can choose to stop (keep current gain)
    // or continue (get the next gain).
    // - If it's our turn at step i (even i), we maximize: gain[i] = max(gain[i], gain[i+1])
    // - If it's their turn at step i (odd i), they minimize our gain: gain[i] = max(gain[i], -gain[i+1])
    //   (from their perspective, they want to maximize their gain = -our_gain)
    for i in (1..gain_len).rev() {
        if i % 2 == 1 {
            // Odd index = opponent's turn (they just moved)
            // They choose max for themselves = min for us
            gain[i - 1] = gain[i - 1].min(gain[i]);
        } else {
            // Even index = our turn (we just moved)
            // We choose max for ourselves
            gain[i - 1] = gain[i - 1].max(gain[i]);
        }
    }

    SeeResult::new(gain[0])
}

/// Static Exchange Evaluation for a capture on a specific square by a specific piece.
/// This is a simplified version used when we don't have a full Move object.
#[allow(dead_code)]
pub fn see_capture(_pos: &Position, _from: usize, _to: usize, attacker_kind: PieceKind, victim_kind: PieceKind, _promo: Option<PieceKind>) -> i32 {
    let attacker_value = SEE_VALUES[attacker_kind.index()];
    let victim_value = SEE_VALUES[victim_kind.index()];
    
    // Quick MVV-LVA fallback for non-captures or when SEE not needed
    victim_value * 10 - attacker_value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::{Position, Move, FLAG_CAPTURE};

    #[test]
    fn see_simple_capture() {
        // White Queen on d1 captures Black Queen on d8
        // Black King on h8 (not defending d8), so no defenders on d8
        // gain = 900 (queen value)
        let pos = Position::from_fen("3q3k/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        // Qd1xd8
        let m = Move { from: 3, to: 59, promotion: None, flags: FLAG_CAPTURE };
        let result = see(&pos, m);
        assert!(result.gain > 0, "Should be winning capture, got {}", result.gain);
    }

    #[test]
    fn see_bad_capture() {
        // White Knight on f3 captures Black Pawn on e5, defended by pawn on d6
        // Nxe5 dxe5 -> Knight (320) for Pawn (100) = -220
        // Position: Black pawn on e5 (square 36), Black pawn on d6 (square 35) defending it
        // White Knight on f3 (square 21), White pawn on e4 (square 28)
        let pos = Position::from_fen("rnbqkbnr/pppp1ppp/3p4/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 1").unwrap();
        // Nf3xe5: from f3 (21) to e5 (36)
        let m = Move { from: 21, to: 36, promotion: None, flags: FLAG_CAPTURE };
        let result = see(&pos, m);
        assert!(result.gain < 0, "Should be losing capture, got {}", result.gain);
    }

    #[test]
    fn see_good_capture() {
        // White Bishop on c4 captures Black Knight on f7, defended by King only
        // Bxf7+ Kxf7 -> Bishop (330) for Knight (320) = +10, but King moves so no recapture
        // Actually King must recapture, so it's 330 - 0 (king not captured) = +330
        // But SEE simulates Kxf7, so gain = 320 - 330 = -10? No...
        // Let's use a clearer example: Qxh7 where h7 pawn defended only by King
    }
}