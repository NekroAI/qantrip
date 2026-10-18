. "$PSScriptRoot\drv.ps1"
. "$PSScriptRoot\clip.ps1"
New-Item -ItemType Directory -Force "$Dev\data" | Out-Null
[IO.File]::WriteAllText("$Dev\data\settings.json", '{ "copyOnly": true }')
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }
$np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { "focus: " + [Drv]::Focus($np.MainWindowHandle) }
$before = (Get-Process Notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowTitle
[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
Start-Sleep -Milliseconds 150
[Drv]::Type("en", 10); Start-Sleep -Milliseconds 200
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 1200
Shot "copyonly" 1720 700 1720 740 0.5
$after = (Get-Process Notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowTitle
"notepad title before/after (unchanged = not pasted): $before / $after"
$clip = [System.Windows.Forms.Clipboard]::GetText()
"clipboard now holds the prompt: " + ($clip.Length -gt 0) + " (" + $clip.Length + " chars)"
"fg: " + [Drv]::Fg()
Get-Content $LogFile -Encoding UTF8 | Select-String '"insert"' | Select-Object -Last 1
Remove-Item "$Dev\data\settings.json"
