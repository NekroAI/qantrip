# dmgbuild 配置：DMG 打开后的安装窗口（背景、图标位置、窗口大小）。
# 由 scripts/package-macos.sh 调用：dmgbuild -s settings.py -D app=<Qantrip.app 路径> -D background=<background.tiff 路径> "<卷名>" <输出.dmg>
import os.path

app = defines["app"]  # noqa: F821  dmgbuild 注入

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon_locations = {os.path.basename(app): (170, 190), "Applications": (490, 190)}

background = defines["background"]  # noqa: F821
window_rect = ((200, 140), (660, 472))
default_view = "icon-view"
show_toolbar = False
show_status_bar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False
icon_size = 112
text_size = 13
arrange_by = None
hide_extensions = [os.path.basename(app)]
