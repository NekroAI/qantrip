# Linux 快捷键验证记录 / Linux hotkey validation

日期：2026-10-19。范围：本次 D41 实现及 D42 默认快捷键调整。D37 的旧版 GNOME/KDE/X11 实测不计入本次结果。

本机环境：Linux x64、Rust 1.99.1、GTK 4.22.4、libadwaita 1.9.3、KWin 6.7.4、niri 26.04、Hyprland 0.56.2。未修改共享 core、其他平台或云端，未升级现有依赖版本。

## 自动化结果

| 检查 | 本次结果 |
|---|---|
| `cargo check --locked --manifest-path apps/linux/Cargo.toml` | 通过 |
| `cargo check --locked --manifest-path apps/linux/Cargo.toml --all-features` | 通过 |
| `cargo test --locked --manifest-path apps/linux/Cargo.toml --all-features` | 25 通过；1 个需隔离 X server 的测试在默认运行中忽略 |
| `cargo clippy --locked --manifest-path apps/linux/Cargo.toml --all-targets --all-features -- -D warnings` | 通过，无警告 |
| `cargo build --locked --manifest-path apps/linux/Cargo.toml --features debug-hooks` | 通过 |
| `uv run tests/linux/hotkeys_smoke.py` | 通过；其中显式运行上述 1 个隔离 X server 测试并通过 |
| `niri validate --config <临时配置文件>` | 安装文档中的 `repeat=false`、空格路径、独立 `toggle` 参数配置通过 |
| `Hyprland --verify-config --config <临时 Lua/hyprlang 文件>` | 本机 0.56.2 对两种示例均报告 config ok；未启动 Hyprland 会话 |
| `git diff --check` | 通过 |
| `cargo test --locked --manifest-path core/Cargo.toml` | 更新到 0.5.0 基线后运行，118 通过；本 PR 不修改 core |

### 无真实桌面依赖的测试

- 后端选择不把 Wayland/Xwayland 判成原生 X11 全局能力；重复 start、旧 generation 与关闭后结果失效。
- 类型化按下/释放/命令动作、长按去重、暂停期间释放处理、手动入口不承诺释放能力。
- X11 控制队列：fake 后端验证注册/暂停/关闭的顺序与最终清理，模拟慢服务器时 GLib 主循环仍能处理计时器。
- mock X grab：动态锁定键/Alt 映射、重复注册、部分新增失败、保留重叠旧组合、回滚清理、成功替换、core repeat 对判定。
- fake Portal：运行自己的无服务自动激活目录的 D-Bus；不接触真实桌面 Portal。真实使用 ashpd/zbus 的 CreateSession、BindShortcuts、订阅和 Close 调用；另用总线策略拒绝 AddMatch 验证订阅失败不会报告成功。覆盖等待授权不是可用、取消不循环请求、显式重绑清理、旧结果无效、session+ID 过滤、重复按下、释放与激活 token 传递、系统改键、绑定缺少目标、v1/v2 配置能力、Session.Closed、服务 owner 消失、连接/信号结束、等待中取消以及协调器 Drop 清理。
- .desktop Exec 转义与 GLib 解析往返；niri KDL 引号/反斜杠和禁用重复。

测试断言包含激活 token 的传递，但不把 token 写入日志。fake Portal 测试中的生产日志调用关闭，避免写入用户应用日志。

### 隔离图形会话实测

`tests/linux/hotkeys_smoke.py` 创建临时 XDG 配置/数据/状态目录、0700 runtime 目录、无自动激活服务目录的私有 D-Bus，启动 KWin virtual framebuffer 与 Xwayland。不会修改或使用当前桌面的配置、快捷键、应用实例或 Portal 授权。

本次确认：

1. 无 GlobalShortcuts 服务时显示 Unsupported、重试和手动入口，不显示注册成功；英文设置页截图检查通过。
2. `toggle` 连续两次开/关；`show` 重复调用保持打开；Esc 关闭。
3. 八个并发 `toggle` 各执行一次，最终保持原来的关闭状态；D-Bus 主进程 PID 不变。
4. 两个并发冷启动 `show` 只有一个主实例；正常退出释放总线名称。
5. GTK 原生 Wayland 和强制 Xwayland 两种运行方式均选择 Portal；缺少会话总线时非零退出，不生成非唯一 GUI。
6. 在隔离 Xwayland 上实际执行 XGrabKey/XUngrabKey，验证部分冲突时旧绑定保留、新组合清理、成功改绑、暂停释放、异步协调器的录制冲突回滚/恢复、关闭时丢弃尚未执行的恢复请求、共用连接仍可读鼠标/窗口信息、后台事件线程可结束。
7. Xwayland 的 XTest 会经过合成器输入路径，虚拟 seat 不足以模拟真实键盘长按。因此该分支用受控 X 协议事件验证重复按下、修饰键先释放、事件时间戳和只产生一对 Pressed/Released。**这不是原生 X11 物理键盘实测，也不是 Wayland 全局覆盖测试。**

脚本结束打印临时 artifact 目录，包含设置截图、运行日志与通过标记。临时会话数据被清理；截图/日志留给本地复查，不加入仓库。

## 真实桌面矩阵

| 环境 | 已验证内容 | 本次未验证 / 待人工执行 |
|---|---|---|
| 原生 X11 | mock 映射/事务/重复测试；隔离 Xwayland 上的实际抓取操作与受控事件 | 原生 Xorg 上物理 Ctrl+Shift+Q 长按；CapsLock/NumLock 重映射；切换键盘布局/热插拔；录制成功/冲突/取消；排除应用按键穿透；自动粘贴/定位回归 |
| GNOME Wayland | Portal 协议通过 fake 服务验证 | 真实版本的授权/取消/拒绝、重启记忆、系统改键、服务重启、面板焦点；不沿用 D37 历史结论 |
| KDE Plasma Wayland | KWin 6.7.4 隔离虚拟会话的 CLI、窗口显示与降级；fake Portal v2 配置 | 完整 Plasma 会话真实 Portal 绑定/设置页/授权记忆、KWin 前台识别/排除、物理快捷键与激活 token 焦点 |
| Hyprland | 官方语法核实；本机 0.56.2 对 Lua/hyprlang 两种配置通过 `--verify-config` | 合成器实机加载配置、Portal 后端、长按、窗口焦点；未启动实际 Hyprland 会话 |
| Sway | 对照官方 `sway(5)` 的 `--no-repeat`/exec 说明核实示例 | 配置加载、命令激活上下文、物理长按与焦点；本机未进行 Sway 实测 |
| niri | 本机 26.04 配置校验与热加载通过；本次真实会话 Portal 绑定失败，原生 spawn/toggle 唤起面板并取得焦点；用户确认 Ctrl+Shift+Q 正常开关 | 物理长按、其他布局、会话重启、无激活 token 情况；本次不宣称 Portal 注册成功 |

建议每个真实桌面执行：启动→确认状态→授权/取消→显式重试→改键→长按→排除应用/恢复→录制或系统配置取消→服务重启→退出重启。手动入口另测含空格的 AppImage 路径、移动镜像后的路径更新、无激活 token 的焦点限制，以及避免同时设置 Portal/自定义绑定。

## 已知边界

- Portal 的 `Response=2` 无具体原因；如实表示未说明/临时失败，不将其推断为用户拒绝或键冲突。
- Portal 和用户配置的合成器快捷键无法由应用实现 X11 式按键穿透；前台应用识别维持 D35/D37 的桌面能力限制。
- CLI 是一次动作，没有物理释放事件；由合成器关闭重复。缺少合法令牌时不能保证焦点，不绕过合成器策略。
- 当前验证环境高于 GTK 4.14/libadwaita 1.5 基线。本次没有在最低版本发行版重新构建，也没有制作/安装 AppImage、deb、rpm；.desktop 路径转义由自动化覆盖。

English summary: this revision passes formatting checks for modified Rust files, default/all-feature checks, 25 automated tests, strict Clippy, a debug build, an isolated KWin/Xwayland IPC/grab smoke test, and niri/Hyprland configuration validation. Portal tests use a fake service on a private bus. Real niri command activation/focus and a user-confirmed Ctrl+Shift+Q press pass (see follow-up below); niri Portal binding failed in this session. GNOME, Plasma, Hyprland, Sway authorization/focus and native X11 physical-key tests remain unverified. Historical D37 results are explicitly excluded.

## D42 补充：Ctrl+Shift+Q 默认值与改键（2026-10-19）

- 新安装/未设置或空的 `hotKey` 使用 Ctrl+Shift+Q；新增配置兼容测试确认已有 Ctrl+Q 和其他自定义组合保持原值。
- 隔离 Xwayland 测试使用新默认值实际注册，经设置页同一协调器改成 Ctrl+Q，再恢复 Ctrl+Shift+Q；普通修改和录制后的冲突都回滚到新默认。另验证 GTK 加速键转换出的 Portal 偏好是 `CTRL+SHIFT+q`。
- 设置页以解析后的按键/修饰键判断是否默认，避免 GTK 录制时修饰键顺序不同而误显示“恢复默认”。隔离会话的英文设置截图已检查，新默认及失败提示显示正常。
- 上表 fmt、默认/全特性 check、test（25 通过、1 默认忽略）、严格 Clippy、构建和隔离 smoke 均在修改后重新运行并通过；被忽略的 X11 测试由 smoke 显式运行并通过。niri 26.04 与 Hyprland 0.56.2 的配置校验也对新的 Ctrl+Shift+Q 示例重新通过。
- 未覆盖系统已经记住的真实 Portal 绑定迁移和真实桌面物理键盘；应用保留系统绑定，用户通过系统配置入口修改。上面的真实桌面矩阵仍适用。

English: D42 changes the Linux default to Ctrl+Shift+Q and preserves saved custom/system bindings. All listed checks were rerun; the isolated X11 test covers registration, changing the combination, restoring the default and conflict rollback. The subsequent niri follow-up below records a user-confirmed physical key press; other real-desktop authorization and physical-key checks remain pending.

## 本机 niri 唤醒问题复测（2026-10-19）

- **现象与原因：** 开发目录已构建新版本，但实际运行的仍是 `~/.local/bin/qantrip` 的旧版本；niri 当前会话的 Portal 绑定请求失败，配置中也没有 Qantrip 原生绑定。新进程的命令被正常转发给旧实例，不会替换运行程序。
- **实际验证：** 在真实 niri 26.04 会话执行与绑定相同的 `spawn … toggle`，面板可见、浮动并取得键盘焦点；确认仅一个主实例且运行的是已更新文件。随后用户实际按 Ctrl+Shift+Q，确认可正常打开/关闭面板。
- **边界：** 这是本机原生绑定的真实验证；Portal 仍失败，不据此声称 niri Portal 可用。未验证物理长按、重启会话及其他键盘布局。安装包未重新打包，旧二进制和 niri 配置均保留本机备份。

## Portal 失败根因调试（2026-10-19）

本次针对“是否只是 Qantrip 的 Portal 接入错误”做真实会话诊断，保留已经工作的 niri 原生绑定。

- **准确环境：** `niri-shorin-fork-git 26.04.r115.g42a2e06-1`（运行版本 `v26.04-115-g42a2e06e`）、xdg-desktop-portal 1.22.1-2、xdg-desktop-portal-gnome 50.0-1。用户 niri-portals.conf 以 `gnome;gtk;` 为默认后端，没有单独覆盖 GlobalShortcuts；GNOME 后端实际正在运行，不能以其 systemd 单元为 inactive 误判服务不存在。
- **最小复现：** 在同一真实会话总线上新建独立客户端连接，使用现有 `ai.nekro.qantrip` 和 `toggle-panel`，先登记应用身份，再订阅自身 Request.Response，执行 CreateSession 与 BindShortcuts。只打印阶段、响应码和结果键名，不输出激活令牌或原始总线负载；最后关闭会话与连接。

| 步骤 | 实测 |
|---|---|
| host Registry.Register | 成功 |
| GlobalShortcuts.version | 前端与 GNOME 后端均为 1 |
| CreateSession | Response=0，返回 session_handle |
| BindShortcuts（CTRL+SHIFT+q） | Response=2，结果字典为空 |
| Session.Close 与客户端断开 | 成功 |
| GNOME 后端 → Settings.GlobalShortcutsProvider.BindShortcuts | D-Bus 请求序号与错误 reply_serial 对应，收到 `org.freedesktop.DBus.Error.ServiceUnknown` |
| Settings.GlobalShortcutsProvider / org.gnome.Shell | 两个名称均无 owner，也不在可自动激活服务列表；本机未安装 gnome-control-center |

**定位结果：** 直接失败点是 GNOME 后端调用不存在的 `org.gnome.Settings.GlobalShortcutsProvider`。对照与安装版本相同的 [GNOME 50.0 后端源码](https://github.com/GNOME/xdg-desktop-portal-gnome/blob/50.0/src/globalshortcuts.c)，`handle_bind_shortcuts` 先调用该配置服务；`settings_bind_shortcuts_done` 出错后把它转换为无详情的 Response=2，正好解释 Qantrip 只能拿到未说明的失败。该函数成功后仍要调用 `org.gnome.Shell.GrabAccelerators`，当前会话也不提供它，因此仅安装 GNOME 控制中心不能补齐这条链路。该结论基于真实调用、服务状态与对应源码，不依据桌面名称直接推断。

**替代接口检查：** 当前运行的 Wayland registry 没有广告 `vicinae_hotkey_manager_v1` 或其他 hotkey/global-shortcuts 扩展；快捷键相关仅见 `zwp_keyboard_shortcuts_inhibit_manager_v1`（它不是注册全局快捷键的接口）。`niri msg --help` 也未提供全局快捷键动态注册入口。上游 [vicinae-hotkey-v1 PR #4145](https://github.com/niri-wm/niri/pull/4145) 在本次检查时仍为 Open，不能把提案当成本机已有能力。

**处理：** 保留 niri 原生绑定。Qantrip 的注册调用顺序无需为此次环境问题修改；也不把所有 Response=2 硬编码解释成此原因。未重启或切换桌面 Portal，未安装 GNOME Shell/控制中心或修改合成器。此次只更新调试记录，`git diff --check` 通过；没有重跑与本次文档诊断无关的 Cargo 测试。

## 合并时的修改（2026-10-19）

合并到主线时做了以下调整（docs/decisions.md D41、D42）：

- **X11 换键后第一次按下无效：** 录制新快捷键时，抓取建立时那个键往往还按着；它的释放事件不会发给本程序（被动抓取只在按下时激活），按住状态一直不清除，下一次按下被当成长按丢弃。现在抓取时检测到按住的键，事件线程临时改为每 20ms 查询一次键盘状态，直到它松开。
- **Portal 后端不发 Deactivated 时快捷键失效：** 按住期间再次收到 Activated、且与上一次间隔超过 400ms 时视为新的按下（自动重复的间隔只有几十毫秒）；新增单元测试。
- **注册失败被"已暂停"掩盖：** X11 暂停时保留失败状态与"重试"入口。
- **Wayland 面板窗口：** GNOME 与 KDE 保留 D37 实测过的透明最大化窗口（卡片在屏幕上方约 1/3 处）；其他合成器使用本 PR 的固定尺寸 CSD 窗口。
- **文档：** 安装说明恢复为面向用户的简短版本；快捷键设置与手动绑定移到 `docs/linux-shortcuts.md`，从源码构建移到 `apps/linux/README.md`。

验证：在 Ubuntu 24.04 容器中，Linux 外壳默认与 `debug-hooks` 两套 Clippy（`-D warnings`）和全部测试通过（含 fake Portal）。以上三处行为修复没有在真实桌面上复测；GNOME / KDE 的面板窗口回到了 D37 的实现与实测结论。

