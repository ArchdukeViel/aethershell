<#
.SYNOPSIS
    AetherShell Automated Test Inventory & Workspace Accounting Auditor

.DESCRIPTION
    Programmatically audits and inventories tests defined across the AetherShell Cargo workspace.
    Supports both test discovery logs (`cargo test --workspace -- --list`) and execution logs
    (`cargo test --workspace`). Reconstructs exact per-crate and per-suite breakdowns, distinguishes
    discovered vs executed tests, detects repeated test executions, verifies Cargo process exit codes,
    and validates workspace member accounting against Cargo metadata.

.PARAMETER InputFile
    Optional path to a saved cargo test output file. If omitted, invokes cargo directly.

.PARAMETER Mode
    Audit mode: 'Auto' (default), 'Discovered' (for --list logs), or 'Executed' (for test run logs).

.PARAMETER Detailed
    Switch to output every individual test name per suite.

.PARAMETER Json
    Switch to output results as structured JSON (suitable for CI quality gates).

.PARAMETER Strict
    Enforce strict validation (fail immediately on unknown suites or incomplete parsing).

.EXAMPLE
    .\scripts\test_inventory.ps1

.EXAMPLE
    .\scripts\test_inventory.ps1 -Detailed

.EXAMPLE
    .\scripts\test_inventory.ps1 -Json

.EXAMPLE
    .\scripts\test_inventory.ps1 -InputFile "logs\cargo_test.log" -Mode Executed
#>

[CmdletBinding()]
param(
    [string]$InputFile = "",
    [ValidateSet("Auto", "Discovered", "Executed")]
    [string]$Mode = "Auto",
    [switch]$Detailed = $false,
    [switch]$Json = $false,
    [switch]$Strict = $false
)

$ErrorActionPreference = "Stop"

$WorkspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$CargoTomlPath = Join-Path $WorkspaceRoot "Cargo.toml"

if (-not (Test-Path $CargoTomlPath)) {
    throw "Cargo.toml not found at $CargoTomlPath"
}

# 1. Enumerate Workspace Crates from Cargo Metadata (with fallback to Cargo.toml)
$workspaceCrateNames = @()
$metadataAcquired = $false

try {
    $metadataRaw = & cargo metadata --no-deps --format-version 1 2>$null
    if ($LASTEXITCODE -eq 0 -and $metadataRaw) {
        $metaJson = $metadataRaw | ConvertFrom-Json
        if ($metaJson.workspace_members) {
            foreach ($pkgId in $metaJson.workspace_members) {
                # Format: "pkg_name version (path+file:...)"
                if ($pkgId -match '^([a-zA-Z0-9_-]+)\s+') {
                    $workspaceCrateNames += $Matches[1]
                }
            }
            $metadataAcquired = $true
        }
    }
} catch {
    # Fallback to Cargo.toml parsing
}

if (-not $metadataAcquired -or $workspaceCrateNames.Count -eq 0) {
    $cargoTomlContent = Get-Content $CargoTomlPath -Raw
    if ($cargoTomlContent -match '(?ms)members\s*=\s*\[(.*?)\]') {
        $membersRaw = $Matches[1]
        $workspaceCrateNames = [regex]::Matches($membersRaw, '"([^"]+)"') | ForEach-Object {
            Split-Path -Leaf $_.Groups[1].Value
        }
    }
}

$workspaceCrateNames = $workspaceCrateNames | Sort-Object -Unique
$workspaceCratesCount = $workspaceCrateNames.Count

if ($workspaceCratesCount -eq 0) {
    throw "Failed to enumerate workspace crates from metadata or Cargo.toml"
}

# 2. Acquire Cargo Output
$rawOutput = ""
$cargoExitCode = 0

if ($InputFile -and (Test-Path $InputFile)) {
    $rawOutput = (Get-Content $InputFile -Raw) -replace "`r", ""
    if ([string]::IsNullOrWhiteSpace($rawOutput)) {
        throw "Specified InputFile '$InputFile' is empty"
    }
} else {
    Push-Location $WorkspaceRoot
    $oldEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        if ($Mode -eq "Executed") {
            $rawOutput = (& cargo test --workspace 2>&1 | Out-String) -replace "`r", ""
        } else {
            $rawOutput = (& cargo test --workspace -- --list 2>&1 | Out-String) -replace "`r", ""
        }
        $cargoExitCode = $LASTEXITCODE

        if ($cargoExitCode -ne 0) {
            throw "Cargo process exited with non-zero exit code $cargoExitCode`n$rawOutput"
        }
    } finally {
        $ErrorActionPreference = $oldEAP
        Pop-Location
    }
}

# 3. Detect Log Mode (Discovered vs Executed)
$detectedMode = $Mode
if ($detectedMode -eq "Auto") {
    if ($rawOutput -match 'test result:\s*(?:ok|FAILED)\.\s*\d+\s+passed') {
        $detectedMode = "Executed"
    } else {
        $detectedMode = "Discovered"
    }
}

# 4. Parse Suites and Tests
$pattern = '(?ms)^\s*Running\s+([^\r\n]+)\s+\(([^\r\n]+?)\)[\r\n]+(.*?)(?=^\s*(?:Running|Doc-tests|\Z))'
$matchesFound = [regex]::Matches($rawOutput, $pattern)

if ($matchesFound.Count -eq 0 -and $rawOutput.Length -gt 0) {
    if ($Strict) {
        throw "Failed to parse any test suites from log output (malformed or empty suite listing)"
    }
}

$suites = @()
$globalUniqueTestMap = [System.Collections.Generic.Dictionary[string, object]]::new()
$repeatedExecutionList = [System.Collections.Generic.List[object]]::new()
$unknownSuites = @()

foreach ($m in $matchesFound) {
    $target = $m.Groups[1].Value.Trim()
    $exePath = $m.Groups[2].Value.Trim()
    $exeName = Split-Path -Leaf $exePath
    $body = $m.Groups[3].Value

    # Classify crate
    $crate = ""
    $suiteType = "Unit"

    if ($target -match '^[tT]ests[\\/]([^\\/]+)\.rs$') {
        $suiteType = "Integration"
        if ($exeName -match '^runtime_benchmark_integration_test-') {
            $crate = "aethershell-diagnostics"
        } else {
            $crate = "aethershell-app"
        }
    } elseif ($exeName -match '^aethershell_([a-z0-9_]+)-') {
        $crate = "aethershell-" + ($Matches[1] -replace '_', '-')
    } elseif ($exeName -match '^aethershell-[a-f0-9]{16}\.exe$') {
        $crate = "aethershell-app"
    }

    if ($crate -and ($workspaceCrateNames -notcontains $crate)) {
        # Check if known alias or exact match
        $matched = $workspaceCrateNames | Where-Object { $_ -eq $crate }
        if (-not $matched) {
            $unknownSuites += [PSCustomObject]@{
                Target = $target
                ExeName = $exeName
                GuessedCrate = $crate
            }
        }
    } elseif (-not $crate) {
        $unknownSuites += [PSCustomObject]@{
            Target = $target
            ExeName = $exeName
            GuessedCrate = "unknown"
        }
    }

    $suiteTests = @()
    $passedCount = 0
    $failedCount = 0
    $ignoredCount = 0

    if ($detectedMode -eq "Executed") {
        # Parse executed tests: "test <name> ... ok|FAILED|ignored"
        $execPattern = '(?m)^\s*test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)\b'
        foreach ($lineMatch in [regex]::Matches($body, $execPattern)) {
            $tName = $lineMatch.Groups[1].Value
            $status = $lineMatch.Groups[2].Value
            if ($status -eq "ok") { $passedCount++ }
            elseif ($status -eq "FAILED") { $failedCount++ }
            elseif ($status -eq "ignored") { $ignoredCount++ }

            $suiteTests += $tName

            $globalKey = "$crate::$tName"
            if ($globalUniqueTestMap.ContainsKey($globalKey)) {
                $repeatedExecutionList.Add([PSCustomObject]@{
                    TestName = $tName
                    Crate = $crate
                    Suite = $target
                    Status = $status
                })
            } else {
                $globalUniqueTestMap[$globalKey] = [PSCustomObject]@{
                    TestName = $tName
                    Crate = $crate
                    Suite = $target
                    Type = $suiteType
                    Status = $status
                }
            }
        }

        # Validate parsed count against suite summary line if present
        if ($body -match 'test result:\s*(?:ok|FAILED)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;\s*(\d+)\s+ignored') {
            $expectedPassed = [int]$Matches[1]
            $expectedFailed = [int]$Matches[2]
            $expectedIgnored = [int]$Matches[3]
            if ($passedCount -ne $expectedPassed -or $failedCount -ne $expectedFailed) {
                if ($Strict) {
                    throw "Suite '$target' count mismatch: parsed $passedCount passed / $failedCount failed, but summary reported $expectedPassed / $expectedFailed"
                }
            }
        }
    } else {
        # Parse discovered tests: "<name>: test"
        foreach ($lineMatch in [regex]::Matches($body, '(?m)^\s*(\S+):\s*test\s*$')) {
            $tName = $lineMatch.Groups[1].Value
            $suiteTests += $tName

            $globalKey = "$crate::$tName"
            if ($globalUniqueTestMap.ContainsKey($globalKey)) {
                $repeatedExecutionList.Add([PSCustomObject]@{
                    TestName = $tName
                    Crate = $crate
                    Suite = $target
                    Status = "discovered"
                })
            } else {
                $globalUniqueTestMap[$globalKey] = [PSCustomObject]@{
                    TestName = $tName
                    Crate = $crate
                    Suite = $target
                    Type = $suiteType
                    Status = "discovered"
                }
            }
        }
    }

    $suites += [PSCustomObject]@{
        Crate = $crate
        Suite = $target
        Binary = $exeName
        Type = $suiteType
        Count = $suiteTests.Count
        Passed = $passedCount
        Failed = $failedCount
        Ignored = $ignoredCount
        Tests = $suiteTests
    }
}

if ($Strict -and $unknownSuites.Count -gt 0) {
    throw "Strict validation failed: $($unknownSuites.Count) unknown test suites detected: $($unknownSuites | ConvertTo-Json)"
}

# 5. Aggregate by Crate
$crateSummary = @()
foreach ($cName in ($workspaceCrateNames | Sort-Object)) {
    $crateSuites = $suites | Where-Object { $_.Crate -eq $cName }
    $unitCount = ($crateSuites | Where-Object { $_.Type -eq "Unit" } | Measure-Object -Property Count -Sum).Sum
    $integCount = ($crateSuites | Where-Object { $_.Type -eq "Integration" } | Measure-Object -Property Count -Sum).Sum
    if ($null -eq $unitCount) { $unitCount = 0 }
    if ($null -eq $integCount) { $integCount = 0 }

    $crateSummary += [PSCustomObject]@{
        Crate = $cName
        UnitTests = [int]$unitCount
        IntegrationTests = [int]$integCount
        TotalTests = [int]($unitCount + $integCount)
        SuitesCount = @($crateSuites).Count
    }
}

$totalUnit = ($crateSummary | Measure-Object -Property UnitTests -Sum).Sum
$totalInteg = ($crateSummary | Measure-Object -Property IntegrationTests -Sum).Sum
$totalUnique = ($crateSummary | Measure-Object -Property TotalTests -Sum).Sum
$totalExecutions = ($suites | Measure-Object -Property Count -Sum).Sum
$totalPassed = ($suites | Measure-Object -Property Passed -Sum).Sum
$totalFailed = ($suites | Measure-Object -Property Failed -Sum).Sum

# 6. Structured Output / JSON
if ($Json) {
    $report = [PSCustomObject]@{
        WorkspaceRoot = $WorkspaceRoot
        WorkspaceCratesCount = $workspaceCratesCount
        WorkspaceCrates = $workspaceCrateNames
        AuditMode = $detectedMode
        TotalUniqueTests = [int]$totalUnique
        TotalUnitTests = [int]$totalUnit
        TotalIntegrationTests = [int]$totalInteg
        TotalTestExecutions = [int]$totalExecutions
        TotalPassed = [int]$totalPassed
        TotalFailed = [int]$totalFailed
        RepeatedExecutionsCount = $repeatedExecutionList.Count
        RepeatedExecutions = $repeatedExecutionList
        UnknownSuitesCount = $unknownSuites.Count
        UnknownSuites = $unknownSuites
        CrateSummary = $crateSummary
        Suites = ($suites | Select-Object Crate, Suite, Binary, Type, Count, Passed, Failed, Ignored)
    }
    $report | ConvertTo-Json -Depth 5
    return
}

# 7. Human-Readable Terminal Presentation
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "                  AETHERSHELL WORKSPACE TEST INVENTORY AUDIT                    " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "Workspace Root  : $WorkspaceRoot"
Write-Host "Workspace Crates: $workspaceCratesCount (Verified against Cargo metadata/toml)"
Write-Host "Audit Mode      : $detectedMode"
Write-Host ""

Write-Host "--- Crate Test Ledger ---" -ForegroundColor Yellow
$crateSummary | Format-Table -Property Crate, UnitTests, IntegrationTests, TotalTests, SuitesCount -AutoSize

Write-Host "--- Per-Suite Breakdown ---" -ForegroundColor Yellow
$suites | Where-Object { $_.Count -gt 0 } | Format-Table -Property Count, Passed, Failed, Type, Crate, Suite, Binary -AutoSize

if ($Detailed) {
    Write-Host "--- Detailed Test Listing ---" -ForegroundColor Yellow
    foreach ($s in ($suites | Where-Object { $_.Count -gt 0 })) {
        Write-Host "`n[$($s.Crate)] $($s.Suite) ($($s.Count) tests):" -ForegroundColor Green
        foreach ($t in $s.Tests) {
            Write-Host "  - $t"
        }
    }
}

if ($repeatedExecutionList.Count -gt 0) {
    Write-Host "--- Repeated Test Executions Detected ---" -ForegroundColor Magenta
    Write-Host "  Repeated Count: $($repeatedExecutionList.Count)"
    foreach ($rep in $repeatedExecutionList) {
        Write-Host "  * $($rep.Crate) :: $($rep.TestName) (in $($rep.Suite))" -ForegroundColor Gray
    }
    Write-Host ""
}

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "SUMMARY AUDIT VERDICT:" -ForegroundColor White
Write-Host "  Unique Unit Tests        : $totalUnit"
Write-Host "  Unique Integration Tests : $totalInteg"
Write-Host "  Total Unique Tests       : $totalUnique"
if ($detectedMode -eq "Executed") {
    Write-Host "  Total Executed Passed    : $totalPassed" -ForegroundColor Green
    if ($totalFailed -gt 0) {
        Write-Host "  Total Executed Failed    : $totalFailed" -ForegroundColor Red
    }
}
Write-Host "================================================================================" -ForegroundColor Cyan
