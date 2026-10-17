. "$PSScriptRoot\drv.ps1"
# Second open after the user switched the panel to Chinese: is the panel back in English?
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }
$np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { "focus: " + [Drv]::Focus($np.MainWindowHandle) }
foreach ($round in 1, 2) {
  [Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
  [Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
  Start-Sleep -Milliseconds 200
  [Drv]::TypeVk("rev", 40); [Drv]::Combo($VK.SPACE) | Out-Null
  if ($round -eq 1) {
    [Drv]::Combo($VK.SHIFT) | Out-Null; Start-Sleep -Milliseconds 200   # user switches to Chinese
    [Drv]::TypeVk("zhong", 60); [Drv]::Combo($VK.SPACE) | Out-Null
  }
  Start-Sleep -Milliseconds 300
  [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300
  if ([Drv]::WaitFgProc("Notepad", 300) -lt 0) { [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 300 }
}
Get-Content $LogFile -Encoding UTF8 | Select-String 'debug_query|"show"' | Select-Object -Last 4
