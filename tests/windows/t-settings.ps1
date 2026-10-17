# 设置窗口：通过 UI 自动化操作控件（不点击鼠标），检查 settings.json 持久化、即时生效、快捷键录制与冲突、登录时启动，最后清理。
. "$PSScriptRoot\drv.ps1"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class HK {
  [DllImport("user32.dll", SetLastError=true)] public static extern bool RegisterHotKey(IntPtr h, int id, uint mods, uint vk);
  [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr h, int id);
}
"@
$A = [Windows.Automation.AutomationElement]
$S = [Windows.Automation.TreeScope]
$data = "$Dev\data"
New-Item -ItemType Directory -Force $data | Out-Null
$settingsFile = "$data\settings.json"
# 带一个不认识的键：保存时必须原样保留
[IO.File]::WriteAllText($settingsFile, '{ "language": "zh", "futureKey": { "x": 1 } }')
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$hadAutostart = $null -ne (Get-ItemProperty $runKey -Name NekroQantrip -ErrorAction SilentlyContinue)
"autostart entry before test: $hadAutostart"
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }
# 非 ASCII 文字转义输出（irun 日志经过多次编码转换，直接输出中文会乱码）
function U([string]$t) { -join ($t.ToCharArray() | ForEach-Object { if ([int]$_ -gt 127) { '\u{0:x4}' -f [int]$_ } else { $_ } }) }
function J { Get-Content $settingsFile -Raw -Encoding UTF8 | ConvertFrom-Json }

function Win([string]$name) {
  for ($i = 0; $i -lt 40; $i++) {
    $w = $A::RootElement.FindFirst($S::Children, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $name)))
    if ($w) { return $w }
    Start-Sleep -Milliseconds 150
  }
}
function El($win, [string]$id) {
  for ($i = 0; $i -lt 20; $i++) {
    $e = $win.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::AutomationIdProperty, $id)))
    if ($e) { return $e }
    Start-Sleep -Milliseconds 100
  }
  "!! element not found: $id"
}
function Pat($e, $p) { $e.GetCurrentPattern($p) }
function Toggle($win, [string]$id) { (Pat (El $win $id) ([Windows.Automation.TogglePattern]::Pattern)).Toggle(); Start-Sleep -Milliseconds 300 }
function Choose($win, [string]$id, [string]$item) {
  $c = El $win $id
  (Pat $c ([Windows.Automation.ExpandCollapsePattern]::Pattern)).Expand(); Start-Sleep -Milliseconds 400
  $it = $c.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $item)))
  if (-not $it) { $it = $A::RootElement.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $item))) }
  (Pat $it ([Windows.Automation.SelectionItemPattern]::Pattern)).Select(); Start-Sleep -Milliseconds 500
}

# 打开设置（已运行的实例收到 --settings 后打开设置窗口）
Start-Process $App -ArgumentList "--settings"
$w = Win "Nekro Qantrip 设置"
"settings window (zh): " + [bool]$w
Start-Sleep -Milliseconds 600
Shot "settings-zh" 0 0 0 0 0.5

# 插入方式 → 只复制；展开粘贴关；间隔 400；记录使用关
Choose $w "insertMode" "只复制，手动粘贴"
(Pat (El $w "expandInterval") ([Windows.Automation.RangeValuePattern]::Pattern)).SetValue(400); Start-Sleep -Milliseconds 300
Toggle $w "expandPaste"
"interval slider disabled with expand paste off: " + (-not (El $w "expandInterval").Current.IsEnabled)
Toggle $w "recordUsage"
$j = J
"after edits: copyOnly=$($j.copyOnly) expandPasteEnabled=$($j.expandPasteEnabled) expandPasteIntervalMs=$($j.expandPasteIntervalMs) recordUsage=$($j.recordUsage) futureKey.x=$($j.futureKey.x)"

# 快捷键：录制 Alt+W（经 SendInput 的真实按键）
$rec = El $w "hotkeyRecorder"
$rec.SetFocus(); (Pat $rec ([Windows.Automation.InvokePattern]::Pattern)).Invoke(); Start-Sleep -Milliseconds 300
"recording label: " + (U $rec.Current.Name)
[Drv]::Combo($VK.ALT, $VK.W) | Out-Null; Start-Sleep -Milliseconds 500
"hotkey message: " + (U (El $w "hotkeyMessage").Current.Name)
"settings hotKey: " + (J).hotKey
# 新快捷键立即生效：Alt+W 唤起面板，旧的 Alt+Q 不再生效
[Drv]::Combo($VK.ALT, $VK.W) | Out-Null
Start-Sleep -Milliseconds 400
"Alt+W opens panel: " + ([Drv]::Fg() -match "title=Nekro Qantrip$")
Start-Sleep -Milliseconds 300
Shot "settings-panel-altw" 880 40 1700 1000 0.5
[Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300

# 恢复默认快捷键
[Drv]::Focus([IntPtr]$w.Current.NativeWindowHandle) | Out-Null
(Pat (El $w "hotkeyRestore") ([Windows.Automation.InvokePattern]::Pattern)).Invoke(); Start-Sleep -Milliseconds 300
"after restore: " + (J).hotKey

# 登录时启动：开 → 检查注册表 → 关（测试前已有登记时跳过，不改动）
if (-not $hadAutostart) {
  Toggle $w "autostart"
  $v = (Get-ItemProperty $runKey -Name NekroQantrip -ErrorAction SilentlyContinue).NekroQantrip
  "autostart on → Run value: $v"
  Toggle $w "autostart"
  "autostart off → removed: " + ($null -eq (Get-ItemProperty $runKey -Name NekroQantrip -ErrorAction SilentlyContinue))
}

# 界面语言 → English：设置窗口与面板即时切换
Choose $w "uiLanguage" "English"
$wEn = Win "Nekro Qantrip Settings"
"settings window switched to English: " + [bool]$wEn
Start-Sleep -Milliseconds 500
Shot "settings-en" 0 0 0 0 0.5
"settings language: " + (J).language
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 1500) | Out-Null
Start-Sleep -Milliseconds 300
Shot "settings-panel-en" 880 40 1700 1000 0.5
[Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300

# 关闭设置窗口，重启应用后读取：launch 事件中的设置快照
if ($wEn) { (Pat $wEn ([Windows.Automation.WindowPattern]::Pattern)).Close() }
Start-Sleep -Milliseconds 300
"settings.json:"; Get-Content $settingsFile -Encoding UTF8
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }
Get-Content $LogFile -Encoding UTF8 | Select-String '"launch"' | Select-Object -Last 1

# 冲突：测试进程先占用 Alt+F9，设置里的快捷键改为 Alt+F9 后启动 → 注册失败，设置窗口显示冲突提示
$okReg = [HK]::RegisterHotKey([IntPtr]::Zero, 77, 0x4001, 0x78)
"test process holds Alt+F9: $okReg"
$j = J; $j | Add-Member -NotePropertyName hotKey -NotePropertyValue "Alt+F9" -Force
[IO.File]::WriteAllText($settingsFile, ($j | ConvertTo-Json -Depth 5))
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }
Get-Content $LogFile -Encoding UTF8 | Select-String '"launch"' | Select-Object -Last 1 | ForEach-Object { "launch hotkey_ok: " + (($_ | ConvertFrom-Json).hotkey_ok) + " error: " + (($_ | ConvertFrom-Json).hotkey_error) }
Start-Process $App -ArgumentList "--settings"
$w = Win "Nekro Qantrip Settings"
Start-Sleep -Milliseconds 500
"conflict message: " + (U (El $w "hotkeyMessage").Current.Name)
Shot "settings-conflict" 0 0 0 0 0.5
[HK]::UnregisterHotKey([IntPtr]::Zero, 77) | Out-Null
# 冲突解除后录制 Alt+Q 恢复
$rec = El $w "hotkeyRecorder"
$rec.SetFocus(); (Pat $rec ([Windows.Automation.InvokePattern]::Pattern)).Invoke(); Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null; Start-Sleep -Milliseconds 500
"after re-recording: " + (U (El $w "hotkeyMessage").Current.Name) + " / " + (J).hotKey

# 清理：删除测试设置，退出应用
Remove-Item $settingsFile
Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force
"autostart entry after test: " + ($null -ne (Get-ItemProperty $runKey -Name NekroQantrip -ErrorAction SilentlyContinue))
