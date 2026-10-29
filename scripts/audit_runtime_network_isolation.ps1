<#
.SYNOPSIS
    Audits runtime network socket isolation for AetherShell (Gate G16).

.DESCRIPTION
    Verifies that AetherShell processes open strictly zero inbound listening ports
    and zero outbound TCP/UDP network connections, upholding the strict zero-telemetry
    and privacy guarantees mandated by PRD Gate G16.

.PARAMETER TargetPid
    Optional Process ID to audit. If omitted, searches for any running 'aethershell' processes.

.PARAMETER Strict
    Fails closed with a non-zero exit code if no target process exists or if any active
    network socket is detected.

.PARAMETER SpawnTestProcess
    Spawns an isolated test AetherShell process in a sandboxed temporary environment,
    audits its socket tables while active, and cleanly terminates it upon audit completion.

.PARAMETER BinaryPath
    Optional path to the aethershell executable when using -SpawnTestProcess.
#>

[CmdletBinding()]
param(
    [int]$TargetPid = 0,
    [switch]$Strict,
    [switch]$SpawnTestProcess,
    [string]$BinaryPath = ""
)

$ErrorActionPreference = "Stop"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "                 AETHERSHELL RUNTIME NETWORK ISOLATION AUDIT (G16)              " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$pidsToAudit = [System.Collections.Generic.List[int]]::new()
$spawnedProcess = $null
$tempSandboxDir = $null

try {
    if ($SpawnTestProcess) {
        $resolvedBin = $null
        if ($BinaryPath -and (Test-Path $BinaryPath)) {
            $resolvedBin = (Resolve-Path $BinaryPath).Path
        } else {
            $candidatePaths = @(
                (Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release\aethershell.exe"),
                (Join-Path $workspaceRoot "target\release\aethershell.exe"),
                (Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\debug\aethershell.exe"),
                (Join-Path $workspaceRoot "target\debug\aethershell.exe")
            )
            foreach ($candidate in $candidatePaths) {
                if (Test-Path $candidate) {
                    $resolvedBin = $candidate
                    break
                }
            }
        }

        if (-not $resolvedBin) {
            Write-Error "Cannot spawn test process: aethershell executable not found. Compile the binary before running with -SpawnTestProcess."
            exit 1
        }

        $tempSandboxDir = Join-Path ([System.IO.Path]::GetTempPath()) "aether_net_audit_$([System.Guid]::NewGuid().ToString('N'))"
        [void](New-Item -ItemType Directory -Path $tempSandboxDir -Force)

        Write-Host "Spawning isolated test AetherShell process: $resolvedBin" -ForegroundColor Yellow
        $psi = [System.Diagnostics.ProcessStartInfo]::new()
        $psi.FileName = $resolvedBin
        $psi.Arguments = "--benchmark 3"
        $psi.WorkingDirectory = $tempSandboxDir
        $psi.EnvironmentVariables["AETHERSHELL_PORTABLE_MODE"] = "1"
        $psi.EnvironmentVariables["LOCALAPPDATA"] = $tempSandboxDir
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true

        $spawnedProcess = [System.Diagnostics.Process]::Start($psi)
        if (-not $spawnedProcess) {
            Write-Error "Failed to launch test AetherShell process."
            exit 1
        }

        $pidsToAudit.Add($spawnedProcess.Id)
        Write-Host "Spawned test instance PID $($spawnedProcess.Id) for active socket inspection." -ForegroundColor Yellow
        Start-Sleep -Milliseconds 400
    } elseif ($TargetPid -gt 0) {
        $proc = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
        if (-not $proc) {
            Write-Host "[!] Target PID $TargetPid does not exist." -ForegroundColor Red
            Write-Error "Target PID $TargetPid does not exist."
            exit 1
        }
        $pidsToAudit.Add($TargetPid)
        Write-Host "Auditing specified Target PID: $TargetPid ($($proc.ProcessName))" -ForegroundColor Yellow
    } else {
        $aetherProcs = Get-Process | Where-Object { $_.ProcessName -like "*aethershell*" }
        if ($aetherProcs) {
            foreach ($p in $aetherProcs) {
                $pidsToAudit.Add($p.Id)
                Write-Host "Discovered running AetherShell process: PID $($p.Id) ($($p.ProcessName))" -ForegroundColor Yellow
            }
        } else {
            if ($Strict) {
                Write-Host "[!] Runtime network isolation audit failed: No running AetherShell processes found to audit. Cannot certify network isolation without an active process (Gate G16). Use -SpawnTestProcess to audit a live instance." -ForegroundColor Red
                Write-Error "Runtime network isolation audit failed: No running AetherShell processes found to audit. Cannot certify network isolation without an active process (Gate G16). Use -SpawnTestProcess to audit a live instance."
                exit 1
            }
            Write-Host "No active AetherShell instances currently running on host." -ForegroundColor Yellow
            Write-Host "Warning: Non-strict audit concluded without inspecting an active process." -ForegroundColor Yellow
        }
    }

    $violations = [System.Collections.Generic.List[string]]::new()

    if ($pidsToAudit.Count -gt 0) {
        foreach ($pidToCheck in $pidsToAudit) {
            # 1. Query TCP connections
            $tcpConns = Get-NetTCPConnection -OwningProcess $pidToCheck -ErrorAction SilentlyContinue
            if ($tcpConns) {
                foreach ($conn in $tcpConns) {
                    $violations.Add("Active TCP connection detected on PID $pidToCheck : $($conn.LocalAddress):$($conn.LocalPort) -> $($conn.RemoteAddress):$($conn.RemotePort) (State: $($conn.State))")
                }
            }

            # 2. Query UDP endpoints
            $udpEndpoints = Get-NetUDPEndpoint -OwningProcess $pidToCheck -ErrorAction SilentlyContinue
            if ($udpEndpoints) {
                foreach ($ep in $udpEndpoints) {
                    $violations.Add("Active UDP listener detected on PID $pidToCheck : $($ep.LocalAddress):$($ep.LocalPort)")
                }
            }
        }
    }

    Write-Host "`nNetwork Isolation Audit Results:" -ForegroundColor Cyan
    Write-Host "  Audited Process Count : $($pidsToAudit.Count)"
    Write-Host "  Socket Violations     : $($violations.Count)"

    if ($violations.Count -gt 0) {
        Write-Host "`n[!] Privacy Violations Detected:" -ForegroundColor Red
        foreach ($v in $violations) {
            Write-Host "    - $v" -ForegroundColor Red
        }
        if ($Strict) {
            Write-Error "Runtime network isolation audit failed: AetherShell processes must never bind or open network sockets (Gate G16)."
            exit 1
        }
    } else {
        Write-Host "`n[PASS] Zero network sockets or remote connections detected. Gate G16 privacy requirements satisfied." -ForegroundColor Green
    }

    Write-Host "`n================================================================================" -ForegroundColor Cyan
} finally {
    if ($spawnedProcess -and -not $spawnedProcess.HasExited) {
        try {
            $spawnedProcess.Kill()
            [void]$spawnedProcess.WaitForExit(3000)
        } catch { }
    }
    if ($tempSandboxDir -and (Test-Path $tempSandboxDir)) {
        Remove-Item -Path $tempSandboxDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
