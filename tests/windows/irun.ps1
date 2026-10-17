# 在已登录用户的交互会话中运行一个 PowerShell 脚本（隐藏窗口），等待结束并输出其日志。
# 用法：irun.ps1 <script.ps1> [timeoutSec] [args...]
param([string]$Script, [int]$Timeout = 120, [string]$ScriptArgs = "")
$dir = $PSScriptRoot
$log = "$dir\irun.log"
$done = "$dir\irun.done"
Remove-Item $log, $done -ErrorAction SilentlyContinue
# 包装脚本：运行目标脚本，输出写入日志（UTF-8），结束时写标记文件
$wrap = "$dir\irun-wrap.ps1"
@"
[Console]::OutputEncoding=[Text.Encoding]::UTF8
`$ProgressPreference='SilentlyContinue'
try { Invoke-Expression "& '$Script' $ScriptArgs" *>&1 | Out-File -FilePath '$log' -Encoding utf8 } catch { `$_ | Out-File -FilePath '$log' -Append -Encoding utf8 }
'x' | Out-File -FilePath '$done'
"@ | Out-File $wrap -Encoding utf8
# wscript 是 GUI 子系统，不会弹出控制台窗口抢占前台
$vbs = "$dir\irun.vbs"
$q = [char]34
$line = 'CreateObject("WScript.Shell").Run "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ' + $q + $q + $wrap + $q + $q + '", 0, False'
$line | Out-File $vbs -Encoding ascii
$action = New-ScheduledTaskAction -Execute "wscript.exe" -Argument ($q + $vbs + $q)
$principal = New-ScheduledTaskPrincipal -UserId (whoami) -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 30)
Register-ScheduledTask -TaskName "qantrip-irun" -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName "qantrip-irun"
$sw = [Diagnostics.Stopwatch]::StartNew()
while (-not (Test-Path $done) -and $sw.Elapsed.TotalSeconds -lt $Timeout) { Start-Sleep -Milliseconds 300 }
if (-not (Test-Path $done)) { "TIMEOUT after $Timeout s" }
if (Test-Path $log) { Get-Content $log -Encoding UTF8 }
