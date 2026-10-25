# AetherShell MSIX Packaging Automation Script (PRD Section 16 & Gate G15)
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = "$PSScriptRoot\..\..\target\dist\msix"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Packaging AetherShell MSIX ===" -ForegroundColor Cyan

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
$BinDir = "$WorkspaceRoot\target\$Configuration"
if (-not (Test-Path "$BinDir\aethershell.exe")) {
    $BinDir = "$WorkspaceRoot\target\x86_64-pc-windows-msvc\$Configuration"
}
$StageDir = "$PSScriptRoot\layout"

# 1. Prepare clean layout directory
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path "$StageDir\Assets" | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# 2. Copy binaries
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

# 3. Copy manifest, assets, schemas, licenses
Copy-Item "$PSScriptRoot\AppxManifest.xml" "$StageDir\AppxManifest.xml"
Copy-Item "$WorkspaceRoot\LICENSE" "$StageDir\LICENSE"
Copy-Item "$WorkspaceRoot\NOTICE.md" "$StageDir\NOTICE.md"
Copy-Item -Recurse "$WorkspaceRoot\assets" "$StageDir\assets"
Copy-Item -Recurse "$WorkspaceRoot\schemas" "$StageDir\schemas"

# 4. Generate minimal placeholder visual assets if missing
$AssetFiles = @("StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png", "Wide310x150Logo.png", "Square71x71Logo.png", "SplashScreen.png")
foreach ($asset in $AssetFiles) {
    $dst = "$StageDir\Assets\$asset"
    if (-not (Test-Path $dst)) {
        # Create minimal 1x1 PNG bytes
        $pngBytes = [byte[]]@(137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1, 13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130)
        [System.IO.File]::WriteAllBytes($dst, $pngBytes)
    }
}

# 5. Locate MakeAppx.exe from Windows SDK
$SdkPaths = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^10\." } | Sort-Object Name -Descending
$MakeAppx = $null
foreach ($sdk in $SdkPaths) {
    $candidate = "$($sdk.FullName)\x64\makeappx.exe"
    if (Test-Path $candidate) {
        $MakeAppx = $candidate
        break
    }
}

if ($MakeAppx) {
    Write-Host "Creating MSIX package using $MakeAppx..." -ForegroundColor Cyan
    & $MakeAppx pack /d $StageDir /p "$OutputDir\ArchdukeViel.AetherShell_1.0.0.0_x64.msix" /nv /o
    Write-Host "MSIX generated: $OutputDir\ArchdukeViel.AetherShell_1.0.0.0_x64.msix" -ForegroundColor Green
} else {
    Write-Warning "makeappx.exe not located in Windows SDK paths. Staged layout preserved in $StageDir for packaging."
}
