# Install Nekro Qantrip

## macOS

Requires macOS 14 or later.

1. Download the DMG: `Qantrip-macOS-arm64.dmg` for Apple silicon (M series), `Qantrip-macOS-x86_64.dmg` for Intel.
2. Open it and drag **Qantrip** into Applications.
3. Qantrip isn't notarized by Apple yet, so macOS blocks the first launch: open **System Settings → Privacy & Security** and click **Open Anyway** at the bottom.
4. Follow the welcome guide to confirm the shortcut (⌃Q by default) and allow Accessibility so Qantrip can paste for you. Without it, prompts are copied and you paste them yourself.

To update, download the new version and replace the app; permissions, templates and settings are kept. If you use sync, macOS asks once after each update whether Qantrip may use its keychain item; choose **Always Allow**.

## Windows (preview)

Requires Windows 10 version 2004 or later, x64. Windows 11 recommended.

1. Download `Qantrip-Windows-x64.zip` and extract it, for example into your Documents folder.
2. Run `Qantrip\Qantrip.exe`. It isn't code-signed yet, so SmartScreen may warn you: click **More info → Run anyway**.
3. Press **Alt+Q** anywhere. Qantrip lives in the notification area; right-click its icon for Settings, the template library and Quit. Turn on **Launch at login** in Settings if you like.

To update, quit Qantrip and replace the folder. Templates and settings are kept in `%APPDATA%\Qantrip`.

---

# 安装 Nekro Qantrip

## macOS

需要 macOS 14 或更新版本。

1. 下载安装包：Apple 芯片（M 系列）选 `Qantrip-macOS-arm64.dmg`，Intel 芯片选 `Qantrip-macOS-x86_64.dmg`。
2. 打开 DMG，把 **Qantrip** 拖进「应用程序」。
3. Qantrip 尚未经过 Apple 公证，首次打开会被系统拦截：到 **系统设置 → 隐私与安全性**，在页面底部点 **仍要打开**。
4. 跟随引导确认唤起快捷键（默认 ⌃Q），并授予「辅助功能」权限以便自动粘贴；不授权也能用，Prompt 会复制到剪贴板。

更新时下载新版本覆盖即可，权限、模板与设置都会保留。开启同步后，每次更新系统会询问一次是否允许 Qantrip 使用它的钥匙串条目，选择 **始终允许**。

## Windows（预览版）

需要 Windows 10 2004 或更新版本（x64），推荐 Windows 11。

1. 下载 `Qantrip-Windows-x64.zip` 并解压，例如解压到「文档」。
2. 运行 `Qantrip\Qantrip.exe`。程序暂未签名，SmartScreen 可能拦截：点 **更多信息 → 仍要运行**。
3. 在任何地方按 **Alt+Q**。Qantrip 常驻在通知区域，右键图标可打开设置、模板库或退出；需要时在设置中开启「登录时启动」。

更新时退出 Qantrip，替换整个文件夹即可；模板与设置保存在 `%APPDATA%\Qantrip`。
