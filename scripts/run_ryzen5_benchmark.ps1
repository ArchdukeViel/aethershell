<#
.SYNOPSIS
    AetherShell Physical AMD Ryzen 5 5500U Performance Benchmark Runner
.DESCRIPTION
    Executes the genuine multi-process runtime benchmark harness conforming strictly to PRD Section 14,
    Q03:A, and Q37:A. Runs 5 repetitions of 60 seconds each with 5-second warmup on the physical host
    under isolated AppData sandbox, ensuring zero mutation of production %LOCALAPPDATA%\AetherShell.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$exePath = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release\aethershell.exe"
if (-not (Test-Path $exePath)) {
    $fallbackPath = Join-Path $workspaceRoot "target\release\aethershell.exe"
    if (Test-Path $fallbackPath) {
        $exePath = $fallbackPath
    } else {
        throw "Release executable not found at $exePath or $fallbackPath. Run 'cargo build --release --workspace' first."
    }
}
$outputJson = Join-Path $workspaceRoot "docs\qualification\ryzen5_5500u_benchmark_ledger.json"

# 1. Verify pre-test host cleanliness
$localAppDataDir = "$env:LOCALAPPDATA\AetherShell"
if (Test-Path $localAppDataDir) {
    $preCount = (Get-ChildItem $localAppDataDir).Count
    Write-Host "Pre-benchmark %LOCALAPPDATA%\AetherShell item count: $preCount"
    if ($preCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell is not clean before test ($preCount items found)."
    }
}

# 2. Establish isolated AppData sandbox
$sandboxDir = Join-Path $workspaceRoot "target\benchmark_sandbox"
if (Test-Path $sandboxDir) {
    Remove-Item -Recurse -Force $sandboxDir
}
New-Item -ItemType Directory -Path $sandboxDir -Force | Out-Null

$env:AETHERSHELL_APP_DATA_DIR = $sandboxDir
$env:AETHERSHELL_SESSION_TOKEN_PATH = Join-Path $sandboxDir "session.token"

Write-Host "Isolated AppData configured at: $sandboxDir"
Write-Host "Launching background Core 'aethershell.exe'..."

# 3. Launch Core process
$coreProcess = Start-Process -FilePath $exePath -PassThru
Start-Sleep -Seconds 5

try {
    if ($coreProcess.HasExited) {
        throw "Core process exited prematurely with exit code $($coreProcess.ExitCode)"
    }

    Write-Host "Core process active (PID: $($coreProcess.Id)). Starting formal PRD Section 14 benchmark..."
    Write-Host "Procedure: 5 repetitions of 60s each with 5s warmup (~305 seconds total)..."

    # 4. Execute formal benchmark
    & $exePath --benchmark-runtime minimal_idle `
        --pid-core $($coreProcess.Id) `
        --formal `
        --repetitions 5 `
        --duration 60 `
        --warmup 5 `
        --output $outputJson

    $benchmarkExit = $LASTEXITCODE
    Write-Host "Benchmark process completed with exit code: $benchmarkExit"
} finally {
    Write-Host "Terminating Core process (PID: $($coreProcess.Id))..."
    if (-not $coreProcess.HasExited) {
        Stop-Process -Id $coreProcess.Id -Force
        $coreProcess.WaitForExit(5000)
    }

    # Clean up sandbox
    if (Test-Path $sandboxDir) {
        Remove-Item -Recurse -Force $sandboxDir
    }
}

# 5. Verify post-test host cleanliness
if (Test-Path $localAppDataDir) {
    $postCount = (Get-ChildItem $localAppDataDir).Count
    Write-Host "Post-benchmark %LOCALAPPDATA%\AetherShell item count: $postCount"
    if ($postCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell was modified during test ($postCount items found)!"
    }
}

Write-Host "Benchmark completed successfully. Ledger saved to: $outputJson"
