<#
.SYNOPSIS
    Automated Regression Tests for Gate G06 Desktop Layering & Icon Passthrough (PRD Section 18.2, Section 5.1, Section 5.2, Section 5.3, Q31:A, Q37:A)
.DESCRIPTION
    Validates that:
    1. Cargo aethershell-desktop unit tests pass cleanly (32/32 tests).
    2. Cargo aethershell-app desktop_layering_integration_test suite passes all 8 tests.
    3. 4-tier desktop stacking order (Wallpaper < Widgets < Icons < Apps) is strictly enforced.
    4. Extended styles enforce click pass-through (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) and strictly prohibit WS_EX_TOPMOST.
    5. Failure case F44 & Section 5.1 honest fallback cleanly disables widget layer when hierarchy is absent.
    6. Safety boundary Q37:A enforcement prevents disruptive operations on physical daily-use host.
    7. Host AppData (%LOCALAPPDATA%\AetherShell) remains strictly unpolluted at 0 items.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$binDir = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\debug"
if (-not (Test-Path $binDir)) {
    $binDir = Join-Path $workspaceRoot "target\debug"
}
if (-not (Test-Path $binDir)) {
    $binDir = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release"
}
if (-not (Test-Path $binDir)) {
    $binDir = Join-Path $workspaceRoot "target\release"
}
$coreExe = Join-Path $binDir "aethershell.exe"

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

Write-Host "Running desktop_layering_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Test 1: Cargo aethershell-desktop unit tests
# -----------------------------------------------------------------------------
Run-TestCase "Cargo aethershell-desktop unit tests pass cleanly" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-desktop 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "(\d+)\s+passed" -or [int]$matches[1] -lt 32) {
        throw "Expected at least 32 passed tests in aethershell-desktop, got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 2: Cargo desktop_layering_integration_test suite
# -----------------------------------------------------------------------------
Run-TestCase "Cargo desktop_layering_integration_test passes all 8 tests" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-app --test desktop_layering_integration_test 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "8 passed") {
        throw "Expected 8 passed tests in desktop_layering_integration_test, got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 3: Stacking tier and honest fallback contract
# -----------------------------------------------------------------------------
Run-TestCase "Stacking tier hierarchy and F44 honest disabled fallback contract" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-desktop --lib test_desktop_stacking_tier 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "test result: ok\.") {
        throw "Expected test result: ok., got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 4: Extended window styles enforce click pass-through and zero WS_EX_TOPMOST
# -----------------------------------------------------------------------------
Run-TestCase "Extended styles enforce WS_EX_TRANSPARENT | WS_EX_NOACTIVATE and zero WS_EX_TOPMOST" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-desktop --lib test_surface_extended_styles 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "test result: ok\.") {
        throw "Expected test result: ok., got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 5: Live desktop hierarchy discovery and validation
# -----------------------------------------------------------------------------
Run-TestCase "Live desktop hierarchy discovers SHELLDLL_DefView and validates stacking targets" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-win --lib test_desktop_probing_safe_execution 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "test result: ok\.") {
        throw "Expected test result: ok., got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 6: Safety boundary Q37:A fails closed on physical host
# -----------------------------------------------------------------------------
Run-TestCase "Safety boundary Q37:A fails closed on physical daily-use host" {
    $guestScript = Join-Path $workspaceRoot "scripts\guest_sandbox_qualification.ps1"
    if (-not (Test-Path $guestScript)) {
        throw "Guest sandbox qualification script missing at $guestScript"
    }
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File `"$guestScript`" 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 2) {
        throw "Expected exit code 2 (safety boundary enforcement), got $exitCode. Output: $outStr"
    }
    if ($outStr -notmatch "SAFETY BOUNDARY ENFORCEMENT") {
        throw "Expected 'SAFETY BOUNDARY ENFORCEMENT' in output, got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 7: Host AppData zero pollution
# -----------------------------------------------------------------------------
Run-TestCase "Host AppData (%LOCALAPPDATA%\AetherShell) remains strictly unpolluted at 0 items" {
    $appData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $aetherDir = Join-Path $appData "AetherShell"
    if (Test-Path $aetherDir) {
        $count = (Get-ChildItem -Path $aetherDir -Recurse -Force).Count
        if ($count -ne 0) {
            throw "Expected 0 items in $aetherDir, found $count"
        }
    }
}

# -----------------------------------------------------------------------------
# Summary and Disposition
# -----------------------------------------------------------------------------
Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Gray
$passed = @($testResults | Where-Object { $_.Status -eq "Passed" }).Count
$failed = @($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$total = $testResults.Count

Write-Host "Desktop Layering Test Summary: $passed/$total passed ($failed failed)" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Red" })

if ($failed -gt 0) {
    exit 1
} else {
    exit 0
}
