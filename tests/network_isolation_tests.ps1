<#
.SYNOPSIS
    Automated Regression Tests for scripts/audit_runtime_network_isolation.ps1 (Gate G16)
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditScript = Join-Path $workspaceRoot "scripts\audit_runtime_network_isolation.ps1"
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

Write-Host "Running network_isolation_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: Standard audit with -Strict fails closed when no process exists (Finding P2)
Run-TestCase "audit_runtime_network_isolation.ps1 fails closed in -Strict mode when no process exists" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -eq 0) {
        throw "Expected non-zero exit code when no processes exist in -Strict mode, got 0"
    }
    if ($outputStr -notmatch "No running AetherShell processes found to audit") {
        throw "Expected fail-closed error message, got: $outputStr"
    }
}

# Test 2: Live process isolation audit with -SpawnTestProcess succeeds and verifies 0 sockets
Run-TestCase "audit_runtime_network_isolation.ps1 with -Strict and -SpawnTestProcess succeeds on live process" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -Strict -SpawnTestProcess 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "Zero network sockets or remote connections detected") {
        throw "Expected zero network sockets confirmation, got: $outputStr"
    }
    if ($outputStr -notmatch "Audited Process Count\s*:\s*[1-9]") {
        throw "Expected at least 1 audited process in results table, got: $outputStr"
    }
}

# Test 3: Non-existent PID fails gracefully
Run-TestCase "Non-existent PID produces error and fails gracefully" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -TargetPid 99999999 2>&1"
    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 0) {
        throw "Expected non-zero exit code for invalid PID, got 0"
    }
}

# Summary
Write-Host "`nTest Execution Summary:" -ForegroundColor Cyan
$failedCount = @($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$passedCount = @($testResults | Where-Object { $_.Status -eq "Passed" }).Count

Write-Host "  Passed: $passedCount" -ForegroundColor Green
if ($failedCount -gt 0) {
    Write-Host "  Failed: $failedCount" -ForegroundColor Red
    exit 1
} else {
    Write-Host "  All $passedCount tests passed cleanly." -ForegroundColor Green
    exit 0
}
