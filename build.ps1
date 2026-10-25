<#
.SYNOPSIS
    Builds mdview.exe (Windows x64, MSVC toolchain) into .\dist
    Result is ONE self-contained exe: WebView2 loader and C runtime are linked in; KaTeX,
    Mermaid and highlight.js are embedded. Needs only the Edge WebView2 Runtime at run time
    (already present on up-to-date Windows 10/11).
#>
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust not found. Install it from https://rustup.rs (choose the default MSVC toolchain)," -ForegroundColor Yellow
    Write-Host "plus 'Visual Studio Build Tools' with the 'Desktop development with C++' workload." -ForegroundColor Yellow
    exit 1
}
rustup target add x86_64-pc-windows-msvc | Out-Null

cargo test --release
if ($LASTEXITCODE -ne 0) { Write-Error "tests failed" }

cargo build --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { Write-Error "build failed" }

New-Item -ItemType Directory -Force -Path dist | Out-Null
Copy-Item "target\x86_64-pc-windows-msvc\release\mdview.exe" dist\mdview.exe -Force
$size = [math]::Round((Get-Item dist\mdview.exe).Length / 1MB, 1)
Write-Host "Built dist\mdview.exe ($size MB)" -ForegroundColor Green
