<#
.SYNOPSIS
    Automated Regression Tests for Cryptographic Release Acceptance Ledger (Gate G18, PRD Section 18.2)
.DESCRIPTION
    Validates the cryptographic release acceptance ledger structure, gate classifications,
    anti-hallucination rules (0 VERIFIED), NO-GO release disposition, artifact hashes,
    BOM-free JSON encoding, and host AppData zero pollution.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$generatorScript = Join-Path $workspaceRoot "scripts\generate_release_acceptance_ledger.ps1"
$ledgerPath = Join-Path $workspaceRoot "docs\qualification\FINAL_RELEASE_ACCEPTANCE_LEDGER.json"

if (-not (Test-Path $generatorScript)) {
    throw "Generator script not found at $generatorScript"
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

Write-Host "Running release_acceptance_ledger_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# -----------------------------------------------------------------------------
# Test 1: Generator script executes in VerifyOnly mode
# -----------------------------------------------------------------------------
Run-TestCase "generate_release_acceptance_ledger.ps1 executes cleanly in -VerifyOnly mode" {
    $out = powershell -NoProfile -ExecutionPolicy Bypass -File "$generatorScript" -VerifyOnly 2>&1
    $exitCode = $LASTEXITCODE
    $outStr = ($out | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outStr"
    }
    if ($outStr -notmatch "Verified cryptographic ledger structure and integrity") {
        throw "Expected integrity confirmation, got: $outStr"
    }
}

# -----------------------------------------------------------------------------
# Test 2: Final acceptance ledger exists and parses as valid JSON
# -----------------------------------------------------------------------------
Run-TestCase "FINAL_RELEASE_ACCEPTANCE_LEDGER.json exists and conforms to schema" {
    if (-not (Test-Path $ledgerPath)) {
        throw "Ledger file not found at $ledgerPath"
    }

    $ledgerJson = Get-Content -LiteralPath $ledgerPath -Raw | ConvertFrom-Json
    if ($ledgerJson.LedgerType -ne "AetherShellFinalReleaseAcceptanceLedger") {
        throw "Expected LedgerType == 'AetherShellFinalReleaseAcceptanceLedger', got: $($ledgerJson.LedgerType)"
    }
    if ($ledgerJson.ReleaseDisposition -ne "NO-GO") {
        throw "Expected ReleaseDisposition == 'NO-GO', got: $($ledgerJson.ReleaseDisposition)"
    }
    if ($ledgerJson.CoreStableCandidate -ne $false) {
        throw "Expected CoreStableCandidate == false, got: $($ledgerJson.CoreStableCandidate)"
    }
    if ($ledgerJson.ProductionVersionTag -notmatch "RESERVED") {
        throw "Expected ProductionVersionTag to be RESERVED, got: $($ledgerJson.ProductionVersionTag)"
    }
}

# -----------------------------------------------------------------------------
# Test 3: Reconciled 20 gates strictly adhere to PRD Section 18.2
# -----------------------------------------------------------------------------
Run-TestCase "Gate accounting strictly enforces 2 VERIFIED (G01, G02), 16 PARTIAL, 1 N/A, 1 DEFERRED" {
    $ledgerJson = Get-Content -LiteralPath $ledgerPath -Raw | ConvertFrom-Json
    $summary = $ledgerJson.GateAccountingSummary

    if ($summary.TotalGates -ne 20) {
        throw "Expected 20 TotalGates, got: $($summary.TotalGates)"
    }
    if ($summary.Verified -ne 2) {
        throw "GOVERNANCE BREACH: Verified count must be strictly 2 (Gates G01, G02), got: $($summary.Verified)"
    }
    if ($summary.PartiallyVerified -ne 16) {
        throw "Expected 16 PartiallyVerified gates, got: $($summary.PartiallyVerified)"
    }
    if ($summary.NotApplicable -ne 1) {
        throw "Expected 1 NotApplicable gate (G05), got: $($summary.NotApplicable)"
    }
    if ($summary.Deferred -ne 1) {
        throw "Expected 1 Deferred gate (G15), got: $($summary.Deferred)"
    }

    # Verify G01 specifically
    $g01 = $ledgerJson.AcceptanceGates | Where-Object { $_.GateId -eq "G01" }
    if ($g01.Status -ne "VERIFIED") {
        throw "Expected G01 to be VERIFIED, got: $($g01.Status)"
    }
    if ($g01.Evidence -notmatch "EVD-050") {
        throw "Expected G01 Evidence to cite EVD-050, got: $($g01.Evidence)"
    }

    # Verify G02 specifically
    $g02 = $ledgerJson.AcceptanceGates | Where-Object { $_.GateId -eq "G02" }
    if ($g02.Status -ne "VERIFIED") {
        throw "Expected G02 to be VERIFIED, got: $($g02.Status)"
    }
    if ($g02.Evidence -notmatch "EVD-051") {
        throw "Expected G02 Evidence to cite EVD-051, got: $($g02.Evidence)"
    }

    # Verify G05 and G15 specifically
    $g05 = $ledgerJson.AcceptanceGates | Where-Object { $_.GateId -eq "G05" }
    if ($g05.Status -ne "NOT_APPLICABLE") {
        throw "Expected G05 to be NOT_APPLICABLE, got: $($g05.Status)"
    }
    $g15 = $ledgerJson.AcceptanceGates | Where-Object { $_.GateId -eq "G15" }
    if ($g15.Status -ne "DEFERRED") {
        throw "Expected G15 to be DEFERRED, got: $($g15.Status)"
    }
}

# -----------------------------------------------------------------------------
# Test 4: Cryptographic digests are valid 64-character lowercase hex strings
# -----------------------------------------------------------------------------
Run-TestCase "Cryptographic artifact digests are valid 64-character SHA-256 strings" {
    $ledgerJson = Get-Content -LiteralPath $ledgerPath -Raw | ConvertFrom-Json
    $digests = $ledgerJson.CryptographicArtifacts

    $keys = @("sbom.json", "THIRD_PARTY_LICENSES.md", "PRD.md", "AGENTS.md", "LICENSE", "NOTICE.md")
    foreach ($key in $keys) {
        $val = $digests.$key
        if ([string]::IsNullOrWhiteSpace($val)) {
            throw "Digest for '$key' is missing or empty"
        }
        if ($val -notmatch '^[a-f0-9]{64}$') {
            throw "Digest for '$key' is not a valid 64-char hex SHA-256: $val"
        }
    }
}

# -----------------------------------------------------------------------------
# Test 5: Verify BOM-free UTF-8 encoding
# -----------------------------------------------------------------------------
Run-TestCase "Ledger file is encoded in BOM-free UTF-8" {
    $bytes = [System.IO.File]::ReadAllBytes($ledgerPath)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        throw "Found UTF-8 BOM in ledger file"
    }
}

# -----------------------------------------------------------------------------
# Test 6: Verify production %LOCALAPPDATA%\AetherShell is preserved at strictly 0 items
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

Write-Host "Release Acceptance Ledger Test Summary: $passedCount/$($testResults.Count) passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($failedCount -gt 0) {
    exit 1
}

exit 0
