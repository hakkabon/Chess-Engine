pub type BB = u64;

pub const FILE_A: BB = 0x0101_0101_0101_0101;
pub const FILE_B: BB = FILE_A << 1;
pub const FILE_C: BB = FILE_A << 2;
pub const FILE_D: BB = FILE_A << 3;
pub const FILE_E: BB = FILE_A << 4;
pub const FILE_F: BB = FILE_A << 5;
pub const FILE_G: BB = FILE_A << 6;
pub const FILE_H: BB = FILE_A << 7;

pub const RANK_1: BB = 0x0000_0000_0000_00FF;
pub const RANK_2: BB = RANK_1 << 8;
pub const RANK_3: BB = RANK_1 << 16;
pub const RANK_4: BB = RANK_1 << 24;
pub const RANK_5: BB = RANK_1 << 32;
pub const RANK_6: BB = RANK_1 << 40;
pub const RANK_7: BB = RANK_1 << 48;
pub const RANK_8: BB = RANK_1 << 56;

#[inline(always)]
pub const fn bit(sq: usize) -> BB {
    1u64 << sq
}

#[inline(always)]
pub const fn sq_index(file: usize, rank: usize) -> usize {
    rank * 8 + file
}

#[inline(always)]
pub const fn file_of(sq: usize) -> usize {
    sq & 7
}

#[inline(always)]
pub const fn rank_of(sq: usize) -> usize {
    sq >> 3
}

#[inline(always)]
pub const fn north(b: BB) -> BB {
    b << 8
}

#[inline(always)]
pub const fn south(b: BB) -> BB {
    b >> 8
}

#[inline(always)]
pub const fn east(b: BB) -> BB {
    (b & !FILE_H) << 1
}

#[inline(always)]
pub const fn west(b: BB) -> BB {
    (b & !FILE_A) >> 1
}

#[inline(always)]
pub const fn north_east(b: BB) -> BB {
    (b & !FILE_H) << 9
}

#[inline(always)]
pub const fn north_west(b: BB) -> BB {
    (b & !FILE_A) << 7
}

#[inline(always)]
pub const fn south_east(b: BB) -> BB {
    (b & !FILE_A) >> 7
}

#[inline(always)]
pub const fn south_west(b: BB) -> BB {
    (b & !FILE_H) >> 9
}

pub fn square_name(sq: usize) -> String {
    let file = (b'a' + file_of(sq) as u8) as char;
    let rank = (b'1' + rank_of(sq) as u8) as char;
    format!("{}{}", file, rank)
}

/// Parse a square name like `"e4"` into a square index.
pub fn square_from_name(s: &str) -> Result<usize, String> {
    if s.len() != 2 {
        return Err(format!("invalid square name: {s:?}"));
    }
    let b = s.as_bytes();
    let file = (b[0] - b'a') as usize;
    let rank = (b[1] - b'1') as usize;
    if file > 7 || rank > 7 {
        return Err(format!("invalid square name: {s:?}"));
    }
    Ok(rank * 8 + file)
}

/// Render a move in UCI notation: source + destination + promotion piece
/// (e.g. `"e2e4"` or `"e7e8q"`).
pub fn move_to_uci(m: &crate::position::Move) -> String {
    let mut s = square_name(m.from as usize);
    s.push_str(&square_name(m.to as usize));
    if let Some(k) = m.promotion {
        s.push(match k {
            crate::pieces::PieceKind::Queen => 'q',
            crate::pieces::PieceKind::Rook => 'r',
            crate::pieces::PieceKind::Bishop => 'b',
            crate::pieces::PieceKind::Knight => 'n',
            _ => ' ',
        });
    }
    s
}

/// Pretty-print a bitboard as an 8x8 board (rank 8 at the top), used for debugging.
pub fn pretty(b: BB) -> String {
    let mut s = String::new();
    for rank in (0..8).rev() {
        s.push_str(&format!("{} ", rank + 1));
        for file in 0..8 {
            let sq = sq_index(file, rank);
            s.push(if (b & bit(sq)) != 0 { '1' } else { '.' });
            s.push(' ');
        }
        s.push('\n');
    }
    s.push_str("  a b c d e f g h\n");
    s
}
