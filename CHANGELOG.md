# Changelog

All notable changes to this project are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Website at [qantrip.nekro.ai](https://qantrip.nekro.ai) with an interactive demo running the real core compiled to WebAssembly.
- Sync server (`apps/cloud/`): GitHub sign-in, desktop sign-in with PKCE, and the end-to-end encrypted sync API (vaults, records with compare-and-swap, signed roster, key envelopes, device-join relay). The server stores ciphertext only; the sync client is in progress.
- Sync security design v0.2 ([`docs/sync-design.md`](docs/sync-design.md)), revised after an adversarial review.

### Changed
- Repository renamed from `NekroAI/nekro-qantrip` to `NekroAI/qantrip`.

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

[Unreleased]: https://github.com/NekroAI/qantrip/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.1.0
