# kindle-chess
KUAL-app written in Rust, implementing the free and open lichess APIs to enable (online) chess games on amazon's (jailbroken) kindle fire 7

# Features
- Retrieve ongoing games via lichessapi
- Continue ongoing chess game
- Authenticate via phone through QR
- Update directly via github release possible

# Compile to Kindle binary:
`RUSTFLAGS="-C target-feature=+crt-static" cross build --target armv7-unknown-linux-musleabi --release`

# Documentation

| Doc | What it covers |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Whole-system overview, runtime model, current branch state, second-machine setup |
| [`docs/kindle-usb-ssh.md`](docs/kindle-usb-ssh.md) | USB networking + SSH to the device, made interface-name-independent; flashing |
| [`docs/ci-cd.md`](docs/ci-cd.md) | GitHub Actions release pipeline and the in-app updater |

Per-module docs live next to the code:
[`api`](kindle_chess/src/api/README.md) ·
[`app`](kindle_chess/src/app/README.md) ·
[`models`](kindle_chess/src/models/README.md) ·
[`ui`](kindle_chess/src/ui/README.md) ·
[`local`](kindle_chess/src/local/README.md) ·
[`examples`](kindle_chess/src/examples/README.md)
