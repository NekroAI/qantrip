#!/usr/bin/env bash
# 正式打包：不含调试钩子的构建 + DMG（Qantrip.app 与「应用程序」快捷方式）。
#   ARCH=arm64|x86_64 SIGN_IDENTITY=… SIGN_KEYCHAIN=… scripts/package-macos.sh
# 产物：dist/release/Qantrip-<版本>-<架构>.dmg
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCH="${ARCH:-$(uname -m)}"
VERSION="$(tr -d '[:space:]' < "$ROOT/VERSION")"
OUT="$ROOT/dist/release"
STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT

unset QANTRIP_DEBUG_HOOKS
DIST="$STAGE/build" ARCH="$ARCH" "$ROOT/scripts/build-macos.sh"

mkdir -p "$STAGE/dmg" "$OUT"
cp -R "$STAGE/build/Qantrip.app" "$STAGE/dmg/"
ln -s /Applications "$STAGE/dmg/应用程序"
DMG="$OUT/Qantrip-$VERSION-$ARCH.dmg"
rm -f "$DMG"
hdiutil create -volname "Nekro Qantrip $VERSION" -srcfolder "$STAGE/dmg" -fs HFS+ -format UDZO -ov "$DMG" >/dev/null
if [ -n "${SIGN_IDENTITY:-}" ]; then
  codesign --force --sign "$SIGN_IDENTITY" ${SIGN_KEYCHAIN:+--keychain "$SIGN_KEYCHAIN"} "$DMG"
fi
shasum -a 256 "$DMG"
echo "完成：$DMG"
