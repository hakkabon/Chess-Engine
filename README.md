# Chess Engine (Rust)

A chess engine written in Rust, exposed to other languages through a
[uniffi](https://github.com/mozilla/uniffi-rs) foreign-function interface (FFI).
The FFI module is named **`ChessEngineKit`** and is consumed today by the Swift
package in `../Swift-Chess` (a multiplatform macOS + iOS/iPadOS SwiftUI app).

This crate is both a normal Rust library (so it can be unit- and perft-tested
with `cargo test`) and a C-compatible library (`cdylib` + `staticlib`) that
uniffi turns into Swift bindings and an XCFramework.

---

## Features

- **Bitboard move generation** for all piece types, with en passant and castling.
- **Legal move filtering** (king-safety check after each pseudo-legal move).
- **Search**: negamax with alpha-beta pruning, iterative deepening, a
  transposition table, quiescence search, and standard move ordering.
- **Evaluation**: material + piece-square tables (Simplified/Michniewski style),
  mirrored correctly for both sides, with a separate king table for the endgame.
- **Draw detection**: checkmate, stalemate, insufficient material, threefold
  repetition, and the fifty-move rule.
- **Zobrist hashing** for fast transposition-table keys and repetition checks.
- **Opening book** — a small, curated set of common opening lines so the engine
  plays sensible first moves without spending search time.
- **PGN import/export** through the FFI (`load_pgn`, `save_pgn`, tag get/set,
  and the played move list).
- **UCI protocol** — a standalone `chess-engine` binary speaks the Universal
  Chess Interface for engine-vs-engine play, CLI testing, and GUI integration.
- **Computer vs Computer** — `GameCore::self_play` drives *both* sides with the
  search (see `game.rs`). The Swift app exposes this as a "Computer vs Computer"
  mode that plays the whole game automatically.
- **Perft-verified** move generation (see `tests/`).

The FFI does **not** expose SAN/UCI *input* parsing — moves are made by
**square index**, and SAN is produced as *output* on each move
(see `MoveData.san`). The **UCI binary**, however, parses and emits UCI move
strings (e.g. `e2e4`).

---

## Coordinate convention

A square is a single `u8` index in `0..64`:

```
index = rank * 8 + file
```

- `file` is `0..7` for files **a..h**.
- `rank` is `0..7` for ranks **1..8** (rank 0 = White's first rank).
- So `a1 = 0`, `h1 = 7`, `a8 = 56`, `h8 = 63`.

This index is used everywhere in the public API: `MoveData.from_square`,
`MoveData.to_square`, and `CellState.square`.

---

## Public FFI API

The FFI surface is defined with uniffi `#[uniffi::export]` proc-macros in
`src/lib.rs`. Types are automatically bridged to Swift.

### `ChessGame` (object)

| Method | Signature | Notes |
| --- | --- | --- |
| `new` | `() -> Arc<ChessGame>` | Starts from the standard initial position. |
| `reset` | `()` | Back to the initial position, clears history. |
| `load_fen` | `(fen: String) -> Result<(), ChessError>` | Standard FEN (placement, side, castling, en passant, half/full move). |
| `get_state` | `() -> GameState` | Snapshot of the current position. |
| `get_cells` | `() -> Vec<CellState>` | Only occupied squares are returned. |
| `legal_moves` | `() -> Vec<MoveData>` | All legal moves with pre-computed SAN. |
| `make_move` | `(from_square, to_square, promotion?) -> Result<GameState, ChessError>` | Applies a move and returns the new state. |
| `ai_move` | `(depth: u8) -> Result<MoveData, ChessError>` | Searches to `depth` with a 1500 ms soft time budget, **applies** the move, and returns it. |
| `ai_move_timed` | `(max_depth: u8, time_ms: u64) -> Result<MoveData, ChessError>` | Same, but you choose the time budget. |
| `undo_move` | `() -> Result<GameState, ChessError>` | Reverts the last move. |
| `evaluate` | `() -> i32` | Static evaluation from **White's** perspective (centipawns). |
| `load_pgn` | `(pgn: String) -> Result<(), ChessError>` | Loads a game from PGN (replays the movetext; honors a `FEN` tag). |
| `save_pgn` | `() -> String` | Serializes the current game to PGN. |
| `set_tag` | `(key: String, value: String)` | Sets a PGN metadata tag (e.g. player names, event, date). |
| `get_tag` | `(key: String) -> String?` | Gets a PGN metadata tag value, if present. |
| `moves` | `() -> [String]` | The SAN move list played so far. |

> `ai_move` / `ai_move_timed` are *stateful*: they play the move on the game,
> not just compute it. Call `get_state()` afterwards to read the result.

### Value types

```rust
enum Side { White, Black }

enum PieceKind { Pawn, Knight, Bishop, Rook, Queen, King }

struct CellState { square: u8, side: Option<Side>, kind: Option<PieceKind> }

struct MoveData {
    from_square: u8,
    to_square: u8,
    promotion: Option<PieceKind>,
    is_capture: bool,
    san: String,
}

struct GameState {
    fen: String,
    turn: Side,
    is_in_check: bool,
    is_checkmate: bool,
    is_stalemate: bool,
    is_draw: bool,
    evaluation_cp: i32,   // White's perspective, centipawns
}

enum ChessError {
    IllegalMove,
    InvalidFen { reason: String },
    GameOver,
    NothingToUndo,
}
```

---

## UCI engine (CLI)

In addition to the FFI library, the crate builds a standalone **UCI** binary
named `chess-engine` (from `src/main.rs`, which calls `uci::run_uci`). It
implements the Universal Chess Interface so the engine can play against other
UCI engines, be driven from a terminal, or be embedded in a chess GUI.

```bash
# Build and run, then type UCI commands:
cargo run --bin chess-engine

# A self-contained example session:
#   uci
#   position startpos
#   go movetime 1000
#   quit
```

Supported commands: `uci`, `isready`, `ucinewgame`, `setoption`
(including `UCI_OpeningBook`), `position` (`startpos`/`fen` plus `moves`),
`go` (`depth` / `movetime` / `infinite` / time-control fields), `stop`, `quit`.

The engine prints standard `info depth … score cp|mate … pv …` lines followed
by a `bestmove` reply. When the opening book is enabled (the default), it replies
with a book move from `src/opening.rs` during the first few plies and falls back
to the search afterwards. Disable it with
`setoption name UCI_OpeningBook value false`.

---

## Building the library

```bash
# Debug build (for `cargo test`)
cargo build

# Release build (produces cdylib + staticlib in target/release)
cargo build --release
```

The crate is configured with `crate-type = ["rlib", "staticlib", "cdylib"]`, so a
single build yields everything the FFI pipeline needs.

### Cross-compiling the static library for Apple targets

The Swift XCFramework links the **static** library (`libengine_ffi.a`). Build
one slice per architecture you intend to ship:

```bash
# macOS (universal = x86_64 + arm64)
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
# iOS device + simulator (arm64)
cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim
```

The artifacts land in `target/<triple>/release/libengine_ffi.a`.

---

## Generating the Swift bindings

Use the bundled `uniffi-bindgen` binary. It requires the `uniffi/cli` feature
and the compiled `cdylib`:

```bash
cargo run --bin uniffi-bindgen --features="uniffi/cli" -- \
  generate --library target/release/libchess_engine.dylib \
  --language swift --out-dir <some-dir>
```

This emits three files:

- `ChessEngineKit.swift` — the Swift wrapper (drop into `swift/Sources/ChessEngineKit/`).
- `ChessEngineKitFFI.h` — the C header.
- `ChessEngineKitFFI.modulemap` — rename to `module.modulemap` when bundling.

> Only regenerate when you change the Rust FFI surface (the `#[uniffi::export]`
> items). Pure internal changes (search/eval/movegen) do **not** require
> regenerating bindings — just rebuild the static library.

---

## Assembling the XCFramework

The Swift package consumes a binary target at
`swift/Frameworks/ChessEngineKitFFI.xcframework`. To (re)build it from the
static libraries and headers:

1. Put the generated `ChessEngineKitFFI.h` and a `module.modulemap` (renamed
   from `ChessEngineKitFFI.modulemap`) into a `Headers/` folder.
2. Merge the two macOS slices into one universal archive, then create the
   multi-platform framework (macOS + iOS device + iOS simulator):

```bash
lipo -create \
  target/x86_64-apple-darwin/release/libengine_ffi.a \
  target/aarch64-apple-darwin/release/libengine_ffi.a \
  -output /tmp/libmac.a
cp target/aarch64-apple-ios/release/libengine_ffi.a        /tmp/libios.a
cp target/aarch64-apple-ios-sim/release/libengine_ffi.a   /tmp/libsim.a

xcodebuild -create-xcframework \
  -library /tmp/libmac.a      -headers Headers \
  -library /tmp/libios.a      -headers Headers \
  -library /tmp/libsim.a      -headers Headers \
  -output Swift-Chess/Frameworks/ChessEngineKitFFI.xcframework
```

### Quick rebuild (no API change)

If you only changed Rust internals, rebuild the static lib(s) and overwrite the
existing `.a` inside the framework — no need to recreate it or regenerate the
Swift file:

```bash
cargo build --release --target x86_64-apple-darwin
cp target/x86_64-apple-darwin/release/libengine_ffi.a \
   Swift-Chess/Frameworks/ChessEngineKitFFI.xcframework/macos-arm64_x86_64/libengine_ffi.a
```

Then, in the Swift package, force a relink:

```bash
cd Swift-Chess && swift package reset && swift build
```

---

## Testing

```bash
cargo test                 # unit tests + integration tests
cargo test --lib           # library unit tests only
```

Coverage includes:

- **Perft** (move-generation correctness) against well-known node counts:
  startpos, Kiwipete, and several tactical positions (`tests/perft*`).
- **Search** tests: mate-in-one detection, preferring the capture of a free
  queen, returning a legal move within a time budget, and Zobrist consistency.
- **Engine/integration** tests: the AI returns legal moves and produces valid
  SAN strings (regression tests for the bugs fixed in `src/game.rs`).

Run a single test by name, e.g. `cargo test perft_kiwipete`.

---

## Project layout

```
src/
  lib.rs          # FFI surface (uniffi exports) + type bridging
  position.rs     # board representation, FEN, make_move, legality helpers
  movegen.rs      # pseudo-legal generation + legal filtering
  san.rs          # SAN string generation for moves
  bitboard.rs     # bitboard primitives, square helpers
  pieces.rs       # Color, PieceKind, Piece
  eval.rs         # material + piece-square tables
  search.rs       # negamax, TT, move ordering, quiescence, iterative deepening
  zobrist.rs      # hash keys
  game.rs         # GameCore: history, status, best_move_timed, PGN
  pgn.rs          # PGN parsing/serialization
  opening.rs      # opening-book lookup (common lines)
  uci.rs          # UCI protocol loop + the runnable engine binary
  perft.rs        # perft / perft-divide (used by tests)
  tables.rs       # attack tables
  bin/uniffi-bindgen.rs  # wrapper exposing the uniffi CLI binary
  tests/            # perft and search integration tests
  Swift-Chess/      # SwiftPM package + macOS app + iOS/iPadOS Xcode app
                    # (see Swift-Chess/README.md)
```

---

## Known limitations

- The shipped XCFramework contains **macOS** (x86_64 + arm64) and **iOS**
  (arm64 device + arm64 simulator) slices. There is no x86_64 iOS simulator
  slice, so the iOS simulator only runs on Apple Silicon Macs.
- No UCI/SAN *input* parsing in the **FFI**; moves are made by square index.
  (UCI input/output *is* supported by the separate `chess-engine` binary — see
  the "UCI engine (CLI)" section above.)
- This is a learning/reference-strength engine, not a tournament engine.
