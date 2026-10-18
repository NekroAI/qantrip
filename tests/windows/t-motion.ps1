# 动效（D15）抽帧：唤起入场、回车被阻止时抖动；QANTRIP_REDUCE_MOTION=1 时应无动效（模拟系统关闭"动画效果"）。
param([string]$Reduce = "0")
. "$PSScriptRoot\drv.ps1"
if (Test-Path "$Dev\data\settings.json") { Remove-Item "$Dev\data\settings.json" }
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = "$Dev\data"; QANTRIP_REDUCE_MOTION = $Reduce }
$tag = if ($Reduce -eq "1") { "reduced" } else { "motion" }
$sw = [Diagnostics.Stopwatch]::StartNew()
[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 2000) | Out-Null
"panel fg at $($sw.ElapsedMilliseconds) ms"
foreach ($i in 0..2) { Shot "$tag-enter-$i" 880 40 1700 1000 0.35; "  shot at $($sw.ElapsedMilliseconds) ms" }
Start-Sleep -Milliseconds 400
[Drv]::Type("r --nope", 10); Start-Sleep -Milliseconds 300
$sw.Restart()
[Drv]::Combo($VK.ENTER) | Out-Null
foreach ($i in 0..1) { Shot "$tag-shake-$i" 880 40 1700 1000 0.35; "  shot at $($sw.ElapsedMilliseconds) ms" }
Start-Sleep -Milliseconds 400
Shot "$tag-shake-end" 880 40 1700 1000 0.35
[Drv]::Combo($VK.ESC) | Out-Null
Get-Content $LogFile -Encoding UTF8 | Select-String '"launch"' | Select-Object -Last 1 | ForEach-Object { "motion enabled: " + ($_ | ConvertFrom-Json).motion }
