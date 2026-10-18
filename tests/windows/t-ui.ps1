param([string]$Lang = "auto")
. "$PSScriptRoot\drv.ps1"
New-Item -ItemType Directory -Force "$Dev\data" | Out-Null
[IO.File]::WriteAllText("$Dev\data\settings.json", "{ ""language"": ""$Lang"" }")
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }
$np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { [Drv]::Focus($np.MainWindowHandle) | Out-Null }
[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
Start-Sleep -Milliseconds 400
Shot "ui-$Lang-home" 880 40 1700 1000 0.6
[Drv]::Type("r", 10); Start-Sleep -Milliseconds 300
Shot "ui-$Lang-r" 880 40 1700 1000 0.6
[Drv]::Type(" --sc", 10); Start-Sleep -Milliseconds 300
Shot "ui-$Lang-param" 880 40 1700 1000 0.6
[Drv]::Combo($VK.TAB) | Out-Null; Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.DOWN) | Out-Null; Start-Sleep -Milliseconds 200
Shot "ui-$Lang-value" 880 40 1700 1000 0.6
[Drv]::Combo($VK.TAB) | Out-Null; Start-Sleep -Milliseconds 200
[Drv]::Type(" --foo", 10); Start-Sleep -Milliseconds 300
[Drv]::Type(" ", 10); Start-Sleep -Milliseconds 300
Shot "ui-$Lang-error" 880 40 1700 1000 0.6
[Drv]::Combo($VK.ESC) | Out-Null
Get-Content $LogFile -Encoding UTF8 | Select-String 'debug_query' | Select-Object -Last 1
Remove-Item "$Dev\data\settings.json"
