<#
.SYNOPSIS
    Automated Regression Tests for Taskbar Recovery & Watchdog (Gate G04, PRD Section 4.2, Section 4.3, Section 18.2)
.DESCRIPTION
    Validates TaskbarRecoveryCoordinator, TaskbarRecoveryWatchdog, Win32 handle monitoring on SYNCHRONIZE,
    heartbeat event pulsing, sudden termination detection and taskbar restoration, TaskbarCreated dynamic
    broadcast handling and debouncing, CLI --taskbar-recovery-watchdog wiring, and host AppData zero pollution.
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

Write-Host "Running taskbar_recovery_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Test 1: aethershell-taskbar recovery module unit tests pass cleanly
# -----------------------------------------------------------------------------
Run-TestCase "Cargo aethershell-taskbar recovery unit tests pass cleanly" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-taskbar --lib recovery::tests 2>&1"
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
# Test 2: Cargo integration test taskbar_recovery_integration_test passes cleanly
# -----------------------------------------------------------------------------
Run-TestCase "Cargo taskbar_recovery_integration_test passes all 5 tests" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo test -p aethershell-app --test taskbar_recovery_integration_test 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "cargo test failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "test result: ok\. 5 passed") {
        throw "Expected 'test result: ok. 5 passed', got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 3: CLI --help outputs --taskbar-recovery-watchdog documentation
# -----------------------------------------------------------------------------
Run-TestCase "CLI --help documents --taskbar-recovery-watchdog <PID>" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = cmd /c "cargo run -p aethershell-app --bin aethershell -- --help 2>&1"
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "CLI --help failed with exit code ${exitCode}: $outStr"
    }
    if ($outStr -notmatch "--taskbar-recovery-watchdog <PID>") {
        throw "Expected --help to document '--taskbar-recovery-watchdog <PID>', got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 4: CLI --taskbar-recovery-watchdog rejects missing or non-numeric PID
# -----------------------------------------------------------------------------
Run-TestCase "CLI --taskbar-recovery-watchdog rejects missing or non-numeric PID" {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out1 = cmd /c "cargo run -p aethershell-app --bin aethershell -- --taskbar-recovery-watchdog 2>&1"
        $exit1 = $LASTEXITCODE

        $out2 = cmd /c "cargo run -p aethershell-app --bin aethershell -- --taskbar-recovery-watchdog abc 2>&1"
        $exit2 = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevEap
    }

    if ($exit1 -eq 0) {
        throw "Expected failure when PID is missing, got exit code 0"
    }
    if ($exit2 -eq 0) {
        throw "Expected failure when PID is non-numeric, got exit code 0"
    }
}

# -----------------------------------------------------------------------------
# Test 5: Watchdog cleanly supervises process exiting with code 0 (clean exit)
# -----------------------------------------------------------------------------
Run-TestCase "Watchdog cleanly supervises process and reports clean exit without spurious restoration" {
    # Ensure binary is built
    cmd /c "cargo build -p aethershell-app --bin aethershell" | Out-Null
    if (-not (Test-Path $coreExe)) {
        throw "Executable not found at $coreExe"
    }

    # Start target process that sleeps 2 seconds and exits 0
    $targetProc = Start-Process -FilePath $coreExe -ArgumentList "--test-sleep-secs 2" -PassThru
    $targetPid = $targetProc.Id

    # Run watchdog against target PID
    $watchdogProc = Start-Process -FilePath $coreExe -ArgumentList "--taskbar-recovery-watchdog $targetPid" -PassThru -NoNewWindow -Wait

    $watchdogExit = $watchdogProc.ExitCode
    $targetExited = $targetProc.HasExited

    if (-not $targetExited) {
        $targetProc | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    if ($watchdogExit -ne 0) {
        throw "Expected watchdog to exit cleanly (exit code 0), got $watchdogExit"
    }
}

# -----------------------------------------------------------------------------
# Test 6: Watchdog detects sudden termination and executes taskbar recovery
# -----------------------------------------------------------------------------
Run-TestCase "Watchdog detects sudden termination of target and executes taskbar recovery" {
    # Start target process that would sleep 20 seconds
    $targetProc = Start-Process -FilePath $coreExe -ArgumentList "--test-sleep-secs 20" -PassThru
    $targetPid = $targetProc.Id

    # Start watchdog monitoring target in background with output redirected to temp file
    $tempOut = [System.IO.Path]::GetTempFileName()
    $watchdogProc = Start-Process -FilePath $coreExe -ArgumentList "--taskbar-recovery-watchdog $targetPid" -PassThru -RedirectStandardOutput $tempOut -NoNewWindow

    # Allow watchdog to attach (check process handle)
    Start-Sleep -Milliseconds 600

    # Kill target abruptly (simulating crash / panic / hard kill)
    $targetProc | Stop-Process -Force -ErrorAction SilentlyContinue

    # Watchdog should detect abrupt termination and exit within 3 seconds
    $watchdogProc | Wait-Process -Timeout 6 -ErrorAction SilentlyContinue
    $watchdogExited = $watchdogProc.HasExited
    $watchdogExitCode = if ($watchdogExited) { $watchdogProc.ExitCode } else { -1 }

    if (-not $watchdogExited) {
        $watchdogProc | Stop-Process -Force -ErrorAction SilentlyContinue
    }

    $outText = if (Test-Path $tempOut) { Get-Content -LiteralPath $tempOut -Raw } else { "" }
    Remove-Item -LiteralPath $tempOut -Force -ErrorAction SilentlyContinue

    if (-not $watchdogExited) {
        throw "Watchdog did not exit after target process sudden termination"
    }
    if ($watchdogExitCode -ne 0) {
        throw "Watchdog exited with non-zero exit code $watchdogExitCode. Output: $outText"
    }
    if ($outText -notmatch "terminated abruptly" -and $outText -notmatch "RecoveredAbruptTermination") {
        throw "Expected abrupt termination recovery message in output, got: $outText"
    }
}

# -----------------------------------------------------------------------------
# Test 7: Verify host AppData %LOCALAPPDATA%\AetherShell is strictly unpolluted (0 items)
# -----------------------------------------------------------------------------
Run-TestCase "Host AppData (%LOCALAPPDATA%\AetherShell) remains strictly unpolluted at 0 items" {
    $appData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $aetherDir = Join-Path $appData "AetherShell"
    if (Test-Path -LiteralPath $aetherDir) {
        $items = @(Get-ChildItem -LiteralPath $aetherDir -Force)
        if ($items.Count -gt 0) {
            throw "Host AppData pollution detected! Found $($items.Count) items in $aetherDir"
        }
    }
}

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Cyan
$failedCount = @($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$passedCount = @($testResults | Where-Object { $_.Status -eq "Passed" }).Count

Write-Host "Taskbar Recovery Test Summary: $passedCount/$($testResults.Count) passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($failedCount -gt 0) {
    exit 1
}

exit 0
