param([int]$N = 30)
. "$PSScriptRoot\drv.ps1"
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data" }
$np = Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($np) { "focus notepad: " + [Drv]::Focus($np.MainWindowHandle) }
$startLines = (Get-Content $LogFile -Encoding UTF8).Count
$fg = @(); $back = @()
for ($i = 0; $i -lt $N; $i++) {
  [Drv]::Combo($VK.ALT, $VK.Q) | Out-Null
  $fg += [Drv]::WaitFgProc("Qantrip", 2000)
  Start-Sleep -Milliseconds 250
  [Drv]::Combo($VK.ESC) | Out-Null
  $back += [Drv]::WaitFgProc("Notepad", 2000)
  Start-Sleep -Milliseconds 250
}
function Stats($name, $xs) {
  $s = $xs | Sort-Object
  $p = { param($q) $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($q * $s.Count))] }
  "{0}: n={1} first={2} median={3} p95={4} max={5}" -f $name, $s.Count, $xs[0], (& $p 0.5), (& $p 0.95), $s[-1]
}
Stats "driver: Alt+Q -> panel foreground (ms)" $fg
Stats "driver: Esc -> previous window foreground (ms)" $back
$shows = Get-Content $LogFile -Encoding UTF8 | Select-Object -Skip $startLines | Where-Object { $_ -match '"event":"show"' } | ForEach-Object { $_ | ConvertFrom-Json }
Stats "app: handler -> panel visible+focused (ms)" @($shows | ForEach-Object { [double]$_.handler_to_shown_ms })
Stats "app: handler -> first rendered frame (ms)" @($shows | ForEach-Object { [double]$_.handler_to_frame_ms })
Stats "app: WM_HOTKEY queue delay (ms)" @($shows | ForEach-Object { [double]$_.event_to_handler_ms })
"foreground ok: " + (@($shows | Where-Object { $_.foreground_ok }).Count) + "/" + $shows.Count
Get-Process Qantrip | Select-Object @{n="WorkingSetMB";e={[int]($_.WorkingSet64/1MB)}}, @{n="PrivateMB";e={[int]($_.PrivateMemorySize64/1MB)}}, @{n="CPUs";e={$_.CPU}}
