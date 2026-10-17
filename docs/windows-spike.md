# Windows 可行性报告：WinUI 3 外壳 Spike

**日期：** 2026-10-18
**结论：** 通过（输入法一项有条件通过）。Windows 外壳按 D32 采用 C# + WinUI 3（Windows App SDK 1.8，非打包、自包含）；Rust 核心改用 C ABI + JSON 接入（原因见「绑定」），已做出接入真实核心的最小可用面板。
**环境：** Windows 11 Pro（26200）x64，3440×1440 @200%，微软拼音；rustc 1.99.1（MSVC）、.NET SDK 8.0.426、VS 生成工具 2026（MSVC 14.51，机器上原有）、Windows SDK 10.0.26100。全部测试经 SSH 用计划任务（`/IT`）在已登录桌面上以真实按键驱动，脚本在 `tests/windows/`。

## 结果总览

| # | 验证项 | 结果 | 依据 |
|---|---|---|---|
| W1 | 核心在 Windows 构建与测试 | 通过 | `cargo build --release`（x86_64-pc-windows-msvc）冷构建 95s；核心 89 项测试、C ABI 1 项测试全部通过 |
| W2 | C# 绑定 | 通过（改用 C ABI） | uniffi-bindgen-cs 最新版（v0.11.x）只支持 uniffi 0.31，核心用 0.32；改为 `apps/windows/native`（`qantrip_ffi.dll`，JSON 传参） |
| W3 | Alt+Q 全局热键 | 通过 | `RegisterHotKey`（`MOD_NOREPEAT`），无需权限；WM_HOTKEY 排队延迟中位数 0ms |
| W4 | 面板窗口与唤起延迟 | 通过 | 30 次：处理开始 → 面板可见并取得焦点中位数 14ms、p95 30ms；→ 首帧渲染中位数 21ms、p95 34ms；测试端从发出 Alt+Q 到面板成为前台中位数 32ms、p95 36ms（含 1ms 轮询）。启动后首次唤起（已预热）29ms。前台切换 30/30 直接成功 |
| W5 | 剪贴板 + Ctrl+V 插入 | 通过 | 记事本：178 字 11 行中文、含 Emoji 与引号的 46 字逐字一致；多格式剪贴板（文本 + HTML + 自定义二进制）完整恢复，哈希一致；Win+V 剪贴板历史不出现 Prompt，恢复也不产生重复条目 |
| W6 | Windows Terminal 多行 | 通过，有前提 | 程序开启 bracketed paste 时（Claude Code、Codex 等）收到 `ESC[200~…ESC[201~`，行间是 `\r`，不弹警告、不逐行执行。未开启时（Windows PowerShell 5.1、cmd）WT 默认弹出多行粘贴警告；用户选择"仍然粘贴"则除最后一行外逐行执行 |
| W7 | 中文输入法 | 有条件通过 | 首次唤起靠 InputScope 默认英文；用户在面板里切到中文后，再次唤起靠模拟一次 Shift 切回英文（实验性）。IMM32 / TSF 直接设置均无效。详见「输入法」 |
| W8 | 只复制降级 | 通过 | 只复制模式：不粘贴，Prompt 留在剪贴板（不加排除标记），托盘通知提示 Ctrl+V。目标以管理员身份运行时同样降级（代码已实现，未实测，需 UAC） |
| W9 | 资源 | 可接受 | 工作集 176MB、私有内存 125MB（macOS 约 45MB）；启动 0.3s，预热 0.28s；发布目录 168MB |

## 实现要点

- **绑定：** `qantrip-ffi` 包装 `Engine`，导出 13 个 `extern "C"` 函数（`qt_engine_new`、`qt_analyze`、`qt_render`、`qt_edit_param` 等）。字符串为 UTF-8，返回值由 `qt_string_free` 释放；结构化数据用 JSON（与官网 wasm 演示同一套 serde 输出）；所有入口 `catch_unwind`。文本位置是 UTF-16 偏移，与 C# `string` 索引一致。C# 侧 `Core/Models.cs` 手写对应类型（`System.Text.Json`，snake_case），核心类型变化时需要同步。不需要修改 `core/src`。uniffi-bindgen-cs 支持 0.32 后可以再评估切换。
- **热键：** 隐藏的顶层消息窗口（`WS_EX_TOOLWINDOW`，从不显示）接收 `WM_HOTKEY`、托盘回调与 `TaskbarCreated`，由 WinUI 的消息循环分发。注册失败（被其他程序占用）时托盘通知，不抢占。
- **面板：** WinUI 3 `Window` + `OverlappedPresenter`：保留细边框、去掉标题栏（Windows 11 据此绘制圆角与阴影），置顶，不可调整大小；`IsShownInSwitchers=false` + `WS_EX_TOOLWINDOW` 不进任务栏与 Alt+Tab；`DesktopAcrylicBackdrop` 之上再加一层半透明底色，背后内容杂乱时仍可读。按目标窗口所在显示器的有效 DPI 计算尺寸，水平居中、垂直 32% 处。启动后离屏预热一次。
- **激活（与 macOS 不同）：** Windows 上必须激活面板才能接收键盘。唤起时记下前台窗口，`SetForegroundWindow` 激活面板——刚收到热键的进程拥有前台权，30/30 直接成功；`AttachThreadInput` 兜底已实现但未触发。插入或 Esc 时先趁面板仍在前台把前台还给原窗口，再隐藏面板；点击外部（失去激活）只隐藏、不切换。
- **插入：** 前台还给目标 → 快照剪贴板全部 HGLOBAL 格式（GDI 句柄类格式由系统从 DIB 等合成，跳过）→ 写入 CF_UNICODETEXT（`\n` 转 CRLF）并附加 `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory=0`、`CanUploadToCloudClipboard=0` → 确认目标仍在前台后 SendInput Ctrl+V（虚拟键 `VK_V`，与键盘布局无关；先补发仍按着的 Shift / Alt / Win 的抬起）→ 500ms 后若剪贴板序列号未变则恢复（恢复时同样附加排除标记，对应 D18）。实测：动作到发出 Ctrl+V 3–18ms，快照 0–6ms。
- **UIPI：** 目标进程以管理员身份运行而本程序不是时，SendInput 会被静默拦截且无法从返回值发现；因此预先检查目标令牌的提升状态，直接降级为只复制。
- **Windows Terminal：** WT 在按下 Ctrl+V 时读取剪贴板并保存文本，多行警告弹出期间我们恢复剪贴板不影响最终粘贴的内容。
- **输入框：** `RichEditBox` 用 `ITextRange` 着色（token 颜色、错误波浪线），灰字补全按 `GetRect` 得到的末尾位置叠加显示。着色会触发 `TextChanged` 并写入撤销记录：以"文本是否变化"区分，撤销暂时关闭。粘贴只接受纯文本并把换行换成空格。组字期间不解析、不处理 Enter / Esc / 方向键。

## 输入法

| 做法 | 结果 |
|---|---|
| `ImmGetContext(GetFocus())` + `ImmSetConversionStatus` | 无效。WinUI 输入框直接走 TSF，IMM32 读数始终是 `open=false`（即使微软拼音处于中文） |
| TSF：线程管理器隔间 `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION` 写入 | 写入成功但不影响输入框；`ITfThreadMgr::GetFocus` 取不到 WinUI 的文档管理器 |
| 输入框 `InputScope=Url` | **有效**：输入框第一次获得焦点时微软拼音使用英文，Shift 可切回中文输入附加文本；只作用于本输入框，关闭后记事本仍是中文，无需恢复 |
| InputScope 重新赋值 / 每次新建输入框 / 交替两种英文 InputScope | 无效：用户在面板里切到中文后，再次唤起仍是中文 |
| 读到线程隔间为中文时，等修饰键全部松开后模拟一次 Shift | **有效**（`imeShiftToggle`，默认开）：第二次唤起回到英文。隔间在面板首次打字前为空，此时不发送 |

D9 的目标在微软拼音下达成。模拟 Shift 依赖输入法的中英切换键为 Shift（微软拼音、搜狗等的默认值）；若用户改成 Ctrl+Space 或关闭，Shift 只是一次无害的按键。其他输入法（搜狗、QQ、五笔、日韩）未验证。

## 已知限制

- 默认快捷键 Alt+Q 与 Microsoft 365（Word / Excel / Outlook 的"告诉我 / 搜索"）、JetBrains IDE 的 Alt+Q 冲突：程序运行期间这些应用收不到 Alt+Q。只验证了注册成功，未做常用软件的冲突普查（D5 待办）。
- Windows PowerShell 5.1 / cmd 不支持 bracketed paste，多行 Prompt 粘贴后由 WT 警告兜底；若用户关闭了 WT 的多行警告，除最后一行外会逐行执行。PowerShell 7（PSReadLine 2.2+）未测。Claude Code / Codex 在 Windows 上未实测，只用开启 bracketed paste 的探针验证了 WT 的行为；Ctrl+↩ 展开粘贴未实测。
- 用户模板文件带 UTF-8 BOM 时核心不识别（PowerShell 5.1 的 `Set-Content -Encoding UTF8`、部分编辑器会写 BOM）。建议核心加载模板时去掉 BOM（核心改动，本次未做）。
- 内存占用约为 macOS 的 4 倍（WinUI + .NET 运行时）；发布目录 168MB。只引用 WinUI / DWrite 组件包（不用元包）已去掉 AI / ML 组件约 47MB；进一步可评估 .NET 裁剪或 Native AOT。
- 尚未实现：设置界面（暂用 `settings.json`）、模板库与编辑器、同步、开机启动、动效（D15）、内置字体（D17，暂用 Cascadia Mono / Segoe UI Variable）、安装包与签名、输入框撤销。
- 未实测：管理员目标的降级、多显示器不同缩放、Ditto 等第三方剪贴板管理器、深浅色切换。

## 测试方法

- 通过 SSH 启动的进程在非交互会话（session 0），看不到窗口、热键也注册不到桌面。`tests/windows/irun.ps1` 注册一个"仅交互"计划任务，经 `wscript`（GUI 子系统，不弹控制台、不抢前台）在已登录桌面上运行测试脚本并收集输出。
- 测试脚本用 SendInput 发送真实按键（Alt+Q、`KEYEVENTF_UNICODE` 打字或虚拟键码打字以经过输入法），以前台窗口轮询计时，读取 `%LOCALAPPDATA%\Qantrip\logs\spike.jsonl` 中的应用侧计时；`QANTRIP_DEBUG_HOOKS=1` 时应用额外记录最后一次插入的文本与面板关闭时的输入，供脚本比对。
- 注意：测试驱动若用"按一下 Alt"的办法抢前台，会激活记事本的菜单，导致第一次 Ctrl+V 丢失（真实使用中 Alt+Q 的 Alt 抬起发生在面板上，不受影响）；脚本改用 `AttachThreadInput`。
- PowerShell 5.1 按系统代码页（GBK）读取无 BOM 的脚本，含中文的 `.ps1` 必须带 UTF-8 BOM。
