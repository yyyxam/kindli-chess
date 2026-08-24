# `src/examples` — scratch snippets

**Status: not compiled.** This directory is not part of the crate.

`src/main.rs` declares exactly six modules — `api`, `app`, `local`, `models`, `ui`,
`version` — and `examples` is not among them. There is no `mod.rs` here and no
`[[example]]` target in `Cargo.toml`. (Note: Cargo's convention for real examples is a
top-level `examples/` directory next to `src/`, not `src/examples/`.)

`examples.rs` is a single block comment: previously-working call sites kept around as
usage notes, covering

- fetching the daily puzzle,
- `logout()` / token deletion,
- board interaction (`resign_game`, `move_piece`),
- listing recent games and picking a `game_id` to stream.

Because it never compiles, **it drifts**. Several snippets already reference the
pre-typestate API — free functions like `get_ongoing_games(&auth, 5)` and
`resign_game(&game_id, &auth_token)` where the current code has
`BoardAPI<Idle>::get_ongoing_games(n)` and `BoardAPI<InGame>::resign_game()`. Treat the
snippets as historical intent, not as copy-pasteable code.

## If you want these to stay honest

Pick one:

- **Delete it.** The same flows are exercised for real in `src/ui/screens.rs`
  (`kick_*` functions), which the compiler does check.
- **Promote it** to a real `examples/` directory at the crate root with runnable
  `fn main()`s, so `cargo build --examples` catches the drift. Note these all need
  network and a valid token, so they would not be runnable in CI.
- **Move the useful notes** into the per-module docs, which are at least reviewed
  alongside the code.
