# Nekro Qantrip for Windows

C# + WinUI 3（Windows App SDK 1.8，非打包、自包含）外壳，Rust 核心通过 C ABI（`native/`，JSON 传参）接入（docs/decisions.md D32，绑定方式见 `docs/windows-spike.md`）。

当前状态：可行性验证 + 最小可用面板。全局快捷键 Alt+Q 唤起面板，输入命令，回车把 Prompt 粘贴到唤起前的窗口；托盘图标提供"打开面板 / 退出"。设置界面、模板库窗口、同步尚未实现。

## 目录

| 路径 | 内容 |
|---|---|
| `native/` | `qantrip-ffi`：包装 `qantrip_core::Engine` 的 C ABI 动态库（`qantrip_ffi.dll`） |
| `Qantrip/Core/` | P/Invoke 封装与 JSON 模型（与 `core/src/grammar.rs` 的 serde 输出对应） |
| `Qantrip/Platform/` | Win32：热键、托盘、前台切换、剪贴板快照 / 恢复、Ctrl+V 插入 |
| `Qantrip/UI/` | 面板窗口与状态（移植自 macOS `LauncherModel` / `LauncherView`） |
| `build.ps1` | 一键构建：Rust 核心 + 外壳 → 自包含目录 |

## 构建

需要：Rust（MSVC 工具链，`x86_64-pc-windows-msvc`）、.NET 8 SDK、Visual Studio 生成工具（"使用 C++ 的桌面开发"工作负载 + Windows 11 SDK）。不需要完整的 Visual Studio。

```powershell
# 在仓库根目录
powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1            # 核心测试 + 构建 + 发布
powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1 -SkipTests # 跳过测试
```

产物在 `dist\windows\Qantrip\`（约 170MB，含 .NET 运行时与 Windows App SDK），运行其中的 `Qantrip.exe` 即可；整个目录可以拷到其他 Windows 10 1904x+ / Windows 11 x64 机器直接运行。

## 运行与设置

- 启动后常驻托盘（通知区域，可能在"隐藏的图标"里）。Alt+Q 唤起 / 关闭面板；再次运行 `Qantrip.exe` 也会打开面板。
- 数据目录：`%APPDATA%\Qantrip\`（`templates\*.md` 用户模板、`usage.json` 使用记录、`settings.json` 设置）。
- 设置界面尚未实现，可手动编辑 `settings.json`（缺省值）：

  ```json
  {
    "hotKey": "Alt+Q",
    "language": "auto",
    "forceAsciiInput": true,
    "imeShiftToggle": true,
    "copyOnly": false,
    "recordUsage": true,
    "expandPasteEnabled": true,
    "expandPasteIntervalMs": 250,
    "restoreDelayMs": 500
  }
  ```

  `language` 为 `auto`（跟随 Windows 显示语言）/ `zh` / `en`；`forceAsciiInput` 让面板默认英文输入（D9），`imeShiftToggle` 为再次唤起时仍是中文则模拟一次 Shift（实验性，见 `docs/windows-spike.md`）。修改后重启生效。
- 埋点日志：`%LOCALAPPDATA%\Qantrip\logs\spike.jsonl`（只记录时长、字符数等，不记录 Prompt 正文）。

## 测试用环境变量

| 变量 | 作用 |
|---|---|
| `QANTRIP_DATA_DIR` | 改用另一个数据目录（不碰真实模板与使用记录） |
| `QANTRIP_DEBUG_HOOKS=1` | 记录最后一次插入的文本（`logs\last-insert.txt`）与面板关闭时的输入，供回归脚本比对。**仅限开发测试** |
| `QANTRIP_IME_SCOPE` | 覆盖输入框的 InputScope（默认 `Url`），用于输入法实验 |

## 通过 SSH 开发的注意事项

从 SSH 会话启动的进程运行在非交互会话（session 0）中，看不到窗口、热键也注册不到用户桌面。需要在已登录用户的交互会话中运行时，用带 `/IT`（仅交互）选项的计划任务启动，截图也要在交互会话中进行。
