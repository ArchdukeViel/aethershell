<#
.SYNOPSIS
    Automated Regression Tests for scripts/provision_disposable_vm.ps1 (Q48:A)
.DESCRIPTION
    Validates host identity and preflight checks, Windows Sandbox (.wsb) XML profile generation,
    resource allocations (2048 MB RAM, 2 vCPUs), airgap network disabling, BOM-free UTF-8 encoding,
    Hyper-V specification validation, and host AppData cleanliness.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$provisionScript = Join-Path $workspaceRoot "scripts\provision_disposable_vm.ps1"
if (-not (Test-Path $provisionScript)) {
    throw "Provisioning script not found at $provisionScript"
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

Write-Host "Running provision_disposable_vm.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: PreflightOnly mode executes and passes cleanly without generating files
Run-TestCase "PreflightOnly mode satisfies resource and safety checks" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$provisionScript' -Mode PreflightOnly 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "Preflight Verdict: PASSED") {
        throw "Expected Preflight Verdict: PASSED confirmation, got: $outputStr"
    }
}

# Test 2: Sandbox DryRun mode executes and produces valid .wsb configuration
Run-TestCase "Sandbox DryRun generates valid .wsb configuration" {
    $tempWsb = Join-Path ([System.IO.Path]::GetTempPath()) "AetherShell-Test-$([System.Guid]::NewGuid().ToString('N')).wsb"
    try {
        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$provisionScript' -Mode Sandbox -DryRun -OutputWsb '$tempWsb' 2>&1"
        $exitCode = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($exitCode -ne 0) {
            throw "Expected exit code 0, got $exitCode. Output: $outputStr"
        }
        if (-not (Test-Path $tempWsb)) {
            throw "Expected .wsb file to be created at $tempWsb"
        }

        # Validate XML content
        $xmlContent = [System.IO.File]::ReadAllText($tempWsb)
        [xml]$xmlDoc = $xmlContent

        # Verify Q48:A memory allocation
        $memNode = $xmlDoc.SelectSingleNode("//Configuration/MemoryInMB")
        if ($null -eq $memNode -or $memNode.InnerText -ne "2048") {
            throw "Expected MemoryInMB to be 2048 per Q48:A, got: $($memNode.InnerText)"
        }

        # Verify Airgap network isolation
        $netNode = $xmlDoc.SelectSingleNode("//Configuration/Networking")
        if ($null -eq $netNode -or $netNode.InnerText -ne "Disable") {
            throw "Expected Networking to be Disable, got: $($netNode.InnerText)"
        }

        # Verify vGPU disabling for isolation
        $gpuNode = $xmlDoc.SelectSingleNode("//Configuration/VGpu")
        if ($null -eq $gpuNode -or $gpuNode.InnerText -ne "Disable") {
            throw "Expected VGpu to be Disable, got: $($gpuNode.InnerText)"
        }

        # Verify BOM-free UTF-8 encoding
        $bytes = [System.IO.File]::ReadAllBytes($tempWsb)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "Expected BOM-free UTF-8 file, but found UTF-8 BOM"
        }
    } finally {
        if (Test-Path $tempWsb) {
            Remove-Item -LiteralPath $tempWsb -Force -ErrorAction SilentlyContinue
        }
    }
}

# Test 3: HyperV DryRun mode executes cleanly and validates parameters
Run-TestCase "HyperV DryRun mode executes cleanly" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$provisionScript' -Mode HyperV -DryRun 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "Startup RAM\s+:\s+2048 MB") {
        throw "Expected 2048 MB Startup RAM per Q48:A, got: $outputStr"
    }
    if ($outputStr -notmatch "vCPU Count\s+:\s+2") {
        throw "Expected 2 vCPUs per Q48:A, got: $outputStr"
    }
}

# Test 4: Verify production %LOCALAPPDATA%\AetherShell is preserved and clean
Run-TestCase "Verify production %LOCALAPPDATA%\AetherShell is preserved and clean (0 items)" {
    $localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $prodDir = Join-Path $localAppData "AetherShell"
    if (Test-Path $prodDir) {
        $items = @(Get-ChildItem -Path $prodDir -Recurse -Force -ErrorAction SilentlyContinue)
        if ($items.Count -ne 0) {
            throw "Production AppData directory is not clean: contains $($items.Count) items"
        }
    }
}

# Summary
Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Cyan
$failedCount = @($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$passedCount = @($testResults | Where-Object { $_.Status -eq "Passed" }).Count
if ($failedCount -eq 0) {
    Write-Host "Disposable VM provisioning regression test suite PASSED: $passedCount tests passed cleanly." -ForegroundColor Green
    exit 0
} else {
    Write-Host "Disposable VM provisioning regression test suite FAILED: $failedCount failed, $passedCount passed." -ForegroundColor Red
    exit 1
}
