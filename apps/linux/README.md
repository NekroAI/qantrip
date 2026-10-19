# Qantrip for Linux

GTK4 / libadwaita shell on the shared Rust core (docs/decisions.md D37). Installation for users: [docs/install.md](../../docs/install.md); shortcut setup: [docs/linux-shortcuts.md](../../docs/linux-shortcuts.md).

## Build from source

Needs a Rust toolchain (edition 2024), C/C++ build tools, pkg-config, and development files for GTK 4.14+, libadwaita 1.5+ and Fontconfig.

- Ubuntu 24.04: `build-essential pkg-config libgtk-4-dev libadwaita-1-dev libfontconfig-dev`, plus a Rust toolchain.
- Arch Linux: `base-devel rust pkgconf gtk4 libadwaita fontconfig`.

From the repository root:

```sh
cargo run --locked --manifest-path apps/linux/Cargo.toml
cargo test --locked --manifest-path apps/linux/Cargo.toml
cargo build --release --locked --manifest-path apps/linux/Cargo.toml
```

The release binary is `apps/linux/target/release/qantrip`; the shared core is built along with it.

Packaging: `scripts/package-linux.sh` writes AppImage, .deb and .rpm to `dist/linux/` (needs `dpkg-deb`, ImageMagick and `rpmbuild`; appimagetool is downloaded unless `APPIMAGETOOL` is set). Release packages are built on Ubuntu 24.04 in CI so they don't depend on newer system libraries.

## Development notes

- **Single instance:** Qantrip is a GApplication. `qantrip toggle` (or `show`) is forwarded over the session bus to the running instance, even from a different build — quit the old instance before trying a new binary.
- **Global shortcuts:** `src/hotkey.rs` coordinates three backends: X11 (`XGrabKey`, `src/x11.rs`), the GlobalShortcuts portal (`src/hotkey/portal.rs`) and manual bindings. A shortcut is only reported as registered after the binding and its event subscription both succeed (D41). Validation record: [docs/linux-hotkeys-validation.md](../../docs/linux-hotkeys-validation.md).
- **Tests:** `cargo test` includes a fake GlobalShortcuts portal on a private D-Bus (needs `dbus-daemon`). `uv run tests/linux/hotkeys_smoke.py` starts an isolated KWin / Xwayland session (needs `kwin_wayland`, Xwayland, `dbus-daemon`) to test real X grabs and CLI forwarding.
- **Debug hooks:** build with `--features debug-hooks`, run with `QANTRIP_DEBUG_HOOKS=1`, then use `qantrip debug …` (see `src/debug.rs`).
