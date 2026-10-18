# Changelog

All notable changes to this project are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] — 2026-10-18

### Added
- Command mode in the panel: type `/` to manage templates without leaving the keyboard. `/new name text` creates a template (`{param}` and `{param:a|b}` placeholders become options; without text it uses the clipboard), plus `/edit`, `/dup`, `/alias`, `/rm` (asks for a second Enter, moves the file to the Trash), `/library`, `/settings`, `/sync` and `/lang auto|zh|en`.
- Windows preview (x64): the panel with Alt+Q, paste insertion with clipboard restore (kept out of clipboard history), command mode, settings, template library, end-to-end encrypted sync and a welcome guide. Distributed as a zip; not code-signed yet.

### Fixed
- Template files that start with a UTF-8 BOM are read correctly.

## [0.2.0] — 2026-10-18

### Added
- Website at [qantrip.nekro.ai](https://qantrip.nekro.ai) with an interactive demo running the real core compiled to WebAssembly.
- Sync server (`apps/cloud/`): GitHub sign-in, desktop sign-in with PKCE, and the end-to-end encrypted sync API (vaults, records with compare-and-swap, signed roster, key envelopes, device-join relay). The server stores ciphertext only.
- End-to-end encrypted sync in the macOS app (Settings › Sync): sign in with GitHub, create a vault with a 24-word recovery phrase, add a Mac by typing a code on each side, recover with the phrase, remove devices with key rotation. Syncs every 5 minutes and after template changes; the panel never waits on the network.
- English interface and English built-in templates. Interface and template language follow the system by default and can be set separately.
- Sync security design v0.3 ([`docs/sync-design.md`](docs/sync-design.md)): bidirectional join confirmation and signed records, after a second adversarial review.
- `llms.txt`, sitemap and structured data on the website; Chinese pages under `/zh`.

### Changed
- Repository renamed from `NekroAI/nekro-qantrip` to `NekroAI/qantrip`.
- Built-in templates are grouped by workflow (understand, write, review, ship, translate).
- Release DMGs are also published as `Qantrip-macOS-arm64.dmg` / `Qantrip-macOS-x86_64.dmg`, so `releases/latest/download/…` links always point to the newest version.

## [0.1.0] — 2026-10-17

First macOS preview.

### Added
- Global hotkey (<kbd>⌃Q</kbd> by default) and a non-activating panel that keeps focus in the app you were using.
- Input grammar with exact / prefix / fuzzy matching, positional values, long and short options, free text, full-width punctuation support and inline completion.
- Parameterized templates in Markdown + YAML front matter rendered with MiniJinja; 17 built-in templates.
- Paste-based insertion with clipboard restore, transient markers for clipboard managers, copy-only fallback, and <kbd>⌘↩</kbd> to expand Claude Code's collapsed pastes.
- Home screen with recent invocations and per-app frecency ranking.
- Template library with form and source editing, live preview, import / export.
- Settings (hotkey recorder with conflict detection, launch at login, input source switching) and first-run onboarding.
- Signed arm64 and x86_64 DMGs built by GitHub Actions.

[Unreleased]: https://github.com/NekroAI/qantrip/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.0
[0.2.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.2.0
[0.1.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.1.0
