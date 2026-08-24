# `src/ui` — rendering, screens, widgets

Everything that draws. `Renderer` is the only thing that speaks x11rb; screens compose
widgets; widgets draw through `Renderer`. Screen *data* lives in `models/ui.rs` — this
directory holds the `impl Screen for …` blocks and the widgets.

## File map

| File | Status | Purpose |
|---|---|---|
| `renderer.rs` | live | The X11 surface: window creation, GCs, primitives, font rasterisation |
| `screens.rs` | live | `impl Screen` for all nine screens + the `kick_*` async helpers (~1300 lines) |
| `events.rs` | live | `AppEvent`, `TouchEvent`, `Square`, `ChessMove`, `RectangleExt` |
| `display.rs` | live | `Display::new()` — wires the channel and the renderer together |
| `widgets.rs` | live | Module decl + re-exports for `widgets/` |
| `widgets/board.rs` | live | The 8×8 board: sprites, partial repaint, touch → move |
| `widgets/sidebar.rs` | live | Live-game sidebar (opponent, turn status, back/settings) |
| `widgets/puzzle_sidebar.rs` | live | Puzzle sidebar (status, rating/themes, back/hint/next/settings) |
| `widgets/button.rs` | live | Rectangle + centred label |
| `widgets/icon_button.rs` | live | Borderless PNG glyph, one-cell touch footprint; `load_icon()` |
| `mod.rs` | live | `pub mod display; events; renderer; screens; widgets;` |
| `widgets/menu.rs` | **dead** | Empty, not declared in `widgets.rs` |
| `lib.rs` | **dead** | Not declared in `mod.rs`; references a `kindle_x11_test` crate that no longer exists |
| `bin/test_ui.rs` | **dead** | Not a `[[bin]]` target in `Cargo.toml`; also references `kindle_x11_test` |

> `flash_display.sh` still tries to deploy a `test_ui` binary. That target does not exist
> any more — the script cannot work as written.

## `renderer.rs`

`Renderer::new()` connects to X11 (`:0` on device, `$DISPLAY` in dev), creates a
**1072 × 1448 `override_redirect` window** — no window manager involved, which is why the
same code works against the Kindle's bare Xorg and against Xvfb/Xephyr — and maps it
stacked above everything.

Five greyscale GCs are pre-created, one per `DrawColor`:

| `DrawColor` | luma |
|---|---|
| `Black` | 0 |
| `DarkGray` | 64 |
| `Gray` | 128 |
| `LightGray` | 192 |
| `White` | 255 |

Primitives: `draw_rectangle`, `draw_circle`, `draw_line`, `draw_image`,
`draw_image_alpha`, `draw_text`, `measure_text`, `clear`, `present`.

Details that matter:

- **`draw_line` restores `line_width` to 0** after drawing a thick segment. Leaking a
  non-zero width would silently make every subsequent unfilled rectangle draw a thick
  border.
- **`draw_image` matches the drawable depth** — 1 byte/px at depth 8 (the Kindle),
  4 bytes/px BGRX at 24/32 (a typical dev machine), scanlines padded to 4 bytes. Getting
  this wrong is the classic "works in Xephyr, garbage on device" bug.
- **`draw_image_alpha`** composites RGBA onto a solid background colour using BT.601 luma
  weights, then dispatches through `draw_image`. This is how transparent piece sprites
  pick up their square's colour.
- **`draw_text`** rasterises with `fontdue` into a transient greyscale buffer initialised
  to white, then `put_image`s it. Consequence: **text always paints an opaque white
  background box**. There is no transparent text. `size_px` is cap height; Adwaita Sans
  cap ≈ 0.7 × em. The font is `include_bytes!`-embedded, so no asset file is needed.
- **`present()`** flushes only when `dirty` is set. `Drop` destroys the window.

## `events.rs`

`AppEvent` is the single channel type for X11 input, async task results, and inter-screen
messages. Grouped as declared:

| Group | Variants |
|---|---|
| Auth | `AuthSuccess`, `AuthFailed`, `QrReady`, `ChessReady` |
| Ongoing games | `OngoingGamesLoaded`, `OngoingGamesFailed` |
| Update | `UpdateAvailable`, `UpdateUpToDate`, `UpdateCheckFailed`, `UpdateApplied`, `UpdateApplyFailed` |
| Game stream | `GameFullReceived { … }`, `TurnChanged { … }`, `MoveRejected` |
| Puzzle | `PuzzleLoaded`, `PuzzleLoadFailed`, `OpenPuzzleSettings`, `NextPuzzle`, `ShowPuzzleHint`, `PuzzleOpponentMove`, `ApplyPuzzleParams` |
| Chess input | `MoveMade`, `SquareSelected` |
| Navigation | `ExitToMenu`, `OpenGameActions`, `Quit` |
| X11 | `Touch`, `Expose`, `WindowUnmapped`, `Redraw`, `Tick` |

`Redraw` and `Tick` are declared but never sent.

`Square` is `{ file: 0-7, rank: 0-7 }` in **true board coordinates** (never display
coordinates — the widget flips on the way in and out). `Rectangle` is an alias for
`xproto::Rectangle`, extended by the `RectangleExt` trait with `new()` and `contains()`;
`contains` is the basis of every hit test in the app.

## `screens.rs` — the navigation graph

```
HomeScreen ──┬─▶ ChessAuthScreen        (pushed on AuthFailed; pops back with ChessReady)
             ├─▶ ChessGameScreen        ("Demo" — idle backend, no stream)
             ├─▶ PuzzleScreen ──────────┬─▶ PuzzleSettingsScreen
             │                          └─(pop)
             ├─▶ OngoingChessGamesScreen ─▶ ChessGameScreen ─▶ GameActionsScreen
             └─▶ SettingsScreen ─────────▶ UpdateScreen
```

Per-screen notes:

- **`HomeScreen`** — kicks `kick_auth_bootstrap` on first render: load the cached token,
  validate it with `get_user_info`, post `ChessReady` or `AuthFailed`. It never calls
  `authenticate()` itself; the QR flow belongs to `ChessAuthScreen`. Chess and
  Ongoing-Games stay inert until `app` is populated. **Puzzle and Settings are always
  live** — the daily puzzle needs no token, and an offline user still needs the updater.
  A tap that hits no button returns `Stay`, deliberately avoiding a full-screen e-ink flash.

- **`ChessAuthScreen`** — spawns `authenticate()`, draws the QR when `QrReady` lands, then
  on `AuthSuccess` posts `ChessReady` to itself, re-emits it, and pops so `HomeScreen`
  catches it. `AuthFailed` currently only logs — no visible error, no retry affordance.

- **`ChessGameScreen`** — kicks the game-state stream once. Applies
  `GameFullReceived`/`TurnChanged` to its own `ChessApp` copy, the board and the sidebar.
  Orients the board with `set_flipped(!player0_white)`. Delegates touches to the board
  first, then the sidebar, and **recurses into `handle_event`** with whatever `AppEvent`
  the widget returns. `on_reveal` invalidates both widgets.

- **`OngoingChessGamesScreen`** — 4 games per page, 8 fetched. `set_page` bakes labels into
  the buttons (`VS <opponent>`, wrapped in `> … <` when it's your turn) so `render` is a
  pure draw pass. `game_at_slot` makes over-run slots inert. `on_reveal` clears the cache
  so the list re-fetches on return.

- **`SettingsScreen`** — shows `version::VERSION` / `GIT_SHA` / `BUILD_TIMESTAMP` and the
  entry point to `UpdateScreen`.

- **`UpdateScreen`** — the `UpdateState` machine
  (`Checking → UpToDate | Available → Downloading → Applied | Failed`). The action button
  is only drawn *and* only live when there is something to do. Once `Applied` it becomes
  "Quit", because the swap happens in the launcher script at next start.

- **`PuzzleScreen`** — no stream; everything is local against `PuzzleSession`. On
  `MoveMade` it calls `try_move`, re-syncs the widget unconditionally (correct either way,
  since a wrong move leaves the position untouched), and if a reply is pending spawns a
  task that sleeps 1 s and posts `PuzzleOpponentMove` — so the solver sees their own move
  land before the reply. That handler re-checks `opponent_reply_pending()` because the
  puzzle may have been replaced during the delay.

- **`PuzzleSettingsScreen`** — tap-to-cycle `Difficulty`/`Phase`/`Color`; "Get new puzzle"
  emits `ApplyPuzzleParams` and pops, letting `PuzzleScreen` do the fetch.

- **`GameActionsScreen`** — resign / abort, fire-and-forget. The still-running game stream
  on `ChessGameScreen` picks up the terminal state after the pop.

## Rendering model

Two strategies coexist, and the split is intentional: full-screen e-ink repaints are slow
and flash, so anything that updates frequently repaints only what changed.

**Full repaint** — most screens: `clear(White)` then draw everything.

**Partial repaint** — `BoardWidget`, `SidebarWidget`, `PuzzleSidebarWidget`. Each keeps a
snapshot of what it last painted and diffs against it.

`BoardWidget::render` runs four diff passes in a fixed order, tracking a `repainted: u64`
mask so no square is painted twice:

1. **Position diff** — repaint squares whose occupant changed.
2. **Last-move diff** — scrub squares whose bracket is going away.
3. **Selection diff** — scrub the square losing its highlight (at most one).
4. **Re-stamp** — last-move brackets, then the selection cut on top.

The **white scrub** (`repaint_square(.., scrub: true)`) paints white before the target
colour. This forces the e-ink panel through a full bright→dark waveform instead of trying
to converge from "mostly the previous content", which is what stops thick borders and
diagonal cuts from ghosting.

`force_full_repaint` is set only by an orientation flip or an external `invalidate()`.
Position and selection changes always take the partial path.

Both sidebars use **content-width centred bands** (`opponent_band`, `status_band`,
`info_band`) rather than clearing their whole area. The bands are narrow enough to leave a
wide gap to the edge-mounted icon buttons, so refreshing a band never disturbs a button —
which is why the buttons are only painted on a full repaint.

`invalidate()` on all three widgets exists for `on_reveal()`: the partial path assumes the
framebuffer still holds what the widget last painted, and a screen drawn on top breaks
that assumption.

## `widgets/board.rs`

Constants: `SQUARE_SIZE = 134` (1072 / 8), `PIECE_DRAW_SIZE = 96`,
`SEGMENT_LEN = 134 / 8`, `SELECTION_STROKE = 4`, `LAST_MOVE_STROKE = 6`.

- **Sprites** — loaded once in `PieceSprites::load()` from `env!("ASSETS_DIR")`. Naming
  convention: white pieces carry a `_w` suffix (`chess-queen_w.png`), black pieces don't
  (`chess-queen.png`). A missing sprite logs a warning and renders nothing.
- **Square colour is intrinsic to `(file, rank)`**, not to the display position:
  `(rank + file) % 2 == 0` → `DarkGray`. Flipping the board changes where a square is
  drawn, not what colour it is.
- **Touch → move** — first tap selects and emits `SquareSelected`; a second tap on a
  different square emits `MoveMade` and clears the selection; a second tap on the same
  square deselects. Only `TouchKind::Down` is handled.
- **`move_to_uci`** appends a `q` promotion suffix when a pawn reaches its last rank.
  There is no promotion picker; `PuzzleSession::try_move` compensates by matching on
  from/to only.
- **`select_square(Some(sq))`** lets a caller mark a square without a tap — the puzzle
  hint button uses it to flag the piece that should move.
- **Highlight colours invert against the square** (`highlight_color`): dark squares get
  `LightGray` decorations, light squares get `Black`. Selection is four 45° corner cuts;
  last-move is four corner "L" brackets built from filled rectangles.

## `widgets/icon_button.rs`

`ICON_BUTTON_SIZE = 134` — one chess cell, so touch targets stay finger-sized — with the
glyph drawn at 100 px (overridable via `with_glyph_size`). `load_icon(name)` resolves
against `env!("ASSETS_DIR")` and returns `None` with a warning on failure, so a missing
PNG degrades to an invisible-but-still-tappable button rather than a crash.

Icons in use: `back-nav.png`, `back-page.png`, `next-page.png`, `settings.png`,
`hint.png`, `check.png`, `x.png`, `logo.png`.
