# Contributing to Qantrip

Thanks for your interest! Qantrip is a small project, so the process is light. This guide covers how to build, test and submit changes.

## Before you start

- **Bugs:** open an issue with the [bug report template](https://github.com/NekroAI/qantrip/issues/new?template=bug_report.yml). Include your macOS version, chip, Qantrip version and the exact command you typed.
- **Features:** open an issue first so we can agree on the approach. Product decisions and their reasoning are recorded in [`docs/decisions.md`](docs/decisions.md) — please read the relevant entries before proposing a change to something already decided.
- **Security issues:** do not open a public issue. See [SECURITY.md](SECURITY.md).

## Setting up

| Component | Requirements |
|---|---|
| Core (`core/`) | Rust stable (`rustup`) |
| macOS app (`apps/macos/`) | macOS 14+, Xcode Command Line Tools (a full Xcode is not required) |
| Website & sync server (`apps/cloud/`) | Node.js 22, pnpm 11 |
| WebAssembly demo (`core/wasm/`) | `rustup target add wasm32-unknown-unknown`, `cargo install wasm-bindgen-cli --version 0.2.129` |

```sh
cargo test --manifest-path core/Cargo.toml
scripts/build-macos.sh              # dist/Qantrip.app, signed with a local dev certificate
cd apps/cloud && pnpm install && pnpm test && pnpm typecheck
```

`scripts/build-macos.sh` creates a dedicated keychain with a self-signed "Qantrip Dev" certificate so macOS keeps the Accessibility permission across rebuilds. It never touches your login keychain.

### Insertion tests

The clipboard / paste pipeline has an end-to-end test that drives a probe app:

```sh
QANTRIP_DEBUG_HOOKS=1 scripts/build-macos.sh
defaults write ai.nekro.qantrip debugHooks -bool true
open dist/Qantrip.app      # grant Accessibility once
python3 tests/spike/run_insert_tests.py
```

Debug hooks are compiled only when `QANTRIP_DEBUG_HOOKS=1` and are never part of release builds.

## Making changes

- **Keep the core platform-independent.** Parsing, rendering and sync logic belong in `core/`; platform shells only handle UI and OS integration.
- **Match the surrounding code.** Comment density, naming and idioms should look like the code around your change. Design documents in `docs/` are written in Chinese; code comments follow the file you are editing.
- **Templates:** built-in templates live in `core/templates/`. `cargo test` renders every template with every choice, so a missing branch in a mapping fails the build.
- **Sync and cryptography:** changes to `docs/sync-design.md` or anything that touches keys, envelopes or the roster need a written rationale and test vectors. Don't add new cryptographic constructions — use the existing primitives.
- **Server:** every D1 query must use a primary key or an index (D1 bills by rows scanned), and the server must never contain decryption logic.

## Commits and pull requests

- Write commit messages that say what changed and why. A short prefix such as `core:`, `macos:`, `cloud:` or `docs:` helps.
- One logical change per pull request; include tests for behavior changes.
- CI must pass: core tests, clippy (`-D warnings`), the macOS build, and the cloud type check and contract tests.
- By contributing, you agree that your contributions are licensed under the project's licenses: GPL-3.0-or-later for the client and core, AGPL-3.0-or-later for `apps/cloud/`.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
