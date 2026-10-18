# Changelog

All notable changes to this project are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Linux preview (x86_64 and ARM64): a native GTK4 / libadwaita app with the same panel, command mode, template library, settings, welcome guide, tray menu, end-to-end encrypted sync and update notice as on macOS and Windows. Ctrl+Q opens the panel — registered through the desktop's global shortcuts portal on GNOME and KDE (Wayland), grabbed directly on X11, or bound to `qantrip toggle` in your keyboard settings elsewhere. Auto-paste uses the remote desktop portal on Wayland (allowed once, then remembered) and XTest on X11; terminals get Ctrl+Shift+V. Distributed as AppImage, .deb and .rpm; needs GTK 4.14+ and libadwaita 1.5+.
- `qantrip list`, `qantrip render <input>` and `qantrip complete <input>` on Linux use the same templates and grammar from the command line.

### Changed
- Windows is no longer marked as a preview.
- In Chinese, the Linux app calls the trash 回收站 like the desktop does.

## [0.3.6] — 2026-10-18

### Changed
- Enter inserts what the preview shows — including fuzzy matches — and never rewrites the input; Tab completes (the highlighted suggestion, the grey hint, or a missing required option). In command mode an unambiguous prefix like `/sy` runs on Enter.
- Download in the update notice starts downloading the installer for your platform directly.
- Sync wording says "device" instead of "Mac"; code fields are left-aligned.
- About shows that Qantrip is made by NekroAI, with a link to nekro.ai.

### Fixed
- Windows: the template list no longer flickers or loses its scroll position when you pick a template or switch settings pages.
- Windows: aligned panel input, centered settings pages, Try preview inside its card, word wrap in the template body and source editors, brand purple instead of the system accent color.

## [0.3.5] — 2026-10-18

### Fixed
- Checking for updates works on networks where api.github.com is unreachable: the app asks qantrip.nekro.ai first (which caches the latest GitHub release) and falls back to GitHub. When the check fails, Settings › About links to the download page.

## [0.3.4] — 2026-10-18

### Fixed
- macOS: ⌘A, ⌘C, ⌘V, ⌘X, ⌘Z and other editing shortcuts work in the template library and settings.
- macOS: the DMG window is tall enough that the install hint isn't covered.
- macOS: prompts written to the clipboard during insertion stay on this Mac (not sent to other devices via Universal Clipboard).
- macOS: clipboard writes carry the `org.nspasteboard.source` marker, so clipboard managers that let you ignore apps can skip Qantrip.

### Changed
- macOS template library: icon Save button and Form / Source switch, cleaner sidebar and Try pane.
- Settings › Privacy explains that usage recording is local only and never uploaded or synced.

## [0.3.3] — 2026-10-18

### Added
- Windows installer (`Qantrip-Windows-x64-Setup.exe`): per-user, no admin rights, Start menu shortcut, optional launch at login, uninstall from Apps & features. The zip stays as a portable version.
- Update notice: Qantrip checks for a new release about once a day and shows it in the menu bar / tray menu and once as a notification; Settings › About has Check for Updates and Skip This Version. No automatic updates.
- Quick access from the panel: ⌘L / Ctrl+L opens the template library, ⌘, / Ctrl+, opens settings, plus two icons in the panel header.
- Recovery phrase: Copy and Save to File.

### Changed
- Settings and the template library are now one window: Settings is pinned at the bottom of the sidebar.
- macOS: a designed install window in the DMG, and opening the DMG no longer triggers a separate security prompt.

### Fixed
- Inserting into Qantrip's own windows (the welcome guide's practice field, the template editor) now works.
- The panel placeholder lines up with the caret.

## [0.3.2] — 2026-10-18

### Added
- Excluded apps: in apps you add to the list (Settings › General, or from the menu bar / tray menu), the shortcut isn't taken by Qantrip and reaches the app as usual.
- Settings › Sync shows cloud storage usage and the devices signed in to your account, with name, system, version and when each was last seen. Release a device from any other one, including a lost one, when the account's device limit is reached.

### Changed
- Sync uses far fewer requests: an idle check is a single request, checks follow a schedule from the server and happen when you open the panel, and automatic uploads of local edits are spaced out; Sync now uploads immediately. Sync pauses while the screen is locked or the computer sleeps.
- Signed-in sessions last 72 hours between token refreshes.

## [0.3.1] — 2026-10-18

### Added
- Delete any template, including built-in ones: `/rm name` or Delete in the template library. Deleting a built-in frees its name and aliases for your own templates; bring it back with `/restore name` or from the library's Deleted group. `/restore` also discards your changes to a built-in.

### Changed
- Windows: the default shortcut is now Ctrl+Q, the same as ⌃Q on macOS. Change it in Settings if another app needs Ctrl+Q.

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

[Unreleased]: https://github.com/NekroAI/qantrip/compare/v0.3.6...HEAD
[0.3.6]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.6
[0.3.5]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.5
[0.3.4]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.4
[0.3.3]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.3
[0.3.2]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.2
[0.3.1]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.1
[0.3.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.3.0
[0.2.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.2.0
[0.1.0]: https://github.com/NekroAI/qantrip/releases/tag/v0.1.0
