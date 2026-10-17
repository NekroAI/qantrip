#!/usr/bin/env bash
# 构建 macOS Spike：Rust 核心 → UniFFI Swift 绑定 → SwiftPM → 组装 .app → 签名。
# 产物：dist/Qantrip.app、dist/PasteProbe.app、dist/qantrip-debug
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORE="$ROOT/core"
MAC="$ROOT/apps/macos"
DIST="$ROOT/dist"
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> Rust 核心"
cargo build --release --manifest-path "$CORE/Cargo.toml"
LIBDIR="$CORE/target/release"

echo "==> UniFFI 绑定"
GEN="$(mktemp -d)"; trap 'rm -rf "$GEN"' EXIT
(cd "$CORE" && cargo run -q --release --features cli --bin uniffi-bindgen -- \
  generate --library "$LIBDIR/libqantrip_core.dylib" --language swift --out-dir "$GEN")
mkdir -p "$MAC/Sources/qantrip_coreFFI/include" "$MAC/Generated/lib"
cp "$GEN/qantrip_coreFFI.h" "$MAC/Sources/qantrip_coreFFI/include/"
cp "$GEN/qantrip_coreFFI.modulemap" "$MAC/Sources/qantrip_coreFFI/include/module.modulemap"
cp "$GEN/qantrip_core.swift" "$MAC/Sources/QantripCore/"
# 只放静态库，确保链接进可执行文件而不是依赖 dylib
cp "$LIBDIR/libqantrip_core.a" "$MAC/Generated/lib/"

echo "==> SwiftPM"
swift build -c release --package-path "$MAC"
BIN="$(swift build -c release --package-path "$MAC" --show-bin-path)"

echo "==> 组装"
rm -rf "$DIST"; mkdir -p "$DIST"
bundle() { # <可执行文件名> <Info.plist>
  local app="$DIST/$1.app"
  mkdir -p "$app/Contents/MacOS"
  cp "$BIN/$1" "$app/Contents/MacOS/$1"
  cp "$2" "$app/Contents/Info.plist"
}
bundle Qantrip "$MAC/Resources/Info.plist"
bundle PasteProbe "$MAC/Resources/PasteProbe-Info.plist"
cp "$BIN/QantripDebug" "$DIST/qantrip-debug"

echo "==> 签名"
KC="$HOME/Library/Keychains/qantrip-dev.keychain-db"
"$ROOT/scripts/dev-cert.sh" >/dev/null
security unlock-keychain -p qantrip-dev "$KC"
for target in "$DIST/Qantrip.app" "$DIST/PasteProbe.app" "$DIST/qantrip-debug"; do
  codesign --force --sign "Qantrip Dev" --keychain "$KC" "$target"
done
codesign -dv "$DIST/Qantrip.app" 2>&1 | grep -E "^(Identifier|Authority)"
echo "完成：$DIST"
