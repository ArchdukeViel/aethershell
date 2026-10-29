<#
.SYNOPSIS
    Automated Regression Tests for OS Matrix Compatibility (Gate G02, PRD Section 2.1, Section 18.2)
.DESCRIPTION
    Validates the 7-tier OS matrix audit script, aethershell-win and aethershell-taskbar unit tests,
    Cargo integration tests, capability mappings, honest fallback contracts, BOM-free JSON output,
    and host AppData zero pollution.
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
# Test 2: aethershell-taskbar backend unit tests pass
# -----------------------------------------------------------------------------
Run-TestCase "Cargo aethershell-taskbar backend unit tests pass cleanly" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-taskbar --lib backend 2>&1"
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
# Test 3: os_matrix_compatibility_integration_test passes cleanly
# -----------------------------------------------------------------------------
Run-TestCase "Cargo os_matrix_compatibility_integration_test passes cleanly" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test --test os_matrix_compatibility_integration_test 2>&1"
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
# Test 4: audit_os_matrix_compatibility.ps1 executes and produces valid JSON
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
# Test 5: Validate all 7 authoritative PRD Section 2.1 tier IDs and architectures
# -----------------------------------------------------------------------------
Run-TestCase "Validate all 7 authoritative PRD Section 2.1 tier IDs and architectures" {
    $tempReport = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell_OsAudit_Tiers_$([Guid]::NewGuid().ToString('N').Substring(0, 8)).json"
    try {
        $null = powershell -NoProfile -ExecutionPolicy Bypass -File "$auditScript" -OutputJson "$tempReport" 2>&1
        $reportJson = Get-Content -LiteralPath $tempReport -Raw | ConvertFrom-Json

        $expectedTierIds = @(
            "VM-WIN11-26H2",
            "VM-WIN11-25H2",
            "VM-WIN11-24H2",
            "VM-WIN11-23H2",
            "VM-WIN11-22H2",
            "VM-WIN10-22H2",
            "VM-WIN11-26H1"
        )

        foreach ($expectedId in $expectedTierIds) {
            $tier = $reportJson.AuthoritativeTiers | Where-Object { $_.TierId -eq $expectedId }
            if ($null -eq $tier) {
                throw "Missing expected TierId '$expectedId' in audit ledger"
            }
            if ([string]::IsNullOrWhiteSpace($tier.TaskbarArchitecture)) {
                throw "Tier '$expectedId' has empty TaskbarArchitecture"
            }
            if ([string]::IsNullOrWhiteSpace($tier.HonestFallbackContract)) {
                throw "Tier '$expectedId' has empty HonestFallbackContract"
            }
        }
    } finally {
        if (Test-Path $tempReport) {
            Remove-Item -LiteralPath $tempReport -Force -ErrorAction SilentlyContinue
        }
    }
}

# -----------------------------------------------------------------------------
# Test 6: Verify honest fallback contracts across legacy and XAML tiers
# -----------------------------------------------------------------------------
Run-TestCase "Verify honest fallback contracts across legacy and XAML tiers" {
    $tempReport = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell_OsAudit_Fallback_$([Guid]::NewGuid().ToString('N').Substring(0, 8)).json"
    try {
        $null = powershell -NoProfile -ExecutionPolicy Bypass -File "$auditScript" -OutputJson "$tempReport" 2>&1
        $reportJson = Get-Content -LiteralPath $tempReport -Raw | ConvertFrom-Json

        $win10Tier = $reportJson.AuthoritativeTiers | Where-Object { $_.TierId -eq "VM-WIN10-22H2" }
        if ($win10Tier.TaskbarArchitecture -notmatch "Legacy Win32") {
            throw "Expected Win10 taskbar architecture to be Legacy Win32, got: $($win10Tier.TaskbarArchitecture)"
        }
        if ($win10Tier.HonestFallbackContract -notmatch "Mica & Acrylic unsupported") {
            throw "Expected Win10 honest fallback contract to declare Mica & Acrylic unsupported"
        }

        $win11GaTier = $reportJson.AuthoritativeTiers | Where-Object { $_.TierId -eq "VM-WIN11-26H2" }
        if ($win11GaTier.TaskbarArchitecture -notmatch "XAML Island") {
            throw "Expected Win11 26H2 taskbar architecture to be XAML Island, got: $($win11GaTier.TaskbarArchitecture)"
        }

        $win11SpecialTier = $reportJson.AuthoritativeTiers | Where-Object { $_.TierId -eq "VM-WIN11-26H1" }
        if ($win11SpecialTier.IsV1Supported) {
            throw "Win11 26H1 (Build 28000) must NOT be marked IsV1Supported"
        }
        if (-not $win11SpecialTier.IsSpecialValidationTier) {
            throw "Win11 26H1 (Build 28000) must have IsSpecialValidationTier == true"
        }
    } finally {
        if (Test-Path $tempReport) {
            Remove-Item -LiteralPath $tempReport -Force -ErrorAction SilentlyContinue
        }
    }
}

# -----------------------------------------------------------------------------
# Test 7: Host Registry contains valid Windows Version and UBR values
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
# Test 8: Verify production %LOCALAPPDATA%\AetherShell is preserved at strictly 0 items
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
# Test 9: AppEngine default coordinator initializes with ExternalAppearanceBackend
# -----------------------------------------------------------------------------
Run-TestCase "AppEngine default coordinator initializes with ExternalAppearanceBackend and honest fallback" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test --test os_matrix_compatibility_integration_test test_app_engine_default_coordinator_uses_external_appearance_with_os_info 2>&1"
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
