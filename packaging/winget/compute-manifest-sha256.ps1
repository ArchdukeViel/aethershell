# WinGet Manifest SHA-256 Computation and Verification Script (PRD Section 16 & Gate G15)
[CmdletBinding()]
param(
    [string]$MsiPath = "",
    [switch]$UpdateManifest = $true
)

$ErrorActionPreference = "Stop"

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
if ([string]::IsNullOrWhiteSpace($MsiPath)) {
    $MsiPath = "$WorkspaceRoot\target\dist\msi\AetherShell-v1.0.0-x64.msi"
}

if (-not (Test-Path $MsiPath)) {
    throw "Target MSI installer not found at '$MsiPath'. Please build MSI first via 'packaging\wix\build-msi.ps1'."
}

Write-Host "=== Computing WinGet Manifest SHA-256 Checksum ===" -ForegroundColor Cyan
Write-Host "MSI Target: $MsiPath"

$hashResult = Get-FileHash -Path $MsiPath -Algorithm SHA256
$sha256 = $hashResult.Hash.ToUpperInvariant()

Write-Host "Calculated SHA-256: $sha256" -ForegroundColor Green

$ManifestPath = "$PSScriptRoot\manifests\a\ArchdukeViel\AetherShell\1.0.0\ArchdukeViel.AetherShell.installer.yaml"

if ($UpdateManifest) {
    if (-not (Test-Path $ManifestPath)) {
        throw "WinGet installer manifest not found at '$ManifestPath'."
    }

    $content = [System.IO.File]::ReadAllText($ManifestPath)
    $pattern = 'InstallerSha256:\s*[A-Fa-f0-9]{64}'
    $replacement = "InstallerSha256: $sha256"

    if ($content -match $pattern) {
        $updatedContent = [System.Text.RegularExpressions.Regex]::Replace($content, $pattern, $replacement)
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($ManifestPath, $updatedContent, $utf8NoBom)
        Write-Host "Successfully updated '$ManifestPath' with SHA-256: $sha256" -ForegroundColor Green
    } else {
        throw "Could not find 'InstallerSha256:' line in '$ManifestPath' to update."
    }
}
