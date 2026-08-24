# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

A KUAL launcher for jailbroken Amazon Kindles that talks to the Lichess API. Written in Rust, cross-compiled to `armv7-unknown-linux-musleabi`, drawing directly via x11rb to the Kindle's native Xorg on `:0`. The same binary runs on a dev machine against an emulated framebuffer (`run-dev.sh`).

## Common commands

All `cargo` commands run from `kindle_chess/` — that's the crate root, the repo root is just a container.

```bash
# Local dev build + run against emulated 1072×1448 e-ink display
./run-dev.sh                       # auto-detects Xephyr / x11vnc / feh
./run-dev.sh --release --no-build  # skip rebuild, use release binary
./run-dev.sh --viewer xephyr       # force a specific viewer

# Cross-compile for Kindle (requires `cross` and Docker/Podman)
cd kindle_chess && RUSTFLAGS="-C target-feature=+crt-static" \
    cross build --target armv7-unknown-linux-musleabi --release

# Deploy to a Kindle on the LAN (scp to root@192.168.15.244 — edit IP in flash.sh)
./flash.sh                         # builds + scp's binary + KUAL bundle
./flash_usb.sh                     # same, but copies to /run/media/mxy/Kindle
./flash_display.sh                 # alternate path: builds via podman musl image, deploys test_ui
```

The flash scripts intentionally `mv` `kindle_KUAL/hellokindle/secrets/token.json` aside before scp/cp and restore it after — this preserves the device's stored OAuth token across deploys. Don't break this dance when editing those scripts.

There is no test suite, no linter config beyond `cargo`, and no formatter config beyond `rustfmt` defaults.

## Build-time environment

`build.rs` loads `.env.debug` (debug profile) or `.env.release` (release profile) and bakes the values in via `cargo:rustc-env=...`. The Rust code reads them with `env!()`, not `std::env::var()` — they are compile-time constants.

- `ROOT_DIR` — base path for `log/`, `assets/`, `secrets/`. Debug points at the absolute path of the local checkout (`/home/mxy/Repos/kindle-chess/kindle_KUAL/hellokindle/`); release points at `/mnt/us/hellokindle/` (the Kindle filesystem). **If you check out the repo elsewhere, edit `.env.debug` or debug builds will write to a missing path.**
- `LICHESS_API_BASE` — `https://lichess.org/api`
- Derived: `LOG_FILE_DIR`, `ASSETS_DIR`, `AUTH_TOKEN` (path to `secrets/token.json`)

## Architecture

### Screen-stack runtime (`kindle_chess/src/app/app.rs`)

`App::run()` is a single-threaded event loop. A background thread blocks on `conn.wait_for_event()` and forwards X11 events into an mpsc channel. The main loop drains the channel and routes each event to the screen on top of `screen_stack: Vec<Box<dyn Screen>>`. Screens return a `Transition` (`Stay` / `Redraw` / `Push` / `Pop` / `Quit`) which the loop applies. A global triple-tap detector (within 50 px / 500 ms, three taps) short-circuits to quit on any screen.

`Display` (in `models/ui.rs`) is the long-lived X11 resource — connection, `Renderer`, event channel ends, tap-tracking state. There is exactly one. Screens borrow it; they don't own X11 state.

### Screens

`Screen` is a trait with `render` + `handle_event`. Concrete screens live in `models/ui.rs` (data) and `ui/screens.rs` (`impl Screen`). Current set: `HomeScreen` → `ChessAuthScreen` → `ChessGameScreen`. Adding a new screen means: (1) struct in `models/ui.rs`, (2) `impl Screen for ...` in `ui/screens.rs`, (3) something pushes it via `Transition::Push`.

### Async work from inside an event handler

The loop is sync, but `Screen::handle_event` runs inside `#[tokio::main]`, so handlers spawn work with `tokio::spawn` and feed results back as `AppEvent`s through a cloned `display.event_tx`. The auth flow is the canonical example: `HomeScreen` spawns auth, returns `Push(ChessAuthScreen)` immediately, then the auth task posts `QrReady` / `AuthSuccess` / `AuthFailed` which the now-active `ChessAuthScreen` handles.

### OAuth flow (`api/oauth.rs`)

PKCE against `https://lichess.org/oauth`. On auth, the app:
1. picks the host's LAN IP (`local_ip_address::local_ip()`) and starts an axum server on `0.0.0.0:<redirect_port>/callback`.
2. generates a QR code containing the Lichess auth URL — user scans it with a phone.
3. Lichess redirects to the callback, axum pulls the code, exchanges it, sends the `TokenInfo` over a `oneshot` channel, then shuts down.
4. token is persisted to `secrets/token.json` (path = `env!("AUTH_TOKEN")`).

`load_token()` is called on the Home screen's chess-button press; if the token is fresh, the app skips straight to `AuthSuccess` without the QR detour.

### Module layout

- `src/app/` — `App` (lifecycle/event loop), `ChessApp` (game session)
- `src/api/` — Lichess HTTP + OAuth client
- `src/local/` — offline board (no network)
- `src/models/` — data types; split between API DTOs (`board_api.rs`, `oauth.rs`) and UI structs (`ui.rs`, `app.rs`)
- `src/ui/` — `Renderer` (x11rb GCs + double-buffering), `Screen` impls, `widgets/` (board, sidebar), event types
- `src/ui/bin/test_ui.rs` — secondary binary used by `flash_display.sh` for UI iteration on-device
- `kindle_KUAL/extensions/hellokindle/menu.json` — KUAL menu wiring; entries shell out to scripts in `/mnt/us/hellokindle/`
- `kindle_KUAL/hellokindle/chess_app.sh` — on-device launcher, currently runs `kindle-hello` under a 180 s `timeout` for safety. The lipc / `disableEnablePillow` block that hands the display from the Kindle UI to X11 is commented out — re-enable it when you actually want X11 to own the framebuffer instead of running over the Kindle home screen.

### Coordinate system / display

Hard-coded for 1072 × 1448 (Kindle Paperwhite 4). Screen layouts use absolute pixel coords (see `HomeScreen::new`, `ChessGameScreen::new`). `run-dev.sh` matches this exactly so layouts are pixel-identical between dev and device.
