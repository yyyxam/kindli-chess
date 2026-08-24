# `src/api` — network clients

Everything that talks HTTP. Two upstreams: **Lichess** (auth, board play, puzzles)
and **GitHub Releases** (the in-app updater). No X11, no widgets — these modules
return data or send `AppEvent`s, they never draw.

All Lichess URLs are built from `env!("LICHESS_API_BASE")` (`https://lichess.org/api`,
baked in by `build.rs`). GitHub URLs are hard-coded in `github.rs`.

## File map

| File | Status | Purpose |
|---|---|---|
| `oauth.rs` | live | PKCE OAuth flow, local callback server, token persistence, `authenticated_request` helper |
| `board.rs` | live | `BoardAPI<S>` impls — ongoing games, move/resign/abort, the two NDJSON streams |
| `api.rs` | live | Lichess puzzle endpoints (`/puzzle/daily`, `/puzzle/{id}`, `/puzzle/next`, `/puzzle/activity`) |
| `github.rs` | live | `GET /repos/{owner}/{repo}/releases/latest`, version comparison, `UpdateInfo` |
| `update.rs` | live | Streams the release asset, verifies SHA256, stages it at `<exe>.new` |
| `account.rs` | **dead** | Empty file, not declared in any `mod` block |
| `mod.rs` | **dead** | Shadowed — `main.rs` declares `pub mod api { pub mod api; … }` *inline*, so `mod.rs` is never compiled. It is also stale (missing `github`/`update`). |

> The authoritative module list for this directory is the inline `pub mod api { … }`
> block at the top of `src/main.rs`, **not** `src/api/mod.rs`. Adding a file here
> means editing `main.rs`.

## `oauth.rs` — the authentication flow

PKCE against `https://lichess.org/oauth`, with the redirect landing on a
short-lived HTTP server running **on the Kindle itself**.

1. `OAuth2Client::new` picks the host's LAN IP via `local_ip_address::local_ip()`
   and builds `http://<lan-ip>:8080/callback` as the redirect URI.
2. `start_auth_flow` generates the PKCE challenge + CSRF state and returns the
   authorization URL.
3. `generate_qr_code` renders that URL as a QR bitmap; the caller (`ChessAuthScreen`)
   draws it and the user scans it with a phone.
4. `run_auth_server` binds axum on `0.0.0.0:8080`, serves `/callback`, exchanges the
   code for a token, ships the `TokenInfo` over a `oneshot`, and shuts down.
5. `authenticate` writes the token to `env!("AUTH_TOKEN")`
   (`<ROOT_DIR>/secrets/token.json`) and fetches the `LichessUser` via `/api/account`.

**The phone and the Kindle must be on the same network** — the redirect goes to the
Kindle's LAN IP. Over USB-only networking there is no route from the phone, so the QR
flow needs Wi-Fi on the device.

Other entry points:

- `load_token()` — reads the cached token from disk. `Ok(None)` for missing *or empty*
  file (empty is treated as "re-authenticate", not as a parse error).
- `get_user_info(&token)` — the liveness check. The home screen's silent bootstrap
  uses a successful call here as proof the cached token still works.
- `authenticated_request(url, token, method)` — thin bearer-auth wrapper used by
  `board.rs`. `HttpMethod::STREAM` is identical to `GET`; it exists only to document intent.
- `logout()` — deletes the token file.

Known rough edges: token expiry is not handled (`TODO (#6)`); the non-GET arms of
`authenticated_request` `.unwrap()` on transport errors instead of propagating.

## `board.rs` — the typestate board client

`BoardAPI<S>` is generic over a typestate marker (`Idle` / `InGame`, defined in
`models/board_api.rs`). What you can call is decided at compile time:

- `impl<S> BoardAPI<S>` — `get_ongoing_games(n)`, `stream_event()` (account-level event
  stream). Valid in both states.
- `impl BoardAPI<Idle>` — `new(token, user)`, `attach_game(game_id, my_turn) -> BoardAPI<InGame>`.
- `impl BoardAPI<InGame>` — `move_piece`, `resign_game`, `abort_game`, `stream_game_event`.

`stream_game_event` is the heart of live play. It opens
`/board/game/stream/{id}` as NDJSON and, for each event:

- **`GameFull`** — parses `initial_fen` into a `Bitboards`, replays the whole move list,
  derives `player0_white` by matching the white player's id against `self.user.id`, and
  sends `AppEvent::GameFullReceived`.
- **`GameState`** — every such event carries the **full move list from move 1**, so the
  position is rebuilt from `initial_board` rather than tracked incrementally. Sends
  `AppEvent::TurnChanged`.
- **`GameOver`** — rebuilt the same way from the over-event's own move list (the mating
  move can land in either `GameState` or `GameOver`).

Two things to keep in mind:

- The task owns a **clone** of the API. Its `self.state` mutations are local bookkeeping
  used to compute whose turn it is; they do **not** propagate back to the screen. Every
  state change the UI needs travels as an `AppEvent`.
- Empty keep-alive lines from Lichess surface as `"EOF while parsing"` parse errors and
  are deliberately swallowed.

Helpers at the bottom of the file:

- `is_terminal_status` — anything other than `"started"` / `"created"` ends the game.
- `resolve_winner` — maps Lichess's winning *colour* back to a display name.
- `last_move_mask` — replays all moves but the last, then diffs the two positions. This
  catches castling rook squares and en-passant captures without parsing UCI shapes.

## `api.rs` — puzzles

- `get_daily_puzzle()` / `get_puzzle_by_id(id)` — anonymous; both include `fen` and `lastMove`.
- `get_next_puzzle(token, params)` — `/puzzle/next` **omits `fen`/`lastMove`**, so this
  takes only the id from the response and re-fetches the complete record by id.
  Authentication is optional and *degrading*: a token needs the `puzzle:read` scope, and
  tokens minted before that scope was added return 403 — so a failed authenticated
  request silently retries anonymously.
- `get_home_puzzle(token, params)` — what the home screen's Puzzle button uses. Returns
  the daily puzzle, unless `daily_already_played` finds it in the account's last 50
  `/puzzle/activity` records, in which case a fresh `/puzzle/next` is returned instead.
  Any failure in that check degrades to the daily puzzle.

Errors are mapped to `String` before crossing an `await` in a few places on purpose: a
non-`Send` `Box<dyn Error>` held across an await would make the future non-`Send`, and
these futures get `tokio::spawn`ed.

## `github.rs` + `update.rs` — the updater

Constants in `github.rs` are the **contract with `.github/workflows/release.yml`**:

```rust
pub const OWNER: &str = "mxyyz";
pub const REPO:  &str = "kindle-chess";
pub const ASSET_NAME: &str = "kindle-chess-armv7-musl";
pub const SHA_NAME:   &str = "kindle-chess-armv7-musl.sha256";
```

`check_for_update()` returns a deliberately three-way result:

- `Ok(Some(UpdateInfo))` — a strictly newer tag **and** both assets are present.
- `Ok(None)` — up to date. The UI shows "You're up to date".
- `Err(_)` — unparseable tag, or a release whose assets aren't uploaded yet (common while
  `release.yml` is still cross-building). Surfacing this as an error gets the user
  "try again in a minute" rather than a misleading "up to date".

`apply_update()` streams the asset while hashing it, compares against the `.sha256`
sidecar, and on mismatch deletes the partial file leaving the running binary untouched.
On success it leaves the verified file at **`<current_exe>.new`** and stops there — it
does *not* rename over the running binary, because `/mnt/us` is VFAT and replacing a busy
executable in place is unreliable there. `kindle_KUAL/hellokindle/chess_app.sh` does the
`mv` at next launch, while no instance is running.

See [`docs/ci-cd.md`](../../../docs/ci-cd.md) for the full release → download → install path.

> **Note for old binaries:** `OWNER` was `yyyxam` until 2026-08-24 (the GitHub account was
> renamed to `mxyyz`). Anything already deployed still queries the old owner and relies on
> GitHub's 301 redirect for renamed accounts — which reqwest follows, so those binaries can
> still update themselves onto a build carrying the corrected constant.
