<#
.SYNOPSIS
    AetherShell Multi-Channel Release Packaging Pipeline (Gate G03, G15, G16, G20)
.DESCRIPTION
    Builds release binaries with compiler path remapping, stages portable ZIP and WiX MSI
    packages, and optionally runs the package artifact auditor to verify zero Explorer hooks.
#>

[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$OutputDir = "",
    [switch]$SkipMsi,
    [switch]$SkipPortable,
    [switch]$Audit
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = "$workspaceRoot\target\dist"
}

Write-Host "=== AetherShell Packaging Pipeline ===" -ForegroundColor Cyan
Write-Host "Workspace Root : $workspaceRoot"
Write-Host "Output Directory: $OutputDir"
Write-Host "Configuration   : $Configuration"

# 1. Build release binaries with compiler path remapping
Write-Host "`n1. Compiling release binaries with path remapping..." -ForegroundColor Cyan
$userHome = $env:USERPROFILE
$remapFlags = "--remap-path-prefix=$workspaceRoot=. --remap-path-prefix=$userHome=~"
$env:RUSTFLAGS = "-C target-feature=+crt-static $remapFlags"

cargo build --workspace --bin aethershell --bin aethershell-script-host --bin aethershell-settings --release
if ($LASTEXITCODE -ne 0) {
    throw "Cargo release build failed with exit code $LASTEXITCODE"
}

# 2. Package Portable ZIP
if (-not $SkipPortable) {
    Write-Host "`n2. Packaging Self-Contained Portable ZIP..." -ForegroundColor Cyan
    $portableScript = "$workspaceRoot\packaging\portable\build-portable.ps1"
    $portableOut = "$OutputDir\portable"
    & $portableScript -Configuration $Configuration -OutputDir $portableOut
    if ($LASTEXITCODE -ne 0) {
        throw "Portable ZIP build failed"
    }
}

# 3. Package WiX MSI
if (-not $SkipMsi) {
    Write-Host "`n3. Packaging WiX MSI Package..." -ForegroundColor Cyan
    $msiScript = "$workspaceRoot\packaging\wix\build-msi.ps1"
    $msiOut = "$OutputDir\msi"
    try {
        & $msiScript -Configuration $Configuration -OutputDir $msiOut
    } catch {
        Write-Warning "WiX MSI compilation skipped or failed: $($_.Exception.Message)"
        Write-Host "WiX Toolset v4 is optional for offline local staging; portable package is primary." -ForegroundColor Yellow
    }
}

# 4. Optional Package Artifact Audit (Gate G03, G16, G20)
if ($Audit) {
    Write-Host "`n4. Running Package Artifact Audit..." -ForegroundColor Cyan
    $auditScript = "$workspaceRoot\scripts\audit_package_artifacts.ps1"
    $portableZip = "$OutputDir\portable\AetherShell-v1.0.0-portable-win-x64.zip"
    if (Test-Path $portableZip) {
        & $auditScript -ZipArchive $portableZip -Strict -OutputManifestPath "$OutputDir\portable_manifest.json"
        if ($LASTEXITCODE -ne 0) {
            throw "Package artifact audit failed on portable ZIP"
        }
    }
}

Write-Host "`nPackaging Pipeline Completed Successfully." -ForegroundColor Green
