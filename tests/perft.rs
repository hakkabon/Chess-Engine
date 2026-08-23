use chess_engine::movegen::generate_legal;
use chess_engine::position::Position;

fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = generate_legal(pos);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0u64;
    for m in moves {
        let mut child = pos.clone();
        child.make_move(m);
        nodes += perft(&child, depth - 1);
    }
    nodes
}

fn check(name: &str, fen: &str, depth: u32, expected: u64) {
    let pos = Position::from_fen(fen).expect("valid fen");
    let got = perft(&pos, depth);
    assert_eq!(got, expected, "perft {name} depth {depth}");
    println!("ok  perft {name} d{depth} = {got}");
}

#[test]
fn perft_startpos() {
    check("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 1, 20);
    check("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 2, 400);
    check("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 3, 8902);
    check("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 4, 197_281);
}

#[test]
fn perft_kiwipete() {
    let kiwi = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
    check("kiwipete", kiwi, 1, 48);
    check("kiwipete", kiwi, 2, 2039);
    check("kiwipete", kiwi, 3, 97_862);
}

#[test]
fn perft_position_3() {
    let p3 = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
    check("pos3", p3, 1, 14);
    check("pos3", p3, 2, 191);
    check("pos3", p3, 3, 2812);
    check("pos3", p3, 4, 43238);
}

#[test]
fn perft_position_4() {
    let p4 = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
    check("pos4", p4, 1, 6);
    check("pos4", p4, 2, 264);
    check("pos4", p4, 3, 9467);
}

#[test]
fn perft_position_5() {
    let p5 = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";
    check("pos5", p5, 1, 44);
    check("pos5", p5, 2, 1486);
    check("pos5", p5, 3, 62379);
}

#[test]
fn perft_position_6() {
    let p6 = "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10";
    check("pos6", p6, 1, 46);
    check("pos6", p6, 2, 2079);
    check("pos6", p6, 3, 89890);
}
