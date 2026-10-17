# 应用图标

- `AppIcon-1024.png`：方案 A「魔杖 Q」原图（docs/decisions.md D21）。
- `render-app-icon.swift`：生成原图的脚本，说明见文件头部。

生成 `apps/macos/Resources/AppIcon.icns`：

```sh
mkdir AppIcon.iconset
for s in 16 32 128 256 512; do
  sips -z $s $s AppIcon-1024.png --out AppIcon.iconset/icon_${s}x${s}.png
  sips -z $((s*2)) $((s*2)) AppIcon-1024.png --out AppIcon.iconset/icon_${s}x${s}@2x.png
done
iconutil -c icns AppIcon.iconset -o ../../apps/macos/Resources/AppIcon.icns
```
