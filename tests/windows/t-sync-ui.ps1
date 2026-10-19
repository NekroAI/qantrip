# 设置 › 同步（UI 自动化，不点鼠标）。
# 1) 真实客户端（staging 服务端，未登录）：状态为未登录；点"使用 GitHub 登录"后本机回调监听启动（调试构建不打开浏览器），
#    用伪造的授权码请求回调地址 → 回调页面返回"登录完成"，换取令牌被服务端拒绝 → 界面显示错误。不产生任何凭据。
# 2) QANTRIP_DEBUG_SYNC_MOCK 模拟各个状态并截图：创建 / 加入 / 校验码 / 确认码 / 恢复短语 / 已加入 / 停止写入。
. "$PSScriptRoot\drv.ps1"
. "$PSScriptRoot\uia.ps1"
$data = "$Dev\data"
$settingsFile = "$data\settings.json"
New-Item -ItemType Directory -Force $data | Out-Null
$staging = "https://qantrip-staging.nekro.ai"
[IO.File]::WriteAllText($settingsFile, "{ `"syncServer`": `"$staging`" }")
$syncDir = "$data\sync\qantrip-staging.nekro.ai"

function OpenSync {
  Start-Process $App -ArgumentList "--settings"
  $w = Win "Nekro Qantrip 设置"
  Start-Sleep -Milliseconds 800
  SelectItem (El $w "nav_sync")
  Start-Sleep -Milliseconds 1200
  return $w
}

# 1) 真实客户端：未登录
[Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_MOCK", $null)
[Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_ADVANCED", "1")
EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }
$start = (Get-Content $LogFile -Encoding UTF8).Count
$w = OpenSync
"signed-out page: signin button=" + [bool](TryEl $w "sync_signin") + " server field=" + (Val (El $w "sync_server")) + " editable=" + (El $w "sync_server").Current.IsEnabled
ShotWin "sync-signedout" $w
Invoke (El $w "sync_signin")
Start-Sleep -Milliseconds 1500
"working: " + (U (El $w "sync_working").Current.Name)
$ev = Get-Content $LogFile -Encoding UTF8 | Select-Object -Skip $start | Where-Object { $_ -match '"debug_signin"' } | Select-Object -Last 1 | ConvertFrom-Json
"redirect: " + $ev.redirect + " authorize host: " + $ev.authorize_host
ShotWin "sync-signing-in" $w
# 无关请求（如 favicon）返回 404，不结束监听
try { Invoke-WebRequest -UseBasicParsing "$($ev.redirect -replace '/callback$', '/favicon.ico')" | Out-Null } catch { "favicon: " + $_.Exception.Response.StatusCode.value__ }
$page = Invoke-WebRequest -UseBasicParsing "$($ev.redirect)?code=qantrip-test-invalid-code&state=qantrip-test-state"
"callback page: status=" + $page.StatusCode + " signed-in text=" + ($page.Content -match "Qantrip")
Start-Sleep -Seconds 4
$err = TryEl $w "sync_error"
"error shown: " + [bool]$err
Get-Content $LogFile -Encoding UTF8 | Select-Object -Skip $start | Where-Object { $_ -match '"sync_error"|"sync_start"' }
ShotWin "sync-signin-error" $w
"still signed out: " + [bool](TryEl $w "sync_signin")
CloseWin $w
"secrets file after failed sign-in: " + (Test-Path "$syncDir\secrets.bin")

# 2) 模拟状态截图
foreach ($state in @("novault", "join", "join-sas", "enrolled", "blocked", "phrase")) {
  [Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_MOCK", $state)
  [Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_ADVANCED", $(if ($state -eq "enrolled") { "1" } else { $null }))
  EnsureApp -Restart -Env @{ QANTRIP_DEBUG_HOOKS = "1"; QANTRIP_DATA_DIR = $data }
  $w = OpenSync
  switch ($state) {
    "join-sas" { "sas: " + (El $w "sync_sas").Current.Name }
    "enrolled" { "approved: " + (El $w "sync_approved").Current.Name + " / choices: " + (El $w "sync_choice0").Current.Name + " / last sync: " + (U (El $w "sync_lastsync").Current.Name) + " / server locked: " + (-not (El $w "sync_server").Current.IsEnabled) }
    "phrase" {
      $done = ByName $w "完成"
      "phrase dialog: shown=" + [bool](TryEl $w "sync_phrasesaved") + " done enabled before check=" + $done.Current.IsEnabled
    }
  }
  ShotWin "sync-$state" $w
  if ($state -eq "enrolled") {
    # 滚到底部：设备、轮换密钥、重置、高级
    (Pat (El $w "settings_scroll") ([Windows.Automation.ScrollPattern]::Pattern)).SetScrollPercent(-1, 100); Start-Sleep -Milliseconds 500
    ShotWin "sync-enrolled-bottom" $w
    Invoke (El $w "sync_remove_B3D5F7H9")
    Start-Sleep -Milliseconds 300
    ShotWin "sync-revoke-dialog" $w
    Invoke (ByName $w "取消")
  }
  if ($state -eq "phrase") {
    # Esc 不能关闭；勾选"已抄写"后才能完成
    [Drv]::Combo($VK.ESC) | Out-Null; Start-Sleep -Milliseconds 400
    "after Esc: dialog still open=" + [bool](TryEl $w "sync_phrasesaved")
    (Pat (El $w "sync_phrasesaved") ([Windows.Automation.TogglePattern]::Pattern)).Toggle(); Start-Sleep -Milliseconds 300
    $done = ByName $w "完成"
    "done enabled after check=" + $done.Current.IsEnabled
    Invoke $done
    Start-Sleep -Milliseconds 500
    "dialog closed: " + (-not (TryEl $w "sync_phrasesaved"))
  }
  CloseWin $w
}

# 清理：去掉测试用环境变量与设置、同步状态目录，退出应用
[Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_MOCK", $null)
[Environment]::SetEnvironmentVariable("QANTRIP_DEBUG_SYNC_ADVANCED", $null)
Get-Process Qantrip -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $settingsFile -ErrorAction SilentlyContinue
Remove-Item "$data\sync" -Recurse -Force -ErrorAction SilentlyContinue
"cleaned: " + (-not (Test-Path "$data\sync"))
