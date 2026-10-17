# Nekro Qantrip for Windows

C# + WinUI 3（.NET 10，Windows App SDK 2.5，非打包、自包含）外壳，Rust 核心通过 C ABI（`native/`，JSON 传参）接入（docs/decisions.md D32，绑定方式见 `docs/windows-spike.md`）。

当前状态：可用的面板 + 设置。全局快捷键 Alt+Q 唤起面板，输入命令，回车把 Prompt 粘贴到唤起前的窗口；以 `/` 开头的输入为指令模式（`/new`、`/rm` 等，D31）；托盘图标提供"打开面板 / 设置 / 退出"。模板库窗口、同步尚未实现。

## 目录

| 路径 | 内容 |
|---|---|
| `native/` | `qantrip-ffi`：包装 `qantrip_core::Engine` 的 C ABI 动态库（`qantrip_ffi.dll`） |
| `Qantrip/Core/` | P/Invoke 封装与 JSON 模型（与 `core/src/grammar.rs`、`core/src/command.rs` 的 serde 输出对应） |
| `Qantrip/Platform/` | Win32：热键、托盘、前台切换、剪贴板快照 / 恢复、Ctrl+V 插入、回收站、登录时启动 |
| `Qantrip/UI/` | 面板（移植自 macOS `LauncherModel` / `LauncherView`）、设置窗口、提示浮层、动效 |
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

- 启动后常驻托盘（通知区域，可能在"隐藏的图标"里）。Alt+Q 唤起 / 关闭面板；再次运行 `Qantrip.exe` 也会打开面板，`Qantrip.exe --settings` 打开设置窗口。
- 数据目录：`%APPDATA%\Qantrip\`（`templates\*.md` 用户模板，指令模式也写在这里；`usage.json` 使用记录；`settings.json` 设置）。
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
- 指令模式：`/new 名称 [正文]` 不写正文时使用剪贴板文字（每次唤起只在输入 `/new ` 时读一次）；`/rm` 需要再按一次回车，文件移到回收站；完成后面板关闭并在屏幕下方显示提示。`/edit`、`/dup` 需要的模板库窗口尚未实现，只给出提示。
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
| `QANTRIP_DEBUG_HOOKS=1` | 记录最后一次插入的文本（`logs\last-insert.txt`）、面板关闭时的输入、指令结果与提示文字，供回归脚本比对。**仅限开发测试** |
| `QANTRIP_IME_SCOPE` | 覆盖输入框的 InputScope（默认 `Url`），用于输入法实验 |
| `QANTRIP_REDUCE_MOTION=1` | 按"动画效果已关闭"处理 |

## 通过 SSH 开发的注意事项

从 SSH 会话启动的进程运行在非交互会话（session 0）中，看不到窗口、热键也注册不到用户桌面。需要在已登录用户的交互会话中运行时，用带 `/IT`（仅交互）选项的计划任务启动，截图也要在交互会话中进行（`tests/windows/irun.ps1`）。
