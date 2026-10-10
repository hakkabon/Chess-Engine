use crate::bitboard::*;
use crate::pieces::*;
use crate::position::*;
use crate::tables::*;

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

// ============================================================================
// KING SAFETY EVALUATION
// ============================================================================
// King safety evaluates how safe the king is from attack. In the middlegame,
// a king with a shattered pawn shield and many enemy attackers is very vulnerable.
// We evaluate:
// 1. Pawn shield: pawns in front of the king (f,g,h files for castled king)
// 2. King tropism: enemy pieces close to the king increase danger
// 3. Attack weights: number and type of attackers on king zone
// 4. Open files near king: semi-open and open files near king are dangerous

/// Weights for king safety evaluation (in centipawns)
// Pawn shield penalties for missing pawns on f,g,h files relative to king
const KING_SHIELD_WEIGHTS: [i32; 3] = [0, -50, -100]; // 0, 1, 2+ missing pawns

// King tropism: penalty for enemy pieces near our king
// Indexed by Chebyshev distance (max of file/rank distance)
// Closer pieces = more dangerous
const KING_TROPISM_WEIGHTS: [i32; 8] = [
    0,    // distance 0 (same square - impossible)
    -80,  // distance 1
    -50,  // distance 2
    -30,  // distance 3
    -20,  // distance 4
    -10,  // distance 5
    -5,   // distance 6
    0,    // distance 7+
];

// Attack unit weights: each attacker contributes based on piece type
const ATTACKER_WEIGHTS: [i32; 6] = [
    0,    // Pawn (handled separately)
    40,   // Knight
    40,   // Bishop
    80,   // Rook
    120,  // Queen
    0,    // King (doesn't attack in middlegame)
];

// King zone: the 8 squares around the king + the king square itself
// We evaluate attacks on these squares
fn king_zone_squares(king_sq: usize) -> [usize; 9] {
    let f = file_of(king_sq) as isize;
    let r = rank_of(king_sq) as isize;
    let mut squares = [64; 9];
    let mut idx = 0;
    
    // King square itself
    squares[idx] = king_sq;
    idx += 1;
    
    // 8 surrounding squares
    for df in -1..=1 {
        for dr in -1..=1 {
            if df == 0 && dr == 0 { continue; }
            let nf = f + df;
            let nr = r + dr;
            if nf >= 0 && nf < 8 && nr >= 0 && nr < 8 {
                squares[idx] = sq_index(nf as usize, nr as usize);
                idx += 1;
            }
        }
    }
    squares
}

/// Evaluate king safety for a given color.
/// Returns a score from White's perspective (negative = king in danger).
fn king_safety(pos: &Position, color: Color, endgame: bool) -> i32 {
    // In endgame, king safety is less important (king becomes active piece)
    if endgame {
        return 0;
    }
    
    let king_sq = match pos.king_square(color) {
        Some(sq) => sq,
        None => return 0, // No king (shouldn't happen in legal positions)
    };
    
    let them = color.opposite();
    let mut safety_score = 0;
    
    // -----------------------------------------------------------
    // 1. PAWN SHIELD EVALUATION
    // -----------------------------------------------------------
    // Check the three pawns in front of the king (on f, g, h files
    // relative to king's position). Missing pawns weaken the shield.
    let king_file = file_of(king_sq);
    let king_rank = rank_of(king_sq);
    let forward = if color == Color::White { 1 } else { -1 };
    
    let mut missing_pawns = 0;
    for df in -1..=1 {
        let shield_file = (king_file as isize + df) as isize;
        if shield_file < 0 || shield_file > 7 { continue; }
        let shield_rank = (king_rank as isize + forward) as isize;
        if shield_rank < 0 || shield_rank > 7 { continue; }
        let shield_sq = sq_index(shield_file as usize, shield_rank as usize);
        if pos.piece_at(shield_sq) != Some(Piece::new(color, PieceKind::Pawn)) {
            missing_pawns += 1;
        }
    }
    
    // Also check second rank of shield (two ranks ahead)
    let mut missing_second_rank = 0;
    for df in -1..=1 {
        let shield_file = (king_file as isize + df) as isize;
        if shield_file < 0 || shield_file > 7 { continue; }
        let shield_rank = (king_rank as isize + 2 * forward) as isize;
        if shield_rank < 0 || shield_rank > 7 { continue; }
        let shield_sq = sq_index(shield_file as usize, shield_rank as usize);
        if pos.piece_at(shield_sq) != Some(Piece::new(color, PieceKind::Pawn)) {
            missing_second_rank += 1;
        }
    }
    
    safety_score += KING_SHIELD_WEIGHTS[missing_pawns.min(2)];
    safety_score += KING_SHIELD_WEIGHTS[missing_second_rank.min(2)] / 2; // Second rank less important
    
    // -----------------------------------------------------------
    // 2. KING TROPISM (enemy pieces close to king)
    // -----------------------------------------------------------
    // Enemy pieces near our king increase danger
    for kind in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn] {
        let mut bb = pos.pieces_bb(them, kind);
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            
            let df = (file_of(sq) as isize - file_of(king_sq) as isize).abs();
            let dr = (rank_of(sq) as isize - rank_of(king_sq) as isize).abs();
            let dist = df.max(dr); // Chebyshev distance
            
            if dist < 8 {
                safety_score += KING_TROPISM_WEIGHTS[dist as usize];
            }
        }
    }
    
    // -----------------------------------------------------------
    // 3. ATTACKERS ON KING ZONE
    // -----------------------------------------------------------
    // Count attackers on the king's surrounding squares
    let zone = king_zone_squares(king_sq);
    let mut attack_units = 0;
    
    for &sq in &zone {
        if sq == 64 { continue; }
        // Check each enemy piece type attacking this square
        for (kind_idx, kind) in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight].iter().enumerate() {
            let attackers = match kind {
                PieceKind::Queen => {
                    let occ = pos.occupied();
                    (queen_attacks(sq, occ) & pos.pieces_bb(them, PieceKind::Queen)).count_ones()
                }
                PieceKind::Rook => {
                    let occ = pos.occupied();
                    (rook_attacks(sq, occ) & pos.pieces_bb(them, PieceKind::Rook)).count_ones()
                }
                PieceKind::Bishop => {
                    let occ = pos.occupied();
                    (bishop_attacks(sq, occ) & pos.pieces_bb(them, PieceKind::Bishop)).count_ones()
                }
                PieceKind::Knight => {
                    (knight_attacks(sq) & pos.pieces_bb(them, PieceKind::Knight)).count_ones()
                }
                _ => 0,
            };
            attack_units += attackers as i32 * ATTACKER_WEIGHTS[kind_idx + 1]; // +1 to skip pawn
        }
        
        // Pawn attacks (special case - pawn attacks from opposite color perspective)
        let pawn_attackers = pawn_attacks(color, sq) & pos.pieces_bb(them, PieceKind::Pawn);
        attack_units += pawn_attackers.count_ones() as i32 * 20; // Pawn attacks worth less
    }
    
    // Convert attack units to penalty (non-linear: more attackers = exponentially worse)
    if attack_units > 0 {
        // Scale: attack_units^2 / 16 gives roughly:
        // 1 attacker -> 6cp, 2 -> 25cp, 3 -> 56cp, 4 -> 100cp, 5 -> 156cp, 10 -> 625cp
        safety_score -= (attack_units * attack_units) / 16;
    }
    
    // -----------------------------------------------------------
    // 4. OPEN FILES NEAR KING
    // -----------------------------------------------------------
    // Semi-open or open files near the king are dangerous
    // (enemy rooks/queens can penetrate)
    let king_file_idx = king_file;
    for df in -1..=1 {
        let f = king_file_idx as isize + df;
        if f < 0 || f > 7 { continue; }
        let file_bb = FILE_A << f;
        let our_pawns = pos.pieces_bb(color, PieceKind::Pawn) & file_bb;
        let their_pawns = pos.pieces_bb(them, PieceKind::Pawn) & file_bb;
        
        if our_pawns == 0 && their_pawns != 0 {
            // Semi-open file (enemy pawns but no our pawns) - dangerous
            safety_score -= 30;
        } else if our_pawns == 0 && their_pawns == 0 {
            // Fully open file - very dangerous
            safety_score -= 50;
        }
    }
    
    safety_score
}

// ============================================================================
// PAWN STRUCTURE EVALUATION
// ============================================================================
// Pawn structure evaluation covers:
// 1. Passed pawns (major factor, especially in endgames)
// 2. Isolated pawns (no friendly pawns on adjacent files)
// 3. Doubled pawns (multiple pawns on same file)
// 4. Backward pawns (cannot be supported by other pawns)
// 5. Pawn islands (groups of connected pawns - fewer is better)
// 6. Connected passed pawns bonus

/// Evaluate pawn structure for a given color.
/// Returns score from White's perspective.
fn pawn_structure(pos: &Position, color: Color, endgame: bool) -> i32 {
    let pawns = pos.pieces_bb(color, PieceKind::Pawn);
    let them = color.opposite();
    let enemy_pawns = pos.pieces_bb(them, PieceKind::Pawn);
    let mut score = 0;
    
    // File occupancy counts for structure analysis
    let mut file_counts = [0i32; 8];
    let mut pawn_squares = Vec::new();
    
    let mut p = pawns;
    while p != 0 {
        let sq = p.trailing_zeros() as usize;
        p &= p - 1;
        let f = file_of(sq);
        let r = rank_of(sq);
        file_counts[f] += 1;
        pawn_squares.push((sq, f, r));
    }
    
    // -----------------------------------------------------------
    // 1. DOUBLED PAWNS
    // -----------------------------------------------------------
    // Penalty for each pawn beyond the first on a file
    for f in 0..8 {
        if file_counts[f] > 1 {
            score -= 30 * (file_counts[f] - 1);
        }
    }
    
    // -----------------------------------------------------------
    // 2. ISOLATED PAWNS
    // -----------------------------------------------------------
    // A pawn is isolated if no friendly pawns on adjacent files
    for f in 0..8 {
        if file_counts[f] == 0 { continue; }
        let left = f > 0 && file_counts[f - 1] > 0;
        let right = f < 7 && file_counts[f + 1] > 0;
        if !left && !right {
            score -= 25; // Isolated pawn penalty
        }
    }
    
    // -----------------------------------------------------------
    // 3. BACKWARD PAWNS
    // -----------------------------------------------------------
    // A pawn is backward if it cannot be supported by a pawn on an
    // adjacent file and cannot advance safely
    for &(_sq, f, r) in &pawn_squares {
        let forward = if color == Color::White { 1 } else { -1 };
        let back_rank = (r as isize - forward) as isize;
        
        // Check if pawns on adjacent files are behind this pawn
        let mut supported = false;
        for df in -1..=1 {
            if df == 0 { continue; }
            let adj_f = f as isize + df;
            if adj_f < 0 || adj_f > 7 { continue; }
            let adj_sq = sq_index(adj_f as usize, back_rank as usize);
            if pos.piece_at(adj_sq) == Some(Piece::new(color, PieceKind::Pawn)) {
                supported = true;
                break;
            }
        }
        
        // Check if square in front is controlled by enemy pawn
        let front_rank = (r as isize + forward) as isize;
        let front_sq = if front_rank >= 0 && front_rank < 8 {
            Some(sq_index(f, front_rank as usize))
        } else {
            None
        };
        
        let blocked = front_sq.map_or(false, |fs| pos.piece_at(fs).is_some());
        let attacked_by_enemy_pawn = front_sq.map_or(false, |fs| {
            pawn_attacks(them, fs) & pawns != 0
        });
        
        if !supported && !blocked && attacked_by_enemy_pawn {
            score -= 15; // Backward pawn penalty
        }
    }
    
    // -----------------------------------------------------------
    // 4. PASSED PAWNS
    // -----------------------------------------------------------
    // A pawn is passed if no enemy pawns on same or adjacent files
    // ahead of it. Bonus increases dramatically with rank.
    for &(_sq, f, r) in &pawn_squares {
        let mut passed = true;
        
        // Check all ranks ahead of this pawn
        let start_r = if color == Color::White { r + 1 } else { 0 };
        let end_r = if color == Color::White { 7 } else { r - 1 };
        
        for check_r in start_r..=end_r {
            for df in -1..=1 {
                let nf = f as isize + df;
                if nf < 0 || nf > 7 { continue; }
                let check_sq = sq_index(nf as usize, check_r);
                if (enemy_pawns & bit(check_sq)) != 0 {
                    passed = false;
                    break;
                }
            }
            if !passed { break; }
        }
        
        if passed {
            // Passed pawn bonus - increases with rank
            let rank_bonus = if color == Color::White { r as i32 } else { 7 - r as i32 };
            let base_bonus = if endgame { 100 } else { 50 };
            let rank_factor = if endgame { 20 } else { 10 };
            score += base_bonus + rank_bonus * rank_bonus * rank_factor;
            
            // Extra bonus for connected passed pawns
            for df in -1..=1 {
                if df == 0 { continue; }
                let nf = f as isize + df;
                if nf < 0 || nf > 7 { continue; }
                let adj_sq = sq_index(nf as usize, r);
                if pos.piece_at(adj_sq) == Some(Piece::new(color, PieceKind::Pawn)) {
                    // Check if adjacent pawn is also passed
                    // (simplified: just give small bonus)
                    score += if endgame { 30 } else { 15 };
                }
            }
        }
    }
    
    // -----------------------------------------------------------
    // 5. PAWN ISLANDS
    // -----------------------------------------------------------
    // Groups of pawns separated by empty files. Fewer islands = better.
    let mut islands = 0;
    let mut in_island = false;
    for f in 0..8 {
        if file_counts[f] > 0 {
            if !in_island {
                islands += 1;
                in_island = true;
            }
        } else {
            in_island = false;
        }
    }
    score -= islands * 10; // Penalty per island
    
    // -----------------------------------------------------------
    // 6. PHALANX / PAWN CHAINS (small bonus for adjacent pawns)
    // -----------------------------------------------------------
    for f in 0..7 {
        if file_counts[f] > 0 && file_counts[f + 1] > 0 {
            // Check if they're on same or adjacent ranks (mutual support)
            let f1_pawns: Vec<usize> = pawn_squares.iter()
                .filter(|(_, pf, _)| *pf == f)
                .map(|(sq, _, _)| *sq)
                .collect();
            let f2_pawns: Vec<usize> = pawn_squares.iter()
                .filter(|(_, pf, _)| *pf == f + 1)
                .map(|(sq, _, _)| *sq)
                .collect();
            
            for &sq1 in &f1_pawns {
                for &sq2 in &f2_pawns {
                    let r1 = rank_of(sq1);
                    let r2 = rank_of(sq2);
                    if (r1 as isize - r2 as isize).abs() <= 1 {
                        score += 5; // Small phalanx bonus
                    }
                }
            }
        }
    }
    
    score
}

// ============================================================================
// PIECE MOBILITY EVALUATION
// ============================================================================
// Mobility = number of legal moves for a piece, weighted by piece type
// and target square importance.

fn mobility(pos: &Position, color: Color) -> i32 {
    let occ = pos.occupied();
    let mut score = 0;
    
    // Knight mobility
    let knights = pos.pieces_bb(color, PieceKind::Knight);
    let mut k = knights;
    while k != 0 {
        let sq = k.trailing_zeros() as usize;
        k &= k - 1;
        let moves = knight_attacks(sq) & !pos.color_bb(color);
        score += moves.count_ones() as i32 * 4;
    }
    
    // Bishop mobility
    let bishops = pos.pieces_bb(color, PieceKind::Bishop);
    let mut b = bishops;
    while b != 0 {
        let sq = b.trailing_zeros() as usize;
        b &= b - 1;
        let moves = bishop_attacks(sq, occ) & !pos.color_bb(color);
        score += moves.count_ones() as i32 * 5;
    }
    
    // Rook mobility
    let rooks = pos.pieces_bb(color, PieceKind::Rook);
    let mut r = rooks;
    while r != 0 {
        let sq = r.trailing_zeros() as usize;
        r &= r - 1;
        let moves = rook_attacks(sq, occ) & !pos.color_bb(color);
        score += moves.count_ones() as i32 * 3;
    }
    
    // Queen mobility
    let queens = pos.pieces_bb(color, PieceKind::Queen);
    let mut q = queens;
    while q != 0 {
        let sq = q.trailing_zeros() as usize;
        q &= q - 1;
        let moves = queen_attacks(sq, occ) & !pos.color_bb(color);
        score += moves.count_ones() as i32 * 2;
    }
    
    score
}

// ============================================================================
// BISHOP PAIR BONUS
// ============================================================================
// Having both bishops is a significant advantage in open positions.

fn bishop_pair_bonus(pos: &Position, color: Color) -> i32 {
    let bishops = pos.pieces_bb(color, PieceKind::Bishop);
    if bishops.count_ones() >= 2 {
        // Bonus larger in open positions (fewer pawns)
        let total_pawns = (pos.pieces_bb(Color::White, PieceKind::Pawn) 
            | pos.pieces_bb(Color::Black, PieceKind::Pawn)).count_ones() as i32;
        50 - total_pawns * 2 // 50 in open, ~10 in closed
    } else {
        0
    }
}

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
    
    // Material + PST
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
    
    // King safety (only in middlegame)
    if !endgame {
        score += king_safety(pos, Color::White, endgame);
        score -= king_safety(pos, Color::Black, endgame);
    }
    
    // Pawn structure
    score += pawn_structure(pos, Color::White, endgame);
    score -= pawn_structure(pos, Color::Black, endgame);
    
    // Mobility
    score += mobility(pos, Color::White);
    score -= mobility(pos, Color::Black);
    
    // Bishop pair bonus
    score += bishop_pair_bonus(pos, Color::White);
    score -= bishop_pair_bonus(pos, Color::Black);
    
    // Rook on open/semi-open file bonus
    for color in Color::ALL {
        let sign = if color == Color::White { 1 } else { -1 };
        let them = color.opposite();
        let rooks = pos.pieces_bb(color, PieceKind::Rook);
        let mut r = rooks;
        while r != 0 {
            let sq = r.trailing_zeros() as usize;
            r &= r - 1;
            let f = file_of(sq);
            let file_bb = FILE_A << f;
            let our_pawns = pos.pieces_bb(color, PieceKind::Pawn) & file_bb;
            let their_pawns = pos.pieces_bb(them, PieceKind::Pawn) & file_bb;
            
            if our_pawns == 0 && their_pawns == 0 {
                score += sign * 30; // Open file
            } else if our_pawns == 0 {
                score += sign * 15; // Semi-open file
            }
        }
    }
    
    // Queen on open file / 7th rank bonus
    for color in Color::ALL {
        let sign = if color == Color::White { 1 } else { -1 };
        let queens = pos.pieces_bb(color, PieceKind::Queen);
        let mut q = queens;
        while q != 0 {
            let sq = q.trailing_zeros() as usize;
            q &= q - 1;
            let r = rank_of(sq);
            // Queen on 7th rank (enemy's 2nd rank)
            if (color == Color::White && r == 6) || (color == Color::Black && r == 1) {
                score += sign * 20;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;
    
    #[test]
    fn eval_startpos_symmetric() {
        let pos = Position::starting();
        let eval = evaluate(&pos);
        // Starting position should be close to 0 (small first-move advantage)
        assert!(eval.abs() < 50, "Start pos eval should be near 0, got {}", eval);
    }
    
    #[test]
    fn eval_material_difference() {
        // White up a pawn
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval > 30, "White up a pawn should be positive, got {}", eval);
        
        // Black up a pawn
        let pos = Position::from_fen("rnbqkbnr/pppp1ppp/8/4p3/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval < -30, "Black up a pawn should be negative, got {}", eval);
    }
    
    #[test]
    fn eval_passed_pawn() {
        // White has passed pawn on e7 (near promotion)
        let pos = Position::from_fen("8/4P3/8/8/8/8/8/4k2K w - - 0 1").unwrap();
        let eval = evaluate(&pos);
        // Passed pawn on 7th rank should be worth significant bonus
        assert!(eval > 200, "Passed pawn on 7th should be large bonus, got {}", eval);
    }
    
    #[test]
    fn eval_doubled_pawns() {
        // White has doubled pawns on c-file
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/2P5/2P5/PP1PPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        // Should have penalty for doubled pawns
        assert!(eval < 100, "Doubled pawns should reduce eval, got {}", eval);
    }
}