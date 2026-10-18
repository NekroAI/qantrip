#!/usr/bin/env bash
# 打包 Linux 版（docs/decisions.md D37）：AppImage、.deb、.rpm，架构为本机架构（x86_64 / aarch64）。
#
#   scripts/package-linux.sh
#
# 产物：dist/linux/Qantrip-<版本>-Linux-<arm64|x64>.{AppImage,deb,rpm}
# 依赖：Rust 工具链、libgtk-4-dev（≥ 4.14）、libadwaita-1-dev（≥ 1.5）、dpkg-deb、ImageMagick；
#       rpmbuild 可选（没有时跳过 .rpm）；appimagetool 可选（环境变量 APPIMAGETOOL 指定，没有时下载官方版本）。
#
# AppImage 不内置 GTK / libadwaita：使用系统的版本，系统的输入法模块（IBus / Fcitx5）、主题与门户才能正常工作。
# 因此三种包都要求系统提供 GTK 4.14+ 与 libadwaita 1.5+（Ubuntu 24.04、Debian 13、Fedora 40 及更新版本）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(tr -d '[:space:]' < VERSION)"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64; LABEL=x64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64; LABEL=arm64 ;;
  *) echo "不支持的架构：$ARCH" >&2; exit 1 ;;
esac

OUT="$ROOT/dist/linux"
STAGE="$OUT/stage"
# 发布文件名统一为 Qantrip-<版本>-<系统>-<arm64|x64>.<扩展名>（docs/decisions.md D38）
NAME="Qantrip-$VERSION-Linux-$LABEL"
rm -rf "$STAGE" "$OUT/AppDir" "$OUT/rpmbuild"
mkdir -p "$OUT"

echo "==> 构建 qantrip $VERSION（$ARCH）"
cargo build --release --locked --manifest-path apps/linux/Cargo.toml
BIN="apps/linux/target/release/qantrip"

echo "==> 组装文件"
install -Dm755 "$BIN" "$STAGE/usr/bin/qantrip"
install -Dm644 apps/linux/resources/ai.nekro.qantrip.desktop "$STAGE/usr/share/applications/ai.nekro.qantrip.desktop"
install -Dm644 apps/linux/resources/ai.nekro.qantrip.metainfo.xml "$STAGE/usr/share/metainfo/ai.nekro.qantrip.metainfo.xml"
if command -v magick >/dev/null; then IM=(magick); else IM=(convert); fi
for s in 16 24 32 48 64 128 256 512; do
  d="$STAGE/usr/share/icons/hicolor/${s}x${s}/apps"
  mkdir -p "$d"
  "${IM[@]}" tools/icon/AppIcon-1024.png -resize "${s}x${s}" "$d/ai.nekro.qantrip.png"
done
# 应用自带的符号图标（魔杖、托盘）与字体（D17，SIL OFL 1.1）
mkdir -p "$STAGE/usr/share/qantrip"
cp -R apps/linux/resources/icons "$STAGE/usr/share/qantrip/icons"
install -Dm644 apps/macos/Resources/Fonts/MonaspaceNeonVar.ttf "$STAGE/usr/share/qantrip/fonts/MonaspaceNeonVar.ttf"
install -Dm644 apps/macos/Resources/Fonts/Fraunces.ttf "$STAGE/usr/share/qantrip/fonts/Fraunces.ttf"
install -Dm644 apps/macos/Resources/Fonts/LICENSE-Monaspace.txt "$STAGE/usr/share/qantrip/fonts/LICENSE-Monaspace.txt"
install -Dm644 apps/macos/Resources/Fonts/LICENSE-Fraunces.txt "$STAGE/usr/share/qantrip/fonts/LICENSE-Fraunces.txt"
install -Dm644 LICENSE "$STAGE/usr/share/doc/qantrip/copyright"
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/ai.nekro.qantrip.desktop"
fi

SUMMARY="Insert parameterized prompts anywhere with a shortcut"
DESCRIPTION="Press Ctrl+Q, type a few letters and insert a ready-made prompt into the app you are using. Templates are plain Markdown files with parameters; sync between devices is end-to-end encrypted."

echo "==> .deb"
DEB="$OUT/debroot"
rm -rf "$DEB"
cp -a "$STAGE" "$DEB"
mkdir -p "$DEB/DEBIAN"
SIZE="$(du -sk "$DEB/usr" | cut -f1)"
cat > "$DEB/DEBIAN/control" <<EOF
Package: qantrip
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: NekroAI <https://github.com/NekroAI/qantrip>
Installed-Size: $SIZE
Depends: libc6 (>= 2.39), libgtk-4-1 (>= 4.14), libadwaita-1-0 (>= 1.5), libglib2.0-0t64 (>= 2.80) | libglib2.0-0 (>= 2.80), libfontconfig1
Recommends: xdg-desktop-portal
Suggests: gnome-shell-extension-appindicator, wtype
Section: utils
Priority: optional
Homepage: https://qantrip.nekro.ai
Description: $SUMMARY
 $DESCRIPTION
EOF
dpkg-deb --root-owner-group --build "$DEB" "$OUT/$NAME.deb" >/dev/null
rm -rf "$DEB"

if command -v rpmbuild >/dev/null; then
  echo "==> .rpm"
  TOP="$OUT/rpmbuild"
  mkdir -p "$TOP"/{BUILD,RPMS,SPECS,SOURCES,BUILDROOT}
  cat > "$TOP/SPECS/qantrip.spec" <<EOF
Name: qantrip
Version: $VERSION
Release: 1
Summary: $SUMMARY
License: GPL-3.0-or-later
URL: https://qantrip.nekro.ai
Requires: gtk4 >= 4.14
Requires: libadwaita >= 1.5
AutoProv: no
%define _build_id_links none
%define __strip /bin/true

%description
$DESCRIPTION

%install
mkdir -p %{buildroot}
cp -a $STAGE/. %{buildroot}/

%files
/usr/bin/qantrip
/usr/share/applications/ai.nekro.qantrip.desktop
/usr/share/metainfo/ai.nekro.qantrip.metainfo.xml
/usr/share/icons/hicolor/*/apps/ai.nekro.qantrip.png
/usr/share/qantrip
%doc /usr/share/doc/qantrip
EOF
  rpmbuild --quiet --define "_topdir $TOP" --target "$ARCH" -bb "$TOP/SPECS/qantrip.spec"
  cp "$TOP"/RPMS/*/qantrip-"$VERSION"-1.*.rpm "$OUT/$NAME.rpm"
  rm -rf "$TOP"
else
  echo "（没有 rpmbuild，跳过 .rpm）"
fi

echo "==> AppImage"
APPDIR="$OUT/AppDir"
cp -a "$STAGE" "$APPDIR"
cp "$STAGE/usr/share/applications/ai.nekro.qantrip.desktop" "$APPDIR/"
cp "$STAGE/usr/share/icons/hicolor/256x256/apps/ai.nekro.qantrip.png" "$APPDIR/ai.nekro.qantrip.png"
ln -sf ai.nekro.qantrip.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
# AppImage 入口：资源目录指向镜像内部，GTK 与 libadwaita 使用系统的版本
HERE="$(dirname "$(readlink -f "$0")")"
export QANTRIP_DATA_DIR="$HERE/usr/share/qantrip"
export XDG_DATA_DIRS="$HERE/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$HERE/usr/bin/qantrip" "$@"
EOF
chmod +x "$APPDIR/AppRun"
TOOL="${APPIMAGETOOL:-}"
if [ -z "$TOOL" ]; then
  TOOL="$OUT/appimagetool-$ARCH.AppImage"
  if [ ! -x "$TOOL" ]; then
    curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$TOOL"
  fi
fi
# 容器 / CI 中通常没有 FUSE：解包后运行
ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT/$NAME.AppImage" >/dev/null
rm -rf "$APPDIR" "$STAGE"

echo "==> 完成"
ls -l "$OUT"/Qantrip-"$VERSION"-Linux-*
