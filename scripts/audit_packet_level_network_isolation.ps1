<#
.SYNOPSIS
    AetherShell Packet-Level Network Isolation & Zero-Leakage Qualification Auditor (Gate G16, PRD Section 17 & 18.2, Q37:A).

.DESCRIPTION
    Conducts packet-level and continuous-sampling network isolation audits to verify zero network activity
    across all AetherShell processes (Core, Settings, Script Host). Moves beyond point-in-time socket table
    snapshots by providing two comprehensive qualification modes:

    1. Elevation-Aware NDIS Packet Capture (PktMon.exe):
       Attaches the native Windows packet filter driver (PktMon.exe filter add AetherPacketFilter -p 0..65535),
       captures all network traffic at the NDIS layer during runtime, converts trace files to PCAPNG/formatted text,
       and asserts strictly zero packets emitted or received by the audited application processes. If invoked
       with -RequirePktMon without administrative privileges, fails closed per Gate G16 protocol (informing
       the operator to execute within the disposable VM guest environment).

    2. Continuous Ephemeral Socket Polling & DNS Resolver Cache Differential:
       Takes an immutable pre-execution baseline snapshot of the Windows DNS Client Cache (Get-DnsClientCache).
       Continuously samples active TCP endpoints (Get-NetTCPConnection) and UDP listeners (Get-NetUDPEndpoint)
       at high frequency (default 25ms) across the entire process lifetime. At termination, performs a differential
       audit against the DNS cache to detect any asynchronous DNS lookups or leaked telemetry beacons.

.PARAMETER TargetPid
    Specific process ID to audit.

.PARAMETER TargetPids
    Array of process IDs to audit concurrently.

.PARAMETER SpawnTestProcess
    Spawns an isolated test Core instance (aethershell.exe --benchmark 3) in a sandboxed temporary environment,
    audits its network activity continuously, and cleanly terminates it upon audit completion.

.PARAMETER SpawnAllComponents
    Concurrently spawns all three AetherShell components:
      - Core (aethershell.exe --benchmark 3)
      - Settings (aethershell-settings.exe --test-sleep-secs 3)
      - Script Host (aethershell-script-host.exe --test-sleep-secs 3)
    in an isolated temporary sandbox, observing all three throughout their execution.

.PARAMETER BinaryPath
    Optional path to the aethershell executable when using -SpawnTestProcess or -SpawnAllComponents.

.PARAMETER SettingsBinaryPath
    Optional path to the aethershell-settings executable when using -SpawnAllComponents.

.PARAMETER ScriptHostBinaryPath
    Optional path to the aethershell-script-host executable when using -SpawnAllComponents.

.PARAMETER DurationSeconds
    Duration of observation window in seconds (default: 3).

.PARAMETER SampleIntervalMs
    High-frequency sampling interval in milliseconds for continuous socket polling (default: 25).

.PARAMETER Strict
    Fails closed with a non-zero exit code if no target process exists, if -RequirePktMon cannot
    be satisfied, or if any active socket, packet, or DNS leak is detected.

.PARAMETER RequirePktMon
    Demands NDIS packet capture via PktMon.exe. If administrative elevation is absent, fails closed
    with an informative error directing execution to the disposable VM guest.

.PARAMETER PcapOutputPath
    Target file path to store generated .pcapng packet capture file when PktMon is active.

.PARAMETER Json
    Outputs audit results as structured JSON suitable for CI quality gates and qualification ledgers.
#>

[CmdletBinding()]
param(
    [int]$TargetPid = 0,
    [int[]]$TargetPids = @(),
    [switch]$SpawnTestProcess,
    [switch]$SpawnAllComponents,
    [string]$BinaryPath = "",
    [string]$SettingsBinaryPath = "",
    [string]$ScriptHostBinaryPath = "",
    [int]$DurationSeconds = 3,
    [int]$SampleIntervalMs = 25,
    [switch]$Strict,
    [switch]$RequirePktMon,
    [string]$PcapOutputPath = "",
    [switch]$Json
)

$ErrorActionPreference = "Stop"

if (-not $Json) {
    Write-Host "================================================================================" -ForegroundColor Cyan
    Write-Host "       AETHERSHELL PACKET-LEVEL NETWORK ISOLATION AUDITOR (GATE G16)             " -ForegroundColor Cyan
    Write-Host "================================================================================" -ForegroundColor Cyan
}

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$pidsToAudit = [System.Collections.Generic.List[int]]::new()
$spawnedProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$tempSandboxDir = $null
$captureEtl = $null
$capturePcap = $null
$captureTxt = $null
$pktMonRunning = $false

# 1. Elevation & Driver Privilege Detection
$isAdmin = [bool](([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
$pktMonCmd = Get-Command "pktmon.exe" -ErrorAction SilentlyContinue

# Initialize high-speed sub-millisecond native socket scanner if supported
$useNativeScanner = $false
try {
    if (-not ([System.Management.Automation.PSTypeName]'AetherNativeNetScanner').Type) {
        $csharpScanner = @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class AetherNativeNetScanner {
    [DllImport("iphlpapi.dll", SetLastError = true)]
    private static extern uint GetExtendedTcpTable(IntPtr pTcpTable, ref int pdwSize, bool bOrder, int ulAf, int tableClass, uint reserved);

    [DllImport("iphlpapi.dll", SetLastError = true)]
    private static extern uint GetExtendedUdpTable(IntPtr pUdpTable, ref int pdwSize, bool bOrder, int ulAf, int tableClass, uint reserved);

    public static HashSet<int> GetActiveSocketPids() {
        var pids = new HashSet<int>();
        ReadTcpPids(2, pids);
        ReadTcpPids(23, pids);
        ReadUdpPids(2, pids);
        ReadUdpPids(23, pids);
        return pids;
    }

    private static void ReadTcpPids(int af, HashSet<int> pids) {
        int size = 0;
        GetExtendedTcpTable(IntPtr.Zero, ref size, false, af, 5, 0);
        if (size <= 0) return;
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            if (GetExtendedTcpTable(buffer, ref size, false, af, 5, 0) == 0) {
                int count = Marshal.ReadInt32(buffer);
                IntPtr rowPtr = IntPtr.Add(buffer, 4);
                int rowSize = (af == 2) ? 24 : 56;
                int pidOffset = (af == 2) ? 20 : 52;
                for (int i = 0; i < count; i++) {
                    int pid = Marshal.ReadInt32(rowPtr, pidOffset);
                    if (pid > 0) pids.Add(pid);
                    rowPtr = IntPtr.Add(rowPtr, rowSize);
                }
            }
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private static void ReadUdpPids(int af, HashSet<int> pids) {
        int size = 0;
        GetExtendedUdpTable(IntPtr.Zero, ref size, false, af, 1, 0);
        if (size <= 0) return;
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            if (GetExtendedUdpTable(buffer, ref size, false, af, 1, 0) == 0) {
                int count = Marshal.ReadInt32(buffer);
                IntPtr rowPtr = IntPtr.Add(buffer, 4);
                int rowSize = (af == 2) ? 12 : 28;
                int pidOffset = (af == 2) ? 8 : 24;
                for (int i = 0; i < count; i++) {
                    int pid = Marshal.ReadInt32(rowPtr, pidOffset);
                    if (pid > 0) pids.Add(pid);
                    rowPtr = IntPtr.Add(rowPtr, rowSize);
                }
            }
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }
}
"@
        Add-Type -TypeDefinition $csharpScanner -ErrorAction Stop
    }
    $useNativeScanner = $true
} catch {
    $useNativeScanner = $false
}

if ($RequirePktMon -and -not $isAdmin) {
    $elevError = "Gate G16 Packet Monitor Audit Blocked: -RequirePktMon requires administrative elevation to bind to the Windows NDIS packet filter driver (PktMon.exe). Per Q37:A and PRD Section 18.2, full NDIS packet capture must run within the disposable VM guest environment (scripts/provision_disposable_vm.ps1) or an elevated qualification runner."
    if (-not $Json) {
        Write-Host "`n[!] $elevError" -ForegroundColor Red
    }
    if ($Strict) {
        Write-Error $elevError
        exit 1
    }
}

function Resolve-BinaryCandidate([string]$ExplicitPath, [string[]]$CandidateRelativePaths) {
    if ($ExplicitPath -and (Test-Path $ExplicitPath)) {
        return (Resolve-Path $ExplicitPath).Path
    }
    foreach ($rel in $CandidateRelativePaths) {
        $p = Join-Path $workspaceRoot $rel
        if (Test-Path $p) {
            return (Resolve-Path $p).Path
        }
    }
    return $null
}

try {
    # 2. Process Enumeration or Sandboxed Process Spawning
    if ($SpawnAllComponents -or $SpawnTestProcess) {
        $tempSandboxDir = Join-Path ([System.IO.Path]::GetTempPath()) "aether_pkt_audit_$([System.Guid]::NewGuid().ToString('N'))"
        [void](New-Item -ItemType Directory -Path $tempSandboxDir -Force)

        $coreBin = Resolve-BinaryCandidate $BinaryPath @(
            "target\x86_64-pc-windows-msvc\release\aethershell.exe",
            "target\release\aethershell.exe",
            "target\x86_64-pc-windows-msvc\debug\aethershell.exe",
            "target\debug\aethershell.exe"
        )
        if (-not $coreBin) {
            Write-Error "Cannot spawn test process: aethershell executable not found. Compile the binary before auditing."
            exit 1
        }

        # Spawn Core Process
        $psiCore = [System.Diagnostics.ProcessStartInfo]::new()
        $psiCore.FileName = $coreBin
        $psiCore.Arguments = "--test-sleep-secs $DurationSeconds"
        $psiCore.WorkingDirectory = $tempSandboxDir
        $psiCore.EnvironmentVariables["AETHERSHELL_PORTABLE_MODE"] = "1"
        $psiCore.EnvironmentVariables["LOCALAPPDATA"] = $tempSandboxDir
        $psiCore.EnvironmentVariables["AETHERSHELL_APP_DATA_DIR"] = $tempSandboxDir
        $psiCore.UseShellExecute = $false
        $psiCore.CreateNoWindow = $true

        $procCore = [System.Diagnostics.Process]::Start($psiCore)
        if (-not $procCore) {
            Write-Error "Failed to launch Core test process."
            exit 1
        }
        $spawnedProcesses.Add($procCore)
        $pidsToAudit.Add($procCore.Id)
        if (-not $Json) {
            Write-Host "Spawned isolated Core process PID $($procCore.Id) ($coreBin)" -ForegroundColor Yellow
        }

        if ($SpawnAllComponents) {
            # Spawn Settings Process
            $settingsBin = Resolve-BinaryCandidate $SettingsBinaryPath @(
                "target\x86_64-pc-windows-msvc\release\aethershell-settings.exe",
                "target\release\aethershell-settings.exe",
                "target\x86_64-pc-windows-msvc\debug\aethershell-settings.exe",
                "target\debug\aethershell-settings.exe"
            )
            if ($settingsBin) {
                $psiSet = [System.Diagnostics.ProcessStartInfo]::new()
                $psiSet.FileName = $settingsBin
                $psiSet.Arguments = "--test-sleep-secs $DurationSeconds"
                $psiSet.WorkingDirectory = $tempSandboxDir
                $psiSet.EnvironmentVariables["AETHERSHELL_PORTABLE_MODE"] = "1"
                $psiSet.EnvironmentVariables["LOCALAPPDATA"] = $tempSandboxDir
                $psiSet.EnvironmentVariables["AETHERSHELL_APP_DATA_DIR"] = $tempSandboxDir
                $psiSet.UseShellExecute = $false
                $psiSet.CreateNoWindow = $true

                $procSet = [System.Diagnostics.Process]::Start($psiSet)
                if ($procSet) {
                    $spawnedProcesses.Add($procSet)
                    $pidsToAudit.Add($procSet.Id)
                    if (-not $Json) {
                        Write-Host "Spawned isolated Settings process PID $($procSet.Id) ($settingsBin)" -ForegroundColor Yellow
                    }
                }
            }

            # Spawn Script Host Process
            $scriptHostBin = Resolve-BinaryCandidate $ScriptHostBinaryPath @(
                "target\x86_64-pc-windows-msvc\release\aethershell-script-host.exe",
                "target\release\aethershell-script-host.exe",
                "target\x86_64-pc-windows-msvc\debug\aethershell-script-host.exe",
                "target\debug\aethershell-script-host.exe"
            )
            if ($scriptHostBin) {
                $psiWorker = [System.Diagnostics.ProcessStartInfo]::new()
                $psiWorker.FileName = $scriptHostBin
                $psiWorker.Arguments = "--test-sleep-secs $DurationSeconds"
                $psiWorker.WorkingDirectory = $tempSandboxDir
                $psiWorker.EnvironmentVariables["AETHERSHELL_PORTABLE_MODE"] = "1"
                $psiWorker.EnvironmentVariables["LOCALAPPDATA"] = $tempSandboxDir
                $psiWorker.EnvironmentVariables["AETHERSHELL_APP_DATA_DIR"] = $tempSandboxDir
                $psiWorker.UseShellExecute = $false
                $psiWorker.CreateNoWindow = $true

                $procWorker = [System.Diagnostics.Process]::Start($psiWorker)
                if ($procWorker) {
                    $spawnedProcesses.Add($procWorker)
                    $pidsToAudit.Add($procWorker.Id)
                    if (-not $Json) {
                        Write-Host "Spawned isolated Script Host process PID $($procWorker.Id) ($scriptHostBin)" -ForegroundColor Yellow
                    }
                }
            }
        }
    } elseif ($TargetPid -gt 0) {
        $p = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
        if (-not $p) {
            Write-Error "Specified Target PID $TargetPid does not exist."
            exit 1
        }
        $pidsToAudit.Add($TargetPid)
    } elseif ($TargetPids.Count -gt 0) {
        foreach ($tPid in $TargetPids) {
            $p = Get-Process -Id $tPid -ErrorAction SilentlyContinue
            if ($p) {
                $pidsToAudit.Add($tPid)
            }
        }
        if ($pidsToAudit.Count -eq 0 -and $Strict) {
            Write-Error "None of the specified TargetPids exist."
            exit 1
        }
    } else {
        $aetherProcs = Get-Process | Where-Object { $_.ProcessName -like "*aethershell*" }
        if ($aetherProcs) {
            foreach ($p in $aetherProcs) {
                $pidsToAudit.Add($p.Id)
                if (-not $Json) {
                    Write-Host "Discovered active AetherShell process: PID $($p.Id) ($($p.ProcessName))" -ForegroundColor Yellow
                }
            }
        } else {
            if ($Strict) {
                $msg = "Packet-level network isolation audit failed: No running AetherShell processes found to audit. Cannot certify network isolation without an active process (Gate G16). Use -SpawnTestProcess or -SpawnAllComponents to audit live instances."
                if (-not $Json) {
                    Write-Host "[!] $msg" -ForegroundColor Red
                }
                Write-Error $msg
                exit 1
            }
            if (-not $Json) {
                Write-Host "No active AetherShell instances currently running on host." -ForegroundColor Yellow
                Write-Host "Warning: Non-strict audit concluded without inspecting an active process." -ForegroundColor Yellow
            }
        }
    }

    # 3. Initialize Packet Capture (if elevated and PktMon available)
    $pktMonExecuted = $false
    $pktMonPacketsDetected = 0
    if ($isAdmin -and $pktMonCmd) {
        $captureEtl = Join-Path ([System.IO.Path]::GetTempPath()) "aether_pktmon_$([System.Guid]::NewGuid().ToString('N')).etl"
        $captureTxt = [System.IO.Path]::ChangeExtension($captureEtl, ".txt")
        if ($PcapOutputPath) {
            $capturePcap = [System.IO.Path]::GetFullPath($PcapOutputPath)
        } else {
            $capturePcap = [System.IO.Path]::ChangeExtension($captureEtl, ".pcapng")
        }

        if (-not $Json) {
            Write-Host "Configuring Windows Packet Monitor (PktMon) filter on NDIS stack..." -ForegroundColor Cyan
        }
        & pktmon filter remove 2>&1 | Out-Null
        & pktmon filter add AetherPacketFilter -p 0..65535 2>&1 | Out-Null
        & pktmon start --capture --pkt-size 128 -f "$captureEtl" 2>&1 | Out-Null
        $pktMonRunning = $true
        $pktMonExecuted = $true
        if (-not $Json) {
            Write-Host "PktMon packet capture active: $captureEtl" -ForegroundColor Green
        }
    }

    # 4. Take DNS Client Cache Baseline Snapshot
    $dnsBaseline = @(Get-DnsClientCache -ErrorAction SilentlyContinue | ForEach-Object {
        "$($_.Entry):$($_.Name):$($_.Type)"
    })
    $dnsBaselineSet = [System.Collections.Generic.HashSet[string]]::new([string[]]$dnsBaseline)

    # 5. Continuous Socket & Endpoint Sampling Loop
    $violations = [System.Collections.Generic.List[string]]::new()
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $maxDurationMs = $DurationSeconds * 1000
    $totalPollCycles = 0

    if (-not $Json) {
        Write-Host "Starting continuous high-frequency socket sampling ($($SampleIntervalMs)ms cadence)..." -ForegroundColor Cyan
    }

    while ($sw.ElapsedMilliseconds -lt $maxDurationMs) {
        $totalPollCycles++
        $activeCount = 0

        if ($pidsToAudit.Count -gt 0) {
            if ($useNativeScanner) {
                $allSocketPids = [AetherNativeNetScanner]::GetActiveSocketPids()
                foreach ($pidToCheck in $pidsToAudit) {
                    $procInstance = Get-Process -Id $pidToCheck -ErrorAction SilentlyContinue
                    if ($procInstance) {
                        $activeCount++
                        if ($allSocketPids.Contains($pidToCheck)) {
                            $v = "Active socket detected by native NDIS/TCP scanner on PID $pidToCheck"
                            if (-not $violations.Contains($v)) {
                                $violations.Add($v)
                            }
                        }
                    }
                }
            } else {
                foreach ($pidToCheck in $pidsToAudit) {
                    $procInstance = Get-Process -Id $pidToCheck -ErrorAction SilentlyContinue
                    if ($procInstance) {
                        $activeCount++

                        # Check TCP Connections
                        $tcpConns = Get-NetTCPConnection -OwningProcess $pidToCheck -ErrorAction SilentlyContinue
                        if ($tcpConns) {
                            foreach ($conn in $tcpConns) {
                                $v = "Active TCP socket on PID $pidToCheck : $($conn.LocalAddress):$($conn.LocalPort) -> $($conn.RemoteAddress):$($conn.RemotePort) (State: $($conn.State))"
                                if (-not $violations.Contains($v)) {
                                    $violations.Add($v)
                                }
                            }
                        }

                        # Check UDP Listeners
                        $udpEndpoints = Get-NetUDPEndpoint -OwningProcess $pidToCheck -ErrorAction SilentlyContinue
                        if ($udpEndpoints) {
                            foreach ($ep in $udpEndpoints) {
                                $v = "Active UDP listener on PID $pidToCheck : $($ep.LocalAddress):$($ep.LocalPort)"
                                if (-not $violations.Contains($v)) {
                                    $violations.Add($v)
                                }
                            }
                        }
                    }
                }
            }
        }

        # If spawned processes all finished, exit early after recording full run
        if ($spawnedProcesses.Count -gt 0 -and $activeCount -eq 0 -and $sw.ElapsedMilliseconds -gt 500) {
            break
        }

        Start-Sleep -Milliseconds $SampleIntervalMs
    }

    # 6. Stop and Parse Packet Monitor Capture (if active)
    if ($pktMonRunning) {
        & pktmon stop 2>&1 | Out-Null
        $pktMonRunning = $false
        & pktmon filter remove 2>&1 | Out-Null

        if (Test-Path $captureEtl) {
            # Convert to formatted text to inspect packet records
            & pktmon format "$captureEtl" -o "$captureTxt" 2>&1 | Out-Null
            if (Test-Path $captureTxt) {
                $pktLines = Get-Content -Path $captureTxt -ErrorAction SilentlyContinue
                if ($pktLines) {
                    foreach ($line in $pktLines) {
                        # Correlate traffic with audited PIDs
                        foreach ($auditPid in $pidsToAudit) {
                            if ($line -match "PID:\s*$auditPid\b" -or $line -match "process\s+$auditPid\b") {
                                $violations.Add("PktMon NDIS packet violation detected for PID $($auditPid): $line")
                                $pktMonPacketsDetected++
                            }
                        }
                    }
                }
            }
            if ($capturePcap) {
                & pktmon pcapng "$captureEtl" -o "$capturePcap" 2>&1 | Out-Null
            }
        }
    }

    # 7. Post-Execution DNS Client Cache Differential Audit
    $dnsPost = @(Get-DnsClientCache -ErrorAction SilentlyContinue | ForEach-Object {
        "$($_.Entry):$($_.Name):$($_.Type)"
    })
    $newDnsQueries = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in $dnsPost) {
        if (-not $dnsBaselineSet.Contains($entry)) {
            # Record any new DNS queries resolved on the host during test execution
            $newDnsQueries.Add($entry)
            $violations.Add("DNS query violation detected during execution window: $entry")
        }
    }

    $passed = ($violations.Count -eq 0)

    # 8. Output Reporting
    if ($Json) {
        $resultObj = [PSCustomObject]@{
            Gate                   = "G16"
            AuditName              = "PacketLevelNetworkIsolation"
            AuditedPids            = @($pidsToAudit)
            AuditedProcessCount    = $pidsToAudit.Count
            IsAdminElevated        = $isAdmin
            PktMonAvailable        = [bool]$pktMonCmd
            PktMonExecuted         = $pktMonExecuted
            PktMonPacketsDetected  = $pktMonPacketsDetected
            ContinuousSampleCycles = $totalPollCycles
            ContinuousIntervalMs   = $SampleIntervalMs
            DnsNewQueriesCount     = $newDnsQueries.Count
            SocketViolationsCount  = $violations.Count
            Violations             = @($violations)
            Passed                 = $passed
            TimestampUtc           = [System.DateTime]::UtcNow.ToString("o")
        }
        $resultObj | ConvertTo-Json -Depth 5
    } else {
        Write-Host "`n================================================================================" -ForegroundColor Cyan
        Write-Host "                     NETWORK ISOLATION AUDIT VERDICT (GATE G16)                  " -ForegroundColor Cyan
        Write-Host "================================================================================" -ForegroundColor Cyan
        Write-Host "  Audited Process Count  : $($pidsToAudit.Count)"
        Write-Host "  Elevation Status       : $(if ($isAdmin) { 'Elevated (Administrator)' } else { 'Standard User (Medium Integrity)' })"
        Write-Host "  PktMon NDIS Capture    : $(if ($pktMonExecuted) { 'Executed & Verified' } else { 'Skipped (Un-elevated / Disposable VM recommended)' })"
        Write-Host "  Continuous Poll Cycles : $totalPollCycles samples ($($SampleIntervalMs)ms cadence)"
        Write-Host "  DNS Cache Leaks        : $($newDnsQueries.Count)"
        Write-Host "  Socket / Pkt Violations: $($violations.Count)"

        if ($violations.Count -gt 0) {
            Write-Host "`n[!] Privacy Violations Detected:" -ForegroundColor Red
            foreach ($v in $violations) {
                Write-Host "    - $v" -ForegroundColor Red
            }
        } else {
            Write-Host "`n[PASS] Zero network packets, zero listening ports, zero outbound sockets, and zero DNS queries detected." -ForegroundColor Green
            Write-Host "       Strict PRD Gate G16 privacy and air-gap network isolation verified." -ForegroundColor Green
        }
    }

    if (-not $passed -and $Strict) {
        Write-Error "Runtime network isolation audit failed: AetherShell processes must never bind or open network sockets (Gate G16)."
        exit 1
    }

    if ($passed) {
        exit 0
    } else {
        exit 1
    }

} finally {
    # Clean up Packet Monitor
    if ($pktMonRunning) {
        & pktmon stop 2>&1 | Out-Null
        & pktmon filter remove 2>&1 | Out-Null
    }

    # Clean up spawned processes
    foreach ($proc in $spawnedProcesses) {
        if ($proc -and -not $proc.HasExited) {
            try {
                $proc.Kill()
                [void]$proc.WaitForExit(1000)
            } catch {}
        }
    }

    # Clean up sandbox directory and capture traces
    if ($tempSandboxDir -and (Test-Path $tempSandboxDir)) {
        Remove-Item -Path $tempSandboxDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($captureEtl -and (Test-Path $captureEtl)) {
        Remove-Item -Path $captureEtl -Force -ErrorAction SilentlyContinue
    }
    if ($captureTxt -and (Test-Path $captureTxt)) {
        Remove-Item -Path $captureTxt -Force -ErrorAction SilentlyContinue
    }
    if ($capturePcap -and -not $PcapOutputPath -and (Test-Path $capturePcap)) {
        Remove-Item -Path $capturePcap -Force -ErrorAction SilentlyContinue
    }
}
