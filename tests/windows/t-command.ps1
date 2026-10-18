# 指令模式（D31）：/ 帮助、/new 从剪贴板创建模板、使用新模板、/rm 两次回车后移到回收站、/edit 打开模板库，最后清理。
. "$PSScriptRoot\drv.ps1"
Add-Type -AssemblyName System.Windows.Forms
$tpl = "$Dev\data\templates"
New-Item -ItemType Directory -Force $tpl | Out-Null
Remove-Item "$tpl\qttest*.md" -ErrorAction SilentlyContinue
if (Test-Path "$Dev\data\settings.json") { Remove-Item "$Dev\data\settings.json" }
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }

# 保存测试前的剪贴板文字，结束时恢复（只处理文字；测试条目随后从剪贴板历史中删除）
$savedClip = [System.Windows.Forms.Clipboard]::GetText()
$body = "QANTRIP-CMD 请用{level:简单|专业}的语言解释 {topic}"
[System.Windows.Forms.Clipboard]::SetText($body)

function Panel([string]$text) {
  [Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
  [Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
  Start-Sleep -Milliseconds 250
  if ($text) { [Drv]::Type($text, 12); Start-Sleep -Milliseconds 300 }
}
$start = (Get-Content $LogFile -Encoding UTF8).Count
function NewEvents([string]$pattern) { Get-Content $LogFile -Encoding UTF8 | Select-Object -Skip $start | Where-Object { $_ -match $pattern } }

# 1) 只输入 /：右栏显示"指令"帮助
Panel "/"
Shot "cmd-help" 880 40 1700 1000 0.6
# 2) /new qttest：正文取自剪贴板，预览展示将要创建的模板
[Drv]::Type("new qttest", 12); Start-Sleep -Milliseconds 400
Shot "cmd-new" 880 40 1700 1000 0.6
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 400
Shot "cmd-new-hud" 0 0 0 0 0.4
"panel closed after /new: " + ([Drv]::WaitFgProc("Qantrip", 200) -lt 0)
"file created: " + (Test-Path "$tpl\qttest.md")
if (Test-Path "$tpl\qttest.md") { "---- qttest.md"; Get-Content "$tpl\qttest.md" -Encoding UTF8; "----" }
Start-Sleep -Milliseconds 1800

# 3) 新模板立即可用
Panel "qttest --level=专业 Rust 所有权"
Shot "cmd-use" 880 40 1700 1000 0.6
[Drv]::Combo($VK.ESC) | Out-Null
Start-Sleep -Milliseconds 300
NewEvents 'debug_query' | Select-Object -Last 1

# 4) /rm：第一次回车只提示确认，第二次才执行
Panel "/rm qttest"
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 300
Shot "cmd-rm-armed" 880 40 1700 1000 0.6
"after first Enter: panel open=" + ([Drv]::WaitFgProc("Qantrip", 200) -ge 0) + " file exists=" + (Test-Path "$tpl\qttest.md")
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 500
Shot "cmd-rm-hud" 0 0 0 0 0.4
"after second Enter: file exists=" + (Test-Path "$tpl\qttest.md")

# 5) /edit：在模板库窗口中打开该模板（随后关闭窗口）
Start-Sleep -Milliseconds 1800
Panel "/edit review"
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 500
Shot "cmd-edit-library" 0 0 0 0 0.4
. "$PSScriptRoot\uia.ps1"
$lib = Win "模板库"
"library opened by /edit: " + [bool]$lib + " title=" + $(if ($lib) { (El $lib "lib_title").Current.Name })
CloseWin $lib
NewEvents '"debug_command"|"command"|command_failed'

# 6) 回收站中能找到，然后只删除这一项（不动其他回收站内容）
$shell = New-Object -ComObject Shell.Application
$bin = $shell.Namespace(10)
$found = @($bin.Items() | Where-Object { $_.Name -like "qttest*" -and $bin.GetDetailsOf($_, 1) -eq $tpl })
"recycle bin items for qttest.md: " + $found.Count
foreach ($it in $found) {
  $r = $it.Path
  $i = Join-Path (Split-Path $r) ("`$I" + (Split-Path $r -Leaf).Substring(2))
  Remove-Item -LiteralPath $r, $i -Force -ErrorAction SilentlyContinue
}
"purged: " + (@($bin.Items() | Where-Object { $_.Name -like "qttest*" -and $bin.GetDetailsOf($_, 1) -eq $tpl }).Count -eq 0)

# 清理：恢复剪贴板文字，删除剪贴板历史中的测试条目
if ($savedClip) { [System.Windows.Forms.Clipboard]::SetText($savedClip) } else { [System.Windows.Forms.Clipboard]::Clear() }
. "$PSScriptRoot\clip.ps1"
$r = Await ([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()) ([Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult])
$n = 0
foreach ($it in @($r.Items)) { try { if ($it.Content.Contains("Text")) { $t = Await ($it.Content.GetTextAsync()) ([string]); if ($t -like "QANTRIP-CMD*") { [Windows.ApplicationModel.DataTransfer.Clipboard]::DeleteItemFromHistory($it) | Out-Null; $n++ } } } catch {} }
"removed $n test items from clipboard history"

# 退出应用
Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force
