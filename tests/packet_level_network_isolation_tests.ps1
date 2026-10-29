<#
.SYNOPSIS
    Automated Regression Tests for scripts/audit_packet_level_network_isolation.ps1 (Gate G16)
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditScript = Join-Path $workspaceRoot "scripts\audit_packet_level_network_isolation.ps1"
if (-not (Test-Path $auditScript)) {
    throw "Packet audit script not found at $auditScript"
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

Write-Host "Running packet_level_network_isolation_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: Standard audit with -Strict fails closed when no process exists
Run-TestCase "audit_packet_level_network_isolation.ps1 fails closed in -Strict mode when no process exists" {
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

# Test 2: Elevation detection and -RequirePktMon fails closed on non-elevated host
Run-TestCase "Elevation detection & -RequirePktMon fails closed on non-elevated host with diagnostic message" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -RequirePktMon -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -eq 0) {
        throw "Expected non-zero exit code for -RequirePktMon on non-elevated host, got 0"
    }
    if ($outputStr -notmatch "Gate G16 Packet Monitor Audit Blocked: -RequirePktMon requires administrative elevation") {
        throw "Expected elevation requirement diagnostic, got: $outputStr"
    }
}

# Test 3: Non-existent PID fails gracefully
Run-TestCase "Non-existent PID produces error and fails gracefully" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -TargetPid 99999999 -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 0) {
        throw "Expected non-zero exit code for invalid PID, got 0"
    }
}

# Test 4: Live process isolation audit with -SpawnTestProcess succeeds and verifies zero sockets & zero DNS leaks
Run-TestCase "Live process packet-level isolation audit with -SpawnTestProcess succeeds and verifies zero sockets and zero DNS leaks" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -SpawnTestProcess -DurationSeconds 2 -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "Zero network packets, zero listening ports, zero outbound sockets, and zero DNS queries detected") {
        throw "Expected zero network activity confirmation, got: $outputStr"
    }
    if ($outputStr -notmatch "Audited Process Count\s*:\s*1") {
        throw "Expected exactly 1 audited process in results table, got: $outputStr"
    }
    if ($outputStr -notmatch "Continuous Poll Cycles\s*:\s*[1-9]") {
        throw "Expected continuous poll cycles recorded, got: $outputStr"
    }
}

# Test 5: Multi-component live isolation audit with -SpawnAllComponents concurrently audits Core, Settings, and Script Host
Run-TestCase "Multi-component live isolation audit with -SpawnAllComponents concurrently audits Core, Settings, and Script Host" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -SpawnAllComponents -DurationSeconds 2 -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "Audited Process Count\s*:\s*3") {
        throw "Expected 3 audited processes for multi-component audit, got: $outputStr"
    }
    if ($outputStr -notmatch "Zero network packets, zero listening ports") {
        throw "Expected zero network activity across all components, got: $outputStr"
    }
}

# Test 6: Structured JSON mode (-Json) outputs conforming qualification schema
Run-TestCase "Structured JSON mode (-Json) outputs conforming qualification schema" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -SpawnTestProcess -DurationSeconds 1 -Json 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0 for -Json, got $exitCode. Output: $outputStr"
    }
    $jsonObj = $outputStr | ConvertFrom-Json
    if ($jsonObj.Gate -ne "G16") {
        throw "Expected Gate G16 in JSON, got: $($jsonObj.Gate)"
    }
    if ($jsonObj.AuditName -ne "PacketLevelNetworkIsolation") {
        throw "Expected AuditName PacketLevelNetworkIsolation, got: $($jsonObj.AuditName)"
    }
    if ($jsonObj.Passed -ne $true) {
        throw "Expected Passed == true in JSON, got: $($jsonObj.Passed)"
    }
    if ($jsonObj.SocketViolationsCount -ne 0) {
        throw "Expected 0 socket violations, got: $($jsonObj.SocketViolationsCount)"
    }
    if ($jsonObj.DnsNewQueriesCount -ne 0) {
        throw "Expected 0 DNS new queries, got: $($jsonObj.DnsNewQueriesCount)"
    }
    if ($jsonObj.ContinuousSampleCycles -le 0) {
        throw "Expected ContinuousSampleCycles > 0, got: $($jsonObj.ContinuousSampleCycles)"
    }
}

# Test 7: Host AppData hygiene assertion
Run-TestCase "Host AppData directory remains clean at 0 items after all audit runs" {
    $localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $aetherAppData = Join-Path $localAppData "AetherShell"
    if (Test-Path $aetherAppData) {
        $count = (Get-ChildItem -Path $aetherAppData -Recurse -Force -ErrorAction SilentlyContinue).Count
        if ($count -ne 0) {
            throw "Host AppData contains $count items (must remain clean at 0 items)"
        }
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
