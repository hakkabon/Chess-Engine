use std::collections::HashMap;
use std::sync::OnceLock;

use crate::bitboard::square_from_name;
use crate::movegen::generate_legal;
use crate::pieces::PieceKind;
use crate::position::*;

/// A tiny opening book of common lines, encoded as UCI move sequences from the
/// start position. Playing a book move in the early game avoids spending search
/// time on well-understood positions.
pub struct Book {
    /// Position key (normalized FEN) -> the book reply (UCI string).
    map: HashMap<String, String>,
}

impl Book {
    fn build() -> Book {
        let mut map = HashMap::new();
        for line in OPENING_LINES {
            let moves: Vec<&str> = line.split_whitespace().collect();
            let mut pos = Position::starting();
            for uci in moves {
                map.insert(key_fen(&pos), (*uci).to_string());
                match parse_uci_move(&pos, uci) {
                    Some(m) => pos.make_move(m),
                    None => break,
                }
            }
        }
        Book { map }
    }

    /// Return the book reply for `pos`, if one is recorded.
    pub fn lookup(&self, pos: &Position) -> Option<Move> {
        let uci = self.map.get(&key_fen(pos))?;
        parse_uci_move(pos, uci)
    }

    /// Return the book reply as a UCI string, if recorded.
    pub fn lookup_uci(&self, pos: &Position) -> Option<String> {
        self.map.get(&key_fen(pos)).cloned()
    }
}

static BOOK: OnceLock<Book> = OnceLock::new();

/// Access the process-wide opening book (built lazily on first use).
pub fn book() -> &'static Book {
    BOOK.get_or_init(Book::build)
}

/// Convenience: look up a book move for `pos`.
pub fn lookup(pos: &Position) -> Option<Move> {
    book().lookup(pos)
}

/// Normalized FEN used as the book key: placement, side, castling rights and
/// en-passant square. Move counters are dropped so transpositions that differ
/// only in move number still match.
fn key_fen(pos: &Position) -> String {
    let f = pos.to_fen();
    let parts: Vec<&str> = f.split(' ').collect();
    format!("{} {} {} {}", parts[0], parts[1], parts[2], parts[3])
}

/// Parse a UCI move string into a legal `Move` in `pos`.
fn parse_uci_move(pos: &Position, uci: &str) -> Option<Move> {
    if uci.len() < 4 {
        return None;
    }
    let from = square_from_name(&uci[0..2]).ok()? as u8;
    let to = square_from_name(&uci[2..4]).ok()? as u8;
    let promotion = if uci.len() >= 5 {
        match uci.as_bytes()[4] {
            b'q' => Some(PieceKind::Queen),
            b'r' => Some(PieceKind::Rook),
            b'b' => Some(PieceKind::Bishop),
            b'n' => Some(PieceKind::Knight),
            _ => None,
        }
    } else {
        None
    };
    let legal = generate_legal(pos);
    legal
        .into_iter()
        .find(|m| m.from == from && m.to == to && m.promotion == promotion)
}

/// Common opening lines (UCI moves from the start position).
const OPENING_LINES: &[&str] = &[
    // Ruy Lopez
    "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1 f8e7",
    "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5c6 d7c6",
    "e2e4 e7e5 g1f3 b8c6 c2c3",
    // Italian / Giuoco Piano
    "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5",
    "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 c2c3 g8e7",
    "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 e1g1 g8f6",
    // Petrov
    "e2e4 e7e5 g1f3 g8f6",
    "e2e4 e7e5 g1f3 g8f6 f3e5 d7d6 e4f3 f6e4",
    // Scotch
    "e2e4 e7e5 g1f3 b8c6 d2d4 e5d4 f3d4",
    "e2e4 e7e5 g1f3 b8c6 d2d4 e5d4 f3d4 g8f6 b1c3 f8b4",
    // Sicilian
    "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4",
    "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3 a7a6",
    "e2e4 c7c5 g1f3 b8c6 d2d4 c5d4 f3d4 g8f6 b1c3 d7d6",
    // French
    "e2e4 e7e6 d2d4 d7d5",
    "e2e4 e7e6 d2d4 d7d5 g1f3",
    // Caro-Kann
    "e2e4 c7c6 d2d4 d7d5",
    "e2e4 c7c6 d2d4 d7d5 e4d5 c6d5",
    // Scandinavian
    "e2e4 d7d5 e4d5 d8d5",
    // Queen's Gambit
    "d2d4 d7d5 c2c4 e7e6",
    "d2d4 d7d5 c2c4 e7e6 b1c3",
    "d2d4 d7d5 c2c4 g8f6",
    // Slav / QGD
    "d2d4 d7d5 c2c4 e7e6 b1c3 c7c6",
    "d2d4 d7d5 c2c4 e7e6 b1c3 g8f6",
    // King's Indian / Grünfeld setups
    "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d5",
    "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d5 c4d5 f6d5",
    // English / Réti
    "c2c4 e7e5",
    "g1f3 d7d5 d2d4",
    "c2c4 g8f6 b1c3 d7d5 d2d4",
];
