use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::bitboard::{move_to_uci, square_from_name};
use crate::movegen::generate_legal;
use crate::pieces::PieceKind;
use crate::position::*;
use crate::search::{search_info, search_info_time_control, MATE};

/// Parse a UCI move string like `e2e4` or `e7e8q` into a legal `Move`.
pub fn parse_uci(pos: &Position, uci: &str) -> Result<Move, String> {
    if uci.len() < 4 {
        return Err(format!("UCI move too short: {uci:?}"));
    }
    let from = square_from_name(&uci[0..2])? as u8;
    let to = square_from_name(&uci[2..4])? as u8;
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
        .ok_or_else(|| format!("illegal UCI move: {uci:?}"))
}

/// Run the Universal Chess Interface loop, reading commands from stdin and
/// writing engine output to stdout.
pub fn run_uci() {
    use std::io::BufRead;

    let stdin = std::io::stdin();
    let mut pos = Position::starting();
    let mut use_book = true;
    let stop = Arc::new(AtomicBool::new(false));

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();

        match cmd {
            "uci" => {
                println!("id name ChessEngine");
                println!("id author Rust + uniffi");
                println!("option name UCI_OpeningBook type check default true");
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => pos = Position::starting(),
            "setoption" => {
                if let Some(i) = rest.iter().position(|&w| w == "name") {
                    if rest.get(i + 1).copied() == Some("UCI_OpeningBook") {
                        if let Some(j) = rest.iter().position(|&w| w == "value") {
                            if let Some(v) = rest.get(j + 1) {
                                use_book = *v == "true";
                            }
                        }
                    }
                }
            }
            "position" => {
                if rest.first().map_or(false, |w| *w == "startpos") {
                    pos = Position::starting();
                } else if rest.first().map_or(false, |w| *w == "fen") {
                    let mut fen = String::new();
                    let mut it = rest.iter().skip(1);
                    while let Some(tok) = it.next() {
                        if *tok == "moves" {
                            break;
                        }
                        if !fen.is_empty() {
                            fen.push(' ');
                        }
                        fen.push_str(tok);
                    }
                    match Position::from_fen(&fen) {
                        Ok(p) => pos = p,
                        Err(e) => println!("info string invalid fen: {e}"),
                    }
                }
                if let Some(i) = rest.iter().position(|&w| w == "moves") {
                    for uci in &rest[i + 1..] {
                        match parse_uci(&pos, uci) {
                            Ok(m) => pos.make_move(m),
                            Err(e) => {
                                println!("info string {e}");
                                break;
                            }
                        }
                    }
                }
            }
            "go" => {
                let (max_depth, time_ms, use_stop, wtime, btime, winc, binc, moves_to_go) = parse_go(&rest);
                stop.store(false, Ordering::Relaxed);

                let pos_clone = pos.clone();
                let stop_clone = Arc::clone(&stop);
                let use_book_local = use_book;
                let start = Instant::now();
                thread::spawn(move || {
                    if use_book_local {
                        if let Some(mv) = crate::opening::lookup(&pos_clone) {
                            if generate_legal(&pos_clone).contains(&mv) {
                                println!("bestmove {}", move_to_uci(&mv));
                                return;
                            }
                        }
                    }
                    let info = if wtime > 0 || btime > 0 {
                        // Time control mode
                        search_info_time_control(&pos_clone, max_depth, wtime, btime, winc, binc, moves_to_go, if use_stop { Some(stop_clone.as_ref()) } else { None })
                    } else if use_stop {
                        search_info(&pos_clone, max_depth, time_ms, Some(stop_clone.as_ref()))
                    } else {
                        search_info(&pos_clone, max_depth, time_ms, None)
                    };
                    if let Some(mv) = info.best_move {
                        let pv: Vec<String> = info.pv.iter().map(move_to_uci).collect();
                        let elapsed = start.elapsed().as_millis();
                        println!(
                            "info depth {} score {} nodes {} time {} pv {}",
                            info.depth,
                            uci_score(info.score),
                            info.nodes,
                            elapsed,
                            pv.join(" ")
                        );
                        println!("bestmove {}", move_to_uci(&mv));
                    } else {
                        println!("bestmove 0000");
                    }
                });
            }
            "stop" => stop.store(true, Ordering::Relaxed),
            "quit" | "exit" => break,
            _ => println!("info string unknown command: {cmd}"),
        }
    }
}

/// Interpret a `go` command's arguments into search parameters.
fn parse_go(rest: &[&str]) -> (u8, u64, bool, u64, u64, u64, u64, u32) {
    let mut depth: Option<u32> = None;
    let mut movetime: Option<u64> = None;
    let mut infinite = false;
    let mut wtime: u64 = 0;
    let mut btime: u64 = 0;
    let mut winc: u64 = 0;
    let mut binc: u64 = 0;
    let mut moves_to_go: u32 = 0;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "depth" => {
                if i + 1 < rest.len() {
                    depth = rest[i + 1].parse().ok();
                }
                i += 2;
            }
            "movetime" => {
                if i + 1 < rest.len() {
                    movetime = rest[i + 1].parse().ok();
                }
                i += 2;
            }
            "infinite" => {
                infinite = true;
                i += 1;
            }
            "wtime" => {
                if i + 1 < rest.len() {
                    wtime = rest[i + 1].parse().unwrap_or(0);
                }
                i += 2;
            }
            "btime" => {
                if i + 1 < rest.len() {
                    btime = rest[i + 1].parse().unwrap_or(0);
                }
                i += 2;
            }
            "winc" => {
                if i + 1 < rest.len() {
                    winc = rest[i + 1].parse().unwrap_or(0);
                }
                i += 2;
            }
            "binc" => {
                if i + 1 < rest.len() {
                    binc = rest[i + 1].parse().unwrap_or(0);
                }
                i += 2;
            }
            "movestogo" => {
                if i + 1 < rest.len() {
                    moves_to_go = rest[i + 1].parse().unwrap_or(0);
                }
                i += 2;
            }
            "nodes" => i += 2,
            _ => i += 1,
        }
    }

    if let Some(d) = depth {
        (d as u8, 3_600_000, false, wtime, btime, winc, binc, moves_to_go)
    } else if let Some(ms) = movetime {
        (64, ms, false, wtime, btime, winc, binc, moves_to_go)
    } else if infinite {
        (64, 3_600_000, true, wtime, btime, winc, binc, moves_to_go)
    } else {
        // Fixed search as a sensible default (e.g. `go` with no arguments).
        (4, 1000, false, wtime, btime, winc, binc, moves_to_go)
    }
}

/// Format a search score as a UCI `score cp X` / `score mate Y` token.
fn uci_score(score: i32) -> String {
    let bound = MATE - 1000;
    if score >= bound {
        let mate_in = (MATE - score + 1) / 2;
        format!("mate {mate_in}")
    } else if score <= -bound {
        let mate_in = (MATE + score + 1) / 2;
        format!("mate -{mate_in}")
    } else {
        format!("cp {score}")
    }
}
