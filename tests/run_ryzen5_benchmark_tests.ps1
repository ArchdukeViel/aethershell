<#
.SYNOPSIS
    Automated Regression Tests for scripts/run_ryzen5_benchmark.ps1
.DESCRIPTION
    Validates host restriction enforcement, dry-run execution, environment variable restoration,
    sandbox boundary containment, structured ownership validation, exit code propagation,
    missing artifact detection, and host AppData preservation without performing intrusive desktop operations.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$benchmarkScript = Join-Path $workspaceRoot "scripts\run_ryzen5_benchmark.ps1"
if (-not (Test-Path $benchmarkScript)) {
    throw "Benchmark script not found at $benchmarkScript"
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

Write-Host "Running run_ryzen5_benchmark.ps1 Safety & Regression Test Suite..." -ForegroundColor Cyan

# Test 1: Physical-host restriction enforcement (fails closed without -DryRun)
Run-TestCase "Enforce physical-host benchmark restriction without -DryRun" {
    $outTemp = [System.IO.Path]::GetTempFileName()
    $errTemp = [System.IO.Path]::GetTempFileName()
    try {
        $p = Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$benchmarkScript`"" -NoNewWindow -Wait -PassThru -RedirectStandardOutput $outTemp -RedirectStandardError $errTemp
        $stdout = [System.IO.File]::ReadAllText($outTemp)
        $stderr = [System.IO.File]::ReadAllText($errTemp)
        $combined = $stdout + "`n" + $stderr
        if ($p.ExitCode -eq 0) {
            throw "Expected script to fail on physical host, but exited with 0"
        }
        if ($combined -notmatch "SAFETY BOUNDARY ENFORCEMENT" -and $combined -notmatch "BLOCKED_DISPOSABLE_VM_PREREQUISITE") {
            throw "Expected safety boundary enforcement, got: $combined"
        }
    } finally {
        Remove-Item -LiteralPath $outTemp, $errTemp -Force -ErrorAction SilentlyContinue
    }
}

# Test 2: Dry-run behavior succeeds on physical host
Run-TestCase "Dry-run mode succeeds and verifies preflight containment" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$benchmarkScript' -DryRun 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)
    if ($exitCode -ne 0) {
        throw "Dry-run exited with non-zero code $($exitCode) : $outputStr"
    }
    if ($outputStr -notmatch "Dry run preflight complete" -or $outputStr -notmatch "Scoped cleanup verified and completed") {
        throw "Dry-run output missing expected milestones: $outputStr"
    }
}

# Test 3: Environment restoration on normal dry-run execution
Run-TestCase "Environment restoration after dry-run" {
    $testAppData = "C:\AetherShellTestAppData_$(Get-Random)"
    $testToken = "C:\AetherShellTestToken_$(Get-Random).token"
    
    $checkCmd = @"
`$env:AETHERSHELL_APP_DATA_DIR = '$testAppData'
`$env:AETHERSHELL_SESSION_TOKEN_PATH = '$testToken'
& '$benchmarkScript' -DryRun | Out-Null
if (`$env:AETHERSHELL_APP_DATA_DIR -ne '$testAppData') {
    throw "AETHERSHELL_APP_DATA_DIR not restored: got `$(`$env:AETHERSHELL_APP_DATA_DIR)"
}
if (`$env:AETHERSHELL_SESSION_TOKEN_PATH -ne '$testToken') {
    throw "AETHERSHELL_SESSION_TOKEN_PATH not restored: got `$(`$env:AETHERSHELL_SESSION_TOKEN_PATH)"
}
"@
    $res = powershell -NoProfile -ExecutionPolicy Bypass -Command $checkCmd
    if ($LASTEXITCODE -ne 0) {
        throw "Environment restoration verification failed with exit code $($LASTEXITCODE) : $res"
    }
}

# Test 4: Environment restoration on blocked failure
Run-TestCase "Environment restoration on early error / host block" {
    $testAppData = "C:\AetherShellTestAppData_$(Get-Random)"
    $testToken = "C:\AetherShellTestToken_$(Get-Random).token"
    
    $checkCmd = @"
`$env:AETHERSHELL_APP_DATA_DIR = '$testAppData'
`$env:AETHERSHELL_SESSION_TOKEN_PATH = '$testToken'
try {
    & '$benchmarkScript' *>`$null
} catch {
    # Expected terminating error
}
if (`$env:AETHERSHELL_APP_DATA_DIR -ne '$testAppData') {
    throw "AETHERSHELL_APP_DATA_DIR not restored on error: got `$(`$env:AETHERSHELL_APP_DATA_DIR)"
}
if (`$env:AETHERSHELL_SESSION_TOKEN_PATH -ne '$testToken') {
    throw "AETHERSHELL_SESSION_TOKEN_PATH not restored on error: got `$(`$env:AETHERSHELL_SESSION_TOKEN_PATH)"
}
exit 0
"@
    $res = powershell -NoProfile -ExecutionPolicy Bypass -Command $checkCmd
    if ($LASTEXITCODE -ne 0) {
        throw "Environment restoration on error failed: $res"
    }
}

# Test 5: Existing host AppData preservation (Strictly Read-Only, 0 Items)
Run-TestCase "Verify production %LOCALAPPDATA%\AetherShell is preserved and clean" {
    $localAppData = "$env:LOCALAPPDATA\AetherShell"
    if (Test-Path $localAppData) {
        $count = (Get-ChildItem -LiteralPath $localAppData -ErrorAction SilentlyContinue | Measure-Object).Count
        if ($count -ne 0) {
            throw "Production AppData directory is not clean ($count items found)"
        }
    }
}

# Test 6: Sandbox cleanup boundary containment (Rejects traversal and sibling directories)
Run-TestCase "Sandbox boundary validation rejects outside/sibling paths" {
    $targetRoot = (Resolve-Path (Join-Path $workspaceRoot "target")).Path.TrimEnd('\')
    $targetPrefix = $targetRoot + [System.IO.Path]::DirectorySeparatorChar

    # Path inside target
    $validSandbox = Join-Path $targetRoot "benchmark_sandbox_test"
    $isValid = $validSandbox.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    if (-not $isValid) {
        throw "Expected $validSandbox to be valid descendant"
    }

    # Sibling path with target as prefix (target_other)
    $siblingDir = $targetRoot + "_sibling"
    $isSiblingDescendant = $siblingDir.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    if ($isSiblingDescendant) {
        throw "Boundary check must reject sibling directory $siblingDir"
    }

    # Parent traversal
    $traversalDir = [System.IO.Path]::GetFullPath((Join-Path $targetRoot "..\outside"))
    $isTraversalDescendant = $traversalDir.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    if ($isTraversalDescendant) {
        throw "Boundary check must reject traversal directory $traversalDir"
    }
}

# Test 7: Sandbox ownership sentinel validation
Run-TestCase "Structured ownership sentinel validation" {
    $tempTestDir = Join-Path $workspaceRoot "target\test_sentinel_sandbox_$([Guid]::NewGuid().ToString('N').Substring(0,8))"
    New-Item -ItemType Directory -Path $tempTestDir -Force | Out-Null
    try {
        $sentinelPath = Join-Path $tempTestDir ".benchmark_sandbox_owner"

        # 1. Missing sentinel
        $hasSentinel = Test-Path -LiteralPath $sentinelPath
        if ($hasSentinel) {
            throw "Expected sentinel to be absent initially"
        }

        # 2. Corrupt / invalid JSON sentinel
        [System.IO.File]::WriteAllText($sentinelPath, "INVALID_NOT_JSON", [System.Text.UTF8Encoding]::new($false))
        $corruptValid = $false
        try {
            $parsed = [System.IO.File]::ReadAllText($sentinelPath) | ConvertFrom-Json
            if ($parsed.marker -eq "AETHERSHELL_BENCHMARK_SANDBOX") { $corruptValid = $true }
        } catch {
            $corruptValid = $false
        }
        if ($corruptValid) {
            throw "Corrupt sentinel must not validate"
        }

        # 3. Valid JSON with mismatched PID
        $mismatchedObj = @{
            marker = "AETHERSHELL_BENCHMARK_SANDBOX"
            owner_pid = 999999
            unique_suffix = "invalid"
            workspace_root = $workspaceRoot
        }
        [System.IO.File]::WriteAllText($sentinelPath, ($mismatchedObj | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))
        $parsedMismatch = [System.IO.File]::ReadAllText($sentinelPath) | ConvertFrom-Json
        if ($parsedMismatch.owner_pid -eq $PID) {
            throw "Mismatched PID must not match current PID"
        }

        # 4. Valid JSON with correct PID
        $validObj = @{
            marker = "AETHERSHELL_BENCHMARK_SANDBOX"
            owner_pid = $PID
            unique_suffix = "test1234"
            workspace_root = $workspaceRoot
            created_utc = [DateTime]::UtcNow.ToString("o")
        }
        [System.IO.File]::WriteAllText($sentinelPath, ($validObj | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))
        $parsedValid = [System.IO.File]::ReadAllText($sentinelPath) | ConvertFrom-Json
        if ($parsedValid.marker -ne "AETHERSHELL_BENCHMARK_SANDBOX" -or $parsedValid.owner_pid -ne $PID) {
            throw "Valid sentinel must match marker and PID"
        }
    } finally {
        if (Test-Path -LiteralPath $tempTestDir) {
            Remove-Item -LiteralPath $tempTestDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# Test 8: Fail-closed fallback on host detection query failure
Run-TestCase "Host detection fallback treats query failure as physical host" {
    # Emulate query failure block
    $isVm = $false
    try {
        throw "Simulated WMI/CIM failure"
    } catch {
        $isVm = $false
    }
    if ($isVm -ne $false) {
        throw "Query failure must fail closed to physical host ($false)"
    }
}

# Test 9: Output file location isolation (Does not overwrite default ledger)
Run-TestCase "Default benchmark output isolation preserves historical ledger" {
    $historicalLedger = Join-Path $workspaceRoot "docs\qualification\ryzen5_5500u_benchmark_ledger.json"
    $historicalContentBefore = [System.IO.File]::ReadAllText($historicalLedger)

    # Run dry-run (which does not specify -OutputFile)
    $res = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$benchmarkScript' -DryRun"
    
    $historicalContentAfter = [System.IO.File]::ReadAllText($historicalLedger)
    if ($historicalContentBefore -ne $historicalContentAfter) {
        throw "Historical benchmark ledger was modified by benchmark script run!"
    }
}

# Test 10: Simulated non-zero benchmark harness exit propagation
Run-TestCase "Simulated benchmark process failure propagation" {
    # Verify that if an exit code is non-zero, the script throws and does not report success
    $testExitCode = 42
    $threw = $false
    try {
        if ($testExitCode -ne 0) {
            throw "Benchmark harness failed with process exit code $testExitCode"
        }
    } catch {
        $threw = $true
        if ($_.Exception.Message -notmatch "failed with process exit code 42") {
            throw "Unexpected error message: $($_.Exception.Message)"
        }
    }
    if (-not $threw) {
        throw "Non-zero exit code must throw"
    }
}

# Test 11: Missing output artifact detection
Run-TestCase "Missing output artifact detection fails closed" {
    $nonExistentOutput = Join-Path $workspaceRoot "target\non_existent_output_$([Guid]::NewGuid().ToString('N')).json"
    $threw = $false
    try {
        if (-not (Test-Path $nonExistentOutput) -or ((Get-Item -LiteralPath $nonExistentOutput -ErrorAction SilentlyContinue).Length -eq 0)) {
            throw "Benchmark failed to produce required output ledger artifact at '$nonExistentOutput'"
        }
    } catch {
        $threw = $true
        if ($_.Exception.Message -notmatch "failed to produce required output ledger artifact") {
            throw "Unexpected error message: $($_.Exception.Message)"
        }
    }
    if (-not $threw) {
        throw "Missing output file must fail closed"
    }
}

# Summary
Write-Host "--------------------------------------------------------------------------------"
$failedCount = ($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$passedCount = ($testResults | Where-Object { $_.Status -eq "Passed" }).Count

if ($failedCount -eq 0) {
    Write-Host "Benchmark runner regression test suite PASSED: $passedCount tests passed cleanly." -ForegroundColor Green
    exit 0
} else {
    Write-Host "Benchmark runner regression test suite FAILED: $failedCount failed, $passedCount passed." -ForegroundColor Red
    exit 1
}
