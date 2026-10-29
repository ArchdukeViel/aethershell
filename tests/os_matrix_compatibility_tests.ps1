<#
.SYNOPSIS
    Automated Regression Tests for OS Matrix Compatibility (Gate G02, PRD Section 2.1)
.DESCRIPTION
    Validates the 7-tier OS matrix audit script, aethershell-win version detection unit tests,
    capability mappings, BOM-free JSON output, and host AppData zero pollution.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditScript = Join-Path $workspaceRoot "scripts\audit_os_matrix_compatibility.ps1"

if (-not (Test-Path $auditScript)) {
    throw "Audit script not found at $auditScript"
}

$testResults = [System.Collections.Generic.List[object]]::new()

function Run-TestCase([string]$Name, [scriptblock]$TestBlock) {
    try {
        & $TestBlock
        $testResults.Add([PSCustomObject]@{ Name = $Name; Status = "Passed"; Error = $null })
        Write-Host "  [PASS] $Name" -ForegroundColor Green
    } catch {
        $testResults.Add([PSCustomObject]@{ Name = $Name; Status = "Failed"; Error = $_.Exception.Message })
        Write-Host "  [FAIL] $Name : $($_.Exception.Message)" -ForegroundColor Red
    }
}

Write-Host "Running os_matrix_compatibility_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Test 1: aethershell-win OS version unit tests pass
# -----------------------------------------------------------------------------
Run-TestCase "Cargo aethershell-win os_version unit tests pass cleanly" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-win --lib os_version 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "test result: ok\.") {
        throw "Expected 'test result: ok.', got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 2: audit_os_matrix_compatibility.ps1 executes and produces valid JSON
# -----------------------------------------------------------------------------
Run-TestCase "audit_os_matrix_compatibility.ps1 generates valid 7-tier JSON report" {
    $tempReport = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell_OsAudit_$([Guid]::NewGuid().ToString('N').Substring(0, 8)).json"
    try {
        $out = powershell -NoProfile -ExecutionPolicy Bypass -File "$auditScript" -OutputJson "$tempReport" 2>&1
        $exitCode = $LASTEXITCODE
        $outStr = ($out | Out-String)

        if ($exitCode -ne 0) {
            throw "Expected exit code 0, got $exitCode. Output: $outStr"
        }
        if (-not (Test-Path $tempReport)) {
            throw "Expected output report at $tempReport"
        }

        # Parse JSON
        $reportJson = Get-Content -LiteralPath $tempReport -Raw | ConvertFrom-Json
        if ($reportJson.ReportType -ne "OsMatrixCompatibilityAudit") {
            throw "Expected ReportType == 'OsMatrixCompatibilityAudit', got: $($reportJson.ReportType)"
        }
        if ($reportJson.TotalTiers -ne 7) {
            throw "Expected TotalTiers == 7 per PRD Section 2.1, got: $($reportJson.TotalTiers)"
        }
        if ($reportJson.SupportedV1Tiers -ne 6) {
            throw "Expected SupportedV1Tiers == 6, got: $($reportJson.SupportedV1Tiers)"
        }
        if ($reportJson.SpecialTiers -ne 1) {
            throw "Expected SpecialTiers == 1, got: $($reportJson.SpecialTiers)"
        }

        # Validate BOM-free UTF-8
        $bytes = [System.IO.File]::ReadAllBytes($tempReport)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "Found UTF-8 BOM in report"
        }
    } finally {
        if (Test-Path $tempReport) {
            Remove-Item -LiteralPath $tempReport -Force -ErrorAction SilentlyContinue
        }
    }
}

# -----------------------------------------------------------------------------
# Test 3: Host Registry contains valid Windows Version and UBR values
# -----------------------------------------------------------------------------
Run-TestCase "Host registry contains valid Windows Version, Build, and UBR" {
    $regKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
    $build = (Get-ItemProperty -Path $regKey -Name CurrentBuild -ErrorAction SilentlyContinue).CurrentBuild
    $ubr = (Get-ItemProperty -Path $regKey -Name UBR -ErrorAction SilentlyContinue).UBR

    if ([string]::IsNullOrWhiteSpace($build)) {
        throw "CurrentBuild in registry is missing or empty"
    }
    if ($null -eq $ubr) {
        throw "UBR in registry is missing"
    }
    [uint32]$buildNum = $build
    if ($buildNum -lt 19041) {
        throw "Build number $buildNum is below minimum Windows 10 baseline"
    }
}

# -----------------------------------------------------------------------------
# Test 4: Verify production %LOCALAPPDATA%\AetherShell is preserved at strictly 0 items
# -----------------------------------------------------------------------------
Run-TestCase "Verify production %LOCALAPPDATA%\AetherShell is preserved at strictly 0 items" {
    $localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $prodDir = Join-Path $localAppData "AetherShell"
    if (Test-Path $prodDir) {
        $items = @(Get-ChildItem -Path $prodDir -Recurse -Force -ErrorAction SilentlyContinue)
        if ($items.Count -ne 0) {
            throw "Host AppData contains $($items.Count) items! Isolation failed."
        }
    }
}

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Cyan
$failedCount = @($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$passedCount = @($testResults | Where-Object { $_.Status -eq "Passed" }).Count

Write-Host "OS Matrix Compatibility Test Summary: $passedCount/$($testResults.Count) passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($failedCount -gt 0) {
    exit 1
}

exit 0
