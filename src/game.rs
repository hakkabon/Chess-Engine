use crate::eval::{evaluate, insufficient_material};
use crate::movegen::generate_legal;
use crate::pieces::*;
use crate::position::*;
use crate::search::{find_best_move, move_san, MATE};
use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub struct GameCore {
    pub pos: Position,
    history: Vec<Position>,
    sans: Vec<String>,
    tags: HashMap<String, String>,
}

/// Game status flags derived from the current position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub turn: Color,
    pub in_check: bool,
    pub checkmate: bool,
    pub stalemate: bool,
    pub draw: bool,
    pub draw_reason: Option<String>,
}

impl GameCore {
    pub fn new() -> GameCore {
        GameCore {
            pos: Position::starting(),
            history: Vec::new(),
            sans: Vec::new(),
            tags: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.pos = Position::starting();
        self.history.clear();
        self.sans.clear();
        self.tags.clear();
    }

    pub fn load_fen(&mut self, fen: &str) -> Result<(), String> {
        let pos = Position::from_fen(fen)?;
        self.pos = pos;
        self.history.clear();
        self.sans.clear();
        self.tags.clear();
        Ok(())
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        generate_legal(&self.pos)
    }

    fn same_layout(a: &Position, b: &Position) -> bool {
        a.pieces == b.pieces && a.side == b.side && a.castling == b.castling && a.ep == b.ep
    }

    fn repetition_count(&self) -> usize {
        let mut count = 1; // current position
        for h in &self.history {
            if GameCore::same_layout(h, &self.pos) {
                count += 1;
            }
        }
        count
    }

    pub fn status(&self) -> Status {
        let legal = generate_legal(&self.pos);
        let in_check = self.pos.in_check();
        let turn = self.pos.side;
        let checkmate = legal.is_empty() && in_check;
        let stalemate = legal.is_empty() && !in_check;

        let fifty = self.pos.halfmove >= 100;
        let rep = self.repetition_count() >= 3;
        let material = insufficient_material(&self.pos);
        let draw = !checkmate && !stalemate && (fifty || rep || material);

        let mut draw_reason = None;
        if draw {
            draw_reason = if rep {
                Some("threefold repetition".to_string())
            } else if fifty {
                Some("fifty-move rule".to_string())
            } else {
                Some("insufficient material".to_string())
            };
        }

        Status {
            turn,
            in_check,
            checkmate,
            stalemate,
            draw,
            draw_reason,
        }
    }

    pub fn is_over(&self) -> bool {
        let s = self.status();
        s.checkmate || s.stalemate || s.draw
    }

    /// Apply a move specified by from/to squares and optional promotion.
    /// Returns the SAN of the move.
    pub fn apply(&mut self, from: u8, to: u8, promotion: Option<PieceKind>) -> Result<String, String> {
        if self.is_over() {
            return Err("game is over".to_string());
        }
        let legal = generate_legal(&self.pos);
        let m = legal
            .iter()
            .copied()
            .find(|m| m.from == from && m.to == to && m.promotion == promotion)
            .ok_or_else(|| "illegal move".to_string())?;

        let san_str = move_san(&self.pos, &m);
        self.history.push(self.pos.clone());
        self.pos.make_move(m);
        self.sans.push(san_str.clone());
        Ok(san_str)
    }

    pub fn undo(&mut self) -> Result<(), String> {
        match self.history.pop() {
            Some(prev) => {
                self.pos = prev;
                self.sans.pop();
                Ok(())
            }
            None => Err("nothing to undo".to_string()),
        }
    }

    pub fn best_move(&mut self, depth: u8) -> Result<(Move, i32), String> {
        self.best_move_timed(depth, 1500)
    }

    /// Like `best_move`, but with an explicit time budget (milliseconds) for
    /// iterative deepening. The search still respects `depth` as a hard cap.
    pub fn best_move_timed(&mut self, depth: u8, time_ms: u64) -> Result<(Move, i32), String> {
        if self.is_over() {
            return Err("game is over".to_string());
        }
        let depth = depth.clamp(1, 64);
        let (m, score) =
            find_best_move(&self.pos, depth, time_ms).ok_or_else(|| "no legal moves".to_string())?;
        let san_str = move_san(&self.pos, &m);
        self.history.push(self.pos.clone());
        self.pos.make_move(m);
        self.sans.push(san_str);
        Ok((m, score))
    }

    /// Play a full game with **both** sides driven by the engine's search
    /// (Computer vs Computer). Each side searches to `depth` and the resulting
    /// move is applied, exactly as a normal game would proceed.
    ///
    /// Returns the list of SAN moves played. The game stops early when it is
    /// over or once `max_moves` plies have been played.
    pub fn self_play(&mut self, depth: u8, max_moves: u16) -> Vec<String> {
        let mut played = Vec::new();
        let mut steps: u16 = 0;
        while !self.is_over() && steps < max_moves {
            match self.best_move(depth) {
                Ok(_) => {
                    if let Some(san) = self.last_san() {
                        played.push(san.to_string());
                    }
                    steps += 1;
                }
                Err(_) => break,
            }
        }
        played
    }

    pub fn evaluate(&self) -> i32 {
        evaluate(&self.pos)
    }

    pub fn sans(&self) -> &[String] {
        &self.sans
    }

    pub fn last_san(&self) -> Option<&str> {
        self.sans.last().map(|s| s.as_str())
    }

    // --- PGN (Portable Game Notation) -----------------------------------

    /// Load a game from PGN. Resets the board (to the standard start position,
    /// or to the position in a `FEN` tag when present), replays every SAN move
    /// from the movetext, and stores the tag pairs as metadata.
    pub fn load_pgn(&mut self, pgn: &str) -> Result<(), String> {
        let parsed = crate::pgn::parse_pgn(pgn)?;
        self.pos = Position::starting();
        self.history.clear();
        self.sans.clear();
        self.tags.clear();
        if let Some(fen) = parsed.tags.get("FEN") {
            self.pos = Position::from_fen(fen)?;
        }
        self.tags = parsed.tags;
        for san in &parsed.moves {
            let m = crate::san::parse_san(&self.pos, san)
                .map_err(|e| format!("failed to play move '{}': {}", san, e))?;
            self.apply(m.from, m.to, m.promotion)?;
        }
        Ok(())
    }

    /// Serialize the game to PGN. Missing standard tags default to `"?"`.
    ///
    /// The `Result` tag is the actual game result when the position is terminal;
    /// otherwise a previously recorded result (e.g. from a loaded PGN where the
    /// game ended by resignation) is preserved, falling back to `"*"`.
    pub fn to_pgn(&self) -> String {
        let mut tags = self.tags.clone();
        tags.entry("Event".to_string()).or_insert("?".to_string());
        tags.entry("Site".to_string()).or_insert("?".to_string());
        tags.entry("Date".to_string()).or_insert("????.??.??".to_string());
        tags.entry("Round".to_string()).or_insert("?".to_string());
        tags.entry("White".to_string()).or_insert("?".to_string());
        tags.entry("Black".to_string()).or_insert("?".to_string());

        let result = if self.is_over() {
            self.result_string()
        } else {
            match self.tags.get("Result") {
                Some(r) if r != "*" => r.clone(),
                _ => "*".to_string(),
            }
        };
        tags.insert("Result".to_string(), result.clone());
        crate::pgn::render_pgn(&tags, &self.sans, &result)
    }

    /// Set a PGN tag (e.g. player names, event, date).
    pub fn set_tag(&mut self, key: String, value: String) {
        self.tags.insert(key, value);
    }

    /// Get a PGN tag value, if present.
    pub fn get_tag(&self, key: &str) -> Option<String> {
        self.tags.get(key).cloned()
    }

    fn result_string(&self) -> String {
        let s = self.status();
        if s.checkmate {
            match s.turn {
                Color::White => "0-1".to_string(),
                Color::Black => "1-0".to_string(),
            }
        } else if s.draw {
            "1/2-1/2".to_string()
        } else {
            "*".to_string()
        }
    }
}

impl Default for GameCore {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience: score-to-mate distance helper exposed for the UI display.
pub fn is_mate_score(score: i32) -> bool {
    score >= MATE - 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_move_produces_valid_san() {
        let mut g = GameCore::new();
        g.load_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
            .unwrap();
        let legal = g.legal_moves();
        assert!(!legal.is_empty());

        let (m, _score) = g.best_move_timed(3, 2000).unwrap();
        // The returned move must be among the legal moves.
        assert!(
            legal.contains(&m),
            "engine returned an illegal move: {:?}",
            m
        );
        // SAN must have been recorded without panicking on a missing piece.
        let san = g.last_san().expect("SAN should be recorded");
        assert!(!san.is_empty(), "SAN should not be empty");
    }

    #[test]
    fn multiple_best_moves_stay_legal() {
        let mut g = GameCore::new();
        for _ in 0..6 {
            let legal = g.legal_moves();
            let (m, _) = g.best_move_timed(3, 2000).unwrap();
            assert!(
                legal.contains(&m),
                "engine returned an illegal move: {:?}",
                m
            );
        }
    }

    #[test]
    fn self_play_runs_and_records_moves() {
        let mut g = GameCore::new();
        let moves = g.self_play(2, 300);
        assert!(!moves.is_empty(), "self-play should produce moves");
        assert_eq!(moves.len(), g.sans().len(), "every move must be recorded");
        // The game must be over or capped by max_moves.
        assert!(g.is_over() || g.sans().len() >= 300);
    }

    #[test]
    fn parse_san_basics() {
        use crate::san::parse_san;
        let mut g = GameCore::new();
        g.load_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
            .unwrap();
        let pos = &g.pos;
        assert!(parse_san(pos, "e4").is_ok());
        assert!(parse_san(pos, "Nf3").is_ok());

        // Promotion.
        let mut promo = GameCore::new();
        promo.load_fen("8/P7/8/8/8/8/8/k6K w - - 0 1").unwrap();
        assert!(parse_san(&promo.pos, "a8=Q").is_ok());
    }

    #[test]
    fn parse_san_castling() {
        use crate::san::parse_san;
        // Position where White may castle both ways (f1/g1 and b1/c1/d1 empty,
        // king on e1, rooks on a1/h1).
        let mut g = GameCore::new();
        g.load_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")
            .unwrap();
        let pos = &g.pos;
        assert!(parse_san(pos, "O-O").is_ok());
        assert!(parse_san(pos, "O-O-O").is_ok());
    }

    #[test]
    fn pgn_round_trip() {
        let mut g = GameCore::new();
        // Play a short, well-known opening line.
        for san in ["e4", "e5", "Nf3", "Nc6", "Bb5"] {
            let legal = g.legal_moves();
            let m = legal
                .iter()
                .find(|mm| {
                    let s = crate::search::move_san(&g.pos, mm);
                    s == san
                })
                .unwrap_or_else(|| panic!("move {san} not found"));
            g.apply(m.from, m.to, m.promotion).unwrap();
        }
        g.set_tag("White".to_string(), "Alice".to_string());
        g.set_tag("Black".to_string(), "Bob".to_string());
        g.set_tag("Event".to_string(), "Test Match".to_string());

        let pgn = g.to_pgn();
        assert!(pgn.contains("[Event \"Test Match\"]"));
        assert!(pgn.contains("1. e4 e5 2. Nf3 Nc6 3. Bb5"));

        // Reload from the serialized PGN and verify the final position matches.
        let mut g2 = GameCore::new();
        g2.load_pgn(&pgn).unwrap();
        assert_eq!(g2.sans().len(), 5);
        assert_eq!(g2.pos.to_fen(), g.pos.to_fen());
        assert_eq!(g2.get_tag("White"), Some("Alice".to_string()));
    }

    #[test]
    fn pgn_loads_real_game() {
        let pgn = "[Event \"Friendly\"]
[Site \"Home\"]
[Date \"2024.01.01\"]
[Round \"1\"]
[White \"Carlsen\"]
[Black \"Nakamura\"]
[Result \"1-0\"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 1-0
";
        let mut g = GameCore::new();
        g.load_pgn(pgn).unwrap();
        // 5 full moves = 10 plies.
        assert_eq!(g.sans().len(), 10);
        assert_eq!(g.get_tag("White"), Some("Carlsen".to_string()));
        // The result is an annotation on a still-ongoing position, so the
        // engine preserves the recorded tag rather than asserting checkmate.
        assert_eq!(g.get_tag("Result"), Some("1-0".to_string()));

        // Round-trip: re-serializing keeps the result annotation.
        let pgn2 = g.to_pgn();
        assert!(pgn2.contains("[Result \"1-0\"]"));
        let mut g2 = GameCore::new();
        g2.load_pgn(&pgn2).unwrap();
        assert_eq!(g2.sans().len(), 10);
        assert_eq!(g2.get_tag("Result"), Some("1-0".to_string()));
    }

    #[test]
    fn pgn_strips_comments_and_variations() {
        let pgn = "1. e4 {good move} (1. d4 d5 2. c4) e5 2. Nf3 Nc6 *";
        let parsed = crate::pgn::parse_pgn(pgn).unwrap();
        assert_eq!(parsed.moves, vec!["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(parsed.tags.len(), 0);
    }
}
