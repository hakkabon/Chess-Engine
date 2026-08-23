uniffi::setup_scaffolding!("ChessEngineKit");

use std::sync::Mutex;

use chess_engine::game::{GameCore, Status};
use chess_engine::pieces::{Color, PieceKind as CoreKind};
use chess_engine::position::Move as CoreMove;

// ---------------------------------------------------------------------------
// FFI enums
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Side {
    White,
    Black,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl From<Color> for Side {
    fn from(c: Color) -> Self {
        match c {
            Color::White => Side::White,
            Color::Black => Side::Black,
        }
    }
}

impl From<Side> for Color {
    fn from(s: Side) -> Self {
        match s {
            Side::White => Color::White,
            Side::Black => Color::Black,
        }
    }
}

impl From<CoreKind> for PieceKind {
    fn from(k: CoreKind) -> Self {
        match k {
            CoreKind::Pawn => PieceKind::Pawn,
            CoreKind::Knight => PieceKind::Knight,
            CoreKind::Bishop => PieceKind::Bishop,
            CoreKind::Rook => PieceKind::Rook,
            CoreKind::Queen => PieceKind::Queen,
            CoreKind::King => PieceKind::King,
        }
    }
}

impl From<PieceKind> for CoreKind {
    fn from(k: PieceKind) -> Self {
        match k {
            PieceKind::Pawn => CoreKind::Pawn,
            PieceKind::Knight => CoreKind::Knight,
            PieceKind::Bishop => CoreKind::Bishop,
            PieceKind::Rook => CoreKind::Rook,
            PieceKind::Queen => CoreKind::Queen,
            PieceKind::King => CoreKind::King,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI records
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, uniffi::Record)]
pub struct CellState {
    pub square: u8,
    pub side: Option<Side>,
    pub kind: Option<PieceKind>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MoveData {
    pub from_square: u8,
    pub to_square: u8,
    pub promotion: Option<PieceKind>,
    pub is_capture: bool,
    pub san: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct GameState {
    pub fen: String,
    pub turn: Side,
    pub is_in_check: bool,
    pub is_checkmate: bool,
    pub is_stalemate: bool,
    pub is_draw: bool,
    pub evaluation_cp: i32,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ChessError {
    #[error("Illegal move")]
    IllegalMove,
    #[error("Invalid FEN: {reason}")]
    InvalidFen { reason: String },
    #[error("Invalid PGN: {reason}")]
    InvalidPgn { reason: String },
    #[error("Game is over")]
    GameOver,
    #[error("No move to undo")]
    NothingToUndo,
}

// ---------------------------------------------------------------------------
// Object
// ---------------------------------------------------------------------------

fn game_state_from(core: &GameCore) -> GameState {
    let s: Status = core.status();
    GameState {
        fen: core.pos.to_fen(),
        turn: s.turn.into(),
        is_in_check: s.in_check,
        is_checkmate: s.checkmate,
        is_stalemate: s.stalemate,
        is_draw: s.draw,
        evaluation_cp: core.evaluate(),
    }
}

fn move_data_from(m: CoreMove, san: &str) -> MoveData {
    MoveData {
        from_square: m.from,
        to_square: m.to,
        promotion: m.promotion.map(Into::into),
        is_capture: m.is_capture(),
        san: san.to_string(),
    }
}

#[derive(uniffi::Object)]
pub struct ChessGame {
    core: Mutex<GameCore>,
}

#[uniffi::export]
impl ChessGame {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(ChessGame {
            core: Mutex::new(GameCore::new()),
        })
    }

    pub fn reset(&self) {
        self.core.lock().unwrap_or_else(|e| e.into_inner()).reset();
    }

    pub fn load_fen(&self, fen: String) -> Result<(), ChessError> {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .load_fen(&fen)
            .map_err(|reason| ChessError::InvalidFen { reason })
    }

    pub fn get_state(&self) -> GameState {
        let core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        game_state_from(&core)
    }

    pub fn get_cells(&self) -> Vec<CellState> {
        let core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let mut cells = Vec::new();
        for sq in 0..64 {
            if let Some(p) = core.pos.piece_at(sq) {
                cells.push(CellState {
                    square: sq as u8,
                    side: Some(p.color.into()),
                    kind: Some(p.kind.into()),
                });
            }
        }
        cells
    }

    pub fn legal_moves(&self) -> Vec<MoveData> {
        let core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        core.legal_moves()
            .iter()
            .map(|m| {
                let san = chess_engine::search::move_san(&core.pos, m);
                move_data_from(*m, &san)
            })
            .collect()
    }

    pub fn make_move(
        &self,
        from_square: u8,
        to_square: u8,
        promotion: Option<PieceKind>,
    ) -> Result<GameState, ChessError> {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let promo = promotion.map(Into::into);
        core.apply(from_square, to_square, promo)
            .map_err(|_| ChessError::IllegalMove)?;
        Ok(game_state_from(&core))
    }

    pub fn ai_move(&self, depth: u8) -> Result<MoveData, ChessError> {
        self.ai_move_timed(depth, 1500)
    }

    /// Like `ai_move`, but searches within a soft time budget (milliseconds)
    /// using iterative deepening instead of a fixed depth.
    pub fn ai_move_timed(&self, max_depth: u8, time_ms: u64) -> Result<MoveData, ChessError> {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        if core.is_over() {
            return Err(ChessError::GameOver);
        }
        let (m, _score) = core
            .best_move_timed(max_depth, time_ms)
            .map_err(|_| ChessError::GameOver)?;
        let san = core.last_san().unwrap_or("").to_string();
        Ok(move_data_from(m, &san))
    }

    pub fn undo_move(&self) -> Result<GameState, ChessError> {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        core.undo().map_err(|_| ChessError::NothingToUndo)?;
        Ok(game_state_from(&core))
    }

    pub fn evaluate(&self) -> i32 {
        self.core.lock().unwrap_or_else(|e| e.into_inner()).evaluate()
    }

    // --- PGN (Portable Game Notation) -----------------------------------

    /// Load a game from a PGN string. Resets the board (or starts from a `FEN`
    /// tag when present) and replays the movetext.
    pub fn load_pgn(&self, pgn: String) -> Result<(), ChessError> {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .load_pgn(&pgn)
            .map_err(|reason| ChessError::InvalidPgn { reason })
    }

    /// Serialize the current game to a PGN string.
    pub fn save_pgn(&self) -> String {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .to_pgn()
    }

    /// Set a PGN metadata tag (e.g. player names, event, date).
    pub fn set_tag(&self, key: String, value: String) {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_tag(key, value);
    }

    /// Get a PGN metadata tag value, if present.
    pub fn get_tag(&self, key: String) -> Option<String> {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_tag(&key)
    }

    /// The SAN move list played so far (useful for reflecting a loaded game).
    pub fn moves(&self) -> Vec<String> {
        self.core
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sans()
            .to_vec()
    }
}
