use crate::bitboard::*;
use crate::pieces::*;
use crate::tables::*;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

pub const CASTLE_WK: u8 = 1;
pub const CASTLE_WQ: u8 = 2;
pub const CASTLE_BK: u8 = 4;
pub const CASTLE_BQ: u8 = 8;

/// Move flag bits.
pub const FLAG_CAPTURE: u8 = 1;
pub const FLAG_DOUBLE_PUSH: u8 = 1 << 1;
pub const FLAG_EP_CAPTURE: u8 = 1 << 2;
pub const FLAG_KING_CASTLE: u8 = 1 << 3;
pub const FLAG_QUEEN_CASTLE: u8 = 1 << 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promotion: Option<PieceKind>,
    pub flags: u8,
}

impl Move {
    #[inline]
    pub fn is_capture(&self) -> bool {
        (self.flags & FLAG_CAPTURE) != 0
    }
    #[inline]
    pub fn is_ep(&self) -> bool {
        (self.flags & FLAG_EP_CAPTURE) != 0
    }
    #[inline]
    pub fn is_king_castle(&self) -> bool {
        (self.flags & FLAG_KING_CASTLE) != 0
    }
    #[inline]
    pub fn is_queen_castle(&self) -> bool {
        (self.flags & FLAG_QUEEN_CASTLE) != 0
    }
    #[inline]
    pub fn is_promotion(&self) -> bool {
        self.promotion.is_some()
    }
}

/// Castling-rights mask for each square: moving a piece from/to a square
/// clears the corresponding rights. Bits: 1=WK 2=WQ 4=BK 8=BQ.
pub const CASTLE_MASK: [u8; 64] = castle_mask_init();

const fn castle_mask_init() -> [u8; 64] {
    let mut m = [CASTLE_WK | CASTLE_WQ | CASTLE_BK | CASTLE_BQ; 64];
    m[sq_index(0, 0)] &= !CASTLE_WQ; // a1
    m[sq_index(4, 0)] &= !(CASTLE_WK | CASTLE_WQ); // e1
    m[sq_index(7, 0)] &= !CASTLE_WK; // h1
    m[sq_index(0, 7)] &= !CASTLE_BQ; // a8
    m[sq_index(4, 7)] &= !(CASTLE_BK | CASTLE_BQ); // e8
    m[sq_index(7, 7)] &= !CASTLE_BK; // h8
    m
}

#[derive(Clone, PartialEq)]
pub struct Position {
    pub board: [Option<Piece>; 64],
    pub pieces: [u64; 12],
    pub occ: [u64; 3], // 0=white, 1=black, 2=both
    pub side: Color,
    pub castling: u8,
    pub ep: Option<u8>,
    pub halfmove: u16, // for fifty-move rule
    pub fullmove: u16,
    /// Zobrist hash (see `crate::zobrist`). Maintained incrementally by
    /// `make_move` and set from scratch in `from_fen`.
    pub hash: u64,
}

impl std::fmt::Debug for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Position({})", self.to_fen())
    }
}

impl Position {
    pub fn starting() -> Position {
        Position::from_fen(START_FEN).expect("valid start FEN")
    }

    pub fn from_fen(fen: &str) -> Result<Position, String> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(format!("FEN needs at least 4 fields, got {}", parts.len()));
        }
        let mut pos = Position {
            board: [None; 64],
            pieces: [0; 12],
            occ: [0; 3],
            side: Color::White,
            castling: 0,
            ep: None,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
        };

        let rows: Vec<&str> = parts[0].split('/').collect();
        if rows.len() != 8 {
            return Err(format!("FEN placement must have 8 ranks, got {}", rows.len()));
        }
        for (i, row) in rows.iter().enumerate() {
            let rank = 7 - i; // FEN lists rank 8 first
            let mut file = 0usize;
            for ch in row.chars() {
                if let Some(n) = ch.to_digit(10) {
                    file += n as usize;
                } else {
                    let (kind, color) = PieceKind::from_fen_char(ch)
                        .ok_or_else(|| format!("invalid FEN piece char '{ch}'"))?;
                    if file > 7 {
                        return Err("FEN rank overflow".to_string());
                    }
                    pos.put_piece(sq_index(file, rank), Piece::new(color, kind));
                    file += 1;
                }
            }
        }

        pos.side = match parts[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(format!("invalid active color '{other}'")),
        };

        for ch in parts[2].chars() {
            match ch {
                'K' => pos.castling |= CASTLE_WK,
                'Q' => pos.castling |= CASTLE_WQ,
                'k' => pos.castling |= CASTLE_BK,
                'q' => pos.castling |= CASTLE_BQ,
                '-' => {}
                _ => return Err(format!("invalid castling char '{ch}'")),
            }
        }

        if parts[3] != "-" {
            if parts[3].len() != 2 {
                return Err(format!("invalid en passant square '{}'", parts[3]));
            }
            let file = (parts[3].as_bytes()[0] - b'a') as i32;
            let rank = (parts[3].as_bytes()[1] - b'1') as i32;
            if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                return Err(format!("invalid en passant square '{}'", parts[3]));
            }
            pos.ep = Some(sq_index(file as usize, rank as usize) as u8);
        }

        if let Some(h) = parts.get(4) {
            pos.halfmove = h.parse().map_err(|_| format!("invalid halfmove '{h}'"))?;
        }
        if let Some(fm) = parts.get(5) {
            pos.fullmove = fm.parse().map_err(|_| format!("invalid fullmove '{fm}'"))?;
        }

        // Validate exactly one king per side (soft requirement).
        let wk = pos.king_square(Color::White);
        let bk = pos.king_square(Color::Black);
        if wk.is_none() || bk.is_none() {
            return Err("FEN must contain exactly one king per side".to_string());
        }

        pos.hash = crate::zobrist::compute_hash(&pos);
        Ok(pos)
    }

    pub fn to_fen(&self) -> String {
        let mut placement = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let sq = sq_index(file, rank);
                match self.board[sq] {
                    Some(p) => {
                        if empty > 0 {
                            placement.push_str(&empty.to_string());
                            empty = 0;
                        }
                        placement.push(p.fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                placement.push_str(&empty.to_string());
            }
            if rank > 0 {
                placement.push('/');
            }
        }

        let active = self.side.fen_char();
        let mut castle = String::new();
        if (self.castling & CASTLE_WK) != 0 {
            castle.push('K');
        }
        if (self.castling & CASTLE_WQ) != 0 {
            castle.push('Q');
        }
        if (self.castling & CASTLE_BK) != 0 {
            castle.push('k');
        }
        if (self.castling & CASTLE_BQ) != 0 {
            castle.push('q');
        }
        if castle.is_empty() {
            castle.push('-');
        }

        let ep = match self.ep {
            Some(sq) => square_name(sq as usize),
            None => "-".to_string(),
        };

        format!(
            "{} {} {} {} {} {}",
            placement, active, castle, ep, self.halfmove, self.fullmove
        )
    }

    // ----- piece management -----

    fn put_piece(&mut self, sq: usize, p: Piece) {
        self.board[sq] = Some(p);
        let idx = p.index();
        self.pieces[idx] |= bit(sq);
        self.occ[p.color.index()] |= bit(sq);
        self.occ[2] |= bit(sq);
    }

    fn remove_piece(&mut self, sq: usize) {
        if let Some(p) = self.board[sq].take() {
            let idx = p.index();
            self.pieces[idx] &= !bit(sq);
            self.occ[p.color.index()] &= !bit(sq);
            self.occ[2] &= !bit(sq);
        }
    }

    #[inline]
    pub fn piece_at(&self, sq: usize) -> Option<Piece> {
        self.board[sq]
    }

    #[inline]
    pub fn pieces_bb(&self, color: Color, kind: PieceKind) -> u64 {
        self.pieces[piece_index(color, kind)]
    }

    #[inline]
    pub fn color_bb(&self, color: Color) -> u64 {
        self.occ[color.index()]
    }

    #[inline]
    pub fn occupied(&self) -> u64 {
        self.occ[2]
    }

    pub fn king_square(&self, color: Color) -> Option<usize> {
        let bb = self.pieces_bb(color, PieceKind::King);
        if bb == 0 {
            None
        } else {
            Some(bb.trailing_zeros() as usize)
        }
    }

    // ----- attack detection -----

    /// Is square `sq` attacked by any piece of color `by`?
    pub fn attacked_by(&self, sq: usize, by: Color) -> bool {
        let them = by.index();
        let occ = self.occupied();

        // Pawns: a pawn of `by` attacks `sq` iff it sits on a square a pawn of
        // the opposite color on `sq` would attack.
        let pawn_attackers = pawn_attacks(by.opposite(), sq);
        if pawn_attackers & self.pieces[them * 6 + PieceKind::Pawn.index()] != 0 {
            return true;
        }

        let knight_attackers = knight_attacks(sq);
        if knight_attackers & self.pieces[them * 6 + PieceKind::Knight.index()] != 0 {
            return true;
        }

        let king_attackers = king_attacks(sq);
        if king_attackers & self.pieces[them * 6 + PieceKind::King.index()] != 0 {
            return true;
        }

        let bishops = self.pieces[them * 6 + PieceKind::Bishop.index()];
        let queens = self.pieces[them * 6 + PieceKind::Queen.index()];
        let rooks = self.pieces[them * 6 + PieceKind::Rook.index()];
        if bishop_attacks(sq, occ) & (bishops | queens) != 0 {
            return true;
        }
        if rook_attacks(sq, occ) & (rooks | queens) != 0 {
            return true;
        }

        false
    }

    pub fn in_check(&self) -> bool {
        match self.king_square(self.side) {
            Some(k) => self.attacked_by(k, self.side.opposite()),
            None => false,
        }
    }

    // ----- make move (assumes input is at least pseudo-legal) -----

    pub fn make_move(&mut self, m: Move) {
        let us = self.side;
        let them = us.opposite();
        let from = m.from as usize;
        let to = m.to as usize;

        let moving = self.board[from].expect("no piece at move source");

        // Snapshot pre-move state for incremental Zobrist update.
        let old_castling = self.castling;
        let old_ep = self.ep;

        // Captured piece (for en passant the captured square differs).
        let captured_sq = if m.is_ep() {
            if us == Color::White {
                to - 8
            } else {
                to + 8
            }
        } else {
            to
        };
        let captured = self.board[captured_sq].filter(|_| m.is_capture());

        // Remove moving piece and captured piece.
        self.remove_piece(from);
        if m.is_capture() {
            self.remove_piece(captured_sq);
        }

        // Place the (possibly promoted) piece at destination.
        let placed = Piece::new(us, m.promotion.unwrap_or(moving.kind));
        self.put_piece(to, placed);

        // Castling: move the rook too.
        if moving.kind == PieceKind::King {
            if m.is_king_castle() {
                let (rk_from, rk_to) = if us == Color::White {
                    (sq_index(7, 0), sq_index(5, 0))
                } else {
                    (sq_index(7, 7), sq_index(5, 7))
                };
                let rook = self.board[rk_from].expect("castling rook missing");
                self.remove_piece(rk_from);
                self.put_piece(rk_to, rook);
            } else if m.is_queen_castle() {
                let (rk_from, rk_to) = if us == Color::White {
                    (sq_index(0, 0), sq_index(3, 0))
                } else {
                    (sq_index(0, 7), sq_index(3, 7))
                };
                let rook = self.board[rk_from].expect("castling rook missing");
                self.remove_piece(rk_from);
                self.put_piece(rk_to, rook);
            }
        }

        // Update castling rights by cleared squares.
        self.castling &= CASTLE_MASK[from] & CASTLE_MASK[to];

        // En passant target.
        if moving.kind == PieceKind::Pawn && (to as isize - from as isize).abs() == 16 {
            self.ep = Some(((from + to) / 2) as u8);
        } else {
            self.ep = None;
        }

        // Clocks.
        if moving.kind == PieceKind::Pawn || captured.is_some() {
            self.halfmove = 0;
        } else {
            self.halfmove += 1;
        }
        if us == Color::Black {
            self.fullmove += 1;
        }

        self.side = them;

        // Maintain the Zobrist hash incrementally.
        let mut h = self.hash;
        h ^= crate::zobrist::piece_key(us, moving.kind, from);
        if let Some(cap) = captured {
            h ^= crate::zobrist::piece_key(cap.color, cap.kind, captured_sq);
        }
        let placed_kind = m.promotion.unwrap_or(moving.kind);
        h ^= crate::zobrist::piece_key(us, placed_kind, to);
        if moving.kind == PieceKind::King {
            if m.is_king_castle() {
                let (rk_from, rk_to) = if us == Color::White {
                    (sq_index(7, 0), sq_index(5, 0))
                } else {
                    (sq_index(7, 7), sq_index(5, 7))
                };
                h ^= crate::zobrist::piece_key(us, PieceKind::Rook, rk_from)
                    ^ crate::zobrist::piece_key(us, PieceKind::Rook, rk_to);
            } else if m.is_queen_castle() {
                let (rk_from, rk_to) = if us == Color::White {
                    (sq_index(0, 0), sq_index(3, 0))
                } else {
                    (sq_index(0, 7), sq_index(3, 7))
                };
                h ^= crate::zobrist::piece_key(us, PieceKind::Rook, rk_from)
                    ^ crate::zobrist::piece_key(us, PieceKind::Rook, rk_to);
            }
        }
        h ^= crate::zobrist::ZOBRIST_SIDE;
        h ^= crate::zobrist::ZOBRIST_CASTLING[old_castling as usize]
            ^ crate::zobrist::ZOBRIST_CASTLING[self.castling as usize];
        if let Some(e) = old_ep {
            h ^= crate::zobrist::ZOBRIST_EP[e as usize];
        }
        if let Some(e) = self.ep {
            h ^= crate::zobrist::ZOBRIST_EP[e as usize];
        }
        self.hash = h;
    }
}
