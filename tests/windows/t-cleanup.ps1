. "$PSScriptRoot\drv.ps1"
# Close our Notepad test tabs (ime-test.txt / notepad-test.txt) without touching the owner's tabs.
for ($i = 0; $i -lt 4; $i++) {
  $np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -match "ime-test|notepad-test" } | Select-Object -First 1
  if (-not $np) { break }
  "closing: " + $np.MainWindowTitle
  [Drv]::Focus($np.MainWindowHandle) | Out-Null
  [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 200
  [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 200
  [Drv]::Combo($VK.CTRL, $VK.S) | Out-Null; Start-Sleep -Milliseconds 600
  [Drv]::Combo($VK.CTRL, $VK.W) | Out-Null; Start-Sleep -Milliseconds 600
}
Shot "cleanup"
"fg: " + [Drv]::Fg()
