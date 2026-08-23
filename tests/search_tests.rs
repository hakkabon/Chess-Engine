use chess_engine::movegen::generate_legal;
use chess_engine::position::Position;
use chess_engine::search::find_best_move;
use chess_engine::zobrist::compute_hash;

#[test]
fn zobrist_incremental_matches_full() {
    // Apply many random legal move sequences and verify the incrementally
    // maintained `pos.hash` always equals a from-scratch computation.
    let mut pos = Position::starting();
    assert_eq!(pos.hash, compute_hash(&pos));

    let mut seed: u64 = 0xDEAD_BEEF;
    let mut lcg = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 32) as usize
    };

    for _ in 0..400 {
        let moves = generate_legal(&pos);
        if moves.is_empty() {
            break;
        }
        let m = moves[lcg() % moves.len()];
        pos.make_move(m);
        assert_eq!(pos.hash, compute_hash(&pos), "hash drift after a move");
    }
}

#[test]
fn zobrist_distinguishes_positions() {
    let a = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    let b = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
    let c = Position::from_fen("r1bqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    assert_ne!(a.hash, b.hash);
    assert_ne!(a.hash, c.hash);
    assert_ne!(b.hash, c.hash);
}

#[test]
fn finds_mate_in_one() {
    // White to move; Ra8 is checkmate.
    let pos = Position::from_fen("6k1/5ppp/8/8/8/8/8/R6K w - - 0 1").unwrap();
    let (m, score) = find_best_move(&pos, 3, 2000).expect("has a move");
    // Mate-in-one should score at/above the mate threshold and be Ra1-a8.
    assert!(score >= 99_000, "expected a forced mate, got score {score}");
    assert_eq!(m.from, 0); // a1
    assert_eq!(m.to, 56); // a8
}

#[test]
fn search_returns_legal_move_in_time() {
    let pos = Position::starting();
    // Tight budget must still yield a legal move without hanging.
    let (m, _score) = find_best_move(&pos, 6, 300).expect("has a move");
    let legal = generate_legal(&pos);
    assert!(legal.contains(&m), "search returned an illegal move");
}

#[test]
fn search_prefers_capture_of_free_queen() {
    // White queen on d1 can capture a hanging black queen on d8 (Qxd8+).
    let pos = Position::from_fen("3qk3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
    let (m, _score) = find_best_move(&pos, 2, 1000).expect("has a move");
    assert_eq!(m.from, 3); // d1
    assert_eq!(m.to, 59); // d8
}
