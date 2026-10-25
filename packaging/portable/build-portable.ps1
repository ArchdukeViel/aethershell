# AetherShell Self-Contained Portable ZIP Packaging Script (PRD Section 16 & Gate G15 / G20)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = "$PSScriptRoot\..\..\target\dist\portable"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Packaging AetherShell Portable ZIP ===" -ForegroundColor Cyan

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
$BinDir = "$WorkspaceRoot\target\$Configuration"
if (-not (Test-Path "$BinDir\aethershell.exe")) {
    $BinDir = "$WorkspaceRoot\target\x86_64-pc-windows-msvc\$Configuration"
}
$StageDir = "$PSScriptRoot\stage"

# 1. Clean staging directory
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# 2. Build or verify release binaries
$RequiredBins = @("aethershell.exe", "aethershell-settings.exe", "aethershell-script-host.exe")
foreach ($bin in $RequiredBins) {
    $src = "$BinDir\$bin"
    if (-not (Test-Path $src)) {
        Write-Warning "Binary '$src' not found. Building release workspace binaries..."
        cargo build --workspace --bin aethershell --bin aethershell-settings --bin aethershell-script-host --release
        break
    }
}
foreach ($bin in $RequiredBins) {
    Copy-Item "$BinDir\$bin" "$StageDir\$bin"
}

# 3. Create portable mode marker file (PRD Q38:B / Gate G20)
# Presence of this file triggers resolve_app_data_dir() to use adjacent data\ folder instead of %LOCALAPPDATA%
New-Item -ItemType File -Force -Path "$StageDir\aethershell.portable" | Out-Null
New-Item -ItemType Directory -Force -Path "$StageDir\data" | Out-Null

# 4. Copy assets, schemas, licenses, readme
Copy-Item "$WorkspaceRoot\LICENSE" "$StageDir\LICENSE"
Copy-Item "$WorkspaceRoot\NOTICE.md" "$StageDir\NOTICE.md"
Copy-Item "$WorkspaceRoot\README.md" "$StageDir\README.md"
Copy-Item -Recurse "$WorkspaceRoot\assets" "$StageDir\assets"
Copy-Item -Recurse "$WorkspaceRoot\schemas" "$StageDir\schemas"

# 5. Compress into ZIP archive
$ZipOutput = "$OutputDir\AetherShell-v1.0.0-portable-win-x64.zip"
if (Test-Path $ZipOutput) { Remove-Item -Force $ZipOutput }

Write-Host "Compressing portable archive to $ZipOutput..." -ForegroundColor Cyan
Compress-Archive -Path "$StageDir\*" -DestinationPath $ZipOutput -CompressionLevel Optimal

Write-Host "Successfully generated portable archive: $ZipOutput" -ForegroundColor Green
Remove-Item -Recurse -Force $StageDir
