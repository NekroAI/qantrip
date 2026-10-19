# Linux 快捷键

默认唤起快捷键是 **Ctrl+Shift+Q**（Ctrl+Q 在许多 Linux 应用中是"退出"，快捷键没有注册成功时按下它会关掉当前应用）。

## Qantrip 怎样注册快捷键

- **X11：** Qantrip 直接注册，不需要任何操作。
- **Wayland：** 通过系统的全局快捷键服务（xdg-desktop-portal GlobalShortcuts）注册。GNOME 与 KDE Plasma 会请你确认一次，之后由系统记住。
- 注册成功之前，按键仍由当前应用处理。首次引导和「设置 › 通用」会显示当前状态：已注册、等待确认、已取消或失败。取消或失败后不会反复弹窗，需要时点「重试」。

## 修改快捷键

- **X11：** 在「设置 › 通用 › 唤起快捷键」中录制新组合；也可以恢复默认值。新组合注册失败时保留原来的设置。
- **Wayland（GNOME、KDE）：** 快捷键由系统管理，点「在系统设置中修改…」，或到系统的键盘设置中修改。
- 升级不会改动你已保存的组合（包括 Ctrl+Q）或系统中已有的绑定。

## 手动绑定

桌面不支持应用注册快捷键时（niri、Sway、Hyprland 以及缺少相应服务的桌面），在桌面或合成器中添加一个快捷键，命令为 `qantrip toggle`。「设置 › 通用」会按你的桌面给出可以直接复制的配置，并自动使用当前程序的实际路径。

- deb / rpm 安装的命令是 `/usr/bin/qantrip toggle`；AppImage 要写它的**绝对路径**（不要用运行时的临时挂载路径），移动或改名后需要更新。路径中有空格时保留引号。
- 关闭长按重复，也不要同时绑定按下和松开：`toggle` 每次调用就切换一次面板。
- 不要既让系统注册、又手动绑定同一个组合。

**GNOME：** 设置 → 键盘 → 自定义快捷键，添加命令。

**KDE Plasma：** 系统设置 → 键盘 → 快捷键，添加命令。

**Sway**（`~/.config/sway/config`）：

```text
bindsym --no-repeat Ctrl+Shift+q exec '/home/you/Applications/Qantrip.AppImage' toggle
```

**niri**（`~/.config/niri/config.kdl`，可用 `niri validate` 检查）：

```kdl
binds {
    Ctrl+Shift+Q repeat=false { spawn "/home/you/Applications/Qantrip.AppImage" "toggle"; }
}
```

niri 会话中即使装有 GNOME 的门户服务，全局快捷键也可能不可用，这时使用上面的绑定。

**Hyprland 0.55+**（Lua 配置）：

```lua
hl.bind("CTRL + SHIFT + Q", hl.dsp.exec_cmd("'/home/you/Applications/Qantrip.AppImage' toggle"), { repeating = false })
```

旧版配置（`~/.config/hypr/hyprland.conf`，用普通 `bind`，不要加重复标记 `e`）：

```ini
bind = CTRL SHIFT, Q, exec, '/home/you/Applications/Qantrip.AppImage' toggle
```

路径中含有 `$`、`#`、引号或反斜杠时，把命令写进一个路径简单的启动脚本，再绑定这个脚本。

## 其他说明

- `qantrip show` 只打开面板（已打开时不会关闭）。
- 从 SSH 等没有图形会话激活信息的地方执行命令时，面板不一定能取得焦点。
- Wayland 下的面板：GNOME 与 KDE 上显示在屏幕上方；其他合成器由合成器决定位置（平铺式合成器可能会让它浮动或按自己的规则放置）。

---

# Linux shortcuts

The default shortcut is **Ctrl+Shift+Q** (Ctrl+Q quits many Linux apps, so pressing it while the shortcut isn't registered would close the app you're in).

## How Qantrip registers it

- **X11:** registered directly, nothing to do.
- **Wayland:** registered through the system's global shortcuts service (xdg-desktop-portal GlobalShortcuts). GNOME and KDE Plasma ask you once and remember your answer.
- Until it is registered, the keys go to the app you're in. The welcome guide and Settings › General show the status: registered, waiting for your consent, cancelled or failed. Qantrip doesn't ask again on its own after you cancel or it fails; click Retry when you want to.

## Changing it

- **X11:** record a new combination in Settings › General › Panel shortcut, or restore the default. If the new one can't be registered, the old setting stays.
- **Wayland (GNOME, KDE):** the system owns the shortcut. Click "Change in System Settings…" or change it in your keyboard settings.
- Updates keep any combination you saved (including Ctrl+Q) and any existing system binding.

## Binding it yourself

When your desktop doesn't let apps register shortcuts (niri, Sway, Hyprland, or a desktop without the service), add a shortcut there that runs `qantrip toggle`. Settings › General gives a ready-to-copy binding for your desktop, with the app's actual path filled in.

- deb / rpm installs use `/usr/bin/qantrip toggle`. For an AppImage use its **absolute path** (not the temporary mount path while it runs), and update it after moving or renaming the file. Keep quotes around paths with spaces.
- Turn off key repeat and don't bind both press and release: each `toggle` switches the panel once.
- Don't let the system register a combination and also bind the same one yourself.

**GNOME:** Settings → Keyboard → Custom Shortcuts, add the command.

**KDE Plasma:** System Settings → Keyboard → Shortcuts, add a command.

**Sway** (`~/.config/sway/config`):

```text
bindsym --no-repeat Ctrl+Shift+q exec '/home/you/Applications/Qantrip.AppImage' toggle
```

**niri** (`~/.config/niri/config.kdl`; check it with `niri validate`):

```kdl
binds {
    Ctrl+Shift+Q repeat=false { spawn "/home/you/Applications/Qantrip.AppImage" "toggle"; }
}
```

In a niri session, global shortcuts may be unavailable even with GNOME's portal installed; use the binding above.

**Hyprland 0.55+** (Lua configuration):

```lua
hl.bind("CTRL + SHIFT + Q", hl.dsp.exec_cmd("'/home/you/Applications/Qantrip.AppImage' toggle"), { repeating = false })
```

Older configuration (`~/.config/hypr/hyprland.conf`; plain `bind`, without the repeat flag `e`):

```ini
bind = CTRL SHIFT, Q, exec, '/home/you/Applications/Qantrip.AppImage' toggle
```

If the path contains `$`, `#`, quotes or backslashes, put the command in a launcher script at a simple path and bind the script.

## Notes

- `qantrip show` only opens the panel (it doesn't close an open one).
- Run from somewhere without graphical activation info, such as SSH, the panel may not get focus.
- The Wayland panel appears near the top of the screen on GNOME and KDE; elsewhere the compositor decides where it goes (tiling compositors may float it or place it by their own rules).
