# UI 自动化辅助函数（dot-source；不点击鼠标，全部经 UIA 模式操作控件）。
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [Windows.Automation.AutomationElement]
$S = [Windows.Automation.TreeScope]

# 非 ASCII 文字转义输出（irun 日志经过多次编码转换，直接输出中文会乱码）
function U([string]$t) { -join ($t.ToCharArray() | ForEach-Object { if ([int]$_ -gt 127) { '\u{0:x4}' -f [int]$_ } else { $_ } }) }

function Win([string]$name, [int]$tries = 40) {
  for ($i = 0; $i -lt $tries; $i++) {
    $w = $A::RootElement.FindFirst($S::Children, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $name)))
    if ($w) { return $w }
    Start-Sleep -Milliseconds 150
  }
}
function TryEl($win, [string]$id) {
  $win.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::AutomationIdProperty, $id)))
}
function El($win, [string]$id, [int]$tries = 20) {
  for ($i = 0; $i -lt $tries; $i++) {
    $e = TryEl $win $id
    if ($e) { return $e }
    Start-Sleep -Milliseconds 100
  }
  Write-Output "!! element not found: $id"
}
function ByName($win, [string]$name) {
  for ($i = 0; $i -lt 20; $i++) {
    $e = $win.FindFirst($S::Descendants, (New-Object Windows.Automation.PropertyCondition($A::NameProperty, $name)))
    if ($e) { return $e }
    Start-Sleep -Milliseconds 100
  }
}
function Pat($e, $p) { $e.GetCurrentPattern($p) }
function Invoke($e) { (Pat $e ([Windows.Automation.InvokePattern]::Pattern)).Invoke(); Start-Sleep -Milliseconds 400 }
function SetVal($e, [string]$v) { $e.SetFocus(); (Pat $e ([Windows.Automation.ValuePattern]::Pattern)).SetValue($v); Start-Sleep -Milliseconds 400 }
function Val($e) { (Pat $e ([Windows.Automation.ValuePattern]::Pattern)).Current.Value }
function SelectItem($e) { (Pat $e ([Windows.Automation.SelectionItemPattern]::Pattern)).Select(); Start-Sleep -Milliseconds 500 }
function Expand($e) { (Pat $e ([Windows.Automation.ExpandCollapsePattern]::Pattern)).Expand(); Start-Sleep -Milliseconds 400 }
function CloseWin($w) { if ($w) { try { (Pat $w ([Windows.Automation.WindowPattern]::Pattern)).Close() } catch {} ; Start-Sleep -Milliseconds 300 } }

# 截取某个窗口所在区域（物理像素）
function ShotWin([string]$name, $w, [double]$scale = 0.5) {
  $r = $w.Current.BoundingRectangle
  Shot $name ([int]$r.X) ([int]$r.Y) ([int]$r.Width) ([int]$r.Height) $scale
}

# 从回收站中永久删除原位置为 $dir 的项目（只删测试产生的文件，不动其他回收站内容）
function PurgeRecycle([string]$dir) {
  $bin = (New-Object -ComObject Shell.Application).Namespace(10)
  $found = @($bin.Items() | Where-Object { $bin.GetDetailsOf($_, 1) -eq $dir })
  foreach ($it in $found) {
    $r = $it.Path
    $i = Join-Path (Split-Path $r) ("`$I" + (Split-Path $r -Leaf).Substring(2))
    Remove-Item -LiteralPath $r, $i -Force -Recurse -ErrorAction SilentlyContinue
  }
  $left = @($bin.Items() | Where-Object { $bin.GetDetailsOf($_, 1) -eq $dir }).Count
  "recycle bin: purged $($found.Count) test item(s), left $left"
}
