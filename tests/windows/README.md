# Windows 插入链路回归脚本

在已登录用户的交互会话中驱动真实的 `Qantrip.exe`：发送真实按键（Alt+Q、虚拟键码打字经过输入法）、检查前台窗口、比对插入结果与剪贴板、截图。结果与数据见 `docs/windows-spike.md`。

- 先用 `apps\windows\build.ps1` 构建（默认测试 `dist\windows\Qantrip\Qantrip.exe`，可用 `QANTRIP_APP` 覆盖）。
- 测试数据放在 `%USERPROFILE%\qantrip-dev`（可用 `QANTRIP_DEV` 覆盖），应用以 `QANTRIP_DATA_DIR=<该目录>\data`、`QANTRIP_DEBUG_HOOKS=1` 启动，不碰真实模板与使用记录。
- 本机桌面上直接运行：`powershell -File tests\windows\t-notepad.ps1`。通过 SSH 运行时必须经 `irun.ps1` 进入交互会话：`powershell -File tests\windows\irun.ps1 <脚本完整路径> [超时秒] [参数]`，输出写入 `irun.log` 并打印。
- 脚本会占用键盘焦点、改动当前剪贴板（剪贴板历史中的测试条目会被清理），运行期间不要操作电脑。
- `drv.ps1` 的 `EnsureApp` 默认设置 `QANTRIP_SKIP_ONBOARDING=1`（首次启动引导会抢占前台）；`uia.ps1` 为 UI 自动化辅助函数（不点鼠标，全部经 UIA 模式操作控件）。

| 脚本 | 验证内容 |
|---|---|
| `t-show.ps1` | 热键唤起、输入、Esc 关闭与前台归还 |
| `t-latency.ps1` | 唤起 / 关闭延迟（30 次），内存占用 |
| `t-notepad.ps1` | 记事本插入（多行、中文、Emoji、引号）逐字一致，多格式剪贴板完整恢复，剪贴板历史不记录 Prompt |
| `t-wt.ps1 [powershell\|cmd\|bp]` | Windows Terminal：多行粘贴警告、PowerShell 5.1 逐行执行风险、bracketed paste（`bp-probe.py`） |
| `t-ime.ps1 [InputScope 名]` | 中文输入法下面板默认英文、Shift 可切回中文、目标应用输入法不受影响 |
| `t-copyonly.ps1` | 只复制模式与通知 |
| `t-ui.ps1 [auto\|zh\|en]` | 首屏、补全、参数、取值、错误状态的截图 |
| `t-command.ps1` | 指令模式：`/` 帮助、`/new` 从剪贴板创建模板（写入测试数据目录的 `templates\`）、新模板立即可用、`/rm` 第二次回车才执行并移到回收站、`/edit` 打开模板库；最后只删除回收站中的这一项，恢复剪贴板文字 |
| `t-library.ps1` | 模板库（UI 自动化）：托盘菜单打开、自定义内置模板 → 表单修改 → 保存 → 试用预览 → 源码模式实时校验 → 恢复内置版本（回收站）、复制为新模板、新建并改名、面板中立即可用、`/edit` 选中、经系统文件对话框导出 zip 与导入（含重名跳过）、删除；最后清空回收站中原位置为测试目录的项目 |
| `t-sync-ui.ps1` | 设置 › 同步：staging 服务端下未登录状态、登录回调（伪造授权码，回调页 200、state 不符被拒绝并显示错误）；`QANTRIP_DEBUG_SYNC_MOCK` 各状态截图（恢复短语对话框 Esc 关不掉、勾选后才能完成；移除设备确认框）；最后删除同步状态目录 |
| `t-onboarding.ps1` | 首次启动引导：首次启动显示、真实 Alt+Q 确认快捷键、插入方式写入设置、在练习框中施放一次、完成后重启不再显示 |
| `t-settings.ps1` | 设置窗口（UI 自动化，不点鼠标）：各项写入 `settings.json` 且保留未知键、快捷键录制即时生效、启动时快捷键冲突的提示、登录时启动的注册表项、界面语言即时切换、重启后读回 |
| `t-motion.ps1 [0\|1]` | 动效抽帧（唤起、回车被阻止）；参数 1 模拟系统关闭"动画效果" |
| `t-cleanup.ps1` | 关闭记事本中的测试标签页 |
