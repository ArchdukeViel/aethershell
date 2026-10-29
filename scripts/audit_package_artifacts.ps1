<#
.SYNOPSIS
    AetherShell Package Artifact & Shell Isolation Auditor (Gate G03, G16, G20)
.DESCRIPTION
    Audits packaged distribution artifacts (directories or ZIP archives) to verify:
    1. Gate G03 Shell Isolation: Exactly zero in-process Explorer extensions, detours, or global system hooks.
    2. Gate G16 License & Privacy: Presence of Apache-2.0 LICENSE, NOTICE.md, and verified path remapping (no developer paths in binaries).
    3. Gate G20 Packaging Identity: Adherence to PRD Q38:B (zero portable marker files overriding canonical %LOCALAPPDATA%\AetherShell).
    4. Computes cryptographic SHA-256 manifest across all packaged binaries.
#>

[CmdletBinding()]
param(
    [string]$PackageDir,
    [string]$ZipArchive,
    [switch]$Strict,
    [switch]$Json,
    [string]$OutputManifestPath
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditErrors = [System.Collections.Generic.List[string]]::new()
$auditWarnings = [System.Collections.Generic.List[string]]::new()
$binaryManifest = [System.Collections.Generic.List[object]]::new()

# Resolve target directory
$targetDir = $null
$isTempExtracted = $false

if (-not [string]::IsNullOrWhiteSpace($ZipArchive)) {
    if (-not (Test-Path $ZipArchive)) {
        throw "Specified ZIP archive does not exist: $ZipArchive"
    }
    $targetDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aethershell_pkg_audit_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Expand-Archive -Path $ZipArchive -DestinationPath $targetDir -Force
    $isTempExtracted = $true
} elseif (-not [string]::IsNullOrWhiteSpace($PackageDir)) {
    if (-not (Test-Path $PackageDir)) {
        throw "Specified package directory does not exist: $PackageDir"
    }
    $targetDir = (Resolve-Path $PackageDir).Path
} else {
    # Default to portable staging or release directory
    $candidateDirs = @(
        "$workspaceRoot\packaging\portable\stage",
        "$workspaceRoot\packaging\wix\stage",
        "$workspaceRoot\target\dist\portable",
        "$workspaceRoot\target\release"
    )
    foreach ($cand in $candidateDirs) {
        if (Test-Path $cand) {
            $targetDir = $cand
            break
        }
    }
    if ($null -eq $targetDir) {
        $targetDir = "$workspaceRoot\target\release"
    }
}

try {
    Write-Host "=== AetherShell Package Artifact & Shell Isolation Audit ===" -ForegroundColor Cyan
    Write-Host "Audit Target Directory: $targetDir" -ForegroundColor Gray

    # 1. Audit mandatory documentation and schema files (Gate G16)
    $hasLicense = (Test-Path (Join-Path $targetDir "LICENSE")) -or (Test-Path (Join-Path $targetDir "LICENSE.txt"))
    if (-not $hasLicense) {
        $auditErrors.Add("Gate G16 Violation: Missing Apache-2.0 LICENSE file in package root.")
    }

    $hasNotice = (Test-Path (Join-Path $targetDir "NOTICE.md")) -or (Test-Path (Join-Path $targetDir "NOTICE"))
    if (-not $hasNotice) {
        $auditWarnings.Add("Gate G16 Warning: NOTICE.md not found in package root.")
    }

    # 2. Audit PRD Q38:B Portable Marker Violation (Gate G20)
    $portableMarker = Join-Path $targetDir "aethershell.portable"
    if (Test-Path $portableMarker) {
        $auditErrors.Add("Gate G20 Violation: Prohibited 'aethershell.portable' marker found. PRD Q38:B mandates all modes share canonical AppData (%LOCALAPPDATA%\AetherShell).")
    }

    $localDataDir = Join-Path $targetDir "data"
    if (Test-Path $localDataDir) {
        $auditWarnings.Add("Gate G20 Warning: Local 'data' folder found in package layout. All runtime configuration must reside in canonical AppData.")
    }

    # 3. Discover and inspect all binary files (.exe, .dll)
    $binFiles = Get-ChildItem -Path $targetDir -Recurse -Include @("*.exe", "*.dll") -File

    if ($binFiles.Count -eq 0) {
        $msg = "No executable or DLL binaries found in audit target directory ($targetDir)."
        if ($Strict) {
            $auditErrors.Add($msg)
        } else {
            $auditWarnings.Add($msg)
        }
    }

    $forbiddenStrings = @(
        "IShellIconOverlayIdentifier",
        "IContextMenu",
        "IExplorerCommand",
        "detours.dll",
        "minhook.x64.dll"
    )

    $devUserPattern = "C:\\Users\\"

    foreach ($file in $binFiles) {
        $relPath = $file.FullName.Substring($targetDir.Length).TrimStart('\', '/')
        $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
        $sha256 = (Get-FileHash -Path $file.FullName -Algorithm SHA256).Hash

        # String extraction for static symbol / import scanning
        # Scan for prohibited Explorer extension markers and detours
        $contentAscii = [System.Text.Encoding]::ASCII.GetString($bytes)

        $violations = [System.Collections.Generic.List[string]]::new()
        foreach ($pattern in $forbiddenStrings) {
            if ($contentAscii.Contains($pattern)) {
                $violations.Add("Prohibited Explorer extension / hook marker: '$pattern'")
                $auditErrors.Add("Gate G03 Violation in '$relPath': Detected $pattern")
            }
        }

        # Check for non-remapped developer usernames in release binaries
        $hasDevPathLeak = $false
        if ($contentAscii -match 'C:\\Users\\([a-zA-Z0-9_\-\.]+)\\') {
            $detectedUser = $Matches[1]
            # Ignore standard Windows Defender / SDK path strings if generic
            if ($detectedUser -notmatch '^(Default|Public|runneradmin|All Users)$') {
                $hasDevPathLeak = $true
                $auditWarnings.Add("Gate G16 Notice in '$relPath': Potential developer workstation path detected: '$($Matches[0])'")
            }
        }

        $binaryManifest.Add([PSCustomObject]@{
            RelativePath = $relPath
            FileName = $file.Name
            SizeBytes = $file.Length
            SHA256 = $sha256
            Violations = $violations
            HasDevPathLeak = $hasDevPathLeak
        })
    }

    # Print Report
    Write-Host "`nAudited $($binFiles.Count) binaries across package layout:" -ForegroundColor Cyan
    foreach ($item in $binaryManifest) {
        $statusStr = if ($item.Violations.Count -eq 0) { "[PASS]" } else { "[FAIL]" }
        $color = if ($item.Violations.Count -eq 0) { "Green" } else { "Red" }
        Write-Host "  $statusStr $($item.RelativePath) ($([math]::Round($item.SizeBytes / 1KB, 1)) KB) - SHA256: $($item.SHA256.Substring(0, 16))..." -ForegroundColor $color
    }

    Write-Host "`nAudit Summary:" -ForegroundColor Cyan
    Write-Host "  License Verified     : $hasLicense"
    Write-Host "  PRD Q38:B Compliance : $(-not (Test-Path $portableMarker))"
    Write-Host "  Errors Found         : $($auditErrors.Count)" -ForegroundColor $(if ($auditErrors.Count -eq 0) { "Green" } else { "Red" })
    Write-Host "  Warnings Found       : $($auditWarnings.Count)" -ForegroundColor $(if ($auditWarnings.Count -eq 0) { "Green" } else { "Yellow" })

    foreach ($err in $auditErrors) {
        Write-Host "  [ERROR] $err" -ForegroundColor Red
    }
    foreach ($warn in $auditWarnings) {
        Write-Host "  [WARN]  $warn" -ForegroundColor Yellow
    }

    # Write Manifest if requested
    if (-not [string]::IsNullOrWhiteSpace($OutputManifestPath)) {
        $manifestObj = [PSCustomObject]@{
            PackageTarget = $targetDir
            AuditTimestamp = (Get-Date -Format "o")
            TotalBinaries = $binFiles.Count
            Binaries = $binaryManifest
            Errors = $auditErrors
            Warnings = $auditWarnings
            Verdict = if ($auditErrors.Count -eq 0) { "PASSED" } else { "FAILED" }
        }
        $manifestJson = $manifestObj | ConvertTo-Json -Depth 5
        [System.IO.File]::WriteAllText($OutputManifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host "Wrote manifest to: $OutputManifestPath" -ForegroundColor Green
    }

    if ($Json) {
        $manifestObj = [PSCustomObject]@{
            PackageTarget = $targetDir
            AuditTimestamp = (Get-Date -Format "o")
            TotalBinaries = $binFiles.Count
            Binaries = $binaryManifest
            Errors = $auditErrors
            Warnings = $auditWarnings
            Verdict = if ($auditErrors.Count -eq 0) { "PASSED" } else { "FAILED" }
        }
        $manifestObj | ConvertTo-Json -Depth 5
    }

    if ($auditErrors.Count -gt 0) {
        exit 1
    } else {
        exit 0
    }
} finally {
    if ($isTempExtracted -and (Test-Path $targetDir)) {
        Remove-Item -Recurse -Force -Path $targetDir -ErrorAction SilentlyContinue
    }
}
