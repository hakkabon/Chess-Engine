use crate::bitboard::*;
use crate::pieces::Color;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub struct Magic {
    pub mask: u64,
    pub magic: u64,
    pub shift: u32,
    pub offset: usize,
}

pub struct Tables {
    pub knight: [u64; 64],
    pub king: [u64; 64],
    pub white_pawn_attacks: [u64; 64],
    pub black_pawn_attacks: [u64; 64],
    pub rook_magic: [Magic; 64],
    pub bishop_magic: [Magic; 64],
    /// Concatenated attack tables for rook (offset 0) then bishop.
    pub attacks: Vec<u64>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

pub fn tables() -> &'static Tables {
    TABLES.get_or_init(Tables::init)
}

// -------------------------------------------
// Precomputed step directions
// -------------------------------------------

const ROOK_DIRS: [(isize, isize); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(isize, isize); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const KNIGHT_DELTAS: [(isize, isize); 8] = [
    (1, 2), (2, 1), (2, -1), (1, -2),
    (-1, -2), (-2, -1), (-2, 1), (-1, 2),
];
const KING_DELTAS: [(isize, isize); 8] = [
    (1, 0), (1, 1), (0, 1), (-1, 1),
    (-1, 0), (-1, -1), (0, -1), (1, -1),
];

#[inline]
fn on_board(file: isize, rank: isize) -> bool {
    file >= 0 && file < 8 && rank >= 0 && rank < 8
}

fn step(sq: usize, dir: (isize, isize)) -> Option<usize> {
    let f = file_of(sq) as isize + dir.0;
    let r = rank_of(sq) as isize + dir.1;
    if on_board(f, r) {
        Some(sq_index(f as usize, r as usize))
    } else {
        None
    }
}

// -------------------------------------------
// Slow reference attack generation
// -------------------------------------------

fn slow_rook_attacks(sq: usize, occ: u64) -> u64 {
    let mut attacks = 0u64;
    for dir in ROOK_DIRS {
        let mut cur = sq;
        while let Some(next) = step(cur, dir) {
            attacks |= bit(next);
            if (occ & bit(next)) != 0 {
                break;
            }
            cur = next;
        }
    }
    attacks
}

fn slow_bishop_attacks(sq: usize, occ: u64) -> u64 {
    let mut attacks = 0u64;
    for dir in BISHOP_DIRS {
        let mut cur = sq;
        while let Some(next) = step(cur, dir) {
            attacks |= bit(next);
            if (occ & bit(next)) != 0 {
                break;
            }
            cur = next;
        }
    }
    attacks
}

/// Relevant occupancy mask: every square a slider attacks *except* the
/// edge squares along each ray (blockers on the edge cannot change attacks).
fn rook_mask(sq: usize) -> u64 {
    let mut mask = 0u64;
    for dir in ROOK_DIRS {
        let mut cur = sq;
        while let Some(next) = step(cur, dir) {
            // Stop before adding the final edge square.
            if let Some(after) = step(next, dir) {
                let _ = after;
                mask |= bit(next);
            }
            cur = next;
        }
    }
    mask
}

fn bishop_mask(sq: usize) -> u64 {
    let mut mask = 0u64;
    for dir in BISHOP_DIRS {
        let mut cur = sq;
        while let Some(next) = step(cur, dir) {
            if step(next, dir).is_some() {
                mask |= bit(next);
            }
            cur = next;
        }
    }
    mask
}

// -------------------------------------------
// PRNG (xorshift64star) — deterministic
// -------------------------------------------

struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        Rng(0x2545_F491_4F6C_DD1D)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Sparse random number (used to search for magic numbers).
    fn sparse(&mut self) -> u64 {
        let a = self.next();
        let b = self.next();
        let c = self.next();
        a & b & c
    }
}

// -------------------------------------------
// Magic search
// -------------------------------------------

const SENTINEL: u64 = u64::MAX;

fn find_magic(sq: usize, is_rook: bool, rng: &mut Rng, offset: usize) -> (Magic, Vec<u64>) {
    let mask = if is_rook { rook_mask(sq) } else { bishop_mask(sq) };
    let bits = mask.count_ones();
    let size = 1usize << bits;

    // Enumerate every subset of the mask (carry-rippler).
    let mut occupancies = Vec::with_capacity(size);
    let mut references = Vec::with_capacity(size);
    let mut subset = mask;
    loop {
        occupancies.push(subset);
        references.push(if is_rook {
            slow_rook_attacks(sq, subset)
        } else {
            slow_bishop_attacks(sq, subset)
        });
        if subset == 0 {
            break;
        }
        subset = (subset - 1) & mask;
    }

    let mut used = vec![SENTINEL; size];

    for _attempt in 0..10_000_000 {
        let magic = rng.sparse();
        // Heuristic: ensure the top `bits` bits are populated.
        if (magic.wrapping_mul(mask) >> (64 - bits) & 0xFF) < (1u64 << (bits.min(8))).wrapping_sub(1) {
            continue;
        }
        let mut ok = true;
        for i in 0..size {
            let idx = (occupancies[i].wrapping_mul(magic) >> (64 - bits)) as usize;
            let prev = used[idx];
            if prev == SENTINEL {
                used[idx] = references[i];
            } else if prev != references[i] {
                ok = false;
                break;
            }
        }
        if ok {
            let mut attacks = vec![0u64; size];
            for i in 0..size {
                let idx = (occupancies[i].wrapping_mul(magic) >> (64 - bits)) as usize;
                attacks[idx] = used[idx];
            }
            // Self-check: the produced table must match the reference attacks.
            for i in 0..size {
                let idx = (occupancies[i].wrapping_mul(magic) >> (64 - bits)) as usize;
                assert_eq!(
                    attacks[idx], references[i],
                    "magic verify failed sq {sq} rook={is_rook} subset={i}"
                );
            }
            return (
                Magic {
                    mask,
                    magic,
                    shift: 64 - bits,
                    offset,
                },
                attacks,
            );
        }
        // reset for next attempt
        for slot in used.iter_mut() {
            *slot = SENTINEL;
        }
    }
    panic!("failed to find magic for sq {sq} rook={is_rook}");
}

impl Tables {
    fn init() -> Tables {
        let mut knight = [0u64; 64];
        let mut king = [0u64; 64];
        let mut white_pawn = [0u64; 64];
        let mut black_pawn = [0u64; 64];

        for sq in 0..64 {
            let f = file_of(sq) as isize;
            let r = rank_of(sq) as isize;
            let mut k = 0u64;
            for (df, dr) in KNIGHT_DELTAS {
                if on_board(f + df, r + dr) {
                    k |= bit(sq_index((f + df) as usize, (r + dr) as usize));
                }
            }
            knight[sq] = k;
            let mut kg = 0u64;
            for (df, dr) in KING_DELTAS {
                if on_board(f + df, r + dr) {
                    kg |= bit(sq_index((f + df) as usize, (r + dr) as usize));
                }
            }
            king[sq] = kg;
            // Pawn attacks: a white pawn on `sq` attacks the squares one rank up.
            if r + 1 < 8 {
                for df in [-1isize, 1] {
                    if on_board(f + df, r + 1) {
                        white_pawn[sq] |= bit(sq_index((f + df) as usize, (r + 1) as usize));
                    }
                }
            }
            if r - 1 >= 0 {
                for df in [-1isize, 1] {
                    if on_board(f + df, r - 1) {
                        black_pawn[sq] |= bit(sq_index((f + df) as usize, (r - 1) as usize));
                    }
                }
            }
        }

        let mut rng = Rng::new();
        let mut rook_magic = [Magic { mask: 0, magic: 0, shift: 0, offset: 0 }; 64];
        let mut bishop_magic = [Magic { mask: 0, magic: 0, shift: 0, offset: 0 }; 64];
        let mut attacks: Vec<u64> = Vec::new();

        for sq in 0..64 {
            let offset = attacks.len();
            let (magic, att) = find_magic(sq, true, &mut rng, offset);
            rook_magic[sq] = magic;
            attacks.extend(att);
        }
        for sq in 0..64 {
            let offset = attacks.len();
            let (magic, att) = find_magic(sq, false, &mut rng, offset);
            bishop_magic[sq] = magic;
            attacks.extend(att);
        }

        Tables {
            knight,
            king,
            white_pawn_attacks: white_pawn,
            black_pawn_attacks: black_pawn,
            rook_magic,
            bishop_magic,
            attacks,
        }
    }
}

#[inline]
pub fn knight_attacks(sq: usize) -> u64 {
    tables().knight[sq]
}

#[inline]
pub fn king_attacks(sq: usize) -> u64 {
    tables().king[sq]
}

#[inline]
pub fn pawn_attacks(color: Color, sq: usize) -> u64 {
    let t = tables();
    match color {
        Color::White => t.white_pawn_attacks[sq],
        Color::Black => t.black_pawn_attacks[sq],
    }
}

#[inline]
pub fn rook_attacks(sq: usize, occ: u64) -> u64 {
    let m = &tables().rook_magic[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize;
    tables().attacks[m.offset + idx]
}

#[inline]
pub fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    let m = &tables().bishop_magic[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize;
    tables().attacks[m.offset + idx]
}

#[inline]
pub fn queen_attacks(sq: usize, occ: u64) -> u64 {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}
