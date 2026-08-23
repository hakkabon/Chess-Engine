//! Deterministic Zobrist hashing used for the transposition table.
//!
//! Keys are generated at compile time with a splitmix64 PRNG so that the
//! engine is fully deterministic across runs and platforms.

use crate::pieces::*;
use crate::position::*;

const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_9B97_9F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn make_piece() -> [[[u64; 64]; 6]; 2] {
    let mut table = [[[0u64; 64]; 6]; 2];
    let mut seed: u64 = 0xABCDEF01_23456789;
    let mut color = 0;
    while color < 2 {
        let mut kind = 0;
        while kind < 6 {
            let mut sq = 0;
            while sq < 64 {
                seed = splitmix64(seed);
                table[color][kind][sq] = seed;
                sq += 1;
            }
            kind += 1;
        }
        color += 1;
    }
    table
}

const fn make_castling() -> [u64; 16] {
    let mut table = [0u64; 16];
    let mut i = 0;
    let mut seed: u64 = 0x1357_9BDF_2468_ACE0;
    while i < 16 {
        seed = splitmix64(seed);
        table[i] = seed;
        i += 1;
    }
    table
}

const fn make_ep() -> [u64; 64] {
    let mut table = [0u64; 64];
    let mut i = 0;
    let mut seed: u64 = 0xFEDC_BA98_7654_3210;
    while i < 64 {
        seed = splitmix64(seed);
        table[i] = seed;
        i += 1;
    }
    table
}

pub const ZOBRIST_PIECE: [[[u64; 64]; 6]; 2] = make_piece();
pub const ZOBRIST_CASTLING: [u64; 16] = make_castling();
pub const ZOBRIST_EP: [u64; 64] = make_ep();
pub const ZOBRIST_SIDE: u64 = 0x55AA_33CC_11EE_22DD;

#[inline]
pub fn piece_key(color: Color, kind: PieceKind, sq: usize) -> u64 {
    ZOBRIST_PIECE[color.index()][kind.index()][sq]
}

/// Full (from-scratch) Zobrist hash of a position.
pub fn compute_hash(pos: &Position) -> u64 {
    let mut h = 0u64;
    for color in 0..2usize {
        for kind in 0..6usize {
            let mut bb = pos.pieces[color * 6 + kind];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                h ^= ZOBRIST_PIECE[color][kind][sq];
            }
        }
    }
    if pos.side == Color::Black {
        h ^= ZOBRIST_SIDE;
    }
    h ^= ZOBRIST_CASTLING[pos.castling as usize];
    if let Some(ep) = pos.ep {
        h ^= ZOBRIST_EP[ep as usize];
    }
    h
}
