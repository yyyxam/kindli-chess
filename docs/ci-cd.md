# CI/CD and the in-app updater

How a commit becomes a binary on the Kindle, without a cable.

**Short answer to the open question:** both halves are implemented. The app checks GitHub
*and* downloads, SHA256-verifies and installs the new binary. The artifact build is
triggered by pushing a `v*` tag, and release-please pushes that tag automatically when you
merge its release PR.

## The pipeline end to end

```
  conventional commit ──▶ merge to main
            │
            ▼
   release-please.yml  ──▶ opens/updates a "release PR"
            │                (bumps Cargo.toml + CHANGELOG.md + manifest)
            │
      merge that PR
            │
            ▼
   release-please.yml  ──▶ pushes tag v0.X.Y + creates the GitHub release
            │                (authenticated with a PAT — see "the token gotcha")
            ▼
      release.yml      ──▶ cross-build armv7-musl
            │                upload kindle-chess-armv7-musl
            │                upload kindle-chess-armv7-musl.sha256
            ▼
   app: Settings ▸ Check for updates
            │  api/github.rs   GET /repos/{OWNER}/{REPO}/releases/latest
            │  api/update.rs   stream asset, hash it, compare to sidecar
            ▼
        writes <exe>.new  (does NOT replace the running binary)
            │
      user quits, taps KUAL ▸ ChessApp
            ▼
   chess_app.sh: mv bin/kindle-hello.new bin/kindle-hello; chmod +x; exec
```

## The three workflows

### `rust.yml` — CI

Triggers on push and PR to `main`. Two independent jobs, run in parallel:

**`test`** — `cargo test` from `kindle_chess/`, on the runner's **native** target. No
`cross`, no emulation: everything under test (`models/bitboard.rs`, `models/puzzle.rs`) is
pure logic with no X11, no network and no asset files, so the host target is sufficient and
the job finishes in well under a minute. It is the fast-feedback half of CI.

**`build`** — installs `cross`, then

```sh
cd ./kindle_chess && RUSTFLAGS="-C target-feature=+crt-static" \
  cross build --target armv7-unknown-linux-musleabi --release
```

This is the slow half (the `cargo install cross --git` alone dominates), and it proves the
armv7-musl cross-build still works — the same command `release.yml` will run when a tag is
pushed.

Note that `build.rs` runs in the `test` job too, so `.env.debug` must exist and parse in
CI. It is tracked, and its `ROOT_DIR` pointing at a path that does not exist on the runner
is harmless: `build.rs` only reads the string, and none of the tests touch the filesystem.

### `release-please.yml` — versioning

Triggers on every push to `main`. Runs `googleapis/release-please-action@v4` with
`release-please-config.json` and `.release-please-manifest.json`.

On each push it scans Conventional Commit messages since the last release tag and either:

- opens or updates a **release PR** that bumps `kindle_chess/Cargo.toml`, regenerates
  `kindle_chess/CHANGELOG.md`, and updates the manifest; or
- if such a PR was just merged, **pushes the matching `v*` tag** and creates the GitHub
  release.

Config (`release-please-config.json`):

| Setting | Value | Effect |
|---|---|---|
| `release-type` | `rust` | reads/writes `Cargo.toml` |
| `bump-minor-pre-major` | `true` | pre-1.0, `feat:` bumps **minor** |
| `bump-patch-for-minor-pre-major` | `false` | ″ |
| `include-v-in-tag` | `true` | tags are `v0.1.2`, not `0.1.2` |
| `packages.kindle_chess.package-name` | `kindli_chess` | matches the crate name (note the spelling) |
| `include-component-in-tag` | `false` | tag is `v0.1.2`, not `kindli_chess-v0.1.2` |

`.release-please-manifest.json` is the source of truth for the last released version —
currently `{"kindle_chess": "0.1.2"}`.

Commit-message prefixes that matter: `feat:` → minor bump, `fix:` → patch,
`feat!:` / `BREAKING CHANGE:` → would be major (but pre-1.0 rules apply),
`chore:` / `docs:` / `refactor:` → no release.

> The current `puzzle-implementation` branch's HEAD is a `feat:` commit, so merging it and
> then merging the resulting release PR would cut **v0.2.0**.

#### The token gotcha

`release-please.yml` authenticates with `secrets.RELEASE_PLEASE_TOKEN`, not the default
`GITHUB_TOKEN`. This is deliberate and load-bearing: **GitHub suppresses workflow chaining
when a push is made with the default `GITHUB_TOKEN`.** With `GITHUB_TOKEN`, the tag would
be pushed but `release.yml` would never fire, so no binary would ever be built.

`RELEASE_PLEASE_TOKEN` is a classic PAT with `repo` + `workflow` scopes. **Classic PATs
expire.** If releases suddenly stop producing assets — the tag exists, the release exists,
but there is no binary — check whether this token has expired before debugging anything else.

### `release.yml` — the artifact build

Triggers on **push of a `v*` tag**, plus `workflow_dispatch` with a tag input for
re-uploading assets to an existing tag by hand.

Steps:

1. Resolve the tag (from the ref, or from the dispatch input).
2. Checkout that tag with `fetch-depth: 0` — full history, so `build.rs` can resolve a real
   git short SHA rather than baking in `"unknown"`.
3. **Sanity-check** that the tag matches `kindle_chess/Cargo.toml`'s `version`, and fail
   loudly if not. This is what catches a hand-pushed tag that skipped release-please.
4. `cargo install cross --locked`, then cross-build `armv7-unknown-linux-musleabi --release`
   with `RUSTFLAGS="-C target-feature=+crt-static"`.
5. Stage `dist/kindle-chess-armv7-musl` and generate `dist/kindle-chess-armv7-musl.sha256`
   with `sha256sum`.
6. `gh release create` (if needed) then `gh release upload --clobber`.

## The asset-name contract

These four constants in `src/api/github.rs` **must** match `release.yml`:

```rust
pub const OWNER: &str      = "mxyyz";
pub const REPO:  &str      = "kindle-chess";
pub const ASSET_NAME: &str = "kindle-chess-armv7-musl";
pub const SHA_NAME:   &str = "kindle-chess-armv7-musl.sha256";
```

Rename an asset in the workflow and every already-deployed binary stops being able to
update itself — the old binaries look for the old name. Treat these as frozen.

> **Historical note.** `OWNER` was `yyyxam` until 2026-08-24, from before the GitHub
> account was renamed to `mxyyz`. It kept working only because
> `api.github.com/repos/yyyxam/kindle-chess/releases/latest` returns **301** to the repo's
> numeric ID and reqwest follows redirects. **Binaries already on devices still carry
> `yyyxam`**, so they depend on that redirect surviving — which it does as long as nobody
> re-registers the `yyyxam` account. Anything flashed or self-updated after v0.1.2 uses the
> correct owner directly.

## The app side

### Checking — `api/github.rs::check_for_update()`

Anonymous `GET https://api.github.com/repos/{OWNER}/{REPO}/releases/latest`
(60 req/h per IP; checks are user-initiated so this is never close). A `User-Agent` header
is mandatory — GitHub rejects requests without one.

`parse_tag_version` strips a leading `v` or `release-` and parses semver. The result is
deliberately three-way:

| Result | Meaning | Shown as |
|---|---|---|
| `Ok(Some(UpdateInfo))` | newer tag **and** both assets present | "Update available: vX → vY" |
| `Ok(None)` | `latest <= current` | "You're up to date" |
| `Err(_)` | unparseable tag, **or release exists but assets aren't uploaded yet** | "Check failed: …" |

That third row is the interesting one. While `release.yml` is still cross-building, the
release exists with no assets. Reporting that as an error gets the user "try again in a
minute" instead of a wrong "you're up to date". (This is what commit `56503e1`,
*"fix: up-to-date-check giving false true"*, was about.)

`version::current()` parses `CARGO_PKG_VERSION`, so **`Cargo.toml` is the single source of
truth** for what version a binary believes it is. That is also why `release.yml`'s
tag-vs-manifest check matters.

### Applying — `api/update.rs::apply_update()`

1. Stream the asset with `reqwest`, feeding each chunk into a `Sha256` hasher and writing
   it to `<current_exe>.new`.
2. Fetch the `.sha256` sidecar; take the first whitespace-separated token
   (`sha256sum` format is `<hex>  <filename>`).
3. On mismatch: delete the partial file, return an error, **leave the running binary
   untouched**.
4. On match: `chmod 0755` and stop.

**It deliberately does not rename over the running binary.** `/mnt/us` is VFAT, which lacks
the inode-based open-file semantics that make replace-a-running-executable work on ext4.

### Installing — `kindle_KUAL/hellokindle/chess_app.sh`

The launcher does the swap at startup, when no instance is running:

```sh
if [ -f "$BINARY.new" ]; then
    if mv "$BINARY.new" "$BINARY"; then
        chmod +x "$BINARY"
    fi
fi
```

If the `mv` fails the current binary is kept. This is why `UpdateScreen` shows
*"Quit and relaunch from the KUAL menu"* and turns its action button into **Quit** once
applied — the update lands on the *next* launch, not immediately.

### UI path

`HomeScreen ▸ Settings ▸ Check for updates` → `UpdateScreen`, which auto-kicks the check on
first render. `UpdateState` is
`Checking → UpToDate | Available → Downloading → Applied | Failed`. The Apply button is only
drawn, and only live, in states where it does something. Settings is reachable without
authentication on purpose: an unauthenticated user still needs the updater.

## Cutting a release

Normal path — no manual tagging:

1. Land conventional commits on `main`.
2. release-please opens a release PR; review the version bump and CHANGELOG.
3. Merge it. The tag and GitHub release appear automatically.
4. `release.yml` fires; wait ~2–5 min for the cross-build.
5. Confirm both assets exist on the release before telling anyone to update — until they
   do, every device's check reports `Err`.

Re-running a build for an existing tag: Actions ▸ Release ▸ *Run workflow*, pass the tag.
`--clobber` overwrites the existing assets.

## Gaps worth closing

- ~~**`Cargo.lock` is gitignored.**~~ Fixed 2026-08-24 — it is tracked now, so CI builds
  against the same resolved versions you do.
- **No release signing.** SHA256 protects against corruption, not tampering; the hash is
  fetched over the same channel as the binary. Fine for a personal project, worth knowing.
- **`rust.yml` and `release.yml` both `cargo install cross` from git on every run** — slow,
  and unpinned.
