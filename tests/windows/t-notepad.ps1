. "$PSScriptRoot\drv.ps1"
. "$PSScriptRoot\clip.ps1"
$T = "$Dev\t"; New-Item -ItemType Directory -Force $T | Out-Null
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }

[Threading.Thread]::CurrentThread.GetApartmentState()
$orig = "QANTRIP-ORIG-" + (Get-Random)
SetRichClip $orig
Start-Sleep -Milliseconds 300
$before = ClipDigest
"clip before: $before"
"history before:"; HistoryTexts 3

$file = "$T\notepad-test.txt"
Set-Content $file "" -NoNewline
$hadNotepad = [bool](Get-Process Notepad -ErrorAction SilentlyContinue)
Start-Process notepad.exe $file
Start-Sleep -Seconds 2
$np = $null
for ($i = 0; $i -lt 20 -and -not $np; $i++) { $np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1; Start-Sleep -Milliseconds 200 }
"notepad focus: " + [Drv]::Focus($np.MainWindowHandle) + "  fg=" + [Drv]::Fg()

foreach ($cmd in @("r --strict", "zh 多行：第一行。第二行 —— 带 emoji 🎉 与 ""引号""")) {
  [Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
  "panel fg after " + [Drv]::WaitFgProc("Qantrip", 2000) + " ms"
  Start-Sleep -Milliseconds 150
  [Drv]::Type($cmd, 15)
  Start-Sleep -Milliseconds 200
  [Drv]::Combo($VK.ENTER) | Out-Null
  $back = [Drv]::WaitFgProc("Notepad", 2000)
  "notepad fg again after $back ms"
  Start-Sleep -Milliseconds 900
  [Drv]::Combo($VK.ENTER) | Out-Null   # separator line between the two inserts
  [Drv]::Combo($VK.ENTER) | Out-Null
}
Shot "notepad"
[Drv]::Combo($VK.CTRL, $VK.S) | Out-Null
Start-Sleep -Milliseconds 800
$after = ClipDigest
"clip after:  $after"
"clip restored identical: " + ($before -eq $after)
"history after:"; HistoryTexts 6

$saved = (Get-Content $file -Raw -Encoding UTF8) -replace "`r`n", "`n"
$last = (Get-Content "$env:LOCALAPPDATA\Qantrip\logs\last-insert.txt" -Raw -Encoding UTF8) -replace "`r`n", "`n"
"saved chars: $($saved.Length); last insert chars: $($last.Length)"
"last insert found verbatim in notepad: " + $saved.Contains($last)
# close our tab only
[Drv]::Combo($VK.CTRL, $VK.W) | Out-Null
Start-Sleep -Milliseconds 500
if (-not $hadNotepad) { Get-Process Notepad -ErrorAction SilentlyContinue | Stop-Process -Force }
CleanupHistory
LogTail 6
