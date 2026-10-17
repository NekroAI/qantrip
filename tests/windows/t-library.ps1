# 模板库窗口（UI 自动化，不点鼠标）：自定义内置模板 → 表单修改 → 保存 → 恢复内置版本（回收站）；新建 → 改名保存 → 删除；
# 源码模式实时校验；试用预览；/edit 在模板库中打开。全部在测试数据目录中进行，最后清理回收站中的测试文件。
. "$PSScriptRoot\drv.ps1"
. "$PSScriptRoot\uia.ps1"
$data = "$Dev\data"
$tpl = "$data\templates"
New-Item -ItemType Directory -Force $tpl | Out-Null
Remove-Item "$tpl\review.md", "$tpl\my-template*.md", "$tpl\qtlib*.md" -ErrorAction SilentlyContinue
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }

# 从托盘菜单打开：向隐藏的消息窗口发送托盘右键消息，菜单中按 ↓↓ 回车选择"模板库…"
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Tray {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
"@
$mw = [Tray]::FindWindow("NekroQantrip.MessageWindow", [NullString]::Value)
[Tray]::PostMessage($mw, 0x8001, [IntPtr]::Zero, [IntPtr]0x205) | Out-Null   # WM_APP+1, WM_RBUTTONUP
Start-Sleep -Milliseconds 700
Shot "lib-tray-menu" 0 0 0 0 0.35
[Drv]::Combo($VK.DOWN) | Out-Null; Start-Sleep -Milliseconds 150
[Drv]::Combo($VK.DOWN) | Out-Null; Start-Sleep -Milliseconds 150
[Drv]::Combo($VK.ENTER) | Out-Null
$w = Win "模板库"
if (-not $w) { "tray menu did not open the library; falling back to --library"; Start-Process $App -ArgumentList "--library"; $w = Win "模板库" }
"library window: " + [bool]$w
Start-Sleep -Seconds 2
ShotWin "lib-empty" $w

# 1) 选中内置 review：只读表单 + 自定义提示条
SelectItem (El $w "lib_item_review")
"title: " + (El $w "lib_title").Current.Name + " / status: " + (U (El $w "lib_status").Current.Name)
"name field read-only: " + (Pat (El $w "lib_name") ([Windows.Automation.ValuePattern]::Pattern)).Current.IsReadOnly
ShotWin "lib-builtin" $w

# 2) 自定义此模板 → 用户目录出现 review.md，内容与内置源码一致
Invoke (El $w "lib_customize")
Start-Sleep -Milliseconds 500
"customized file: " + (Test-Path "$tpl\review.md") + " / status: " + (U (El $w "lib_status").Current.Name)
$orig = [IO.File]::ReadAllText("$tpl\review.md")

# 3) 表单修改描述 → 未保存、保存按钮可用 → Ctrl+S 等价的保存按钮 → 文件更新
SetVal (El $w "lib_description") "QTLIB 审查（测试）"
"after edit: status=" + (U (El $w "lib_status").Current.Name) + " saveEnabled=" + (El $w "lib_save").Current.IsEnabled
Invoke (El $w "lib_save")
Start-Sleep -Milliseconds 600
$saved = [IO.File]::ReadAllText("$tpl\review.md")
"saved description: " + ($saved -match "description: QTLIB") + " / status: " + (U (El $w "lib_status").Current.Name)
ShotWin "lib-customized" $w

# 4) 试用：review --strict 附加 → 预览包含严格模式与附加要求
SetVal (El $w "lib_try") "review --strict QTLIB-EXTRA"
Start-Sleep -Milliseconds 400
$pv = (El $w "lib_preview").Current.Name
"preview has extra: " + ($pv -match "QTLIB-EXTRA") + " chars=" + $pv.Length
ShotWin "lib-try" $w

# 5) 源码模式：写入错误 → 实时显示问题、不能保存；改回后可以保存
SelectItem (El $w "lib_mode_source")
$src = El $w "lib_source"
SetVal $src ($saved -replace "\{% if strict %\}", "{% if %}")
"broken: problems shown=" + [bool](TryEl $w "lib_problem") + " saveEnabled=" + (El $w "lib_save").Current.IsEnabled
ShotWin "lib-source-error" $w
SetVal $src $saved
Start-Sleep -Milliseconds 300
"fixed: problems shown=" + [bool](TryEl $w "lib_problem")
SelectItem (El $w "lib_mode_form")

# 6) 恢复内置版本：文件移到回收站，回到只读的内置模板
Invoke (El $w "lib_more")
Invoke (El $w "lib_menu_restore")
Start-Sleep -Milliseconds 400
ShotWin "lib-restore-dialog" $w
Invoke (ByName $w "恢复内置")
Start-Sleep -Milliseconds 600
"restored: file gone=" + (-not (Test-Path "$tpl\review.md")) + " status=" + (U (El $w "lib_status").Current.Name) + " customize button=" + [bool](TryEl $w "lib_customize")

# 6b) 复制为新模板：review-copy（别名清空），随后删除
Invoke (El $w "lib_duplicate")
Start-Sleep -Milliseconds 500
"duplicated: " + (Test-Path "$tpl\review-copy.md") + " title=" + (El $w "lib_title").Current.Name + " aliases=" + (Select-String -Path "$tpl\review-copy.md" -Pattern "^aliases" -Quiet)
Invoke (El $w "lib_more")
Invoke (El $w "lib_menu_delete")
Invoke (ByName $w "删除")
Start-Sleep -Milliseconds 500
"duplicate deleted: " + (-not (Test-Path "$tpl\review-copy.md"))

# 7) 新建 → 改名 qtlib-test → 保存（旧文件移到回收站）→ 立即可在面板中使用
Invoke (El $w "lib_new")
Start-Sleep -Milliseconds 500
"new file: " + (Test-Path "$tpl\my-template.md") + " title=" + (El $w "lib_title").Current.Name
SetVal (El $w "lib_name") "qtlib-test"
SetVal (El $w "lib_aliases") "qtl"
Invoke (El $w "lib_addparam")
Start-Sleep -Milliseconds 300
ShotWin "lib-new-param" $w
Invoke (El $w "lib_save")
Start-Sleep -Milliseconds 600
"renamed: new=" + (Test-Path "$tpl\qtlib-test.md") + " old gone=" + (-not (Test-Path "$tpl\my-template.md"))
"---- qtlib-test.md"; Get-Content "$tpl\qtlib-test.md" -Encoding UTF8; "----"
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
Start-Sleep -Milliseconds 250
[Drv]::Type("qtl --param", 12); Start-Sleep -Milliseconds 400
Shot "lib-panel-new" 880 40 1700 1000 0.5
[Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300

# 8) /edit review：在模板库中打开并选中
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
Start-Sleep -Milliseconds 250
[Drv]::Type("/edit review", 12); Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 900
"after /edit: library title=" + (El $w "lib_title").Current.Name + " fg=" + ([Drv]::Fg() -match "Qantrip")

# 8b) 导出全部为 zip：通过系统保存对话框（UI 自动化填写文件名）
$zip = "$Dev\qtlib-export.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
function FileDialog([string]$title, [string]$path) {
  $d = $null
  for ($i = 0; $i -lt 30 -and -not $d; $i++) {
    $d = $w.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $title)))
    if (-not $d) { $d = $A::RootElement.FindFirst($S::Children, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $title))) }
    Start-Sleep -Milliseconds 200
  }
  if (-not $d) { return "!! file dialog not found: $title" }
  # 对话框打开时焦点就在文件名框中：全选后输入完整路径（KEYEVENTF_UNICODE，不经过输入法）
  Start-Sleep -Milliseconds 500
  [Drv]::Combo($VK.CTRL, $VK.A) | Out-Null
  [Drv]::Type($path, 5)
  Start-Sleep -Milliseconds 300
  [Drv]::Combo($VK.ENTER) | Out-Null
  Start-Sleep -Milliseconds 1200
  "file dialog done: $title"
}
Invoke (El $w "lib_export")
FileDialog "另存为" $zip
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path $zip) { "exported zip entries: " + ((([IO.Compression.ZipFile]::OpenRead($zip)).Entries | ForEach-Object { $_.FullName }) -join ",") } else { "!! zip not created" }

# 9) 删除 qtlib-test（移到回收站）
SelectItem (El $w "lib_item_qtlib-test")
Invoke (El $w "lib_more")
Invoke (El $w "lib_menu_delete")
Invoke (ByName $w "删除")
Start-Sleep -Milliseconds 600
"deleted: " + (-not (Test-Path "$tpl\qtlib-test.md")) + " still listed=" + [bool](TryEl $w "lib_item_qtlib-test")

# 10) 从 zip 导入：qtlib-test 回到模板库；再导入一次 → 名称冲突对话框 → 跳过
if (Test-Path $zip) {
  Invoke (El $w "lib_import")
  FileDialog "打开" $zip
  ShotWin "lib-import-done" $w
  Invoke (ByName $w "好")
  "imported: " + (Test-Path "$tpl\qtlib-test.md") + " listed=" + [bool](TryEl $w "lib_item_qtlib-test")
  Invoke (El $w "lib_import")
  FileDialog "打开" $zip
  Start-Sleep -Milliseconds 300
  ShotWin "lib-import-conflict" $w
  Invoke (ByName $w "跳过")
  Start-Sleep -Milliseconds 800
  $ok = ByName $w "好"
  if ($ok) { Invoke $ok }
  "second import: " + (Get-Content $LogFile -Encoding UTF8 | Select-String '"action":"import"' | Select-Object -Last 1)
}

Get-Content $LogFile -Encoding UTF8 | Select-String '"library' | Select-Object -Last 14

# 清理：关闭窗口与应用，删除回收站中的测试文件
CloseWin $w
Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item "$tpl\qtlib-test.md", $zip -ErrorAction SilentlyContinue
PurgeRecycle $tpl
"templates left: " + (@(Get-ChildItem $tpl -Filter *.md).Name -join ",")
