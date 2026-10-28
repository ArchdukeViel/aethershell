<#
.SYNOPSIS
    Automated Regression Tests for scripts/test_inventory.ps1

.DESCRIPTION
    Validates correctness, error handling, strict validation, and accounting accuracy
    of scripts/test_inventory.ps1 using synthetic and recorded mock logs.
#>

$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "..\scripts\test_inventory.ps1"
if (-not (Test-Path $scriptPath)) {
    throw "Auditor script not found at $scriptPath"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("test_inventory_tests_" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

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

Write-Host "Running test_inventory.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: Valid Discovered List Mock Log
Run-TestCase "Parse valid --list log" {
    $mockListLog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_taskbar-07a3fc5697a3472e.exe)

running 2 tests
test_one: test
test_two: test

2 tests, 0 benchmarks
     Running tests\config_ipc_test.rs (target\x86_64-pc-windows-msvc\debug\deps\config_ipc_test-1c29f2e9b2b2b37c.exe)

running 1 test
test_config_one: test

1 test, 0 benchmarks
"@
    $mockFile = Join-Path $tempDir "mock_list.log"
    Set-Content -Path $mockFile -Value $mockListLog

    $resJson = & $scriptPath -InputFile $mockFile -Mode Discovered -Json | ConvertFrom-Json
    if ($resJson.TotalUniqueTests -ne 3) {
        throw "Expected 3 unique tests, got $($resJson.TotalUniqueTests)"
    }
    if ($resJson.AuditMode -ne "Discovered") {
        throw "Expected Discovered mode, got $($resJson.AuditMode)"
    }
    if ($resJson.TotalUnitTests -ne 2) {
        throw "Expected 2 unit tests, got $($resJson.TotalUnitTests)"
    }
    if ($resJson.TotalIntegrationTests -ne 1) {
        throw "Expected 1 integration test, got $($resJson.TotalIntegrationTests)"
    }
}

# Test 2: Valid Executed Run Mock Log
Run-TestCase "Parse valid execution log" {
    $mockExecLog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_taskbar-07a3fc5697a3472e.exe)

running 2 tests
test test_one ... ok
test test_two ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
"@
    $mockFile = Join-Path $tempDir "mock_exec.log"
    Set-Content -Path $mockFile -Value $mockExecLog

    $resJson = & $scriptPath -InputFile $mockFile -Mode Executed -Json | ConvertFrom-Json
    if ($resJson.TotalUniqueTests -ne 2) {
        throw "Expected 2 unique tests, got $($resJson.TotalUniqueTests)"
    }
    if ($resJson.TotalPassed -ne 2) {
        throw "Expected 2 passed tests, got $($resJson.TotalPassed)"
    }
    if ($resJson.TotalFailed -ne 0) {
        throw "Expected 0 failed tests, got $($resJson.TotalFailed)"
    }
    if ($resJson.AuditMode -ne "Executed") {
        throw "Expected Executed mode, got $($resJson.AuditMode)"
    }
}

# Test 3: Empty input file rejection
Run-TestCase "Reject empty input file" {
    $emptyFile = Join-Path $tempDir "empty.log"
    Set-Content -Path $emptyFile -Value ""

    $failedAsExpected = $false
    try {
        & $scriptPath -InputFile $emptyFile -Json 2>$null
    } catch {
        $failedAsExpected = $true
    }
    if (-not $failedAsExpected) {
        throw "Empty input file should have thrown an error"
    }
}

# Test 4: Unknown suite detection in Strict mode
Run-TestCase "Detect unknown suite in Strict mode" {
    $unknownSuiteLog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\malicious_alien_crate-12345678.exe)

running 1 test
test alien_test: test

1 test, 0 benchmarks
"@
    $unknownFile = Join-Path $tempDir "unknown.log"
    Set-Content -Path $unknownFile -Value $unknownSuiteLog

    $failedAsExpected = $false
    try {
        & $scriptPath -InputFile $unknownFile -Strict -Json 2>$null
    } catch {
        $failedAsExpected = $true
    }
    if (-not $failedAsExpected) {
        throw "Strict mode should have failed on unknown suite"
    }
}

# Test 5: Reordered log blocks parsed deterministically
Run-TestCase "Parse reordered test suites deterministically" {
    $orderALog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_taskbar-07a3fc5697a3472e.exe)
running 1 test
test_taskbar: test
1 test, 0 benchmarks
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_core-1111111111111111.exe)
running 1 test
test_core: test
1 test, 0 benchmarks
"@
    $orderBLog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_core-1111111111111111.exe)
running 1 test
test_core: test
1 test, 0 benchmarks
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_taskbar-07a3fc5697a3472e.exe)
running 1 test
test_taskbar: test
1 test, 0 benchmarks
"@
    $fileA = Join-Path $tempDir "orderA.log"
    $fileB = Join-Path $tempDir "orderB.log"
    Set-Content -Path $fileA -Value $orderALog
    Set-Content -Path $fileB -Value $orderBLog

    $resA = & $scriptPath -InputFile $fileA -Json | ConvertFrom-Json
    $resB = & $scriptPath -InputFile $fileB -Json | ConvertFrom-Json

    if ($resA.TotalUniqueTests -ne $resB.TotalUniqueTests -or $resA.TotalUniqueTests -ne 2) {
        throw "Reordered suites produced mismatched counts"
    }
}

# Test 6: Incomplete / mismatch suite count detection in Strict mode
Run-TestCase "Detect incomplete suite count mismatch in Strict mode" {
    $mismatchLog = @"
     Running unittests src\lib.rs (target\x86_64-pc-windows-msvc\debug\deps\aethershell_taskbar-07a3fc5697a3472e.exe)

running 3 tests
test test_one ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
"@
    $mismatchFile = Join-Path $tempDir "mismatch.log"
    Set-Content -Path $mismatchFile -Value $mismatchLog

    $failedAsExpected = $false
    try {
        & $scriptPath -InputFile $mismatchFile -Mode Executed -Strict -Json 2>$null
    } catch {
        $failedAsExpected = $true
    }
    if (-not $failedAsExpected) {
        throw "Strict mode should have rejected count mismatch between parsed tests (1) and reported summary (3)"
    }
}

# Cleanup
Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue

$failedTests = $testResults | Where-Object { $_.Status -eq "Failed" }
Write-Host "--------------------------------------------------------------------------------"
if ($failedTests.Count -gt 0) {
    Write-Host "Regression test suite FAILED: $($failedTests.Count) failures." -ForegroundColor Red
    exit 1
} else {
    Write-Host "Regression test suite PASSED: $($testResults.Count) tests passed cleanly." -ForegroundColor Green
}
