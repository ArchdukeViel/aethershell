# AetherShell WiX MSI Build Automation Script (PRD Section 16 & Gate G15)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = "$PSScriptRoot\..\..\target\dist\msi"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Building AetherShell MSI Package ===" -ForegroundColor Cyan

# 1. Ensure target binaries exist
$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
$BinDir = "$WorkspaceRoot\target\$Configuration"
if (-not (Test-Path "$BinDir\aethershell.exe")) {
    $BinDir = "$WorkspaceRoot\target\x86_64-pc-windows-msvc\$Configuration"
}

$RequiredBins = @("aethershell.exe", "aethershell-settings.exe", "aethershell-script-host.exe")
foreach ($bin in $RequiredBins) {
    $src = "$BinDir\$bin"
    if (-not (Test-Path $src)) {
        Write-Warning "Binary '$src' not found. Building release workspace binaries..."
        cargo build --workspace --bin aethershell --bin aethershell-settings --bin aethershell-script-host --release
        break
    }
}

# 2. Stage staging directory
$StageDir = "$PSScriptRoot\stage"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

foreach ($bin in $RequiredBins) {
    Copy-Item "$BinDir\$bin" "$StageDir\$bin"
}

# 3. Locate or execute WiX
$WixExe = "$env:USERPROFILE\.dotnet\tools\wix.exe"
if (-not (Test-Path $WixExe)) {
    $WixExe = "wix"
}

Write-Host "Compiling MSI using WiX Toolset..." -ForegroundColor Cyan
Push-Location $StageDir
try {
    & $WixExe build "$PSScriptRoot\Package.wxs" -o "$OutputDir\AetherShell-v1.0.0-x64.msi"
    Write-Host "Successfully generated MSI: $OutputDir\AetherShell-v1.0.0-x64.msi" -ForegroundColor Green
}
finally {
    Pop-Location
    if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
}
