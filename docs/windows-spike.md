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
- 用户模板文件带 UTF-8 BOM 时核心不识别（PowerShell 5.1 的 `Set-Content -Encoding UTF8`、部分编辑器会写 BOM）。建议核心加载模板时去掉 BOM（已在核心修复）。
- 内存占用约为 macOS 的 4 倍（WinUI + .NET 运行时）；发布目录 168MB。只引用 WinUI / DWrite 组件包（不用元包）已去掉 AI / ML 组件约 47MB；进一步可评估 .NET 裁剪或 Native AOT。
- 尚未实现：安装包与签名、输入框撤销、源码编辑器语法高亮（设置、开机启动、动效、内置字体已在第二轮完成；模板库、同步界面、首次启动引导已在第三轮完成）。
- 未实测：管理员目标的降级、多显示器不同缩放、Ditto 等第三方剪贴板管理器、深浅色切换。

## 第二轮（2026-10-18）：.NET 10、指令模式、设置、视觉

| 项 | 结果 | 依据 |
|---|---|---|
| 运行时升级 | 完成 | .NET 10（SDK 10.0.402）+ Windows App SDK 2.5（WinUI 2.3.9、DWrite 2.1.0、InteractiveExperiences 2.1.9 组件包），仍为非打包自包含；代码无需改动即可编译运行 |
| 体积 / 内存 | 略增 | 发布目录 168 → 181MB（主要是 Windows SDK 投影 26100 版 25MB），zip 70MB。仅升级时：工作集 168MB、私有 120MB（原 176 / 125）；加上本轮全部功能后 183 / 133MB（20 次唤起后）。唤起到首帧中位数 24ms、p95 49ms，前台 20/20 |
| 指令模式（D31） | 通过 | `qantrip-ffi` 新增 `qt_set_clipboard`、`qt_run_command`（`{"ok","outcome"\|"error"}`）、`qt_clear_usage`；`/` 显示"指令"帮助，`/new qttest` 用剪贴板正文生成带 `{level:简单\|专业}`、`{topic}` 参数的模板并立即可用，`/rm` 第一次回车只提示"再按一次 ↩ 确认"、第二次移到回收站（`SHFileOperation` + `FOF_ALLOWUNDO`，在回收站中按原位置找到），`/edit` 只提示（无模板库窗口）。完成提示用自绘浮层（不抢焦点，勿扰模式下也可见） |
| 设置窗口 | 通过 | Mica + Fluent 设置卡片；UI 自动化驱动：插入方式 / 展开粘贴 / 间隔 / 记录使用写入 `settings.json` 并保留未知键；录制 Alt+W 后立即生效；测试进程占用 Alt+F9 时启动报 1409 并在设置中显示冲突，重录后恢复；登录时启动写入 / 删除 `HKCU\…\Run\NekroQantrip`；界面语言切到 English 后设置窗口与面板即时切换；重启后读回 |
| 字体与动效 | 完成 | Monaspace Neon、Fraunces（静态实例）以 `ms-appx:///Assets/Fonts/…` 在非打包应用中正常加载；入场缩放淡入、候选错落 / 淡入、模板标题滑入、锁定弹跳、抖动、预览闪光均为 Composition 动画；`SPI_GETCLIENTAREAANIMATION` 关闭时不播放 |
| 回归 | 通过 | 记事本插入逐字一致、多格式剪贴板恢复哈希一致、剪贴板历史无 Prompt；只复制模式；输入法第二次唤起 `native -> shift` 后输入为英文 |

注意：

- 核心的指令文字沿用 macOS 的"废纸篓 / Trash"，外壳在不带位置信息的文字（提示、候选说明、诊断、帮助）中替换为"回收站 / Recycle Bin"。更彻底的做法是核心按平台给出用词（核心改动，本轮未做）。（第三轮：核心已按平台给出用词，外壳的替换已删除。）
- 托盘气泡在 Windows 11 上以系统通知显示，会留在通知中心；测试中快捷键冲突与只复制模式各留下一条。当前前台为系统通知弹窗（ShellExperienceHost）时，一次唤起未能取得前台（`foreground_ok=false`，按键落到原窗口），尚未复现，待观察。
- 抽帧截图每张约 130ms，无法完整记录 200ms 级动效，动效只做了目视与"回车被阻止确实触发"的确认。

## 第三轮（2026-10-18）：模板库、同步、首次启动引导

| 项 | 结果 | 依据 |
|---|---|---|
| 清理 | 完成 | 核心已在 Windows 上说"回收站 / Recycle Bin"，删除外壳中的字符串替换；`/rm` 完成提示为"已删除 qttest，可在回收站中找回"（`t-command.ps1`） |
| C ABI | 通过 | `qantrip-ffi` 新增模板库入口（`qt_entries`、`qt_builtin_source`、`qt_check_source`、`qt_parse_doc` / `qt_serialize_doc` / `qt_new_doc`，`TemplateDoc` 以可序列化的镜像类型传 JSON）与同步入口（`qt_sync_new` / `qt_sync_free` / `qt_sync_call`：一个按操作名分派的入口覆盖 `SyncClient` 的全部 21 个方法，错误为 `{"kind":…}` 的结构化 JSON）。Windows 上 `cargo test --release` 6 项通过：DPAPI 往返（磁盘上无明文、篡改后解密失败、清空后不留文件）、未登录时 `status` 为 `signed_in=false` 且不联网、`begin_sign_in` 生成含 `code_challenge` 的授权地址、模板文档往返与校验。不需要修改 `core/src` |
| 模板库窗口 | 通过 | UI 自动化驱动（`t-library.ps1`）：托盘菜单打开；内置 review 只读 → 自定义（文件与内置源码一致）→ 表单改描述（未保存、保存可用）→ 保存写入文件 → 试用 `review --strict QTLIB-EXTRA` 预览含附加要求 → 源码模式写入语法错误时实时显示核心的错误、不能保存，改回后恢复 → 恢复内置版本（文件进回收站，回到只读）→ 复制为新模板（别名清空）→ 新建、改名 `qtlib-test` 保存（旧文件进回收站）→ 面板中立即可用 → `/edit review` 打开并选中 → 经系统保存对话框导出 zip（`templates/qtlib-test.md`）→ 删除 → 从 zip 导入恢复 → 再次导入时重名对话框选"跳过"。打开模板库 0.3s；工作集 157 → 192MB（私有 110 → 133MB），关闭后不回落 |
| 同步界面 | 界面通过，真实登录未测 | `t-sync-ui.ps1`：staging 服务端下未登录状态；点击登录后回调监听启动（调试构建不打开浏览器），无关请求返回 404 且不结束监听；伪造授权码请求回调 → 回调页 200，核心以 state 不符拒绝 → 界面显示"登录回调的 state 不符，已拒绝"，仍为未登录。`QANTRIP_DEBUG_SYNC_MOCK` 模拟状态截图：创建保险库、加入、校验码、输入确认码、恢复短语（Esc 关不掉，勾选"我已经抄写"后才能完成）、已加入（确认码、加入请求、设备、轮换、重置、服务端地址锁定）、停止写入与安全提醒、移除设备确认 |
| 首次启动引导 | 通过 | `t-onboarding.ps1`：没有 `onboardingDone` 时启动即显示并取得前台；真实 Alt+Q 后显示"快捷键可以正常使用"；插入方式单选写入 `copyOnly`；在练习框按 Alt+Q 输入 zh 回车，Prompt 插入练习框并显示"成功了"；"开始使用"后写入 `onboardingDone`，重启不再显示 |
| 回归 | 通过 | 记事本插入逐字一致、多格式剪贴板恢复、剪贴板历史无 Prompt（`t-notepad.ps1`）；指令模式（`t-command.ps1`） |

实现要点：

- **同步密钥存储（SecretStore）：** 在 `qantrip-ffi` 内用 Rust 实现（`native/src/secrets.rs`）。核心用到的所有键（`device_keys`、`vault_keyring`、`access_token`、`refresh_token` 与加入过程中的临时密钥）合成一个映射，序列化后用 DPAPI `CryptProtectData`（当前用户范围、`CRYPTPROTECT_UI_FORBIDDEN`、附加应用熵）加密为一个文件 `%LOCALAPPDATA%\Qantrip\sync\<主机>\secrets.bin`，先写临时文件再替换；与 macOS 把全部密钥合成一个钥匙串条目对应。没有选凭据管理器：单个凭据上限 2560 字节，钥匙环随轮换增长，拆成多个凭据会失去一次读写全部密钥的原子性；凭据管理器本身也是 DPAPI 保护，安全性相同。两者都意味着以同一用户身份运行的其他程序可以解密（与 D30 中"仅本用户可读的文件"相当），Windows 上也没有 macOS 那样"每次更新询问一次"的问题。
- **后台调用：** 同步调用全部在线程池上执行并串行化，结果回到 UI 线程；每 5 分钟同步一次，模板目录变化（`FileSystemWatcher`，300ms 合并后重新加载模板）8 秒后同步；拉取到新内容或冲突时重新加载引擎与模板库。错误按 `kind` 映射为双语文字（与 macOS `describe` 相同）。
- **登录回调：** `TcpListener` 监听 `127.0.0.1:0`（`HttpListener` 绑定 127.0.0.1 需要 URL ACL），只接受 `/callback`，其他路径 404 并继续等待。
- **WinUI 注意：** 把已经在界面树中的元素移到新的父元素（即使先从旧父元素移除）会抛出 `0x800F1000`（"没有检测到已安装的组件"）；模板库与同步页每次重建都新建控件，输入框的文字与焦点另行保存后还原。`TextBox` 的换行是 `\r`，写回源码前统一为 `\n`；`TextChanged` 对程序赋值也会异步触发，表单只在文档序列化结果真的变化时才算修改。

注意：

- 真实的 GitHub 登录、创建 / 加入 / 恢复与双向同步需要浏览器中的授权点击，本轮没有在 Windows 上做端到端测试；核心与协议已在 macOS 上做过完整联调（D30）。
- 同步状态与密钥文件已移到 `%LOCALAPPDATA%`（不漫游），避免域环境的漫游配置文件把设备身份带到另一台电脑。
- 模板库窗口在界面语言切换后不会即时重建（重新打开即可）；源码编辑器没有语法高亮。

## 第四轮（2026-10-18）：按触发原因同步、账号设备、排除的程序

| 项 | 结果 | 依据 |
|---|---|---|
| C ABI | 通过 | `qt_sync_new` 增加设备名称 / 平台 / 应用版本三个参数（核心 `DeviceLabel`）；`qt_sync_call` 新增 `maybe_sync`（`trigger`：`manual` / `timer` / `panel_opened` / `local_change` / `launch` / `wake`，未发请求时返回 `null`）、`account_devices`、`release_device`（`grant_id`）、`account_usage`；状态增加 `usage`、`pending_joins`、`policy`、`next_push_at`、`device_limit`，错误增加 `device_limit`（带 `max_devices`）。Windows 上核心 110 项、`qantrip-ffi` 10 项测试通过（新增：未登录时各自动触发返回 `null` 且不联网、未知触发原因与缺少 / 非法 `grant_id` 返回 `invalid_input`、新状态字段的默认值、`device_limit` 错误与账号设备的 JSON 形状）。不需要修改 `core/src` |
| 同步触发 | 已构建，未实测 | 与 macOS 相同：60 秒定时 `timer`（锁屏 `WTS_SESSION_LOCK`、睡眠 `PBT_APMSUSPEND`、显示器关闭 `GUID_CONSOLE_DISPLAY_STATE` 时跳过，三者都恢复后 `wake`）、启动 `launch`、唤起面板 `panel_opened`（后台执行）、模板目录变化 8 秒后 `local_change`、在 `next_push_at` 再试一次上传；加入请求只在 `pending_joins > 0` 或同步页可见时查询 |
| 设置 › 同步 | 已构建，未实测 | 云端用量条、自动上传时间、账号设备列表（标签、相对时间、解除需确认，本机为退出登录）、已退出登录但仍在保险库中的设备"从保险库移除"、超出设备上限页面；`QANTRIP_DEBUG_SYNC_MOCK=limit` 模拟超出上限，`enrolled` 增加了用量与设备数据 |
| 排除的程序（D35） | 已构建，未实测 | 设置 › 通用 › 排除的程序（`excludedApps`: `[{path, name}]`，保留其他键）；`SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 进程外回调 → `QueryFullProcessImageName` → 在排除列表中时 `UnregisterHotKey`，切到其他程序或 Qantrip 自己的窗口时重新注册；托盘菜单"在「X」中停用 / 恢复快捷键" |

实现要点：

- **前台跟踪：** 回调在创建钩子的 UI 线程上分发，不跳过本进程（切到 Qantrip 自己的窗口时也要恢复快捷键）。UWP 应用的顶层窗口属于 `ApplicationFrameHost.exe`，取其中 `Windows.UI.Core.CoreWindow` 子窗口的进程。点击托盘图标时前台先变成任务栏（`Shell_TrayWnd` 等外壳窗口），托盘菜单项因此针对"最后一个既不是 Qantrip、也不是外壳窗口"的前台程序；此时快捷键已经恢复（前台是任务栏），菜单关闭后回到该程序才重新暂停。
- **暂停与录制：** `HotKey.SetPaused` 只恢复由它注销的热键；录制新快捷键时的注销不会被前台变化意外恢复（录制期间前台是设置窗口）。
- **锁屏 / 睡眠：** 隐藏的消息窗口是顶层窗口（不是 message-only），因此能收到 `WM_POWERBROADCAST`；现代待机下不一定有 `PBT_APMSUSPEND`，显示器关闭通知覆盖了这种情况。

注意：

- 本轮只在负责人的电脑上通过 SSH 构建并运行单元测试（负责人可能正在使用这台电脑，没有运行界面、热键或剪贴板测试）。排除的程序、托盘菜单项、锁屏 / 唤醒触发、同步页新界面都需要在交互会话中验证；真实的设备上限与解除需要服务端配合。

## 第五轮（2026-10-18）：与 macOS 对齐、更新提醒、安装程序

| 项 | 结果 | 依据 |
|---|---|---|
| 插入到 Qantrip 自己的窗口 | 已构建，未实测 | 唤起前的前台是模板库 / 设置或引导窗口时记下其中的焦点控件；回车后恢复该窗口与焦点，直接写入 `TextBox`（`SelectedText`，多行框换行为 `\r`，单行框换成空格）或 `RichEditBox`（`Selection.TypeText`），不模拟按键、不经过剪贴板；焦点不在可编辑输入框时只复制并提示。Esc 关闭面板后焦点回到原输入框 |
| 占位文字与光标对齐 | 已构建，未实测 | 不再用 `RichEditBox.PlaceholderText`：占位文字与灰字补全共用一个 `TextBlock`，按 `ITextRange.GetPoint(Baseline)` 与 `TextBlock.BaselineOffset` 对齐基线；输入框内边距改为外边距，使客户区坐标原点与控件原点一致 |
| 面板快捷入口 | 已构建，未实测 | Ctrl+L 模板库、Ctrl+, 设置（排除 AltGr），顶栏右侧两个图标（带提示）；先打开窗口再隐藏面板，不把焦点还给原程序 |
| 设置并入模板库窗口 | 已构建，未实测 | 侧栏底部固定"设置"，顶部 `SelectorBar` 切换 通用 / 插入 / 同步 / 隐私 / 关于，设置页隐藏新建 / 导入 / 导出；窗口标题随部分切换（"模板库" / "Nekro Qantrip 设置"）；托盘、`--settings`、`/settings`、`/library`、同步提醒都打开这个窗口的对应位置。`t-settings.ps1` 已按分页调整 |
| 恢复短语 | 已构建，未实测 | 复制（普通复制，不加排除标记、不定时清除）与"保存为文件…"（说明 + 编号的词，UTF-8 带 BOM） |
| 更新提醒 | 已构建，未实测 | `UpdateChecker`：GitHub 最新正式版本，启动 10 秒后（12 小时内检查过则跳过）及每 24 小时；托盘通知每个版本一次、菜单项"有新版本"；设置 › 关于：检查更新、下载、跳过此版本、自动检查开关 |
| 安装程序 | 构建通过，未运行 | Inno Setup 6.7.3（winget 按用户安装）编译无警告，47.6MB；按用户安装、开始菜单、首次安装可选登录时启动、安装后启动、卸载保留数据；安装 / 卸载前用 `NekroQantrip.Quit` 消息请运行中的实例退出 |

本轮在负责人电脑上只做了构建与单元测试（核心 110 项、`qantrip-ffi` 10 项通过），没有运行界面、热键、剪贴板测试，也没有运行安装程序。

## 测试方法

- 通过 SSH 启动的进程在非交互会话（session 0），看不到窗口、热键也注册不到桌面。`tests/windows/irun.ps1` 注册一个"仅交互"计划任务，经 `wscript`（GUI 子系统，不弹控制台、不抢前台）在已登录桌面上运行测试脚本并收集输出。
- 测试脚本用 SendInput 发送真实按键（Alt+Q、`KEYEVENTF_UNICODE` 打字或虚拟键码打字以经过输入法），以前台窗口轮询计时，读取 `%LOCALAPPDATA%\Qantrip\logs\spike.jsonl` 中的应用侧计时；`QANTRIP_DEBUG_HOOKS=1` 时应用额外记录最后一次插入的文本与面板关闭时的输入，供脚本比对。
- 注意：测试驱动若用"按一下 Alt"的办法抢前台，会激活记事本的菜单，导致第一次 Ctrl+V 丢失（真实使用中 Alt+Q 的 Alt 抬起发生在面板上，不受影响）；脚本改用 `AttachThreadInput`。
- PowerShell 5.1 按系统代码页（GBK）读取无 BOM 的脚本，含中文的 `.ps1` 必须带 UTF-8 BOM。
