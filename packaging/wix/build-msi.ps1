# AetherShell WiX MSI Build Automation Script (PRD Section 16 & Gate G15)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

Write-Host "=== Building AetherShell MSI Package ===" -ForegroundColor Cyan

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = "$WorkspaceRoot\target\dist\msi"
}

# 1. Ensure target binaries exist
$BinDir = "$WorkspaceRoot\target\$Configuration"
if (-not (Test-Path "$BinDir\aethershell.exe")) {
    $BinDir = "$WorkspaceRoot\target\x86_64-pc-windows-msvc\$Configuration"
}

$RequiredBins = @("aethershell.exe", "aethershell-script-host.exe")
foreach ($bin in $RequiredBins) {
    $src = "$BinDir\$bin"
    if (-not (Test-Path $src)) {
        Write-Warning "Binary '$src' not found. Building release workspace binaries..."
        cargo build --workspace --bin aethershell --bin aethershell-script-host --release
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

# Copy WinUI 3 Settings Application & Self-Contained Runtime (PRD Section 16 Table 16)
$WinUiSettingsDir = "$WorkspaceRoot\apps\aethershell-settings\x64\Release"
if (Test-Path "$WinUiSettingsDir\aethershell-settings.exe") {
    Write-Host "Packaging native WinUI 3 Settings application and runtime dependencies..." -ForegroundColor Green
    Copy-Item "$WinUiSettingsDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
    Get-ChildItem -Path "$WinUiSettingsDir\*" -Include @("*.dll", "*.pri", "*.xbf") -File | ForEach-Object {
        Copy-Item $_.FullName "$StageDir\$($_.Name)"
    }
} elseif (Test-Path "$BinDir\aethershell-settings.exe") {
    Write-Host "Packaging fallback Settings binary..." -ForegroundColor Yellow
    Copy-Item "$BinDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
} else {
    throw "aethershell-settings.exe not found in either $WinUiSettingsDir or $BinDir"
}

# Stage assets and schemas for packaging
Copy-Item -Recurse "$WorkspaceRoot\assets" "$StageDir\assets"
Copy-Item -Recurse "$WorkspaceRoot\schemas" "$StageDir\schemas"

# 3. Locate or execute WiX
$WixExe = "$env:USERPROFILE\.dotnet\tools\wix.exe"
if (-not (Test-Path $WixExe)) {
    $WixCmd = Get-Command wix.exe -ErrorAction SilentlyContinue
    if ($WixCmd) {
        $WixExe = $WixCmd.Source
    } else {
        $WixExe = "wix"
    }
}

Write-Host "Compiling MSI using WiX Toolset..." -ForegroundColor Cyan
Push-Location $StageDir
try {
    & $WixExe build -arch x64 "$PSScriptRoot\Package.wxs" -o "$OutputDir\AetherShell-v1.0.0-x64.msi"
    if ($LASTEXITCODE -ne 0) {
        throw "WiX build failed with exit code $LASTEXITCODE"
    }
    Write-Host "Successfully generated MSI: $OutputDir\AetherShell-v1.0.0-x64.msi" -ForegroundColor Green
}
finally {
    Pop-Location
    if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
}
