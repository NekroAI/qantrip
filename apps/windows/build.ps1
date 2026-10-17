# Nekro Qantrip Windows 构建脚本：Rust 核心（C ABI）+ WinUI 3 外壳 → 自包含目录。
#
# 用法（在仓库根目录或任意位置）：
#   powershell -ExecutionPolicy Bypass -File apps\windows\build.ps1 [-Configuration Release|Debug] [-Out <目录>] [-SkipTests]
# 产物：默认 dist\windows\Qantrip\Qantrip.exe（整个目录可直接拷走运行，无需安装 .NET 或 Windows App SDK）。
param(
    [ValidateSet("Release", "Debug")] [string]$Configuration = "Release",
    [string]$Out = "",
    [switch]$SkipTests
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
