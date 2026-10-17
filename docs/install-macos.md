# 在 macOS 上安装 Nekro Qantrip

需要 macOS 14 或更新版本。

## 1. 选择安装包

| 你的 Mac | 下载 |
|---|---|
| Apple 芯片（M1 及以后） | `Qantrip-<版本>-arm64.dmg` |
| Intel 芯片 | `Qantrip-<版本>-x86_64.dmg` |

不确定时：苹果菜单 → 关于本机，「芯片」一栏写着 Apple M… 即为 Apple 芯片。

## 2. 安装

打开 DMG，把 **Qantrip** 拖到「应用程序」。

## 3. 首次打开

Qantrip 目前使用项目自己的证书签名，没有经过 Apple 公证，所以第一次打开会被系统拦截：

1. 在「应用程序」里双击 Qantrip，系统提示无法验证开发者，点「完成」。
2. 打开 **系统设置 → 隐私与安全性**，在页面底部找到关于 Qantrip 的提示，点 **仍要打开**，再确认一次。

之后就能正常启动。不要为此关闭 Gatekeeper 等系统安全设置。

用 GitHub CLI 下载的安装包不会被标记为来自网络，可以跳过这一步：

```sh
gh release download <版本标签> --repo NekroAI/qantrip --pattern '*arm64.dmg'
```

## 4. 引导与权限

首次启动会出现引导：确认唤起快捷键（默认 ⌃Q），然后授予 **辅助功能** 权限，Qantrip 就能替你按下 ⌘V，把 Prompt 直接插入当前应用。

不想授权也可以：选择跳过后，Prompt 会复制到剪贴板，由你手动粘贴。

## 5. 更新

下载新版本的 DMG，覆盖「应用程序」中的 Qantrip 即可。所有版本使用同一张签名证书，更新后辅助功能权限保持有效，模板与设置也会保留。

模板保存在 `~/Library/Application Support/Qantrip/templates/`，可以直接用编辑器修改。
