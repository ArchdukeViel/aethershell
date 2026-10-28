<#
.SYNOPSIS
    AetherShell Performance Benchmark Runner (PRD Section 14, Q03:A, Q37:A)
.DESCRIPTION
    Executes the multi-process runtime benchmark harness conforming strictly to PRD Section 14,
    Q03:A, and Q37:A. Strictly blocks intrusive application execution on bare-metal physical host,
    mandates isolated disposable VM execution for formal qualification, preserves host AppData,
    guarantees environment restoration in finally, validates boundary-contained sandbox ownership,
    and isolates run artifacts with unique output paths.
.PARAMETER DryRun
    Verifies prerequisites, environment detection, and sandbox creation/cleanup without launching
    the benchmarked Core executable or mutating shell state. Authorized on physical host.
.PARAMETER OutputFile
    Explicit path to save benchmark ledger JSON. Defaults to unique run artifact under target\benchmark_results\.
    Never overwrites historical benchmark evidence by default.
#>

[CmdletBinding()]
param(
    [switch]$DryRun = $false,
    [string]$OutputFile = ""
)

$ErrorActionPreference = "Stop"

# Preserve original environment variable presence and values for guaranteed restoration
$origAppData = [System.Environment]::GetEnvironmentVariable("AETHERSHELL_APP_DATA_DIR", [System.EnvironmentVariableTarget]::Process)
$origSession = [System.Environment]::GetEnvironmentVariable("AETHERSHELL_SESSION_TOKEN_PATH", [System.EnvironmentVariableTarget]::Process)

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

$uniqueSuffix = [Guid]::NewGuid().ToString("N").Substring(0, 8)

if ([string]::IsNullOrWhiteSpace($OutputFile)) {
    $resultsDir = Join-Path $workspaceRoot "target\benchmark_results"
    if (-not (Test-Path $resultsDir)) {
        New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null
    }
    $outputJson = Join-Path $resultsDir "ryzen5_5500u_benchmark_run_${PID}_${uniqueSuffix}.json"
} else {
    $outputJson = [System.IO.Path]::GetFullPath($OutputFile)
    $parentDir = [System.IO.Path]::GetDirectoryName($outputJson)
    if (-not (Test-Path $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
}

# 1. Environment & Host Safety Preflight (PRD Section 18.3, Q03:A, Q37:A)
$isVm = $false
try {
    $cs = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction SilentlyContinue
    if ($cs -and $cs.Model -match 'Virtual|VMware|VirtualBox|HVM|KVM|Bochs') {
        $isVm = $true
    }
    $bios = Get-CimInstance -ClassName Win32_BIOS -ErrorAction SilentlyContinue
    if ($bios -and ($bios.SerialNumber -match 'VMware|Virtual|QEMU' -or $bios.Version -match 'VBOX|VMware|Hyper-V')) {
        $isVm = $true
    }
} catch {
    # Fail-closed: assume physical host if detection query fails
    $isVm = $false
}

Write-Host "Host environment detection: $(if ($isVm) { 'Dedicated VM (Isolated)' } else { 'Physical Host (Daily-Use)' })"

if (-not $isVm) {
    if (-not $DryRun) {
        Write-Error @"
SAFETY BOUNDARY ENFORCEMENT (PRD Section 18.3, Q03:A, Q37:A):
The Core executable ('aethershell.exe') discovers desktop window hierarchies, initializes Direct2D desktop overlay windows across physical monitors, and registers global hotkeys (Win+Ctrl+A), even when '--safe-mode' is specified.
The physical host is a daily-use production computer. Under approved safety decisions Q03:A and Q37:A, disruptive shell operations and desktop layering tests are strictly prohibited on the physical host machine.
Formal acceptance qualification of the live Core engine requires an isolated disposable VM (BLOCKED_DISPOSABLE_VM_PREREQUISITE).
To safely verify benchmark script prerequisites, environment detection, and sandbox containment on the physical host without launching intrusive processes, run with '-DryRun'.
"@
        throw "Formal acceptance benchmark blocked on physical host per Q03:A / Q37:A safety constraints."
    } else {
        Write-Host "Physical host detected. DryRun mode authorized for non-invasive preflight and containment verification."
    }
}

# 2. Verify pre-test host cleanliness (Strictly Read-Only, Never Mutates User AppData)
$localAppDataDir = "$env:LOCALAPPDATA\AetherShell"
if (Test-Path $localAppDataDir) {
    $preCount = (Get-ChildItem -LiteralPath $localAppDataDir -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "Pre-benchmark %LOCALAPPDATA%\AetherShell item count: $preCount"
    if ($preCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell is not clean before test ($preCount items found). Manual inspection required; automated deletion is prohibited."
    }
} else {
    Write-Host "Pre-benchmark %LOCALAPPDATA%\AetherShell does not exist (clean)."
}

# 3. Establish Unique Test-Owned Temporary Sandbox
$sandboxDir = Join-Path $workspaceRoot "target\benchmark_sandbox_${PID}_${uniqueSuffix}"
New-Item -ItemType Directory -Path $sandboxDir -Force | Out-Null

$sentinelPath = Join-Path $sandboxDir ".benchmark_sandbox_owner"
$sentinelObj = @{
    marker = "AETHERSHELL_BENCHMARK_SANDBOX"
    owner_pid = $PID
    unique_suffix = $uniqueSuffix
    workspace_root = $workspaceRoot
    created_utc = ([DateTime]::UtcNow.ToString("o"))
}
$sentinelJson = $sentinelObj | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($sentinelPath, $sentinelJson, [System.Text.UTF8Encoding]::new($false))

$env:AETHERSHELL_APP_DATA_DIR = $sandboxDir
$env:AETHERSHELL_SESSION_TOKEN_PATH = Join-Path $sandboxDir "session.token"

Write-Host "Unique verified AppData sandbox established at: $sandboxDir"

$coreProcess = $null
$benchmarkExit = -1
try {
    if ($DryRun) {
        Write-Host "Dry run preflight complete. Skipping Core launch and proceeding to verified sandbox cleanup."
    } else {
        # 4. Launch Core process under strict containment inside verified VM
        Write-Host "Launching background Core 'aethershell.exe' inside verified disposable VM..."
        $coreProcess = Start-Process -FilePath $exePath -PassThru
        Start-Sleep -Seconds 5

        if ($coreProcess.HasExited) {
            throw "Core process exited prematurely with exit code $($coreProcess.ExitCode)"
        }

        Write-Host "Core process active (PID: $($coreProcess.Id)). Starting formal PRD Section 14 benchmark..."
        Write-Host "Procedure: 5 repetitions of 60s each with 5s warmup (~305 seconds total)..."

        # Execute formal benchmark
        $benchArgs = @(
            "--benchmark-runtime", "minimal_idle",
            "--pid-core", "$($coreProcess.Id)",
            "--formal",
            "--repetitions", "5",
            "--duration", "60",
            "--warmup", "5",
            "--output", "$outputJson"
        )

        & $exePath @benchArgs
        $benchmarkExit = $LASTEXITCODE
        Write-Host "Benchmark process completed with exit code: $benchmarkExit"

        if ($benchmarkExit -ne 0) {
            throw "Benchmark harness failed with process exit code $benchmarkExit"
        }

        if (-not (Test-Path $outputJson) -or ((Get-Item -LiteralPath $outputJson).Length -eq 0)) {
            throw "Benchmark failed to produce required output ledger artifact at '$outputJson'"
        }
    }
} finally {
    # Guaranteed process termination
    if ($null -ne $coreProcess -and -not $coreProcess.HasExited) {
        Write-Host "Terminating Core process (PID: $($coreProcess.Id))..."
        Stop-Process -Id $coreProcess.Id -Force
        $coreProcess.WaitForExit(5000)
    }

    # Guaranteed environment variable restoration
    if ($null -ne $origAppData) {
        $env:AETHERSHELL_APP_DATA_DIR = $origAppData
    } else {
        Remove-Item Env:\AETHERSHELL_APP_DATA_DIR -ErrorAction SilentlyContinue
    }
    if ($null -ne $origSession) {
        $env:AETHERSHELL_SESSION_TOKEN_PATH = $origSession
    } else {
        Remove-Item Env:\AETHERSHELL_SESSION_TOKEN_PATH -ErrorAction SilentlyContinue
    }

    # 5. Strict Ownership Verification & Scoped Sandbox Cleanup
    if (Test-Path -LiteralPath $sandboxDir) {
        $targetRoot = (Resolve-Path (Join-Path $workspaceRoot "target")).Path.TrimEnd('\')
        $targetPrefix = $targetRoot + [System.IO.Path]::DirectorySeparatorChar
        $resolvedSandbox = [System.IO.Path]::GetFullPath($sandboxDir).TrimEnd('\')

        $isDescendant = $resolvedSandbox.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)

        if (-not $isDescendant) {
            Write-Warning "Safety assertion failed: Sandbox path '$resolvedSandbox' is not a descendant of '$targetRoot'. Refusing cleanup."
        } else {
            $sandboxItem = Get-Item -LiteralPath $resolvedSandbox -Force -ErrorAction SilentlyContinue
            $isReparsePoint = $sandboxItem -and (($sandboxItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)

            if ($isReparsePoint) {
                Write-Warning "Safety assertion failed: Sandbox path '$resolvedSandbox' is a reparse point/junction. Refusing cleanup."
            } elseif (-not (Test-Path -LiteralPath $sentinelPath)) {
                Write-Warning "Safety assertion failed: Sandbox sentinel missing at '$sentinelPath'. Refusing cleanup."
            } else {
                try {
                    $sentinelRaw = [System.IO.File]::ReadAllText($sentinelPath)
                    $parsedSentinel = $sentinelRaw | ConvertFrom-Json
                    if ($parsedSentinel.marker -eq "AETHERSHELL_BENCHMARK_SANDBOX" -and
                        $parsedSentinel.owner_pid -eq $PID -and
                        $parsedSentinel.unique_suffix -eq $uniqueSuffix -and
                        $parsedSentinel.workspace_root -eq $workspaceRoot) {
                        Remove-Item -LiteralPath $resolvedSandbox -Recurse -Force -ErrorAction Stop
                        Write-Host "Scoped cleanup verified and completed for sandbox: $resolvedSandbox"
                    } else {
                        Write-Warning "Safety assertion failed: Sandbox sentinel content mismatch. Refusing cleanup."
                    }
                } catch {
                    Write-Warning "Safety assertion failed: Error validating sandbox sentinel: $_. Refusing cleanup."
                }
            }
        }
    }
}

# 6. Verify post-test host cleanliness (Strictly Read-Only, Never Mutates User AppData)
if (Test-Path $localAppDataDir) {
    $postCount = (Get-ChildItem -LiteralPath $localAppDataDir -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "Post-benchmark %LOCALAPPDATA%\AetherShell item count: $postCount"
    if ($postCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell was modified during test ($postCount items found)! Manual inspection required; automated deletion is prohibited."
    }
} else {
    Write-Host "Post-benchmark %LOCALAPPDATA%\AetherShell does not exist (clean)."
}

if ($DryRun) {
    Write-Host "Benchmark runner dry-run verification completed successfully."
} else {
    Write-Host "Benchmark procedure completed successfully. Output ledger saved to: $outputJson"
}
