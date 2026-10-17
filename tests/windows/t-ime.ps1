param([string]$Scope = "Url")
. "$PSScriptRoot\drv.ps1"
$T = "$Dev\t"; New-Item -ItemType Directory -Force $T | Out-Null
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data"; QANTRIP_IME_SCOPE = $Scope }
"scope: $Scope"
$file = "$T\ime-test.txt"
Set-Content $file "" -NoNewline
Start-Process notepad.exe $file
Start-Sleep -Seconds 2
$np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
"notepad focus: " + [Drv]::Focus($np.MainWindowHandle)

# 1) baseline: pinyin in Notepad (Chinese if the IME is in Chinese mode)
[Drv]::TypeVk("nihao", 40); [Drv]::Combo($VK.SPACE) | Out-Null; Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.ENTER) | Out-Null

# 2) panel: real virtual keys through the IME
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
"panel fg after " + [Drv]::WaitFgProc("Qantrip", 2000) + " ms"
Start-Sleep -Milliseconds 200
[Drv]::TypeVk("rev", 40); [Drv]::Combo($VK.SPACE) | Out-Null
[Drv]::TypeVk("security", 30); [Drv]::Combo($VK.SPACE) | Out-Null
Start-Sleep -Milliseconds 300
Shot "ime-panel"
# user switches to Chinese manually (Shift) for the extra text
[Drv]::Combo($VK.SHIFT) | Out-Null; Start-Sleep -Milliseconds 200
[Drv]::TypeVk("nihao", 40); [Drv]::Combo($VK.SPACE) | Out-Null
Start-Sleep -Milliseconds 300
Shot "ime-panel2"
[Drv]::Combo($VK.ESC) | Out-Null
Start-Sleep -Milliseconds 300
if ([Drv]::WaitFgProc("Notepad", 300) -lt 0) { "esc #1 consumed; second esc"; [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300 }
"back to: " + [Drv]::Fg()

# 3) Notepad after closing the panel
[Drv]::TypeVk("nihao", 40); [Drv]::Combo($VK.SPACE) | Out-Null; Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.CTRL, $VK.S) | Out-Null
Start-Sleep -Milliseconds 600
"notepad content: " + ((Get-Content $file -Raw -Encoding UTF8) -replace "`r?`n", " | ")
[Drv]::Combo($VK.CTRL, $VK.W) | Out-Null
Get-Content $LogFile -Encoding UTF8 | Select-String '"show"|debug_query' | Select-Object -Last 2
