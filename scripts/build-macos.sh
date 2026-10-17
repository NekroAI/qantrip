#!/usr/bin/env bash
# 构建 macOS 应用：Rust 核心 → UniFFI Swift 绑定 → SwiftPM → 组装 .app → 签名。
#
# 环境变量：
#   ARCH                 arm64（默认，与本机一致）或 x86_64
#   QANTRIP_DEBUG_HOOKS  1 时编译调试钩子（自动化测试用），正式构建不要设置
#   SIGN_IDENTITY        签名证书名；默认使用本机开发证书 "Qantrip Dev"（scripts/dev-cert.sh）
#   SIGN_KEYCHAIN        签名证书所在钥匙串；与 SIGN_IDENTITY 一起设置
#   DIST                 输出目录，默认 dist/
# 产物：$DIST/Qantrip.app、$DIST/PasteProbe.app、$DIST/qantrip-debug
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORE="$ROOT/core"
MAC="$ROOT/apps/macos"
DIST="${DIST:-$ROOT/dist}"
export PATH="$HOME/.cargo/bin:$PATH"

HOST_ARCH="$(uname -m)"
ARCH="${ARCH:-$HOST_ARCH}"
case "$ARCH" in
  arm64)  RUST_TARGET=aarch64-apple-darwin ;;
  x86_64) RUST_TARGET=x86_64-apple-darwin ;;
  *) echo "不支持的 ARCH：$ARCH" >&2; exit 1 ;;
esac
VERSION="$(tr -d '[:space:]' < "$ROOT/VERSION")"
BUILD_NUMBER="$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 1)"
echo "==> Qantrip $VERSION ($BUILD_NUMBER) · $ARCH"

echo "==> Rust 核心"
cargo build --release --manifest-path "$CORE/Cargo.toml" --target "$RUST_TARGET"
LIBDIR="$CORE/target/$RUST_TARGET/release"

echo "==> UniFFI 绑定"
GEN="$(mktemp -d)"; trap 'rm -rf "$GEN"' EXIT
(cd "$CORE" && cargo run -q --release --features cli --bin uniffi-bindgen -- \
  generate --library "$LIBDIR/libqantrip_core.dylib" --language swift --out-dir "$GEN")
mkdir -p "$MAC/Sources/qantrip_coreFFI/include" "$MAC/Sources/QantripCore" "$MAC/Generated/lib"
cp "$GEN/qantrip_coreFFI.h" "$MAC/Sources/qantrip_coreFFI/include/"
cp "$GEN/qantrip_coreFFI.modulemap" "$MAC/Sources/qantrip_coreFFI/include/module.modulemap"
cp "$GEN/qantrip_core.swift" "$MAC/Sources/QantripCore/"
# 只放静态库，确保链接进可执行文件而不是依赖 dylib
cp "$LIBDIR/libqantrip_core.a" "$MAC/Generated/lib/"

echo "==> SwiftPM"
SWIFT_FLAGS=(--triple "$ARCH-apple-macosx14.0")
[ "${QANTRIP_DEBUG_HOOKS:-0}" = "1" ] && SWIFT_FLAGS+=(-Xswiftc -DQANTRIP_DEBUG_HOOKS)
swift build -c release --package-path "$MAC" "${SWIFT_FLAGS[@]}"
BIN="$(swift build -c release --package-path "$MAC" "${SWIFT_FLAGS[@]}" --show-bin-path)"

echo "==> 组装"
rm -rf "$DIST"; mkdir -p "$DIST"
bundle() { # <可执行文件名> <Info.plist>
  local app="$DIST/$1.app"
  mkdir -p "$app/Contents/MacOS"
  cp "$BIN/$1" "$app/Contents/MacOS/$1"
  cp "$2" "$app/Contents/Info.plist"
}
bundle Qantrip "$MAC/Resources/Info.plist"
mkdir -p "$DIST/Qantrip.app/Contents/Resources"
cp -R "$MAC/Resources/Fonts" "$DIST/Qantrip.app/Contents/Resources/"
cp "$MAC/Resources/AppIcon.icns" "$DIST/Qantrip.app/Contents/Resources/"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" -c "Set :CFBundleVersion $BUILD_NUMBER" \
  "$DIST/Qantrip.app/Contents/Info.plist"
bundle PasteProbe "$MAC/Resources/PasteProbe-Info.plist"
cp "$BIN/QantripDebug" "$DIST/qantrip-debug"

echo "==> 签名"
if [ -z "${SIGN_IDENTITY:-}" ]; then
  SIGN_IDENTITY="Qantrip Dev"
  SIGN_KEYCHAIN="$HOME/Library/Keychains/qantrip-dev.keychain-db"
  "$ROOT/scripts/dev-cert.sh" >/dev/null
  security unlock-keychain -p qantrip-dev "$SIGN_KEYCHAIN"
fi
for target in "$DIST/Qantrip.app" "$DIST/PasteProbe.app" "$DIST/qantrip-debug"; do
  codesign --force --sign "$SIGN_IDENTITY" ${SIGN_KEYCHAIN:+--keychain "$SIGN_KEYCHAIN"} "$target"
done
codesign -dvv "$DIST/Qantrip.app" 2>&1 | grep -E "^(Identifier|Authority)="
lipo -archs "$DIST/Qantrip.app/Contents/MacOS/Qantrip"
echo "完成：$DIST"
