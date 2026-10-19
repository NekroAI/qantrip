# Install Nekro Qantrip

## macOS

Requires macOS 14 or later.

1. Download the DMG: `Qantrip-<version>-macOS-arm64.dmg` for Apple silicon (M series), `Qantrip-<version>-macOS-x64.dmg` for Intel.
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

Requires GTK 4.14+ and libadwaita 1.5+: Ubuntu 24.04, Debian 13, Fedora 40 or later. x64 and ARM64. Shortcut support is verified at runtime, not inferred from your desktop's name. See the [current validation matrix](linux-hotkeys-validation.md); older desktop tests are not evidence for this revision.

1. Download one of:
   - `Qantrip-<version>-Linux-x64.AppImage` / `Qantrip-<version>-Linux-arm64.AppImage` — works on most distributions. Make it executable (`chmod +x`, or Properties → Allow executing) and open it. On first launch it adds itself to your app menu.
   - `Qantrip-<version>-Linux-<arch>.deb` (Debian, Ubuntu) or `.rpm` (Fedora, openSUSE): install with your package manager, e.g. `sudo apt install ./Qantrip-<version>-Linux-x64.deb`.
2. Confirm the shortcut status in the welcome guide or Settings › General before pressing **Ctrl+Shift+Q**. Native X11 uses XGrabKey; Wayland (including an app running through Xwayland) tries the GlobalShortcuts Portal. Registration is only reported after binding and event subscription succeed. The system may ask for consent. Cancellation/refusal does not trigger another dialog automatically; use **Retry** or a manual binding below.
3. Auto-paste: on X11 it just works. On Wayland, allow it once in the welcome guide (the system asks for "remote interaction"; keep "remember" checked). Without it, prompts are copied and you press Ctrl+V.

Qantrip lives in the tray (on GNOME, the AppIndicator extension is needed for a tray icon; without it, open Qantrip from the app menu to reach the library and settings). Templates and settings are in `~/.local/share/qantrip` and `~/.config/qantrip`, kept across updates and uninstalls. Sync keys are stored in your keyring (Secret Service); without one they are kept in a file only you can read.

Known limits on Wayland: the clipboard keeps the prompt after pasting (it is restored on X11); excluded apps work on X11 and KDE Plasma, but not on GNOME, which doesn't tell apps which window is active.

The Wayland panel is a fixed-size window with GTK-managed decorations and an opaque background, without a maximized transparent backdrop or desktop-specific window rules. Its placement and stacking follow the compositor; identical positioning or permanent always-on-top behavior is not guaranteed. It closes on focus loss or Escape. X11 keeps its existing positioning and above-window hints.

### Linux shortcut setup and fallback

The Linux default is **Ctrl+Shift+Q**. Until registration or your manual binding succeeds, the foreground app still handles the keys and may quit or perform another action. Settings distinguishes checking, awaiting consent, cancellation/refusal, conflicts, disconnection and pause. System-side shortcut changes update the displayed trigger. Portal v2 can open the system configuration UI; v1 requires desktop settings. A missing Portal or failed binding offers a manual command on every desktop. Do not bind the same combination through both Portal and a custom command.

If a new build appears unchanged, quit the running instance before launching the new binary: GApplication forwards commands to the existing instance even when the command comes from another build. Building alone does not update an installed binary or a compositor binding. When Portal binding fails, setting a default in Qantrip does not create a niri/Sway/Hyprland binding; add the native binding below.

Saved custom shortcuts (including Ctrl+Q) and existing system Portal bindings are preserved on update. On X11, open **Settings › General › Panel shortcut** to record a new combination or restore Ctrl+Shift+Q. On Wayland, use **Change in System Settings…** for Portal bindings, or edit your custom/compositor binding. The Portal default is only a preference; the actual system-assigned combination is shown in Qantrip.

- **Native X11:** record a combination in Qantrip Settings. A failed replacement preserves the previous setting and any working old grab. CapsLock/NumLock positions are detected from the keyboard map. Recording and excluded apps temporarily release grabs.
- **GNOME Wayland:** try Portal first. If unavailable, add a custom shortcut in Settings → Keyboard → Custom Shortcuts (wording varies by release).
- **KDE Plasma Wayland:** try Portal first. Otherwise use System Settings → Keyboard → Shortcuts to add a command/application shortcut.
- **Hyprland, Sway, niri:** try Portal first; the compositor examples below work through the same `qantrip toggle` entry point. Settings and onboarding provide copyable guidance. Desktop names select guidance only, never a support verdict.

For installed packages, use `/usr/bin/qantrip toggle`. For AppImage or source builds, use the actual **absolute executable path**; do not use the temporary AppImage mount path. Settings uses `APPIMAGE` when available. Update manual bindings after moving or renaming the image. Keep quotes around paths with spaces:

```sh
'/home/you/Applications/My Qantrip.AppImage' toggle
```

**Sway** (`~/.config/sway/config`):

```text
bindsym --no-repeat Ctrl+Shift+q exec '/home/you/Applications/My Qantrip.AppImage' toggle
```

**niri** (`~/.config/niri/config.kdl`; `spawn` does not use a shell):

```kdl
binds {
    Ctrl+Shift+Q repeat=false { spawn "/home/you/Applications/My Qantrip.AppImage" "toggle"; }
}
```

**Hyprland 0.55+ (Lua configuration)**:

```lua
hl.bind("CTRL + SHIFT + Q", hl.dsp.exec_cmd("'/home/you/Applications/My Qantrip.AppImage' toggle"), { repeating = false })
```

For older **hyprlang** configuration (`~/.config/hypr/hyprland.conf`):

```ini
bind = CTRL SHIFT, Q, exec, '/home/you/Applications/My Qantrip.AppImage' toggle
```

Use plain `bind`, without the repeat flag `e`; do not add a second release binding. Remove conflicting bindings before reloading. Run `niri validate` or `sway --validate` as applicable, and check Hyprland's configuration errors. For paths containing config metacharacters such as `$`, `#`, quotes or backslashes, use a launcher script at a simple absolute path: put the shell-quoted `exec '/actual/path/My Qantrip.AppImage' toggle` in that script, and bind the script. niri's copied KDL escapes quotes/backslashes directly.

`toggle` is one command action, with **no physical release event**. Disable repeat in the compositor; do not bind it on both press and release. GApplication forwards commands to the existing process over the desktop session bus, or starts one primary instance when none exists. No UDS or second single-instance mechanism is used. Run it in the current graphical session; without a session bus, the GUI refuses to start. Do not automatically resend a timed-out `toggle`: its result may be uncertain. `qantrip show` is an idempotent alternative when you only want to open the panel.

GTK forwards available activation context. Without a valid compositor-issued token (for example from SSH), focus is not guaranteed. Portal pauses suppress Qantrip activation but cannot return the reserved key to excluded apps; manual compositor bindings also cannot be unregistered by Qantrip. Foreground app identification remains desktop-dependent.

Syntax references: [Sway manual](https://github.com/swaywm/sway/blob/master/sway/sway.5.scd), [niri bindings](https://niri-wm.github.io/niri/Configuration%3A-Key-Bindings.html), [Hyprland Lua](https://wiki.hypr.land/Configuring/Basics/Binds/), [Hyprland 0.54](https://wiki.hypr.land/0.54.0/Configuring/Binds/), [GlobalShortcuts Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html).

---

# 安装 Nekro Qantrip

## macOS

需要 macOS 14 或更新版本。

1. 下载安装包：Apple 芯片（M 系列）选 `Qantrip-<版本>-macOS-arm64.dmg`，Intel 芯片选 `Qantrip-<版本>-macOS-x64.dmg`。
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

需要 GTK 4.14+ 与 libadwaita 1.5+：Ubuntu 24.04、Debian 13、Fedora 40 或更新版本，支持 x64 与 ARM64。快捷键能力以运行时验证为准，不按桌面名称判断。参见[本次验证矩阵](linux-hotkeys-validation.md)，旧版本的桌面测试不代表当前改动已经通过验证。

1. 下载其中一种：
   - `Qantrip-<版本>-Linux-x64.AppImage` / `Qantrip-<版本>-Linux-arm64.AppImage`：适用于大多数发行版。设为可执行（`chmod +x`，或在文件属性中允许执行）后打开，首次运行会把自己加入应用菜单。
   - `Qantrip-<版本>-Linux-<架构>.deb`（Debian、Ubuntu）或 `.rpm`（Fedora、openSUSE）：用软件包管理器安装，例如 `sudo apt install ./Qantrip-<版本>-Linux-x64.deb`。
2. 先在引导或「设置 › 通用」确认快捷键状态，再按 **Ctrl+Shift+Q**。原生 X11 使用 XGrabKey；Wayland（包括应用通过 Xwayland 运行）优先尝试 GlobalShortcuts Portal。完成绑定和事件订阅后才显示成功，系统可能请求授权。取消或拒绝后不会自动再次弹窗，可以显式点「重试」或按下文手动绑定。
3. 自动粘贴：X11 下直接可用；Wayland 下在引导中授权一次（系统会询问“远程交互”，保持勾选“记住”）。不授权时 Prompt 会复制到剪贴板，按 Ctrl+V 粘贴。

Qantrip 常驻托盘（GNOME 需要 AppIndicator 扩展才显示托盘图标；没有时从应用菜单再次打开 Qantrip 即可进入模板库与设置）。模板与设置保存在 `~/.local/share/qantrip` 与 `~/.config/qantrip`，更新和卸载都会保留。同步密钥保存在系统密钥环（Secret Service）中；没有密钥环时保存在仅本用户可读的文件中。

Wayland 下的已知限制：粘贴后剪贴板中保留 Prompt（X11 下会恢复原内容）；排除的应用在 X11 与 KDE Plasma 上有效，GNOME 不向应用提供当前窗口，因此无法生效。

### 面板窗口

Wayland 下使用固定尺寸的普通窗口，由 GTK 客户端装饰管理圆角、阴影和窗口边界，不创建透明最大化背景，也不需要桌面专属窗口规则。面板主体使用不透明背景，避免底层文字透过；桌面自行设置的透明度、焦点边框等全局效果仍由桌面控制。

窗口位置、堆叠和强制平铺策略由合成器决定，因此不保证所有桌面都在相同坐标居中或永久置顶。面板失焦或按 Esc 时关闭。X11 保留现有的鼠标所在屏幕定位与置顶逻辑。

### Linux 快捷键配置与降级

Linux 默认快捷键为 **Ctrl+Shift+Q**；自动注册或手动绑定成功前，按键仍由当前应用处理，可能触发退出等操作。设置中会区分检测中、等待授权、取消/拒绝、冲突、断线和暂停；系统侧改键后会刷新显示。Portal v2 可直接打开配置界面，v1 需到桌面设置中修改。所有桌面在服务缺失或绑定失败时都提供手动入口；不要同时通过 Portal 和自定义命令绑定同一组合。

如果新构建看起来没有生效，先退出运行中的旧实例，再启动新二进制：不同路径的程序也会经 GApplication 把命令转发给现有实例。仅编译不会更新已安装文件或合成器绑定。Portal 绑定失败时，应用中的默认值不会自动创建 niri/Sway/Hyprland 快捷键，需要添加下文的原生绑定。

升级会保留已保存的自定义组合（包括 Ctrl+Q）和系统已有的 Portal 绑定。X11 可在**设置 › 通用 › 唤起快捷键**中录制新组合，或恢复默认 Ctrl+Shift+Q。Wayland 的 Portal 绑定通过**在系统设置中修改…**更改；手动绑定则修改桌面或合成器配置。Portal 的默认值仅为偏好，应用显示系统实际分配的组合。

- **原生 X11：** 在 Qantrip 设置中录制新组合。修改失败会保留原设置和仍可用的旧抓取；CapsLock/NumLock 位置从键盘映射识别。录制或进入排除的应用时临时解除抓取。
- **GNOME Wayland：** 先尝试 Portal；不可用时到「设置 → 键盘 → 自定义快捷键」添加命令，具体名称随版本变化。
- **KDE Plasma Wayland：** 先尝试 Portal；不可用时到「系统设置 → 键盘 → 快捷键」添加命令/应用快捷键。
- **Hyprland、Sway、niri：** 先尝试 Portal，失败后使用合成器原生绑定。设置和引导中有可复制说明；桌面名称仅用于选择指引。

安装包通常使用 `/usr/bin/qantrip toggle`；AppImage 或源码构建需填写实际可执行文件的**绝对路径**，不要使用 AppImage 临时挂载目录。设置页优先采用 `APPIMAGE` 路径；移动或重命名后更新手动配置。含空格的路径保留引号：

```sh
'/home/you/Applications/My Qantrip.AppImage' toggle
```

**Sway**（`~/.config/sway/config`）：

```text
bindsym --no-repeat Ctrl+Shift+q exec '/home/you/Applications/My Qantrip.AppImage' toggle
```

**Hyprland 0.55+ 的 Lua 配置**：

```lua
hl.bind("CTRL + SHIFT + Q", hl.dsp.exec_cmd("'/home/you/Applications/My Qantrip.AppImage' toggle"), { repeating = false })
```

旧版 **hyprlang**（`~/.config/hypr/hyprland.conf`）：

```ini
bind = CTRL SHIFT, Q, exec, '/home/you/Applications/My Qantrip.AppImage' toggle
```

使用普通 `bind`，不要添加重复标记 `e`，也不要再绑定释放动作。重新加载前移除冲突绑定，Sway 可运行 `sway --validate`，Hyprland 检查配置错误。路径包含 `$`、`#`、引号、反斜杠等配置特殊字符时，建议创建固定简单路径的启动脚本，脚本中填写经过 shell 引号转义的 `exec '/实际路径/My Qantrip.AppImage' toggle`，再绑定脚本；不要把一种配置语言的转义直接复制到另一种。

**niri**：

niri 会话中，安装了 GNOME Portal 并不代表全局快捷键可用：接口可能存在，但绑定请求仍会失败。Qantrip 会先尝试 Portal，失败后在设置和首次启动引导中提供可复制的 niri 配置，不再反复要求打开 GNOME/KDE 的键盘设置。

在 niri 配置文件（通常为 `~/.config/niri/config.kdl`）中添加：

```kdl
binds {
    Ctrl+Shift+Q repeat=false { spawn "/absolute/path/to/qantrip" "toggle"; }
}
```

把路径替换为实际二进制或 AppImage 的绝对路径；设置中的“复制配置”会自动使用当前路径。已有 `Ctrl+Shift+Q` 绑定时替换原绑定，或选择其他组合。`repeat=false` 避免长按时反复开关面板。运行 `niri validate` 检查配置，保存后按快捷键验证；成功前不要在有未保存内容的应用中测试 `Ctrl+Shift+Q`。

配置语法见 [niri 快捷键文档](https://niri-wm.github.io/niri/Configuration:-Key-Bindings.html)。

`toggle` 每次调用执行一次命令动作，**不提供物理释放事件**，所以要在合成器中关闭长按重复，不能同时绑定按下和释放。所有命令沿用 GApplication 经桌面会话 D-Bus 转发给现有进程；未启动时只产生一个主实例，不新增 UDS。应在当前图形会话中运行；无法连接会话总线时拒绝启动 GUI。命令超时后结果可能不确定，不要自动重发 `toggle`；只想打开面板时可使用幂等的 `qantrip show`。

GTK 会传递可用的激活上下文；没有合成器签发的有效令牌（如从 SSH 调用）时不保证取得焦点。Portal 暂停只能阻止 Qantrip 唤起，不能把系统已保留的按键还给排除的应用；Qantrip 也不能注销用户自己配置的合成器绑定。前台应用识别仍取决于桌面提供的能力。

语法依据：[Sway 官方手册](https://github.com/swaywm/sway/blob/master/sway/sway.5.scd)、[Hyprland Lua](https://wiki.hypr.land/Configuring/Basics/Binds/)、[Hyprland 0.54](https://wiki.hypr.land/0.54.0/Configuring/Binds/)、[GlobalShortcuts Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)。

### 从源码构建 Linux 版本

需要 Rust 工具链（支持 Rust 2024 edition）、C/C++ 构建工具、pkg-config、GTK 4.14+、libadwaita 1.5+ 和 Fontconfig 开发文件。Arch Linux 对应 `base-devel`、`rust`（或已有的 rustup 工具链）、`pkgconf`、`gtk4`、`libadwaita`、`fontconfig`；Ubuntu 24.04 对应 `build-essential`、`pkg-config`、`libgtk-4-dev`、`libadwaita-1-dev`、`libfontconfig-dev`，另行准备 Rust 工具链。

在仓库根目录执行：

```sh
cargo run --locked --manifest-path apps/linux/Cargo.toml
cargo test --locked --manifest-path apps/linux/Cargo.toml
cargo build --release --locked --manifest-path apps/linux/Cargo.toml
```

发布二进制位于 `apps/linux/target/release/qantrip`。只构建 Linux 外壳会自动编译共享 Rust 核心，无需构建 macOS、Windows 或云服务。开发二进制尚未加入 PATH 时，桌面快捷键需使用其绝对路径。

打包使用 `scripts/package-linux.sh`，产物在 `dist/linux/`。额外需要 `dpkg-deb`、ImageMagick，生成 RPM 还需要 `rpmbuild`；未指定 `APPIMAGETOOL` 时脚本会下载官方 appimagetool。面向发行的包应沿用 CI 的 Ubuntu 24.04 构建基线，避免在 Arch 上编译后依赖过新的系统库。
