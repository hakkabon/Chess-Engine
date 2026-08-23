use crate::bitboard::{file_of, rank_of, sq_index};
use crate::movegen::generate_legal;
use crate::pieces::*;
use crate::position::*;

/// Parse a single SAN move (e.g. `e4`, `Nf3`, `Raxd1`, `O-O`, `e8=Q`) in the
/// context of `pos` and return the matching legal `Move`.
///
/// The move number prefix, check/mate markers (`+`/`#`), annotations (`!`/`?`)
/// and trailing result tokens are ignored, so this is tolerant of real-world
/// PGN tokens.
pub fn parse_san(pos: &Position, input: &str) -> Result<Move, String> {
    let s = normalize_san_token(input);
    if s.is_empty() {
        return Err(format!("empty SAN move: {input:?}"));
    }

    // Castling is detected up front (no destination square is given).
    if s.starts_with('O') || s.starts_with('0') {
        let queen = s.chars().filter(|c| *c == '-').count() >= 2;
        return find_castle(pos, queen);
    }

    // Split off a promotion suffix, e.g. "e8=Q" or "e8Q".
    let mut promo: Option<PieceKind> = None;
    let mut end = s.len();
    if let Some(eq) = s.find('=').or_else(|| s.find('/')) {
        let pc = s.as_bytes()[eq + 1] as char;
        promo = match pc {
            'Q' => Some(PieceKind::Queen),
            'R' => Some(PieceKind::Rook),
            'B' => Some(PieceKind::Bishop),
            'N' => Some(PieceKind::Knight),
            other => return Err(format!("invalid promotion '{other}' in SAN: {input:?}")),
        };
        end = eq;
    } else {
        let b = s.as_bytes();
        if b.len() >= 2 {
            let last = b[b.len() - 1];
            let prev = b[b.len() - 2];
            if matches!(last, b'Q' | b'R' | b'B' | b'N') && prev.is_ascii_digit() {
                promo = match last {
                    b'Q' => Some(PieceKind::Queen),
                    b'R' => Some(PieceKind::Rook),
                    b'B' => Some(PieceKind::Bishop),
                    b'N' => Some(PieceKind::Knight),
                    _ => None,
                };
                end = b.len() - 1;
            }
        }
    }
    let body = &s[..end];

    let bytes = body.as_bytes();
    if bytes.is_empty() {
        return Err(format!("malformed SAN: {input:?}"));
    }

    // Piece kind: an uppercase letter other than 'P' is the piece; otherwise pawn.
    let (kind, after) = if bytes[0].is_ascii_uppercase() && bytes[0] != b'P' {
        match bytes[0] {
            b'K' => (PieceKind::King, &body[1..]),
            b'Q' => (PieceKind::Queen, &body[1..]),
            b'R' => (PieceKind::Rook, &body[1..]),
            b'B' => (PieceKind::Bishop, &body[1..]),
            b'N' => (PieceKind::Knight, &body[1..]),
            other => return Err(format!("unknown piece letter '{other}' in SAN: {input:?}")),
        }
    } else {
        (PieceKind::Pawn, body)
    };

    let ab = after.as_bytes();
    let n = ab.len();
    if n < 2 {
        return Err(format!("SAN missing destination square: {input:?}"));
    }
    let dest_file = (ab[n - 2] - b'a') as usize;
    let dest_rank = (ab[n - 1] - b'1') as usize;
    let dest = sq_index(dest_file, dest_rank);
    let prefix = &after[..n - 2];

    // Disambiguation + capture marker live in the prefix.
    let mut disamb_file: Option<usize> = None;
    let mut disamb_rank: Option<usize> = None;
    let mut capture = false;
    for &b in prefix.as_bytes() {
        match b {
            b'a'..=b'h' => disamb_file = Some((b - b'a') as usize),
            b'1'..=b'8' => disamb_rank = Some((b - b'1') as usize),
            b'x' => capture = true,
            _ => {}
        }
    }

    // For a pawn the leading letter (when present) is the from-file.
    let from_file = if kind == PieceKind::Pawn {
        if n == 2 {
            dest_file
        } else {
            (ab[0] - b'a') as usize
        }
    } else {
        0
    };

    let legal = generate_legal(pos);
    let mut candidates: Vec<Move> = legal
        .into_iter()
        .filter(|m| {
            let p = pos.board[m.from as usize].expect("legal move has a source piece");
            if p.kind != kind {
                return false;
            }
            if (m.to as usize) != dest {
                return false;
            }
            if m.promotion != promo {
                return false;
            }
            if capture && !m.is_capture() {
                return false;
            }
            if kind == PieceKind::Pawn {
                file_of(m.from as usize) == from_file
            } else {
                if let Some(f) = disamb_file {
                    if file_of(m.from as usize) != f {
                        return false;
                    }
                }
                if let Some(r) = disamb_rank {
                    if rank_of(m.from as usize) != r {
                        return false;
                    }
                }
                true
            }
        })
        .collect();

    if candidates.is_empty() {
        return Err(format!("no legal move matches SAN: {input:?}"));
    }
    if candidates.len() > 1 {
        return Err(format!("ambiguous SAN: {input:?}"));
    }
    Ok(candidates.remove(0))
}

fn find_castle(pos: &Position, queen: bool) -> Result<Move, String> {
    let flag = if queen {
        FLAG_QUEEN_CASTLE
    } else {
        FLAG_KING_CASTLE
    };
    generate_legal(pos)
        .into_iter()
        .find(|m| (m.flags & flag) != 0)
        .ok_or_else(|| "no castling move available".to_string())
}

/// Normalize a single SAN token: strip move-number prefixes, check/mate and
/// annotation symbols, and keep only the characters that matter.
fn normalize_san_token(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let is_castling = (chars.first() == Some(&'O') || chars.first() == Some(&'0')) && raw.contains('-');

    let mut out = String::new();
    let mut i = 0;
    if !is_castling {
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
    }
    for c in chars.into_iter().skip(i) {
        match c {
            'O' | '0' | '-' | 'x' | '=' | '/' => out.push(c),
            'a'..='h' => out.push(c),
            'A'..='H' => out.push(c),
            'K' | 'Q' | 'R' | 'N' | 'P' => out.push(c),
            '1'..='8' => out.push(c),
            _ => {}
        }
    }
    out
}
