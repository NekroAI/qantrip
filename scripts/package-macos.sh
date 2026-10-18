#!/usr/bin/env bash
# 正式打包：不含调试钩子的构建 + DMG（Qantrip.app 与「应用程序」快捷方式）。
#   ARCH=arm64|x86_64 SIGN_IDENTITY=… SIGN_KEYCHAIN=… scripts/package-macos.sh
# 产物：dist/release/Qantrip-<版本>-macOS-<arm64|x64>.dmg
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCH="${ARCH:-$(uname -m)}"
VERSION="$(tr -d '[:space:]' < "$ROOT/VERSION")"
OUT="$ROOT/dist/release"
STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT

unset QANTRIP_DEBUG_HOOKS
DIST="$STAGE/build" ARCH="$ARCH" "$ROOT/scripts/build-macos.sh"

mkdir -p "$OUT"
# 发布文件名统一为 Qantrip-<版本>-<系统>-<arm64|x64>.<扩展名>（docs/decisions.md D38）
case "$ARCH" in arm64) LABEL=arm64 ;; x86_64) LABEL=x64 ;; *) echo "不支持的架构：$ARCH" >&2; exit 1 ;; esac
DMG="$OUT/Qantrip-$VERSION-macOS-$LABEL.dmg"
rm -f "$DMG"
# 安装窗口（背景、图标位置）由 dmgbuild 直接写入 .DS_Store，不依赖 Finder，CI 中也可用
DMGBUILD="${DMGBUILD:-dmgbuild}"
"$DMGBUILD" -s "$ROOT/apps/macos/dmg/settings.py" -D app="$STAGE/build/Qantrip.app" -D background="$ROOT/apps/macos/dmg/background.tiff" "Nekro Qantrip $VERSION" "$DMG" >/dev/null
# DMG 本身不签名：没有 Developer ID 时，带不受信任签名的磁盘映像在打开时也会被 Gatekeeper 拦截一次；
# 应用本体已用发布证书签名，只在首次打开应用时需要确认一次
shasum -a 256 "$DMG"
echo "完成：$DMG"
