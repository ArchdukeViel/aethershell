# AetherShell Self-Contained Portable ZIP Packaging Script (PRD Section 16 & Gate G15 / G20 / Q38:B)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

Write-Host "=== Packaging AetherShell Portable ZIP ===" -ForegroundColor Cyan

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = "$WorkspaceRoot\target\dist\portable"
}
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
$RequiredBins = @("aethershell.exe", "aethershell-script-host.exe", "aethershell-settings.exe")
foreach ($bin in $RequiredBins) {
    $src = "$BinDir\$bin"
    if (-not (Test-Path $src)) {
        Write-Warning "Binary '$src' not found. Building release workspace binaries..."
        cargo build --workspace --bin aethershell --bin aethershell-script-host --bin aethershell-settings --release
        break
    }
}
foreach ($bin in $RequiredBins) {
    Copy-Item "$BinDir\$bin" "$StageDir\$bin"
}

# 3. Include WinUI 3 Settings Application & Self-Contained Runtime (PRD Section 16 Table 16 / Q30:A)
$WinUiSettingsDir = "$WorkspaceRoot\apps\aethershell-settings\x64\Release"
if (Test-Path "$WinUiSettingsDir\aethershell-settings.exe") {
    Write-Host "Packaging native WinUI 3 Settings application and self-contained runtime..." -ForegroundColor Green
    Copy-Item "$WinUiSettingsDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
    # Copy self-contained runtime dependencies (DLLs, PRIs, XBFs)
    Get-ChildItem -Path "$WinUiSettingsDir\*" -Include @("*.dll", "*.pri", "*.xbf") -File | ForEach-Object {
        Copy-Item $_.FullName "$StageDir\$($_.Name)"
    }
    if (Test-Path "$WinUiSettingsDir\Microsoft.UI.Xaml") {
        Copy-Item -Recurse "$WinUiSettingsDir\Microsoft.UI.Xaml" "$StageDir\Microsoft.UI.Xaml"
    }
} elseif (Test-Path "$BinDir\aethershell-settings.exe") {
    Write-Host "Packaging fallback Settings binary..." -ForegroundColor Yellow
    Copy-Item "$BinDir\aethershell-settings.exe" "$StageDir\aethershell-settings.exe"
}

# 4. PRD Q38:B Storage Contract:
# Portable application binaries use canonical AppData (%LOCALAPPDATA%\AetherShell), NOT portable settings
# beside the executable. Strictly omit aethershell.portable marker file and local data\ directory.

# 5. Copy assets, schemas, licenses, readme
Copy-Item "$WorkspaceRoot\LICENSE" "$StageDir\LICENSE"
Copy-Item "$WorkspaceRoot\NOTICE.md" "$StageDir\NOTICE.md"
Copy-Item "$WorkspaceRoot\README.md" "$StageDir\README.md"
Copy-Item -Recurse "$WorkspaceRoot\assets" "$StageDir\assets"
Copy-Item -Recurse "$WorkspaceRoot\schemas" "$StageDir\schemas"

# 6. Compress into ZIP archive
$ZipOutput = "$OutputDir\AetherShell-v1.0.0-portable-win-x64.zip"
if (Test-Path $ZipOutput) { Remove-Item -Force $ZipOutput }

Write-Host "Compressing portable archive to $ZipOutput..." -ForegroundColor Cyan
Compress-Archive -Path "$StageDir\*" -DestinationPath $ZipOutput -CompressionLevel Optimal

Write-Host "Successfully generated portable archive: $ZipOutput" -ForegroundColor Green
Remove-Item -Recurse -Force $StageDir
