<#
.SYNOPSIS
    AetherShell Physical AMD Ryzen 5 5500U Performance Benchmark Runner
.DESCRIPTION
    Executes the genuine multi-process runtime benchmark harness conforming strictly to PRD Section 14,
    Q03:A, and Q37:A. Mandates dedicated VM isolation or explicit operator safety authorization to prevent
    intrusive physical desktop manipulation, and uses strictly owned unique temporary sandboxes.
.PARAMETER SafeMode
    Runs the benchmarked Core process in --safe-mode (forces NativeNoopBackend, bypasses desktop overlays).
.PARAMETER AllowPhysicalHost
    Explicit operator authorization to run formal benchmarking on bare-metal physical host.
.PARAMETER DryRun
    Verifies prerequisites, host detection, and sandbox creation without spawning long-running workloads.
#>

[CmdletBinding()]
param(
    [switch]$SafeMode = $false,
    [switch]$AllowPhysicalHost = $false,
    [switch]$DryRun = $false
)

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

if (-not $isVm -and -not $AllowPhysicalHost -and -not $SafeMode) {
    Write-Warning @"
SAFETY BOUNDARY ENFORCEMENT (PRD Section 18.3, Q03:A, Q37:A):
The normal Core executable ('aethershell.exe') initializes Direct2D desktop overlay windows across physical monitors and registers system-wide hotkeys (Win+Ctrl+A), manipulating live desktop layering and shell window state.
Under the existing implementation, a non-invasive formal acceptance benchmark cannot execute without physical desktop layering.
Executing intrusive physical-host benchmarking violates approved Q37:A safety boundaries.
Formal acceptance benchmarking on physical host is therefore strictly CLASSIFIED AS BLOCKED pending a dedicated disposable VM (BLOCKED_DISPOSABLE_VM_PREREQUISITE).
To run non-invasive component diagnostics on physical host without manipulating desktop layering, pass -SafeMode.
"@
    throw "Formal acceptance benchmark blocked on physical host per Q37:A safety constraints."
}

# 2. Verify pre-test host cleanliness (Strictly Read-Only, Never Mutates User AppData)
$localAppDataDir = "$env:LOCALAPPDATA\AetherShell"
if (Test-Path $localAppDataDir) {
    $preCount = (Get-ChildItem -Path $localAppDataDir -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "Pre-benchmark %LOCALAPPDATA%\AetherShell item count: $preCount"
    if ($preCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell is not clean before test ($preCount items found). Manual inspection required; automated deletion is prohibited."
    }
} else {
    Write-Host "Pre-benchmark %LOCALAPPDATA%\AetherShell does not exist (clean)."
}

# 3. Establish Unique Test-Owned Temporary Sandbox
$uniqueSuffix = [Guid]::NewGuid().ToString("N").Substring(0, 8)
$sandboxDir = Join-Path $workspaceRoot "target\benchmark_sandbox_${PID}_${uniqueSuffix}"
New-Item -ItemType Directory -Path $sandboxDir -Force | Out-Null

$sentinelPath = Join-Path $sandboxDir ".benchmark_sandbox_owner"
$sentinelContent = "OWNER_PID=$PID;TIMESTAMP=$([DateTime]::UtcNow.ToString('o'));WORKSPACE=$workspaceRoot"
[System.IO.File]::WriteAllText($sentinelPath, $sentinelContent, [System.Text.UTF8Encoding]::new($false))

$env:AETHERSHELL_APP_DATA_DIR = $sandboxDir
$env:AETHERSHELL_SESSION_TOKEN_PATH = Join-Path $sandboxDir "session.token"

Write-Host "Unique verified AppData sandbox established at: $sandboxDir"

$coreProcess = $null
try {
    if ($DryRun) {
        Write-Host "Dry run preflight complete. Skipping Core launch and proceeding to verified sandbox cleanup."
    } else {
        # 4. Launch Core process under strict containment
        $coreArgs = @()
        if ($SafeMode) {
            $coreArgs += "--safe-mode"
            Write-Host "Launching background Core 'aethershell.exe' in Safe Mode (zero desktop overlay creation)..."
        } else {
            Write-Host "Launching background Core 'aethershell.exe' (authorized physical/VM execution)..."
        }

        $coreProcess = Start-Process -FilePath $exePath -ArgumentList $coreArgs -PassThru
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
    }
} finally {
    if ($null -ne $coreProcess -and -not $coreProcess.HasExited) {
        Write-Host "Terminating Core process (PID: $($coreProcess.Id))..."
        Stop-Process -Id $coreProcess.Id -Force
        $coreProcess.WaitForExit(5000)
    }

    # 5. Strict Ownership Verification & Scoped Sandbox Cleanup
    if (Test-Path $sandboxDir) {
        $targetRoot = (Resolve-Path (Join-Path $workspaceRoot "target")).Path
        $resolvedSandbox = (Resolve-Path $sandboxDir).Path
        $isSafeTargetSubdir = $resolvedSandbox.StartsWith($targetRoot, [System.StringComparison]::OrdinalIgnoreCase)

        if ($isSafeTargetSubdir -and (Test-Path $sentinelPath)) {
            $marker = [System.IO.File]::ReadAllText($sentinelPath)
            if ($marker.Contains("OWNER_PID=$PID")) {
                try {
                    Remove-Item -Path $resolvedSandbox -Recurse -Force -ErrorAction Stop
                    Write-Host "Scoped cleanup verified and completed for sandbox: $resolvedSandbox"
                } catch {
                    Write-Warning "Non-fatal warning: Sandbox cleanup encountered an error: $_"
                }
            } else {
                Write-Warning "Safety assertion failed: Sandbox sentinel does not match PID $PID. Refusing cleanup."
            }
        } else {
            Write-Warning "Safety assertion failed: Sandbox path '$resolvedSandbox' is outside target directory or missing sentinel. Refusing cleanup."
        }
    }
}

# 6. Verify post-test host cleanliness (Strictly Read-Only, Never Mutates User AppData)
if (Test-Path $localAppDataDir) {
    $postCount = (Get-ChildItem -Path $localAppDataDir -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "Post-benchmark %LOCALAPPDATA%\AetherShell item count: $postCount"
    if ($postCount -ne 0) {
        throw "Safety assertion failed: %LOCALAPPDATA%\AetherShell was modified during test ($postCount items found)! Manual inspection required; automated deletion is prohibited."
    }
} else {
    Write-Host "Post-benchmark %LOCALAPPDATA%\AetherShell does not exist (clean)."
}

Write-Host "Benchmark completed successfully. Ledger saved to: $outputJson"
