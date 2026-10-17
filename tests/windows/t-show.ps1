. "$PSScriptRoot\drv.ps1"
EnsureApp -Restart
LogTail 3
"fg before: " + [Drv]::Fg()
[Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
$t = [Drv]::WaitFgProc("Qantrip", 2000)
"panel foreground after $t ms"
Start-Sleep -Milliseconds 400
"fg: " + [Drv]::Fg()
Shot "panel"
[Drv]::Type("r --strict", 30)
Start-Sleep -Milliseconds 400
Shot "panel2"
[Drv]::Combo($VK.ESC) | Out-Null
Start-Sleep -Milliseconds 300
"fg after esc: " + [Drv]::Fg()
LogTail 4
