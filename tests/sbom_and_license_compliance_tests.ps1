<#
.SYNOPSIS
    Automated Regression Tests for scripts/generate_sbom.ps1 (Gate G16)
.DESCRIPTION
    Validates third-party software attribution, license compliance checking,
    SPDX 2.3 SBOM generation, UTF-8 BOM-free encoding, and failure-closed enforcement.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$sbomScript = Join-Path $workspaceRoot "scripts\generate_sbom.ps1"
if (-not (Test-Path $sbomScript)) {
    throw "SBOM generation script not found at $sbomScript"
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

Write-Host "Running sbom_and_license_compliance_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: generate_sbom.ps1 in -Strict mode succeeds with exit code 0
Run-TestCase "generate_sbom.ps1 in -Strict mode succeeds on workspace" {
    $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$sbomScript' -Strict 2>&1"
    $exitCode = $LASTEXITCODE
    $outputStr = ($output | Out-String)

    if ($exitCode -ne 0) {
        throw "Expected exit code 0, got $exitCode. Output: $outputStr"
    }
    if ($outputStr -notmatch "All \d+ dependencies conform 100% to approved permissive licenses") {
        throw "Expected confirmation of 100% permissive compliance, got: $outputStr"
    }
}

# Test 2: THIRD_PARTY_LICENSES.md contains valid ledger and summary
Run-TestCase "THIRD_PARTY_LICENSES.md contains valid attribution ledger" {
    $mdPath = Join-Path $workspaceRoot "docs\licensing\THIRD_PARTY_LICENSES.md"
    if (-not (Test-Path $mdPath)) {
        throw "Expected THIRD_PARTY_LICENSES.md to exist at $mdPath"
    }

    $content = Get-Content -Path $mdPath -Raw
    if ($content -notmatch "# AetherShell Third-Party Software Attribution") {
        throw "Missing expected title in THIRD_PARTY_LICENSES.md"
    }
    if ($content -notmatch "## Complete Dependency Ledger") {
        throw "Missing dependency ledger table in THIRD_PARTY_LICENSES.md"
    }
    if ($content -notmatch "windows-sys") {
        throw "Expected key dependency (e.g. windows-sys) in ledger"
    }
}

# Test 3: sbom.json is valid SPDX-2.3 JSON structure with NOASSERTION concluded licenses
Run-TestCase "sbom.json is valid SPDX-2.3 compliant structure with NOASSERTION concluded licenses" {
    $jsonPath = Join-Path $workspaceRoot "docs\licensing\sbom.json"
    if (-not (Test-Path $jsonPath)) {
        throw "Expected sbom.json to exist at $jsonPath"
    }

    $jsonContent = Get-Content -Path $jsonPath -Raw | ConvertFrom-Json
    if ($jsonContent.spdxVersion -ne "SPDX-2.3") {
        throw "Expected spdxVersion 'SPDX-2.3', got '$($jsonContent.spdxVersion)'"
    }
    if ($jsonContent.dataLicense -ne "CC0-1.0") {
        throw "Expected dataLicense 'CC0-1.0', got '$($jsonContent.dataLicense)'"
    }
    if ($jsonContent.packages.Count -lt 50) {
        throw "Expected at least 50 third-party packages, got $($jsonContent.packages.Count)"
    }

    # Finding P3 verification: licenseConcluded must be NOASSERTION per SPDX 2.3 Section 7.13
    $samplePkg = $jsonContent.packages[0]
    if ($samplePkg.licenseConcluded -ne "NOASSERTION") {
        throw "Expected licenseConcluded to be 'NOASSERTION', got '$($samplePkg.licenseConcluded)'"
    }
    if ([string]::IsNullOrWhiteSpace($samplePkg.licenseDeclared)) {
        throw "Expected licenseDeclared to be populated with upstream package license"
    }
}

# Test 4: Custom OutputDir parameter writes to target directory
Run-TestCase "Custom OutputDir parameter isolates generated artifacts" {
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "aethershell_sbom_test_$([System.Guid]::NewGuid().ToString('N'))"
    try {
        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$sbomScript' -OutputDir '$tempDir' -Strict 2>&1"
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "Expected exit code 0, got $exitCode"
        }

        $expectedMd = Join-Path $tempDir "THIRD_PARTY_LICENSES.md"
        $expectedJson = Join-Path $tempDir "sbom.json"

        if (-not (Test-Path $expectedMd)) { throw "Expected $expectedMd to exist" }
        if (-not (Test-Path $expectedJson)) { throw "Expected $expectedJson to exist" }
    } finally {
        if (Test-Path $tempDir) {
            Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# Test 5: Generated files are BOM-free UTF-8
Run-TestCase "Generated files are UTF-8 without Byte Order Mark (BOM)" {
    $files = @(
        (Join-Path $workspaceRoot "docs\licensing\THIRD_PARTY_LICENSES.md"),
        (Join-Path $workspaceRoot "docs\licensing\sbom.json")
    )
    foreach ($f in $files) {
        $bytes = [System.IO.File]::ReadAllBytes($f)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "File $f contains a UTF-8 BOM, violating repository conventions"
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
