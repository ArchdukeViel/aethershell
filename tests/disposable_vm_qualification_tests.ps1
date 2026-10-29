<#
.SYNOPSIS
    Automated Regression Tests for Disposable VM & Sandbox Qualification (Gates G01, G02, G04, G06, G07, G10, G11, G16, G17, G19)
.DESCRIPTION
    Validates host safety boundaries, preflight checks, guest bundle staging,
    Windows Sandbox (.wsb) XML profile generation, non-disruptive guest qualification execution,
    structured JSON ledger output, and host AppData zero-pollution.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$orchestratorScript = Join-Path $workspaceRoot "scripts\run_disposable_vm_qualification.ps1"
$guestScript = Join-Path $workspaceRoot "scripts\guest_sandbox_qualification.ps1"

if (-not (Test-Path $orchestratorScript)) {
    throw "Orchestrator script not found at $orchestratorScript"
}
if (-not (Test-Path $guestScript)) {
    throw "Guest qualification script not found at $guestScript"
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

Write-Host "Running disposable_vm_qualification_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Test 1: Host safety boundary enforcement fails closed on physical host
# -----------------------------------------------------------------------------
Run-TestCase "Host safety boundary enforcement blocks disruptive execution without -SkipExplorerKill" {
    # On physical host, calling guest script without -SkipExplorerKill must fail closed with exit code 2
    $out = powershell -NoProfile -ExecutionPolicy Bypass -File "$guestScript" 2>&1
    $exitCode = $LASTEXITCODE
    $outStr = ($out | Out-String)

    if ($exitCode -ne 2) {
        throw "Expected exit code 2 (safety block), got $exitCode. Output: $outStr"
    }
    if ($outStr -notmatch "SAFETY BOUNDARY ENFORCEMENT") {
        throw "Expected 'SAFETY BOUNDARY ENFORCEMENT' in output, got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 2: Orchestrator PreflightOnly mode satisfies resource envelopes
# -----------------------------------------------------------------------------
Run-TestCase "Orchestrator PreflightOnly mode executes and passes cleanly" {
    $out = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$orchestratorScript' -PreflightOnly 2>&1"
    $exitCode = $LASTEXITCODE
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outStr"
    }
    if ($outStr -notmatch "Resource preflight: PASSED") {
        throw "Expected 'Resource preflight: PASSED', got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 3: Orchestrator DryRun stages bundle and generates valid .wsb configuration
# -----------------------------------------------------------------------------
Run-TestCase "Orchestrator DryRun mode stages bundle and generates valid .wsb configuration" {
    $tempOutDir = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell_QualTest_$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
    try {
        $out = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$orchestratorScript' -DryRun -OutputDir '$tempOutDir' -Strict 2>&1"
        $exitCode = $LASTEXITCODE
        $outStr = ($out | Out-String)

        if ($exitCode -ne 0) {
            throw "Expected exit code 0, got $exitCode. Output: $outStr"
        }

        # Check bundle directory
        $bundleDir = Join-Path $tempOutDir "guest_bundle"
        if (-not (Test-Path $bundleDir)) {
            throw "Expected guest_bundle directory in $tempOutDir"
        }
        if (-not (Test-Path (Join-Path $bundleDir "bin\aethershell.exe"))) {
            throw "Expected aethershell.exe in guest_bundle\bin"
        }
        if (-not (Test-Path (Join-Path $bundleDir "guest_sandbox_qualification.ps1"))) {
            throw "Expected guest_sandbox_qualification.ps1 in guest_bundle"
        }

        # Check .wsb file
        $wsbFile = Join-Path $tempOutDir "AetherShell-Disruptive-Qualification.wsb"
        if (-not (Test-Path $wsbFile)) {
            throw "Expected .wsb file at $wsbFile"
        }

        # Validate .wsb XML content
        $xmlContent = [System.IO.File]::ReadAllText($wsbFile)
        [xml]$xmlDoc = $xmlContent

        $memNode = $xmlDoc.SelectSingleNode("//Configuration/MemoryInMB")
        if ($null -eq $memNode -or $memNode.InnerText -ne "2048") {
            throw "Expected MemoryInMB to be 2048 per Q48:A, got: $($memNode.InnerText)"
        }

        $netNode = $xmlDoc.SelectSingleNode("//Configuration/Networking")
        if ($null -eq $netNode -or $netNode.InnerText -ne "Disable") {
            throw "Expected Networking to be Disable, got: $($netNode.InnerText)"
        }

        # Validate BOM-free UTF-8
        $bytes = [System.IO.File]::ReadAllBytes($wsbFile)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "Found UTF-8 BOM in .wsb file"
        }
    } finally {
        if (Test-Path $tempOutDir) {
            Remove-Item -LiteralPath $tempOutDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# -----------------------------------------------------------------------------
# Test 4: Guest harness non-disruptive execution generates valid JSON ledger
# -----------------------------------------------------------------------------
Run-TestCase "Guest harness non-disruptive execution generates valid JSON ledger" {
    $tempOutDir = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell_HarnessTest_$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
    $ledgerPath = Join-Path $tempOutDir "test_ledger.json"
    try {
        New-Item -ItemType Directory -Path $tempOutDir -Force | Out-Null

        $out = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$guestScript' -ArtifactsDir '$tempOutDir' -OutputLedgerJson '$ledgerPath' -SkipExplorerKill -Strict 2>&1"
        $exitCode = $LASTEXITCODE
        $outStr = ($out | Out-String)

        if ($exitCode -ne 0) {
            throw "Expected exit code 0, got $exitCode. Output: $outStr"
        }
        if (-not (Test-Path $ledgerPath)) {
            throw "Expected JSON ledger file at $ledgerPath"
        }

        # Parse JSON ledger
        $ledgerJson = Get-Content -LiteralPath $ledgerPath -Raw | ConvertFrom-Json
        if ($ledgerJson.ReportType -ne "DisposableVMQualificationLedger") {
            throw "Expected ReportType == 'DisposableVMQualificationLedger', got: $($ledgerJson.ReportType)"
        }
        if ($ledgerJson.FailedCount -ne 0) {
            throw "Expected FailedCount == 0, got: $($ledgerJson.FailedCount)"
        }
        if ($ledgerJson.PassedCount -lt 6) {
            throw "Expected at least 6 passed gates, got: $($ledgerJson.PassedCount)"
        }

        # Verify BOM-free UTF-8
        $bytes = [System.IO.File]::ReadAllBytes($ledgerPath)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "Found UTF-8 BOM in ledger file"
        }
    } finally {
        if (Test-Path $tempOutDir) {
            Remove-Item -LiteralPath $tempOutDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# -----------------------------------------------------------------------------
# Test 5: Verify host %LOCALAPPDATA%\AetherShell is preserved at strictly 0 items
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

Write-Host "Disposable VM Qualification Test Summary: $passedCount/$($testResults.Count) passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($failedCount -gt 0) {
    exit 1
}

exit 0
