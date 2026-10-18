# Nekro Qantrip Windows 构建脚本：Rust 核心（C ABI）+ WinUI 3 外壳 → 自包含目录。
#
# 用法（在仓库根目录或任意位置）：
#   powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1 [-Configuration Release|Debug] [-Out <目录>] [-SkipTests] [-NoZip] [-NoInstaller]
# 产物：默认 dist\windows\Qantrip\Qantrip.exe（整个目录可直接拷走运行，无需安装 .NET 或 Windows App SDK），
#       该目录的压缩包 dist\windows\Qantrip-Windows-x64.zip（解压后得到 Qantrip\ 文件夹），
#       以及安装程序 dist\windows\Qantrip-Windows-x64-Setup.exe（需要 Inno Setup 6.6+，没有安装时跳过）。
param(
    [ValidateSet("Release", "Debug")] [string]$Configuration = "Release",
    [string]$Out = "",
    [switch]$SkipTests,
    [switch]$NoZip,
    [switch]$NoInstaller
)
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Resolve-Path (Join-Path $here "..\..")
if (-not $Out) { $Out = Join-Path $repo "dist\windows\Qantrip" }
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

function Step($name, [scriptblock]$block) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Write-Host "==> $name"
    & $block
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw "$name 失败（退出码 $LASTEXITCODE）" }
    Write-Host ("    {0:N1}s" -f $sw.Elapsed.TotalSeconds)
}

$native = Join-Path $here "native"
if (-not $SkipTests) {
    Step "核心测试" { cargo test --release --manifest-path (Join-Path $repo "core\Cargo.toml") --quiet }
    Step "C ABI 测试" { cargo test --release --manifest-path (Join-Path $native "Cargo.toml") --quiet }
}
Step "构建 Rust 核心（qantrip_ffi.dll）" { cargo build --release --manifest-path (Join-Path $native "Cargo.toml") }

$proj = Join-Path $here "Qantrip\Qantrip.csproj"
$version = (Get-Content (Join-Path $repo "VERSION") -Raw).Trim()
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
Step "发布 WinUI 外壳（自包含 win-x64）" {
    dotnet publish $proj -c $Configuration -r win-x64 --self-contained true -p:Platform=x64 -p:Version=$version -o $Out --nologo -v:minimal
}
$size = (Get-ChildItem $Out -Recurse | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("完成：{0}\Qantrip.exe（目录 {1:N1} MB）" -f $Out, $size)

if (-not $NoZip) {
    $zip = Join-Path (Split-Path -Parent $Out) "Qantrip-Windows-x64.zip"
    Step "打包 Qantrip-Windows-x64.zip" {
        if (Test-Path $zip) { Remove-Item $zip -Force }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        # 包含顶层 Qantrip\ 文件夹：解压后即可运行其中的 Qantrip.exe
        [IO.Compression.ZipFile]::CreateFromDirectory($Out, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)
    }
    Write-Host ("压缩包：{0}（{1:N1} MB）" -f $zip, ((Get-Item $zip).Length / 1MB))
}

# 安装程序（Inno Setup 6.6+，按用户安装）：找到 ISCC 时生成 Qantrip-Windows-x64-Setup.exe，否则跳过
if (-not $NoInstaller) {
    $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $candidates = @($env:LOCALAPPDATA, ${env:ProgramFiles(x86)}, $env:ProgramFiles) | Where-Object { $_ } |
            ForEach-Object { Get-ChildItem (Join-Path $_ "Programs\Inno Setup *\ISCC.exe"), (Join-Path $_ "Inno Setup *\ISCC.exe") -ErrorAction SilentlyContinue }
        $iscc = ($candidates | Sort-Object FullName -Descending | Select-Object -First 1).FullName
    }
    if ($iscc) {
        $setupDir = Split-Path -Parent $Out
        $setup = Join-Path $setupDir "Qantrip-Windows-x64-Setup.exe"
        Step "打包安装程序 Qantrip-Windows-x64-Setup.exe（$iscc）" {
            if (Test-Path $setup) { Remove-Item $setup -Force }
            & $iscc /Q "/DAppVersion=$version" "/DSourceDir=$Out" "/O$setupDir" (Join-Path $here "installer\Qantrip.iss")
        }
        Write-Host ("安装程序：{0}（{1:N1} MB）" -f $setup, ((Get-Item $setup).Length / 1MB))
    }
    else { Write-Host "==> 跳过安装程序：没有找到 Inno Setup 的 ISCC.exe（winget install JRSoftware.InnoSetup）" }
}
