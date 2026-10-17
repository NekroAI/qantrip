# Nekro Qantrip for Windows

C# + WinUI 3（.NET 10，Windows App SDK 2.5，非打包、自包含）外壳，Rust 核心通过 C ABI（`native/`，JSON 传参）接入（docs/decisions.md D32，绑定方式见 `docs/windows-spike.md`）。

当前状态：面板、设置、模板库与编辑器、同步（设置 › 同步）、首次启动引导。全局快捷键 Alt+Q 唤起面板，输入命令，回车把 Prompt 粘贴到唤起前的窗口；以 `/` 开头的输入为指令模式（`/new`、`/edit`、`/dup`、`/rm` 等，D31）；托盘图标提供"打开面板 / 模板库 / 设置 / 使用引导 / 退出"。尚未实现：安装包与签名、源码编辑器语法高亮。

## 目录

| 路径 | 内容 |
|---|---|
| `native/` | `qantrip-ffi`：包装 `qantrip_core::Engine` 与 `SyncClient` 的 C ABI 动态库（`qantrip_ffi.dll`）；`secrets.rs` 为同步密钥的 DPAPI 存储 |
| `Qantrip/Core/` | P/Invoke 封装与 JSON 模型（与 `core/src/grammar.rs`、`command.rs`、`doc.rs`、`sync/mod.rs` 对应） |
| `Qantrip/SyncService.cs` | 同步外壳：后台串行调用核心、浏览器登录回调、定时同步（对应 macOS `SyncService.swift`） |
| `Qantrip/Platform/` | Win32：热键、托盘、前台切换、剪贴板快照 / 恢复、Ctrl+V 插入、回收站、登录时启动、登录回调监听 |
| `Qantrip/UI/` | 面板（移植自 macOS `LauncherModel` / `LauncherView`）、设置窗口（通用 / 同步）、模板库与编辑器、首次启动引导、提示浮层、动效 |
| `Qantrip/Assets/Fonts/` | 内置字体与许可证（见下文「字体」） |
| `build.ps1` | 一键构建：Rust 核心 + 外壳 → 自包含目录 + zip |

## 构建

需要：Rust（MSVC 工具链，`x86_64-pc-windows-msvc`）、.NET 10 SDK（`winget install Microsoft.DotNet.SDK.10`）、Visual Studio 生成工具（"使用 C++ 的桌面开发"工作负载 + Windows 11 SDK）。不需要完整的 Visual Studio。

```powershell
# 在仓库根目录
powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1            # 核心测试 + 构建 + 发布 + 打包
powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1 -SkipTests # 跳过测试
powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1 -NoZip     # 不生成 zip
```

产物：

- `dist\windows\Qantrip\`（约 180MB，含 .NET 运行时与 Windows App SDK），运行其中的 `Qantrip.exe` 即可；整个目录可以拷到其他 Windows 10 1904x+ / Windows 11 x64 机器直接运行。
- `dist\windows\Qantrip-Windows-x64.zip`（约 70MB）：上述目录的压缩包，解压得到 `Qantrip\` 文件夹。尚未签名，首次运行时 SmartScreen 可能提示"未知发布者"。

构建前须退出正在运行的 Qantrip（否则发布时 `Qantrip.dll` 被占用）。

## 运行与设置

- 启动后常驻托盘（通知区域，可能在"隐藏的图标"里）。Alt+Q 唤起 / 关闭面板；再次运行 `Qantrip.exe` 也会打开面板，`Qantrip.exe --settings` 打开设置窗口，`Qantrip.exe --library` 打开模板库。
- 首次启动显示引导（做什么、确认或更换快捷键、插入方式、练习一次），关闭或完成后不再显示（`settings.json` 的 `onboardingDone`）；托盘菜单"使用引导…"可以再次打开。
- 数据目录：`%APPDATA%\Qantrip\`（`templates\*.md` 用户模板，指令模式、模板库与同步都写在这里；`usage.json` 使用记录；`settings.json` 设置；`sync\<服务端主机>\` 同步状态与加密的密钥文件）。模板目录有变化（包括用其他编辑器修改）时自动重新加载。
- 设置窗口（托盘右键 › 设置…）：唤起快捷键录制（注册失败时提示冲突并保留原快捷键）、登录时启动（`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\NekroQantrip`）、界面语言与内置模板语言、插入方式（自动粘贴 / 只复制）、Ctrl+↩ 展开粘贴与间隔、输入法、记录使用习惯与清除、关于。修改立即生效并写入 `settings.json`（保留文件中其他键），也可以手动编辑该文件（重启后生效）：

  ```json
  {
    "hotKey": "Alt+Q",
    "language": "auto",
    "templateLanguage": "auto",
    "forceAsciiInput": true,
    "imeShiftToggle": true,
    "copyOnly": false,
    "recordUsage": true,
    "expandPasteEnabled": true,
    "expandPasteIntervalMs": 250,
    "restoreDelayMs": 500
  }
  ```

  `language` 为 `auto`（跟随 Windows 显示语言）/ `zh` / `en`；`templateLanguage` 为 `auto`（跟随界面语言）/ `zh` / `en`；`forceAsciiInput` 让面板默认英文输入（D9），`imeShiftToggle` 为再次唤起时仍是中文则模拟一次 Shift（实验性，见 `docs/windows-spike.md`）。
- 指令模式：`/new 名称 [正文]` 不写正文时使用剪贴板文字（每次唤起只在输入 `/new ` 时读一次）；`/rm` 需要再按一次回车，文件移到回收站；`/edit`、`/dup` 在模板库中打开该模板；完成后面板关闭并在屏幕下方显示提示。

## 模板库

托盘 › 模板库…（与 macOS `LibraryWindow` 相同）：

- 左侧：搜索；"我的模板"、"内置"（按分类）、"公共片段"三组；右键菜单可复制为新模板、导出、在资源管理器中显示、删除 / 恢复内置版本。
- 右侧：表单（名称、别名、描述、分类、参数：类型 / 候选值 / 默认值 / 短名 / 位置参数 / 必填 / 范围、正文）与源码两种编辑方式互相同步；核心 `check_source` 实时校验，右栏用"试用"输入预览渲染结果。Ctrl+S 保存。
- 内置模板只读，可"自定义此模板"（复制到用户目录并覆盖内置版本，随时"恢复内置版本"）或"复制为新模板"。删除与改名产生的旧文件都移到回收站。
- 导入 `.md` / `.zip`（也可以把文件拖进窗口），重名时可选覆盖 / 重命名导入 / 跳过；"导出全部"把模板文件夹打包为 zip（含顶层 `templates\`）。

## 同步

设置 › 同步（端到端加密，`docs/sync-design.md`；界面与 macOS `SyncSettingsView` 相同）：

- 未登录 → 使用 GitHub 登录：在 `127.0.0.1` 随机端口监听 `/callback`（PKCE），在默认浏览器中打开授权页，授权后浏览器显示"可以回到 Qantrip"。
- 已登录、本机未加入：服务端没有保险库时"创建保险库"（显示一次 24 个词的恢复短语，勾选"我已经抄写"后才能关闭）；已有保险库时"开始加入"（显示校验码 → 在已同步的设备上输入 → 输入该设备给出的确认码）或"改用恢复短语"。
- 已加入：账号、上次同步、立即同步、安全提醒、新设备加入请求（输入校验码后显示确认码）、设备列表与移除（移除最后一台其他设备需要恢复短语）、轮换密钥、重置这台设备、停止写入时"重新验证"。
- 高级：服务端地址（默认 `https://qantrip.nekro.ai`，登录后锁定）、本机标识。
- 后台：每 5 分钟同步一次，模板目录变化约 8 秒后同步；拉取到新内容或产生冲突副本后重新加载模板。面板唤起不发任何网络请求。
- 密钥存储：所有同步密钥（设备私钥、VK 钥匙环、令牌）合成一个映射，用 DPAPI（`CryptProtectData`，当前用户，附加应用熵）加密后写入 `sync\<主机>\secrets.bin`。不用凭据管理器的原因见 `docs/windows-spike.md` 第三轮。
- 动效（D15）：唤起入场、候选淡入、模板切换、命令锁定图标弹跳、回车被阻止时抖动、预览中新片段闪光。Windows"动画效果"关闭（设置 › 辅助功能 › 视觉效果）时全部不播放。
- 埋点日志：`%LOCALAPPDATA%\Qantrip\logs\spike.jsonl`（只记录时长、字符数等，不记录 Prompt 正文）。

## 字体

与 macOS 相同（D17）：命令行与代码元素用 Monaspace Neon（`MonaspaceNeonVar.ttf`，原样分发），模板名等展示性大字用 Fraunces。XAML 不能设置自定义变量轴，`FrauncesDisplay.ttf` 是从 `apps/macos/Resources/Fonts/Fraunces.ttf` 固定 SOFT=100、WONK=1、wght=600、opsz=24（与 macOS `Theme.display` 的取值一致）生成的静态实例，族名改为 "Fraunces Display"：

```sh
python3 -c "from fontTools.ttLib import TTFont; from fontTools.varLib import instancer; \
  instancer.instantiateVariableFont(TTFont('apps/macos/Resources/Fonts/Fraunces.ttf'), {'wght':600,'SOFT':100,'WONK':1,'opsz':24}).save('FrauncesDisplay.ttf')"
# 随后把 name 表的族名改为 "Fraunces Display"、子族名 "Regular"，OS/2 usWeightClass 设为 400
```

两款字体均为 SIL OFL 1.1，许可证随字体放在 `Qantrip/Assets/Fonts/`（Fraunces 没有保留字体名，可以分发修改版本；Monaspace 有保留字体名，因此不做修改）。

## 测试用环境变量

| 变量 | 作用 |
|---|---|
| `QANTRIP_DATA_DIR` | 改用另一个数据目录（不碰真实模板、使用记录与设置） |
| `QANTRIP_DEBUG_HOOKS=1` | 记录最后一次插入的文本（`logs\last-insert.txt`）、面板关闭时的输入、指令结果与提示文字，供回归脚本比对；同步登录时不打开浏览器，只记录回调地址。**仅限开发测试** |
| `QANTRIP_IME_SCOPE` | 覆盖输入框的 InputScope（默认 `Url`），用于输入法实验 |
| `QANTRIP_REDUCE_MOTION=1` | 按"动画效果已关闭"处理 |
| `QANTRIP_SKIP_ONBOARDING=1` | 不显示首次启动引导（回归脚本默认设置） |
| `QANTRIP_DEBUG_SYNC_MOCK` | 仅与 `QANTRIP_DEBUG_HOOKS=1` 同用：同步页显示模拟状态（`signedout` / `novault` / `join` / `join-sas` / `join-code` / `phrase` / `enrolled` / `blocked`），不调用核心，用于截图 |
| `QANTRIP_DEBUG_SYNC_ADVANCED=1` | 同步页"高级"默认展开（截图用） |

## 通过 SSH 开发的注意事项

从 SSH 会话启动的进程运行在非交互会话（session 0）中，看不到窗口、热键也注册不到用户桌面。需要在已登录用户的交互会话中运行时，用带 `/IT`（仅交互）选项的计划任务启动，截图也要在交互会话中进行（`tests/windows/irun.ps1`）。
