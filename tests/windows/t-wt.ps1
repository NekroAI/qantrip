param([string]$Shell = "powershell")
. "$PSScriptRoot\drv.ps1"
$T = "$Dev\t"; New-Item -ItemType Directory -Force $T | Out-Null
New-Item -ItemType Directory -Force "$Dev\data\templates" | Out-Null
$tpl = @"
---
name: wtt
description: Windows Terminal multi-line paste test
---
echo qantrip-line1 > $T\wt-marker1.txt
echo qantrip-line2 > $T\wt-marker2.txt
"@
[IO.File]::WriteAllText("$Dev\data\templates\wtt.md", $tpl.Replace("`r`n", "`n"), (New-Object Text.UTF8Encoding($false)))
Remove-Item "$T\wt-marker*.txt" -ErrorAction SilentlyContinue
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }

$hadWt = [bool](Get-Process WindowsTerminal -ErrorAction SilentlyContinue)
Remove-Item "$T\bp-out.txt" -ErrorAction SilentlyContinue
$exe = if ($Shell -eq "cmd") { "cmd.exe" } elseif ($Shell -eq "bp") { "python.exe $PSScriptRoot\bp-probe.py $T\bp-out.txt 9" } else { "powershell.exe -NoLogo" }
Start-Process wt.exe -ArgumentList "-w new nt --title QTEST-$Shell --suppressApplicationTitle $exe"
$w = $null
for ($i = 0; $i -lt 40 -and -not $w; $i++) { Start-Sleep -Milliseconds 250; $w = Get-Process WindowsTerminal -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -like "QTEST*" } | Select-Object -First 1 }
if (-not $w) { "no WT window"; exit }
Start-Sleep -Seconds 2
"wt focus: " + [Drv]::Focus($w.MainWindowHandle) + " " + [Drv]::Fg()

[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
"panel fg after " + [Drv]::WaitFgProc("Qantrip", 2000) + " ms"
Start-Sleep -Milliseconds 150
[Drv]::Type("wtt", 20)
Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.ENTER) | Out-Null
"wt fg again after " + [Drv]::WaitFgProc("WindowsTerminal", 2000) + " ms"
Start-Sleep -Milliseconds 1500
Shot "wt-$Shell"
"marker1 exists (line 1 executed): " + (Test-Path "$T\wt-marker1.txt")
"marker2 exists (line 2 executed): " + (Test-Path "$T\wt-marker2.txt")
"fg: " + [Drv]::Fg()
Get-Content $LogFile -Encoding UTF8 | Select-String '"insert"' | Select-Object -Last 1

if ($Shell -eq "bp") {
  Start-Sleep -Seconds 6
  "probe received: " + (Get-Content "$T\bp-out.txt" -Raw -Encoding UTF8)
} else {
  # WT multi-line paste warning: accept ("paste anyway" is the default button) to see what the shell does
  [Drv]::Combo($VK.ENTER) | Out-Null
  Start-Sleep -Milliseconds 1500
  Shot "wt-$Shell-accepted"
  "after accepting warning: marker1=" + (Test-Path "$T\wt-marker1.txt") + " marker2=" + (Test-Path "$T\wt-marker2.txt")
}
# cleanup: dismiss any dialog, clear the line, close the window
[Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.CTRL, 0x43) | Out-Null; Start-Sleep -Milliseconds 300
if (-not $hadWt) { Get-Process WindowsTerminal -ErrorAction SilentlyContinue | Stop-Process -Force }
Remove-Item "$Dev\data\templates\wtt.md"
