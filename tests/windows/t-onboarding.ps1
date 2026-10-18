# 首次启动引导：没有 onboardingDone 时启动即显示；快捷键确认（真实 Ctrl+Q）→ 插入方式 → 在练习框中施放一次 → 开始使用；
# 重启后不再显示。UI 自动化操作，不点鼠标；最后删除测试设置。
. "$PSScriptRoot\drv.ps1"
. "$PSScriptRoot\uia.ps1"
Add-Type -AssemblyName System.Windows.Forms
$data = "$Dev\data"
$settingsFile = "$data\settings.json"
New-Item -ItemType Directory -Force $data | Out-Null
Remove-Item $settingsFile -ErrorAction SilentlyContinue
$savedClip = [System.Windows.Forms.Clipboard]::GetText()
function J { if (Test-Path $settingsFile) { Get-Content $settingsFile -Raw -Encoding UTF8 | ConvertFrom-Json } }

EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data; QANTRIP_SKIP_ONBOARDING = "0" }
$w = Win "欢迎使用 Nekro Qantrip"
"onboarding shown on first launch: " + [bool]$w
Start-Sleep -Seconds 2
"foreground: " + ([Drv]::Fg() -match "Qantrip")
ShotWin "onb-1" $w
"before hotkey: " + (U (El $w "onb_verify").Current.Name)

# 第一步：按一次快捷键（同时会打开面板，Esc 关闭）
[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 1500) | Out-Null
Start-Sleep -Milliseconds 400
[Drv]::Combo($VK.ESC) | Out-Null
Start-Sleep -Milliseconds 500
"after hotkey: " + (U (El $w "onb_verify").Current.Name)
ShotWin "onb-1-verified" $w

# 第二步：插入方式（选只复制 → settings.json，再改回自动粘贴）
Invoke (El $w "onb_next")
Start-Sleep -Milliseconds 600
ShotWin "onb-2" $w
SelectItem (ByName $w "只复制，由我手动粘贴")
"copyOnly after choosing copy only: " + (J).copyOnly
SelectItem (ByName $w "自动粘贴到当前程序（推荐）")
"copyOnly after choosing auto-paste: " + (J).copyOnly

# 第三步：在练习框中施放一次
Invoke (El $w "onb_next")
Start-Sleep -Milliseconds 600
$box = El $w "onb_practice"
[Drv]::Focus([IntPtr]$w.Current.NativeWindowHandle) | Out-Null
$box.SetFocus(); Start-Sleep -Milliseconds 300
[Drv]::Combo($VK.CTRL, $VK.Q) | Out-Null
[Drv]::WaitFgProc("Qantrip", 1500) | Out-Null
Start-Sleep -Milliseconds 300
[Drv]::Type("zh", 30); Start-Sleep -Milliseconds 400
[Drv]::Combo($VK.ENTER) | Out-Null
Start-Sleep -Milliseconds 1500
$text = Val (El $w "onb_practice")
"practice text inserted: " + ($text.Length -gt 0) + " chars=" + $text.Length
"success shown: " + [bool](TryEl $w "onb_success")
ShotWin "onb-3" $w

# 开始使用 → 关闭并记录
Invoke (El $w "onb_done")
Start-Sleep -Milliseconds 500
"window closed: " + (-not (Win "欢迎使用 Nekro Qantrip" 3))
"onboardingDone: " + (J).onboardingDone

# 重启：不再显示
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data; QANTRIP_SKIP_ONBOARDING = "0" }
Start-Sleep -Seconds 2
"shown again after restart: " + [bool](Win "欢迎使用 Nekro Qantrip" 10)
Get-Content $LogFile -Encoding UTF8 | Select-String '"onboarding"' | Select-Object -Last 8

# 清理：退出应用，删除测试设置，恢复剪贴板文字
Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $settingsFile -ErrorAction SilentlyContinue
[Environment]::SetEnvironmentVariable("QANTRIP_SKIP_ONBOARDING", $null)
if ($savedClip) { [System.Windows.Forms.Clipboard]::SetText($savedClip) }
"clipboard restored: " + ([System.Windows.Forms.Clipboard]::GetText() -eq $savedClip)
