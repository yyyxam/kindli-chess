# `src/models` — data types

Struct and enum definitions. Split into two groups: **wire types** that mirror Lichess
JSON, and **UI/domain types** that the rest of the app manipulates. Behaviour mostly lives
elsewhere (`src/api/`, `src/app/`, `src/ui/`) — with three deliberate exceptions that
carry their own logic *and their own tests*: `bitboard.rs`, `puzzle.rs`, and the custom
`Deserialize` impl in `board_api.rs`.

> The user's brief called this directory `modules`; the actual path is `src/models`.

## File map

| File | Group | Contents |
|---|---|---|
| `bitboard.rs` | domain | 12-bitboard position, FEN parsing, UCI move application. **Has tests.** |
| `puzzle.rs` | wire + domain | `Puzzle` wire type, `PuzzleSession` state machine, `/puzzle/next` parameter enums. **Has tests.** |
| `board_api.rs` | wire + domain | `BoardAPI<S>`, typestate markers, `Turn`, and every stream/DTO shape |
| `oauth.rs` | wire + domain | `OAuth2Client`, `AuthConfig`, `TokenInfo`, `LichessUser`, `HttpMethod` |
| `ui.rs` | UI | `Display`, the `Screen` trait, `Transition`, and every screen struct + its layout constants |
| `chess.rs` | domain | `ChessApp`, `ChessBackend`; plus `ChessUI`, an explicitly-marked tombstone |
| `game.rs` | wire | `Game` / `Player` — parsed as part of `PuzzleResponse`, otherwise unused |
| `board_local.rs` | domain | `BoardLocal` — two bools; the offline stub |
| `app.rs` | domain | `App { display, screen_stack }` |

## `bitboard.rs`

One `u64` per (colour, piece) pair, stored flat as `[[u64; 6]; 2]` plus `side_to_move`.

**Square indexing is LERF** (Little-Endian Rank-File): `square = rank * 8 + file`,
`a1 = 0`, `h8 = 63`, bit for square `s` is `1u64 << s`. Rank 0 is White's first rank.
This mapping is assumed by the board widget's highlight masks and by
`last_move_mask` / `diff_mask` — don't change it casually.

- `from_fen` — honours only the position field and side-to-move; castling rights, en
  passant and clocks are ignored because the widget only needs the layout. Accepts the
  literal `"startpos"`, which Lichess sends for standard-opening games.
- `apply_uci_move` — handles castling (detected by the king moving two files),
  en passant (a pawn moving diagonally onto an empty square), and promotion suffixes.
  Flips `side_to_move`.
- `apply_uci_moves` — logs and **skips** malformed moves rather than failing. Safe because
  every streamed snapshot rebuilds the position from scratch.
- `piece_at(sq)` — linear scan over 12 boards. Called 64× per diff; fine at this scale.

Five unit tests cover the starting layout, a pawn capture, kingside castling, en passant,
and promotion.

## `puzzle.rs`

Two layers.

**Wire** — `Puzzle` / `PuzzleResponse`. Note the two `Option` fields: `fen` and `last_move`
are present on `/puzzle/daily` and `/puzzle/{id}` but **absent from `/puzzle/next`**, which
is exactly why `get_next_puzzle` re-fetches by id. `PuzzleResponse.game` is parsed for
completeness and never read.

**Runtime** — `PuzzleSession`, the whole puzzle rulebook, network-free and fully tested:

- `solution` alternates: even indices are the solver's moves, odd are the opponent's
  scripted replies.
- `solver_white` is derived from the FEN's side-to-move.
- `try_move(uci)` matches on **from/to only** (first 4 bytes) and then applies the
  *canonical* solution move. That is what makes an under-promotion work: the board widget
  always submits a `q` suffix, but the stored solution's `n` is what actually lands.
- A wrong move leaves `position` untouched — the board simply stays put and the solver
  can retry.
- A correct non-final move sets status `Correct` and leaves the opponent's reply
  **pending**; `accepts_input()` returns false until `apply_opponent_reply()` runs.
  `PuzzleScreen` delays that by ~1 s so the solver sees their own move land first.
- `next_expected_move()` backs the hint button.

`PuzzleStatus` is `Solving | Correct | Wrong | Solved`.

**Parameter enums** — `Difficulty`, `PuzzlePhase`, `PuzzleColor`, bundled into
`PuzzleParams`. Each exposes `label()` (settings UI), a wire accessor (`as_param()` /
`as_angle()`), and `next()` for the tap-to-cycle buttons. `Any` / `Random` map to `None`,
meaning the query parameter is omitted entirely.

## `board_api.rs`

`BoardAPI<S>` with `S ∈ { Idle, InGame }`. `Idle` is a unit marker; `InGame` carries
`game_id`, both players, `player0_white`, `turn`, `initial_board` and the current `board`.

Keeping `initial_board` around is what allows the "rebuild, don't patch" strategy: every
`GameState` event ships the full move list, so the current position is
`initial_board.clone()` + replay.

`Turn` is `Playing | Waiting | Over { winner: Option<String> }` — `None` covers draws,
stalemate and aborts.

The rest of the file is Lichess DTOs for both NDJSON streams:

- `StreamEvent` (account stream) — `GameStart | GameFinish | Challenge | ChallengeDeclined`
- `GameStateStreamEvent` (game stream) — `GameFull | GameState | GameOver | ChatLine | OpponentGone`

Both are `#[serde(tag = "type")]` internally-tagged.

### `PlayedBy` — the hand-written `Deserialize`

Lichess sends two incompatible opponent shapes:

```
GET /api/account/playing  → { id, username, rating, ai }      (ai = level or null)
game-state stream         → { id, name, title, rating }  or  { aiLevel }
```

`#[serde(untagged)]` with User-first failed on `now_playing`, because `name` is missing
there and every opponent fell through to the all-`Option` AI variant. The fix is a
`PlayedByRaw` proxy with `alias`es that accepts both schemas and discriminates on whether
an AI-level field is set. Worth knowing before "simplifying" it back to `untagged`.

## `oauth.rs`

`OAuth2Client` (config + PKCE state behind `Arc<Mutex<_>>`), `AuthConfig`, `AuthState`,
`AuthCallbackQuery`, `TokenInfo`, `LichessUser`, and the `HttpMethod` enum.

`AuthConfig::default()` is where the **OAuth scopes** are set:

```
challenge:read, challenge:write, bot:play, board:play, puzzle:read
```

`puzzle:read` was added later. Tokens minted before that get a 403 on `/puzzle/next` and
`/puzzle/activity` — handled by falling back to anonymous requests rather than forcing a
re-auth. `redirect_port` is 8080. `client_id` is a fresh UUID per process.

The `arc_mutable_option` serde module serialises the `Arc<Mutex<Option<AuthState>>>` field
as unit — the client is never actually persisted, only `TokenInfo` is.

## `ui.rs`

Two things: the `Display`/`Screen`/`Transition` core, and a struct per screen.

`Display` is the single long-lived X11 resource — connection, `Renderer`, both mpsc ends,
and the triple-tap state. There is exactly one; screens borrow it, they never own X11 state.

```rust
pub trait Screen {
    fn render(&mut self, display: &mut Display) -> Result<(), Box<dyn Error>>;
    fn handle_event(&mut self, event: AppEvent, display: &mut Display)
        -> Result<Transition, Box<dyn Error>>;
    fn on_reveal(&mut self) {}   // default no-op
}
```

Screen structs and their layout: `HomeScreen`, `ChessGameScreen`,
`OngoingChessGamesScreen`, `ChessAuthScreen`, `SettingsScreen`, `UpdateScreen`
(+ `UpdateState`), `PuzzleScreen`, `PuzzleSettingsScreen`, `GameActionsScreen`.

**All layout is absolute pixels against a 1072 × 1448 canvas**, computed in each
`::new()`. Common idiom: `const CENTER_X: i16 = 1072 / 2;` and stack buttons from there.
The board occupies `(0, 0, 1072, 1072)` — exactly 8 × 134 — and both sidebars occupy
`(0, 1072, 1072, 376)`.

Recurring field patterns worth recognising:

- `*_started` / `load_started` / `check_started` / `stream_started` — first-render guards.
  Async work is kicked from `render`, exactly once.
- `games: Option<_> + error: Option<_> + loading: bool` — the tri-state async fetch idiom
  on `OngoingChessGamesScreen`; all three empty means "not yet started".
- `load_home_logo()` pre-scales `logo.png` once at construction so the home screen doesn't
  re-downscale a ~1800 px image on every redraw.

## `chess.rs`, `game.rs`, `board_local.rs`, `app.rs`

Small. `ChessUI` in `chess.rs` is explicitly labelled a tombstone superseded by the
`Screen` architecture and is `#[allow(dead_code)]`. `Game`/`Player` in `game.rs` exist
only because `PuzzleResponse` embeds a `game` field.
