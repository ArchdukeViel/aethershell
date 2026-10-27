# AetherShell Portable Build Pipeline with Dynamic Path Remapping
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [switch]$CheckOnly = $false
)

$ErrorActionPreference = "Stop"

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$UserHome = $env:USERPROFILE

# Dynamically construct path remapping for reproducible, leak-free binaries
$RemapFlags = "--remap-path-prefix=$WorkspaceRoot=. --remap-path-prefix=$UserHome=~"
$env:RUSTFLAGS = "-C target-feature=+crt-static $RemapFlags"

Write-Host "=== AetherShell Portable Build Pipeline ===" -ForegroundColor Cyan
Write-Host "Workspace: $WorkspaceRoot"
Write-Host "Configuration: $Configuration"

if ($CheckOnly) {
    cargo check --workspace
} else {
    if ($Configuration -eq "release") {
        cargo build --workspace --release
    } else {
        cargo build --workspace
    }
}
