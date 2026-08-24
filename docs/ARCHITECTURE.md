# kindle-chess — architecture & current state

*Written 2026-08-24, against branch `puzzle-implementation` @ `004190f`.*

A KUAL launcher app for jailbroken Kindles that plays Lichess. Rust, cross-compiled to
`armv7-unknown-linux-musleabi`, drawing directly via x11rb to the Kindle's native Xorg on
`:0`. The same binary runs on a dev machine against an emulated framebuffer (`run-dev.sh`),
pixel-identically, because every layout is absolute against a hard-coded 1072 × 1448 canvas.

## Repository layout

```
kindle-chess/
├── kindle_chess/            ← the crate. All cargo commands run from here.
│   ├── build.rs             ← bakes .env.* + git SHA + build timestamp into the binary
│   ├── .env.debug/.release  ← ROOT_DIR per profile
│   └── src/
│       ├── main.rs          ← module tree, log4rs setup, App::new().run()
│       ├── version.rs       ← VERSION / GIT_SHA / BUILD_TIMESTAMP
│       ├── api/             ← Lichess + GitHub HTTP clients      → src/api/README.md
│       ├── app/             ← event loop, ChessApp lifecycle     → src/app/README.md
│       ├── models/          ← all data types                     → src/models/README.md
│       ├── ui/              ← renderer, screens, widgets         → src/ui/README.md
│       ├── local/           ← offline board (stub)               → src/local/README.md
│       └── examples/        ← uncompiled snippets                → src/examples/README.md
├── kindle_KUAL/             ← what gets copied to /mnt/us on the device
│   ├── extensions/hellokindle/  ← menu.json + config.xml (KUAL wiring)
│   └── hellokindle/         ← chess_app.sh, bin/, assets/, log/, secrets/
├── .github/workflows/       ← rust.yml, release-please.yml, release.yml → docs/ci-cd.md
├── run-dev.sh               ← local Xvfb/Xephyr harness
└── flash*.sh                ← build + deploy to device        → docs/kindle-usb-ssh.md
```

Only one binary target: `kindle-hello` (`src/main.rs`).

## Build-time environment

`build.rs` loads `.env.debug` (debug profile) or `.env.release` (release profile) and emits
`cargo:rustc-env=…`. The Rust code reads these with **`env!()`, not `std::env::var()`** —
they are compile-time constants, so changing `.env.*` requires a rebuild, and the values
are visible in the binary with `strings`.

| Constant | Source | Debug value | Release value |
|---|---|---|---|
| `ROOT_DIR` | `.env.*` | local checkout path | `/mnt/us/hellokindle/` |
| `LOG_FILE_DIR` | derived | `<ROOT_DIR>log/` | ″ |
| `ASSETS_DIR` | derived | `<ROOT_DIR>assets/` | ″ |
| `AUTH_TOKEN` | derived | `<ROOT_DIR>secrets/token.json` | ″ |
| `LICHESS_API_BASE` | `build.rs` | `https://lichess.org/api` | ″ |
| `GIT_SHA` | `git rev-parse --short HEAD` | | |
| `BUILD_TIMESTAMP` | `date -u +%Y-%m-%dT%H:%M:%SZ` | | |

`build.rs` shells out to `git` and `date` rather than pulling in a build-dep crate. It
re-runs on changes to `../.git/HEAD`, `../.git/index`, `.env.debug` and `.env.release` —
the last two matter, because once a build script declares *any* `rerun-if-changed`, cargo
watches only what is declared. Without them an edited `.env.*` is invisible to cargo and
the old `ROOT_DIR` stays baked into the binary.

## Runtime architecture

```
                    ┌──────────────────────────────────────────┐
   X11 server ─────▶│ listener thread (blocks on wait_for_event)│
       :0           └────────────────┬─────────────────────────┘
                                     │ AppEvent
   tokio tasks ──── event_tx.send ──▶│ (std::sync::mpsc)
   (auth, streams,                   ▼
    fetches, updates)     ┌──────────────────────┐
                          │  App::run() main loop │  single-threaded
                          └──────────┬───────────┘
                                     │ routes to top of stack
                                     ▼
                          screen_stack: Vec<Box<dyn Screen>>
                                     │ Transition
                                     ▼
                          Stay / Redraw / Push / Pop / Quit
```

Three concurrency layers, one of which owns all the state:

1. **The main loop** — synchronous, single-threaded, owns `Display` and the screen stack.
2. **One X11 listener thread** — translates X11 events into `AppEvent`s.
3. **Tokio tasks** — `handle_event` is sync but runs inside `#[tokio::main]`, so handlers
   `tokio::spawn` work with a cloned `event_tx` and return a `Transition` immediately.
   Results come back as `AppEvent`s.

The consequence worth internalising: **a spawned task's result is delivered to whatever
screen is on top when it lands**, not to the screen that started it. The auth flow depends
on this — `ChessAuthScreen` re-emits `ChessReady` and pops, so `HomeScreen` receives it.

### Screen map

```
HomeScreen ──┬─▶ ChessAuthScreen         (on AuthFailed; pops back with ChessReady)
             ├─▶ ChessGameScreen         ("Demo" — idle backend, so no stream attaches)
             ├─▶ PuzzleScreen ───────────▶ PuzzleSettingsScreen
             ├─▶ OngoingChessGamesScreen ─▶ ChessGameScreen ─▶ GameActionsScreen
             └─▶ SettingsScreen ─────────▶ UpdateScreen
```

Nine screens. Adding one means: (1) a struct in `models/ui.rs` with its layout constants,
(2) an `impl Screen` in `ui/screens.rs`, (3) something returning `Transition::Push`.

### Typestate

`BoardAPI<S>` is generic over `Idle` / `InGame`. `move_piece`, `resign_game`, `abort_game`
and `stream_game_event` are implemented **only** on `BoardAPI<InGame>`, so calling them
without a scoped game is a compile error rather than a runtime one. `attach_game` is the
one-way transition, triggered by picking a game from the ongoing-games list.

### Position handling

Every `GameState` event from Lichess carries the **full move list from move 1**. So the
current position is always rebuilt as `initial_board.clone()` + replay, never patched
incrementally. Same for the last-move highlight: replay all-but-the-last move, then diff
the two positions — which catches castling rook squares and en-passant captures without
special-casing UCI strings.

### E-ink rendering discipline

This is the thing most likely to be broken by a well-meaning refactor:

- A tap that hits nothing returns `Transition::Stay`, **not** `Redraw` — a full-screen
  clear+repaint is a visible flash.
- `BoardWidget` and both sidebars diff against a snapshot of what they last painted and
  repaint only what changed.
- Repainting over previously-bright content does a **white scrub pass** first, forcing the
  panel through a full waveform so the old content doesn't ghost through.
- The sidebars clear narrow *centred content bands*, never their full area, so a text
  refresh never disturbs the edge-mounted icon buttons.
- `on_reveal()` / `invalidate()` exist because a pushed screen paints over the framebuffer,
  invalidating the partial-render assumption.
- The 10 Hz bootstrap redraw in `App::run` is capped at 1.5 s and stops at the first X11
  event. It is a workaround for the Kindle's Xorg dropping the first paint — not a
  general-purpose render tick.

## Current state (branch `puzzle-implementation`)

**Not merged.** 25 files changed vs `main`, ~1900 insertions.

What the branch adds:
- Puzzle screen + puzzle sidebar + puzzle settings screen, backed by `PuzzleSession`
- Game-actions screen (resign / abort) reached from the live-game sidebar
- Partial/banded rendering for both sidebars
- `IconButton` widget; home-screen logo; button refinements
- `/api/puzzle/*` client incl. the daily-already-played check and the anonymous fallback

Health check as of writing:

- `cargo build` — clean, no warnings.
- `cargo test` — **9 tests, all passing** (5 in `models/bitboard.rs`, 4 in `models/puzzle.rs`).
  Note `CLAUDE.md` still claims "no test suite"; that is out of date.
- Release pipeline — v0.1.2 is published with both assets; the updater path works end to end.
- The next release-please bump from this branch's `feat:` commit would be **0.2.0**
  (`bump-minor-pre-major: true`).

### Known rough edges

**Dead files** (present, not compiled, and drifting):
`src/api/mod.rs` (shadowed by the inline `pub mod api { … }` in `main.rs`),
`src/api/account.rs` (empty), `src/ui/lib.rs`, `src/ui/bin/test_ui.rs`,
`src/ui/widgets/menu.rs` (empty), `src/examples/examples.rs`.
`src/ui/lib.rs` and `bin/test_ui.rs` both reference a `kindle_x11_test` crate that no
longer exists. (`flash_display.sh`, which deployed that binary, was deleted in `916dd0f`.)

**Functional gaps:**
- `ChessAuthScreen` handles `AuthFailed` by logging only — no visible error, no retry.
- OAuth token expiry is unhandled (`TODO (#6)` in `api/oauth.rs`).
- No promotion picker; `move_to_uci` always appends `q`.
- The offline backend (`src/local/`) is a stub and is never constructed at runtime.
- The home screen's "Demo" button pushes a game screen with an idle backend, so no stream
  attaches and the board stays empty. It is effectively a layout preview.
- `authenticated_request`'s non-GET arms `.unwrap()` on transport errors.

**Repo hygiene:**
- `kindle_chess/Cargo.lock` is now tracked, so CI resolves the same dependency versions
  you build against. (It was gitignored until 2026-08-24.)
- `CLAUDE.md` is listed in `.gitignore`, so it does **not** travel with a clone. Anything
  that must survive a move to another machine belongs in `docs/` or a module README,
  not in `CLAUDE.md`.
- A committed ARM binary lives at `kindle_KUAL/hellokindle/bin/kindle-hello` (~10 MB) and
  is re-committed on every flash.
- There is a stray, remote-less `kindle_chess/.git/` directory. The crate's files are
  tracked by the outer repo; this nested git dir is inert leftover but can confuse tooling.

## Setting up on a second machine

1. `git clone git@github.com:mxyyz/kindle-chess.git`
2. `rustup target add armv7-unknown-linux-musleabi`
3. `cargo install cross --git https://github.com/cross-rs/cross` (needs Docker or Podman)
4. **Edit `kindle_chess/.env.debug`** — `ROOT_DIR` must be the absolute path of *that*
   checkout's `kindle_KUAL/hellokindle/` directory, with a trailing slash.
5. Dev harness deps (Arch): `xorg-server-xvfb`, plus `xorg-server-xephyr` (recommended).
6. `./run-dev.sh`
7. For flashing: see [`kindle-usb-ssh.md`](kindle-usb-ssh.md).
8. `CLAUDE.md` is not in the repo — copy it across by hand if you want it.

> ⚠ **A wrong `ROOT_DIR` fails silently — check it, don't assume it.** log4rs *creates*
> the directory tree it is pointed at, so a bad `ROOT_DIR` still starts cleanly and prints
> "Logger initialized successfully". What you get instead is an app logging into a phantom
> directory, with `ASSETS_DIR` and `AUTH_TOKEN` resolving there too — no piece sprites, no
> icons, no cached token. The symptom is a blank board, not an error.
>
> After editing `.env.debug`, confirm the value actually landed in the binary:
>
> ```sh
> cd kindle_chess && cargo build
> strings target/debug/kindle-hello | grep hellokindle/
> ```
>
> (This bit the current dev machine: `.env.debug` said `/home/mxy/Repos/…` with a capital
> `R` while the checkout was at `/home/mxy/repos/…`. Fixed 2026-08-24, along with the
> missing `rerun-if-changed` in `build.rs` that made the correction a no-op.)
