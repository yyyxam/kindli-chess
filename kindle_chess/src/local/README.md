# `src/local` — offline board

**Status: stub.** Roughly 25 lines of placeholder. Nothing here plays chess.

| File | Contents |
|---|---|
| `mod.rs` | `pub mod board;` |
| `board.rs` | `impl BoardLocal` — `new()` and an unimplemented `get_ongoing_games()` |

The data type is `models::board_local::BoardLocal`, which is two booleans:

```rust
pub struct BoardLocal {
    pub player0_white: bool,  // player 0 had the first turn
    pub player0_turn:  bool,  // it is currently player 0's turn
}
```

`BoardLocal::new(game_id)` prints *"Would start local game … If it were implemented"*,
hard-codes both flags to `true`, and returns. `get_ongoing_games()` logs
`"Not implemented"`. The commented-out `game_id` field hints at the intended
local-savegame direction.

## How it connects to the rest of the app

It is the third arm of the runtime backend enum in `models/chess.rs`:

```rust
enum ChessBackend {
    Offline(BoardLocal),
    OnlineIdle(BoardAPI<Idle>),
    OnlineInGame(BoardAPI<InGame>),
}
```

reachable only through `ChessApp::new_offline()`. **Nothing currently calls
`new_offline()`** — the home screen's bootstrap always builds an online app, and every
other path derives from it. So the `Offline` arm is live code that is never constructed at
runtime; it exists to keep the offline case in the type system.

Its one visible effect is in `ChessApp::token()`, which returns `None` for `Offline` —
which is exactly the path `PuzzleScreen` takes when it makes anonymous puzzle requests.

## What "implementing offline" would mean

Most of the machinery already exists elsewhere and would be reused rather than rewritten:

- **Position and move application** — `models/bitboard.rs` already does FEN parsing, UCI
  application, castling, en passant and promotion.
- **Rendering and input** — `BoardWidget` is backend-agnostic. It takes a `Bitboards` via
  `set_position` and emits `MoveMade`; it has no idea whether a server is involved.
- **A local-move validation loop** — `PuzzleScreen` is the closest working template: it
  validates every move locally against `PuzzleSession` with no network at all.

What is genuinely missing is **legal-move generation**. `Bitboards` can *apply* a move but
cannot tell you whether it is legal — there is no check detection, no pin handling, no
mate/stalemate detection. Online play sidesteps this entirely by letting Lichess reject
illegal moves (surfaced as `AppEvent::MoveRejected`). Offline play would have to bring its
own rules engine, plus persistence for saved games.
