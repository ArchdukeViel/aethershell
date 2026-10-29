<#
.SYNOPSIS
    Generates Third-Party Dependency Attribution, Software Bill of Materials (SBOM),
    and validates license compliance for AetherShell (Gate G16).

.DESCRIPTION
    Extracts all direct and transitive third-party dependencies from Cargo workspace metadata.
    Validates every dependency against an approved permissive license allowlist:
    - MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, CC0-1.0, Unicode-DFS-2016,
      Unicode-3.0, Zlib, BSL-1.0.
    Fails closed if any dependency contains GPL, AGPL, copyleft, or unapproved licenses.
    Outputs:
    - docs/licensing/THIRD_PARTY_LICENSES.md (Human-readable attribution ledger)
    - docs/licensing/sbom.json (SPDX-compatible machine-readable SBOM)

.PARAMETER Strict
    Fails with a non-zero exit code if any dependency violates the approved license policy
    or is missing license metadata.

.PARAMETER OutputDir
    Directory where attribution and SBOM files are written. Defaults to 'docs/licensing'.
#>

[CmdletBinding()]
param(
    [switch]$Strict,
    [string]$OutputDir = "docs/licensing"
)

$ErrorActionPreference = "Stop"

$workspaceRoot = if ($PSScriptRoot) { Split-Path -Parent $PSScriptRoot } else { (Get-Location).Path }
Set-Location $workspaceRoot

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "             AETHERSHELL THIRD-PARTY ATTRIBUTION & SBOM AUDIT (G16)            " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

# 1. Execute cargo metadata
Write-Host "`n[1/4] Querying Cargo workspace metadata..." -ForegroundColor Yellow
$metadataJson = cargo metadata --format-version 1
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($metadataJson)) {
    Write-Error "Failed to execute 'cargo metadata --format-version 1'."
    exit 1
}

$metadata = $metadataJson | ConvertFrom-Json
$workspaceMembers = @($metadata.workspace_members)
Write-Host "Found $($workspaceMembers.Count) workspace member crates." -ForegroundColor Green

# 2. Extract unique third-party packages in workspace dependency graph
Write-Host "`n[2/4] Resolving transitive third-party dependencies..." -ForegroundColor Yellow

$workspacePkgIds = [System.Collections.Generic.HashSet[string]]::new()
foreach ($m in $workspaceMembers) {
    [void]$workspacePkgIds.Add($m)
}

$thirdPartyPackages = [System.Collections.Generic.List[object]]::new()
$seenPackages = [System.Collections.Generic.HashSet[string]]::new()

foreach ($pkg in $metadata.packages) {
    if ($workspacePkgIds.Contains($pkg.id)) {
        continue
    }
    
    $key = "$($pkg.name)@$($pkg.version)"
    if ($seenPackages.Contains($key)) {
        continue
    }
    [void]$seenPackages.Add($key)

    $thirdPartyPackages.Add([PSCustomObject]@{
        Name        = $pkg.name
        Version     = $pkg.version
        License     = if ($pkg.license) { $pkg.license.Trim() } else { "UNKNOWN" }
        Repository  = if ($pkg.repository) { $pkg.repository.Trim() } else { "N/A" }
        Description = if ($pkg.description) { $pkg.description.Trim() } else { "N/A" }
        Authors     = if ($pkg.authors) { ($pkg.authors -join ", ") } else { "N/A" }
    })
}

$sortedPackages = $thirdPartyPackages | Sort-Object Name

Write-Host "Discovered $($sortedPackages.Count) unique third-party dependencies." -ForegroundColor Green

# 3. Validate License Compliance against Allowlist
Write-Host "`n[3/4] Validating license compliance against approved permissive allowlist..." -ForegroundColor Yellow

# Approved SPDX license tokens and common combinations
$allowedExactLicenses = @(
    "MIT",
    "Apache-2.0",
    "MIT OR Apache-2.0",
    "Apache-2.0 OR MIT",
    "MIT / Apache-2.0",
    "Apache-2.0 / MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "CC0-1.0",
    "0BSD",
    "Unicode-DFS-2016",
    "Unicode-3.0",
    "Zlib",
    "BSL-1.0",
    "MIT OR Zlib OR Apache-2.0",
    "Apache-2.0 OR BSD-3-Clause",
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
    "MIT OR Apache-2.0 OR Zlib"
)

$approvedTokens = @(
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "CC0-1.0",
    "0BSD", "Unicode-DFS-2016", "Unicode-3.0", "Zlib", "BSL-1.0", "LLVM-exception"
)

$prohibitedTokens = @("GPL", "AGPL", "SSPL", "Commons Clause")

$complianceViolations = [System.Collections.Generic.List[string]]::new()
$licenseBreakdown = @{}

foreach ($pkg in $sortedPackages) {
    $lic = $pkg.License
    
    if (-not $licenseBreakdown.ContainsKey($lic)) {
        $licenseBreakdown[$lic] = 0
    }
    $licenseBreakdown[$lic]++

    # Check for prohibited copyleft licenses
    $isProhibited = $false
    foreach ($proh in $prohibitedTokens) {
        if ($lic -match "\b$proh\b" -and $lic -notmatch "LGPL-.*-linking-exception") {
            $isProhibited = $true
            $complianceViolations.Add("PROHIBITED COPYLEFT: $($pkg.Name) v$($pkg.Version) uses '$lic'")
            break
        }
    }

    if ($isProhibited) { continue }

    # Check if exact match is in allowed list
    if ($allowedExactLicenses -contains $lic) {
        continue
    }

    # Evaluate OR expressions: if it contains OR and at least one allowed token matches without prohibited
    if ($lic -match "\bOR\b") {
        $parts = $lic -split "\bOR\b"
        $hasApprovedOption = $false
        foreach ($part in $parts) {
            $cleanPart = $part.Trim(" ()/")
            if ($approvedTokens -contains $cleanPart) {
                $hasApprovedOption = $true
                break
            }
        }
        if ($hasApprovedOption) {
            continue
        }
    }

    # If we get here, license is unverified / unapproved
    $complianceViolations.Add("UNAPPROVED / UNKNOWN LICENSE: $($pkg.Name) v$($pkg.Version) uses '$lic'")
}

Write-Host "`n--- License Breakdown ---" -ForegroundColor Cyan
foreach ($entry in ($licenseBreakdown.GetEnumerator() | Sort-Object Value -Descending)) {
    Write-Host ("  {0,-45} : {1,4}" -f $entry.Key, $entry.Value)
}

if ($complianceViolations.Count -gt 0) {
    Write-Host "`n[!] Compliance Violations Found: $($complianceViolations.Count)" -ForegroundColor Red
    foreach ($v in $complianceViolations) {
        Write-Host "    - $v" -ForegroundColor Red
    }
    if ($Strict) {
        Write-Error "Strict license compliance audit failed with $($complianceViolations.Count) violation(s)."
        exit 1
    }
} else {
    Write-Host "`nAll $($sortedPackages.Count) dependencies conform 100% to approved permissive licenses!" -ForegroundColor Green
}

# 4. Generate Output Files
Write-Host "`n[4/4] Writing attribution notices and SBOM..." -ForegroundColor Yellow

$resolvedOutputDir = if ([System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir
} else {
    Join-Path $workspaceRoot $OutputDir
}
if (-not (Test-Path $resolvedOutputDir)) {
    [void](New-Item -ItemType Directory -Path $resolvedOutputDir -Force)
}

# 4a. docs/licensing/THIRD_PARTY_LICENSES.md
$mdPath = Join-Path $resolvedOutputDir "THIRD_PARTY_LICENSES.md"
$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("# AetherShell Third-Party Software Attribution")
[void]$sb.AppendLine()
[void]$sb.AppendLine("This document contains third-party software notices and license attributions for direct and transitive dependencies utilized in AetherShell.")
[void]$sb.AppendLine("In compliance with **Gate G16 (License/Privacy Governance)**, all dependencies are audited to ensure full compatibility with the Apache-2.0 license and absence of non-permissive copyleft requirements.")
[void]$sb.AppendLine()
[void]$sb.AppendLine("## Summary Audit")
[void]$sb.AppendLine("- **Total Third-Party Dependencies**: $($sortedPackages.Count)")
[void]$sb.AppendLine("- **Audit Status**: $(if ($complianceViolations.Count -eq 0) { "100% COMPLIANT (All Permissive)" } else { "VIOLATIONS DETECTED" })")
[void]$sb.AppendLine("- **Generated**: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss UTC')")
[void]$sb.AppendLine()
[void]$sb.AppendLine("### License Distribution")
[void]$sb.AppendLine("| License Expression | Package Count |")
[void]$sb.AppendLine("| :--- | :--- |")
foreach ($entry in ($licenseBreakdown.GetEnumerator() | Sort-Object Value -Descending)) {
    [void]$sb.AppendLine("| ``$($entry.Key)`` | $($entry.Value) |")
}
[void]$sb.AppendLine()
[void]$sb.AppendLine("## Complete Dependency Ledger")
[void]$sb.AppendLine("| Package | Version | License | Repository |")
[void]$sb.AppendLine("| :--- | :--- | :--- | :--- |")
foreach ($pkg in $sortedPackages) {
    $repoLink = if ($pkg.Repository -ne "N/A" -and $pkg.Repository.StartsWith("http")) { "[$($pkg.Repository)]($($pkg.Repository))" } else { $pkg.Repository }
    [void]$sb.AppendLine("| **$($pkg.Name)** | ``$($pkg.Version)`` | $($pkg.License) | $repoLink |")
}
[void]$sb.AppendLine()

[System.IO.File]::WriteAllText($mdPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Generated markdown attribution: $mdPath" -ForegroundColor Green

# 4b. docs/licensing/sbom.json (Machine-readable SPDX-format SBOM)
$sbomPath = Join-Path $resolvedOutputDir "sbom.json"
$sbomPackages = @()
foreach ($pkg in $sortedPackages) {
    $sbomPackages += @{
        name             = $pkg.Name
        versionInfo      = $pkg.Version
        licenseConcluded = $pkg.License
        licenseDeclared  = $pkg.License
        downloadLocation = $pkg.Repository
        description      = $pkg.Description
        originator       = $pkg.Authors
    }
}

$sbomObject = [ordered]@{
    spdxVersion  = "SPDX-2.3"
    dataLicense  = "CC0-1.0"
    SPDXID       = "SPDXRef-DOCUMENT"
    name         = "AetherShell-SBOM"
    documentNamespace = "https://github.com/ArchdukeViel/aethershell/sbom/$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    creationInfo = @{
        creators = @("Tool: AetherShell SBOM Generator v1.0", "Organization: AetherShell Contributors")
        created  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    }
    packages     = $sbomPackages
}

$sbomJson = $sbomObject | ConvertTo-Json -Depth 10
[System.IO.File]::WriteAllText($sbomPath, $sbomJson, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Generated SPDX SBOM json: $sbomPath" -ForegroundColor Green

Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host "                   SBOM & LICENSE AUDIT COMPLETED SUCCESSFULLY                  " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
