# Windows 插入链路回归脚本

在已登录用户的交互会话中驱动真实的 `Qantrip.exe`：发送真实按键（Alt+Q、虚拟键码打字经过输入法）、检查前台窗口、比对插入结果与剪贴板、截图。结果与数据见 `docs/windows-spike.md`。

- 先用 `apps\windows\build.ps1` 构建（默认测试 `dist\windows\Qantrip\Qantrip.exe`，可用 `QANTRIP_APP` 覆盖）。
- 测试数据放在 `%USERPROFILE%\qantrip-dev`（可用 `QANTRIP_DEV` 覆盖），应用以 `QANTRIP_DATA_DIR=<该目录>\data`、`QANTRIP_DEBUG_HOOKS=1` 启动，不碰真实模板与使用记录。
- 本机桌面上直接运行：`powershell -File tests\windows\t-notepad.ps1`。通过 SSH 运行时必须经 `irun.ps1` 进入交互会话：`powershell -File tests\windows\irun.ps1 <脚本完整路径> [超时秒] [参数]`，输出写入 `irun.log` 并打印。
- 脚本会占用键盘焦点、改动当前剪贴板（剪贴板历史中的测试条目会被清理），运行期间不要操作电脑。

| 脚本 | 验证内容 |
|---|---|
| `t-show.ps1` | 热键唤起、输入、Esc 关闭与前台归还 |
| `t-latency.ps1` | 唤起 / 关闭延迟（30 次），内存占用 |
| `t-notepad.ps1` | 记事本插入（多行、中文、Emoji、引号）逐字一致，多格式剪贴板完整恢复，剪贴板历史不记录 Prompt |
| `t-wt.ps1 [powershell\|cmd\|bp]` | Windows Terminal：多行粘贴警告、PowerShell 5.1 逐行执行风险、bracketed paste（`bp-probe.py`） |
| `t-ime.ps1 [InputScope 名]` | 中文输入法下面板默认英文、Shift 可切回中文、目标应用输入法不受影响 |
| `t-copyonly.ps1` | 只复制模式与通知 |
| `t-ui.ps1 [auto\|zh\|en]` | 首屏、补全、参数、取值、错误状态的截图 |
| `t-cleanup.ps1` | 关闭记事本中的测试标签页 |
