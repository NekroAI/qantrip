# Install Nekro Qantrip

## macOS

Requires macOS 14 or later.

1. Download the DMG: `Qantrip-macOS-arm64.dmg` for Apple silicon (M series), `Qantrip-macOS-x86_64.dmg` for Intel.
2. Open it and drag **Qantrip** into Applications.
3. Qantrip isn't notarized by Apple yet, so macOS blocks the first launch: open **System Settings → Privacy & Security** and click **Open Anyway** at the bottom.
4. Follow the welcome guide to confirm the shortcut (⌃Q by default) and allow Accessibility so Qantrip can paste for you. Without it, prompts are copied and you paste them yourself.

To update, download the new version and replace the app; permissions, templates and settings are kept. If you use sync, macOS asks once after each update whether Qantrip may use its keychain item; choose **Always Allow**.

## Windows

Requires Windows 10 version 2004 or later, x64. Windows 11 recommended.

1. Download and run `Qantrip-Windows-x64-Setup.exe`. It installs for your account only (no administrator rights) into `%LOCALAPPDATA%\Programs\Nekro Qantrip` and adds a Start menu shortcut; you can choose to launch Qantrip when you sign in. The installer isn't code-signed yet, so SmartScreen may warn you: click **More info → Run anyway**.
2. Press **Ctrl+Q** anywhere. Qantrip lives in the notification area; right-click its icon for the template library, Settings and Quit.

To update, run the new installer; it closes the running Qantrip first. Qantrip tells you when a new version is out (Settings → About). Uninstall from **Settings → Apps → Installed apps**. Templates and settings are kept in `%APPDATA%\Qantrip` either way.

Prefer not to install? Download `Qantrip-Windows-x64.zip` instead, extract it anywhere and run `Qantrip\Qantrip.exe`; to update, quit Qantrip and replace the folder.

## Linux (preview)

Requires GTK 4.14+ and libadwaita 1.5+: Ubuntu 24.04, Debian 13, Fedora 40 or later. x86_64 and ARM64 (aarch64). Tested on GNOME and KDE Plasma (Wayland) and on X11; Sway, Hyprland and other wlroots desktops are best effort.

1. Download one of:
   - `Qantrip-Linux-x86_64.AppImage` / `Qantrip-Linux-aarch64.AppImage` — works on most distributions. Make it executable (`chmod +x`, or Properties → Allow executing) and open it. On first launch it adds itself to your app menu.
   - `Qantrip-Linux-<arch>.deb` (Debian, Ubuntu) or `Qantrip-Linux-<arch>.rpm` (Fedora, openSUSE): install with your package manager, e.g. `sudo apt install ./Qantrip-Linux-x86_64.deb`.
2. Press **Ctrl+Q** anywhere. On recent GNOME (tested with GNOME 50) and KDE Plasma (Wayland), the system asks you once to allow the shortcut. If your desktop can't register it, add a custom shortcut in your keyboard settings that runs `qantrip toggle` (Settings › General shows the exact command).
3. Auto-paste: on X11 it just works. On Wayland, allow it once in the welcome guide (the system asks for "remote interaction"; keep "remember" checked). Without it, prompts are copied and you press Ctrl+V.

Qantrip lives in the tray (on GNOME, the AppIndicator extension is needed for a tray icon; without it, open Qantrip from the app menu to reach the library and settings). Templates and settings are in `~/.local/share/qantrip` and `~/.config/qantrip`, kept across updates and uninstalls. Sync keys are stored in your keyring (Secret Service); without one they are kept in a file only you can read.

Known limits on Wayland: the clipboard keeps the prompt after pasting (it is restored on X11); excluded apps work on X11 and KDE Plasma, but not on GNOME, which doesn't tell apps which window is active.

---

# 安装 Nekro Qantrip

## macOS

需要 macOS 14 或更新版本。

1. 下载安装包：Apple 芯片（M 系列）选 `Qantrip-macOS-arm64.dmg`，Intel 芯片选 `Qantrip-macOS-x86_64.dmg`。
2. 打开 DMG，把 **Qantrip** 拖进「应用程序」。
3. Qantrip 尚未经过 Apple 公证，首次打开会被系统拦截：到 **系统设置 → 隐私与安全性**，在页面底部点 **仍要打开**。
4. 跟随引导确认唤起快捷键（默认 ⌃Q），并授予「辅助功能」权限以便自动粘贴；不授权也能用，Prompt 会复制到剪贴板。

更新时下载新版本覆盖即可，权限、模板与设置都会保留。开启同步后，每次更新系统会询问一次是否允许 Qantrip 使用它的钥匙串条目，选择 **始终允许**。

## Windows

需要 Windows 10 2004 或更新版本（x64），推荐 Windows 11。

1. 下载并运行 `Qantrip-Windows-x64-Setup.exe`。只为当前账户安装（不需要管理员权限），位置为 `%LOCALAPPDATA%\Programs\Nekro Qantrip`，并添加开始菜单快捷方式；可以选择登录 Windows 时启动。安装程序暂未签名，SmartScreen 可能拦截：点 **更多信息 → 仍要运行**。
2. 在任何地方按 **Ctrl+Q**。Qantrip 常驻在通知区域，右键图标可打开模板库、设置或退出。

更新时运行新版本的安装程序即可，它会先关闭正在运行的 Qantrip；有新版本时 Qantrip 会提示（设置 › 关于）。卸载在 **设置 → 应用 → 已安装的应用** 中进行。模板与设置保存在 `%APPDATA%\Qantrip`，更新和卸载都会保留。

不想安装？也可以下载 `Qantrip-Windows-x64.zip`，解压到任意位置后运行 `Qantrip\Qantrip.exe`；更新时退出 Qantrip，替换整个文件夹即可。

## Linux（预览版）

需要 GTK 4.14+ 与 libadwaita 1.5+：Ubuntu 24.04、Debian 13、Fedora 40 或更新版本，支持 x86_64 与 ARM64（aarch64）。已在 GNOME、KDE Plasma（Wayland）与 X11 上测试；Sway、Hyprland 等 wlroots 桌面尽力支持。

1. 下载其中一种：
   - `Qantrip-Linux-x86_64.AppImage` / `Qantrip-Linux-aarch64.AppImage`：适用于大多数发行版。设为可执行（`chmod +x`，或在文件属性中允许执行）后打开，首次运行会把自己加入应用菜单。
   - `Qantrip-Linux-<架构>.deb`（Debian、Ubuntu）或 `Qantrip-Linux-<架构>.rpm`（Fedora、openSUSE）：用软件包管理器安装，例如 `sudo apt install ./Qantrip-Linux-x86_64.deb`。
2. 在任何地方按 **Ctrl+Q**。较新的 GNOME（已在 GNOME 50 上测试）与 KDE Plasma（Wayland）会请你确认一次快捷键；桌面不支持应用注册快捷键时，在系统的键盘设置中添加一个自定义快捷键，命令为 `qantrip toggle`（设置 › 通用中有完整命令）。
3. 自动粘贴：X11 下直接可用；Wayland 下在引导中授权一次（系统会询问“远程交互”，保持勾选“记住”）。不授权时 Prompt 会复制到剪贴板，按 Ctrl+V 粘贴。

Qantrip 常驻托盘（GNOME 需要 AppIndicator 扩展才显示托盘图标；没有时从应用菜单再次打开 Qantrip 即可进入模板库与设置）。模板与设置保存在 `~/.local/share/qantrip` 与 `~/.config/qantrip`，更新和卸载都会保留。同步密钥保存在系统密钥环（Secret Service）中；没有密钥环时保存在仅本用户可读的文件中。

Wayland 下的已知限制：粘贴后剪贴板中保留 Prompt（X11 下会恢复原内容）；排除的应用在 X11 与 KDE Plasma 上有效，GNOME 不向应用提供当前窗口，因此无法生效。
