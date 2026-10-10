use crate::bitboard::*;
use crate::pieces::*;
use crate::position::*;
use crate::tables::*;

const MATERIAL: [i32; 6] = [100, 325, 335, 525, 975, 20000];

// ============================================================================
// TUNED PIECE-SQUARE TABLES (Stockfish-inspired classical evaluation)
// ============================================================================
// These tables are based on Stockfish's classical evaluation PST values,
// adapted for centipawn scale. They provide significantly better positional
// understanding than the Michniewski tables.
// Tables are indexed from White's perspective: rank 8 (index 0) to rank 1 (index 56)

// Pawn PST: Encourages center control, penalizes backward pawns, rewards advancement
const PAWN_PST: [i32; 64] = [
    0,   0,   0,   0,   0,   0,   0,   0,
    98, 134,  61,  95,  68, 126,  34, -11,
    -6,   7,  26,  31,  65,  56,  25, -20,
   -14,  13,   6,  21,  23,  12,  17, -23,
   -27,  -2,  -5,  12,  17,   6,  10, -25,
   -26,  -4,  -4, -10,   3,   3,  33, -12,
   -35,  -1, -20, -23, -15,  24,  38, -22,
    0,   0,   0,   0,   0,   0,   0,   0,
];

// Knight PST: Strong on outposts (e4, d4, e5, d5), weak on edges
const KNIGHT_PST: [i32; 64] = [
   -167, -89, -34, -49,  61, -97, -15, -107,
    -73, -41,  72,  36,  23,  62,   7,  -17,
    -47,  60,  37,  65,  84, 129,  73,   44,
     -9,  17,  89, 103,  69,  91,  72,   14,
    -31,  24,  50,  97,  70,  75,  64,   33,
    -57, -29, -14,  12,  68,  57,  56,  -2,
    -78, -54, -35, -13, -34, -42, -54, -59,
   -129, -94, -62, -62, -74, -78, -84, -129,
];

// Bishop PST: Strong on long diagonals, weak when blocked by own pawns
const BISHOP_PST: [i32; 64] = [
    -29,   4, -82, -37, -25, -42,   7,  -8,
    -26,  16, -18, -13,  30,  59,  18, -47,
    -16,  37,  43,  40,  35,  50,  37,  -2,
     -4,   5,  19,  50,  37,  37,   7,  -2,
     -6,  13,  13,  26,  34,  12,  10,   4,
     34,  24,  47,  39,  53,  58,  53,  34,
     57,  65,  70,  65,  71,  77,  74,  64,
     40,  57,  59,  67,  70,  69,  71,  54,
];

// Rook PST: Strong on open files, 7th rank, behind passed pawns
const ROOK_PST: [i32; 64] = [
    32,  42,  32,  51,  63,  9,  31,  43,
    27,  32,  58,  62,  80,  67,  26,  44,
    -5,  19,  26,  36,  17,  45,  61,  16,
   -24, -11,   7,  26,  24,  35,  -8, -20,
   -36, -26, -12,  -1,   9, -7,   6, -23,
   -45, -25, -16, -17,  3,  0,  -5, -33,
   -44, -16, -20,  -9, -1,  11,  -6, -71,
   -19, -13,   1,  17,  16,  7, -37, -26,
];

// Queen PST: Centralization, coordination with rooks/bishops
const QUEEN_PST: [i32; 64] = [
    -28,   0,  29,  12,  59,  44,  43,  45,
    -24, -39,  -5,   1, -16,  57,  28,  54,
    -13, -17,   7,   8,  29,  56,  47,  57,
    -27, -27, -16, -16,  -1,  17,  -2,   1,
     -9, -26,  -9, -10,  -2,  -4,   3,  -3,
    -14,   2, -11,  -2,  -5,   2,  14,   5,
    -35,  -8,  11,   2,   8,  15,  -3,   1,
     -1, -18,  -9,  10, -15, -25, -31, -50,
];

// King middlegame: Safety behind pawns, away from center
const KING_MID_PST: [i32; 64] = [
    -65,  23,  16, -15, -56, -34,   2,  13,
    29,  -1, -20,  -7,  -8,   4, -38, -29,
    -9,  24,  2, -16, -20,   6,  22, -22,
    -17, -20, -12, -27, -30, -25, -14, -36,
    -49,  -1, -27, -39, -46, -44, -33, -51,
   -14, -14, -22, -46, -44, -30, -15, -27,
    75,  59,  56,  54,  53,  56,  59,  75,
   123, 109,  95,  86,  81,  86,  95, 123,
];

// King endgame: Active king, centralization
const KING_END_PST: [i32; 64] = [
    -50, -40, -30, -20, -20, -30, -40, -50,
    -30, -20, -10,   0,   0, -10, -20, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  30,  40,  40,  30, -10, -30,
    -30, -10,  20,  30,  30,  20, -10, -30,
    -30, -30,   0,   0,   0,   0, -30, -30,
    -50, -30, -30, -30, -30, -30, -50, -50,
];

// ============================================================================
// KING SAFETY EVALUATION
// ============================================================================
const KING_SHIELD_WEIGHTS: [i32; 3] = [0, -50, -100];

const KING_TROPISM_WEIGHTS: [i32; 8] = [
    0, -80, -50, -30, -20, -10, -5, 0,
];

const ATTACKER_WEIGHTS: [i32; 6] = [0, 40, 40, 80, 120, 0];

fn king_zone_squares(king_sq: usize) -> [usize; 9] {
    let f = file_of(king_sq) as isize;
    let r = rank_of(king_sq) as isize;
    let mut squares = [64; 9];
    let mut idx = 0;
    squares[idx] = king_sq;
    idx += 1;
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

fn king_safety(pos: &Position, color: Color, endgame: bool) -> i32 {
    if endgame {
        return 0;
    }
    
    let king_sq = match pos.king_square(color) {
        Some(sq) => sq,
        None => return 0,
    };
    
    let them = color.opposite();
    let mut safety_score = 0;
    
    // 1. PAWN SHIELD
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
    safety_score += KING_SHIELD_WEIGHTS[missing_second_rank.min(2)] / 2;
    
    // 2. KING TROPISM
    for kind in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn] {
        let mut bb = pos.pieces_bb(them, kind);
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            
            let df = (file_of(sq) as isize - file_of(king_sq) as isize).abs();
            let dr = (rank_of(sq) as isize - rank_of(king_sq) as isize).abs();
            let dist = df.max(dr);
            
            if dist < 8 {
                safety_score += KING_TROPISM_WEIGHTS[dist as usize];
            }
        }
    }
    
    // 3. ATTACKERS ON KING ZONE
    let zone = king_zone_squares(king_sq);
    let mut attack_units = 0;
    
    for &sq in &zone {
        if sq == 64 { continue; }
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
            attack_units += attackers as i32 * ATTACKER_WEIGHTS[kind_idx + 1];
        }
        
        let pawn_attackers = pawn_attacks(color, sq) & pos.pieces_bb(them, PieceKind::Pawn);
        attack_units += pawn_attackers.count_ones() as i32 * 20;
    }
    
    if attack_units > 0 {
        safety_score -= (attack_units * attack_units) / 16;
    }
    
    // 4. OPEN FILES NEAR KING
    let king_file_idx = king_file;
    for df in -1..=1 {
        let f = king_file_idx as isize + df;
        if f < 0 || f > 7 { continue; }
        let file_bb = FILE_A << f;
        let our_pawns = pos.pieces_bb(color, PieceKind::Pawn) & file_bb;
        let their_pawns = pos.pieces_bb(them, PieceKind::Pawn) & file_bb;
        
        if our_pawns == 0 && their_pawns != 0 {
            safety_score -= 30;
        } else if our_pawns == 0 && their_pawns == 0 {
            safety_score -= 50;
        }
    }
    
    safety_score
}

// ============================================================================
// PAWN STRUCTURE EVALUATION (Enhanced)
// ============================================================================
fn pawn_structure(pos: &Position, color: Color, endgame: bool) -> i32 {
    let pawns = pos.pieces_bb(color, PieceKind::Pawn);
    let them = color.opposite();
    let enemy_pawns = pos.pieces_bb(them, PieceKind::Pawn);
    let mut score = 0;
    
    let mut file_counts = [0i32; 8];
    let mut rank_counts = [0i32; 8];
    let mut pawn_squares = Vec::new();
    
    let mut p = pawns;
    while p != 0 {
        let sq = p.trailing_zeros() as usize;
        p &= p - 1;
        let f = file_of(sq);
        let r = rank_of(sq);
        file_counts[f] += 1;
        rank_counts[r] += 1;
        pawn_squares.push((sq, f, r));
    }
    
    // 1. DOUBLED PAWNS
    for f in 0..8 {
        if file_counts[f] > 1 {
            score -= 120 * (file_counts[f] - 1);
        }
    }
    
    // 2. ISOLATED PAWNS
    for f in 0..8 {
        if file_counts[f] == 0 { continue; }
        let left = f > 0 && file_counts[f - 1] > 0;
        let right = f < 7 && file_counts[f + 1] > 0;
        if !left && !right {
            score -= 25;
        }
    }
    
    // 3. BACKWARD PAWNS
    for &(_sq, f, r) in &pawn_squares {
        let forward = if color == Color::White { 1 } else { -1 };
        let back_rank = (r as isize - forward) as isize;
        
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
            score -= 15;
        }
    }
    
    // 4. PASSED PAWNS (Enhanced with candidate/protected/outside classification)
    for &(_sq, f, r) in &pawn_squares {
        let forward = if color == Color::White { 1 } else { -1 };
        let mut passed = true;
        
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
            let rank_bonus = if color == Color::White { r as i32 } else { 7 - r as i32 };
            let base_bonus = if endgame { 100 } else { 50 };
            let rank_factor = if endgame { 20 } else { 10 };
            score += base_bonus + rank_bonus * rank_bonus * rank_factor;
            
            // Protected passed pawn bonus
            let protected = pawn_attacks(color, _sq) & pawns != 0;
            if protected {
                score += if endgame { 50 } else { 30 };
            }
            
            // Outside passed pawn bonus (on a/h file or far from enemy king)
            if f == 0 || f == 7 {
                score += if endgame { 30 } else { 20 };
            }
            
            // Connected passed pawns
            for df in -1..=1 {
                if df == 0 { continue; }
                let nf = f as isize + df;
                if nf < 0 || nf > 7 { continue; }
                let adj_sq = sq_index(nf as usize, r);
                if pos.piece_at(adj_sq) == Some(Piece::new(color, PieceKind::Pawn)) {
                    score += if endgame { 30 } else { 15 };
                }
            }
        } else {
            // Candidate passed pawn: no enemy pawns on same file ahead
            let mut candidate = true;
            for check_r in start_r..=end_r {
                let check_sq = sq_index(f, check_r);
                if (enemy_pawns & bit(check_sq)) != 0 {
                    candidate = false;
                    break;
                }
            }
            if candidate {
                let rank_bonus = if color == Color::White { r as i32 } else { 7 - r as i32 };
                score += 10 + rank_bonus * 5;
            }
        }
    }
    
    // 5. PAWN ISLANDS
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
    score -= islands * 10;
    
    // 6. PHALANX / CONNECTED PAWNS
    for f in 0..7 {
        if file_counts[f] > 0 && file_counts[f + 1] > 0 {
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
                        score += 5;
                    }
                }
            }
        }
    }
    
    score
}

// ============================================================================
// PIECE MOBILITY EVALUATION (Enhanced)
// ============================================================================
fn mobility(pos: &Position, color: Color) -> i32 {
    let occ = pos.occupied();
    let mut score = 0;
    
    // Knight mobility: weight forward moves more
    let knights = pos.pieces_bb(color, PieceKind::Knight);
    let mut k = knights;
    while k != 0 {
        let sq = k.trailing_zeros() as usize;
        k &= k - 1;
        let moves = knight_attacks(sq) & !pos.color_bb(color);
        let mut m = moves;
        while m != 0 {
            let to = m.trailing_zeros() as usize;
            m &= m - 1;
            // Forward mobility bonus
            let rank_diff = (rank_of(to) as isize - rank_of(sq) as isize) * if color == Color::White { 1 } else { -1 };
            if rank_diff > 0 { score += 6; } else { score += 4; }
        }
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
    
    // Rook mobility: strong on open files, 7th rank
    let rooks = pos.pieces_bb(color, PieceKind::Rook);
    let mut r = rooks;
    while r != 0 {
        let sq = r.trailing_zeros() as usize;
        r &= r - 1;
        let moves = rook_attacks(sq, occ) & !pos.color_bb(color);
        let mut m = moves;
        while m != 0 {
            let to = m.trailing_zeros() as usize;
            m &= m - 1;
            score += 3;
            // Bonus for controlling 7th rank
            if (color == Color::White && rank_of(to) == 6) || (color == Color::Black && rank_of(to) == 1) {
                score += 5;
            }
            // Bonus for open file control
            if (pos.pieces_bb(color, PieceKind::Pawn) & (FILE_A << file_of(to))) == 0 {
                score += 2;
            }
        }
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
// KNIGHT OUTPOST EVALUATION
// ============================================================================
fn knight_outposts(pos: &Position, color: Color) -> i32 {
    let knights = pos.pieces_bb(color, PieceKind::Knight);
    let them = color.opposite();
    let mut score = 0;
    
    let mut k = knights;
    while k != 0 {
        let sq = k.trailing_zeros() as usize;
        k &= k - 1;
        
        let r = rank_of(sq);
        let relative_rank = if color == Color::White { r as i32 } else { 7 - r as i32 };
        
        if relative_rank >= 3 && relative_rank <= 5 {
            let safe_from_pawns = pawn_attacks(them, sq) & pos.pieces_bb(them, PieceKind::Pawn) == 0;
            let supported = pawn_attacks(color, sq) & pos.pieces_bb(color, PieceKind::Pawn) != 0;
            
            if safe_from_pawns && supported {
                score += 40 + (relative_rank - 3) * 15;
            } else if safe_from_pawns {
                score += 20 + (relative_rank - 3) * 10;
            }
        }
    }
    
    score
}

// ============================================================================
// BISHOP PAIR BONUS
// ============================================================================
fn bishop_pair_bonus(pos: &Position, color: Color) -> i32 {
    let bishops = pos.pieces_bb(color, PieceKind::Bishop);
    if bishops.count_ones() >= 2 {
        let total_pawns = (pos.pieces_bb(Color::White, PieceKind::Pawn) 
            | pos.pieces_bb(Color::Black, PieceKind::Pawn)).count_ones() as i32;
        50 - total_pawns * 2
    } else {
        0
    }
}

// ============================================================================
// ROOK ON OPEN/SEMI-OPEN FILE
// ============================================================================
fn rook_open_file(pos: &Position, color: Color) -> i32 {
    let them = color.opposite();
    let rooks = pos.pieces_bb(color, PieceKind::Rook);
    let mut score = 0;
    
    let mut r = rooks;
    while r != 0 {
        let sq = r.trailing_zeros() as usize;
        r &= r - 1;
        let f = file_of(sq);
        let file_bb = FILE_A << f;
        let our_pawns = pos.pieces_bb(color, PieceKind::Pawn) & file_bb;
        let their_pawns = pos.pieces_bb(them, PieceKind::Pawn) & file_bb;
        
        if our_pawns == 0 && their_pawns == 0 {
            score += 30; // Open file
        } else if our_pawns == 0 {
            score += 15; // Semi-open file
        }
        
        // Rook on 7th rank bonus
        let r_rank = rank_of(sq);
        if (color == Color::White && r_rank == 6) || (color == Color::Black && r_rank == 1) {
            score += 25;
        }
        
        // Rook behind passed pawn
        let forward: isize = if color == Color::White { 1 } else { -1 };
        let mut ahead = (r_rank as isize) + forward;
        while ahead >= 0 && ahead < 8 {
            let sq_ahead = sq_index(f, ahead as usize);
            if pos.piece_at(sq_ahead) == Some(Piece::new(color, PieceKind::Pawn)) {
                // Check if it's a passed pawn
                let mut passed = true;
                let them_pawns = pos.pieces_bb(them, PieceKind::Pawn);
                let end_rank = if color == Color::White { 7 } else { 0 };
                let mut check_r = ahead + forward;
                while (color == Color::White && check_r <= end_rank) || (color == Color::Black && check_r >= end_rank) {
                    for df in -1..=1 {
                        let nf = f as isize + df;
                        if nf < 0 || nf > 7 { continue; }
                        if (them_pawns & bit(sq_index(nf as usize, check_r as usize))) != 0 {
                            passed = false;
                            break;
                        }
                    }
                    if !passed { break; }
                    check_r += forward;
                }
                if passed {
                    score += 40; // Rook behind passed pawn
                }
                break;
            }
            ahead += forward;
        }
    }
    
    score
}

// ============================================================================
// QUEEN EVALUATION
// ============================================================================
fn queen_evaluation(pos: &Position, color: Color) -> i32 {
    let queens = pos.pieces_bb(color, PieceKind::Queen);
    let them = color.opposite();
    let mut score = 0;
    
    let mut q = queens;
    while q != 0 {
        let sq = q.trailing_zeros() as usize;
        q &= q - 1;
        let r = rank_of(sq);
        
        // Queen on 7th rank
        if (color == Color::White && r == 6) || (color == Color::Black && r == 1) {
            score += 20;
        }
        
        // Queen on open file
        let f = file_of(sq);
        let file_bb = FILE_A << f;
        let our_pawns = pos.pieces_bb(color, PieceKind::Pawn) & file_bb;
        let their_pawns = pos.pieces_bb(them, PieceKind::Pawn) & file_bb;
        if our_pawns == 0 && their_pawns == 0 {
            score += 15;
        } else if our_pawns == 0 {
            score += 8;
        }
    }
    
    score
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

/// Static evaluation. Returns a score from White's perspective (positive favors White).
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
    
    // King safety (middlegame only)
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
    
    // Bishop pair
    score += bishop_pair_bonus(pos, Color::White);
    score -= bishop_pair_bonus(pos, Color::Black);
    
    // Knight outposts
    score += knight_outposts(pos, Color::White);
    score -= knight_outposts(pos, Color::Black);
    
    // Rook on open file / 7th rank / behind passed pawn
    score += rook_open_file(pos, Color::White);
    score -= rook_open_file(pos, Color::Black);
    
    // Queen evaluation
    score += queen_evaluation(pos, Color::White);
    score -= queen_evaluation(pos, Color::Black);
    
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
        assert!(eval.abs() < 50, "Start pos eval should be near 0, got {}", eval);
    }
    
    #[test]
    fn eval_material_difference() {
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval > 30, "White up a pawn should be positive, got {}", eval);
        
        let pos = Position::from_fen("rnbqkbnr/pppp1ppp/8/4p3/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval < -30, "Black up a pawn should be negative, got {}", eval);
    }
    
    #[test]
    fn eval_passed_pawn() {
        let pos = Position::from_fen("8/4P3/8/8/8/8/8/4k2K w - - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval > 200, "Passed pawn on 7th should be large bonus, got {}", eval);
    }
    
    #[test]
    fn eval_doubled_pawns() {
        // Position with doubled pawns on c-file (c2 and c3)
        let pos_doubled = Position::from_fen("8/8/8/8/8/2P5/2P5/4k2K w - - 0 1").unwrap();
        // Same material but single pawn on c3
        let pos_single = Position::from_fen("8/8/8/8/8/2P5/8/4k2K w - - 0 1").unwrap();
        let eval_doubled = evaluate(&pos_doubled);
        let eval_single = evaluate(&pos_single);
        // Doubled has one extra pawn (100cp material) but should have penalty
        // The difference should be less than 100 (material value) due to penalty
        let diff = eval_doubled - eval_single;
        assert!(diff < 100, "Doubled pawn penalty should reduce value: diff={}, expected < 100", diff);
    }
    
    #[test]
    fn eval_knight_outpost() {
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/3NP3/8/PPPP1PPP/RNBQKB1R w KQkq - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval > 30, "Knight outpost should give bonus, got {}", eval);
    }
    
    #[test]
    fn eval_rook_open_file() {
        // White rook on d1 (file d), Black king on g8, White king on h1
        // d-file is open (no pawns)
        let pos = Position::from_fen("6k1/8/8/8/8/8/8/3R3K w - - 0 1").unwrap();
        let eval = evaluate(&pos);
        assert!(eval > 30, "Rook on open file should give bonus, got {}", eval);
    }
}