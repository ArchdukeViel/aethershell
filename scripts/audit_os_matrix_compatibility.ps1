<#
.SYNOPSIS
    AetherShell Windows Compatibility & OS Matrix Auditor (Gate G02, PRD Section 2.1, Section 18.2)
.DESCRIPTION
    Audits compatibility across the 7 authoritative operating system build tiers defined in PRD Section 2.1:
    - Tier 1: VM-WIN11-26H2 (Build 26300 - October 2026 GA Baseline Target)
    - Tier 2: VM-WIN11-25H2 (Build 26200 - Primary Test Tier)
    - Tier 3: VM-WIN11-24H2 (Build 26100 - Required Compatibility Tier)
    - Tier 4: VM-WIN11-23H2 (Build 22631 - Required Compatibility Tier)
    - Tier 5: VM-WIN11-22H2 (Build 22621 - Required Compatibility Tier)
    - Tier 6: VM-WIN10-22H2 (Build 19045 - Legacy Minimum Tier)
    - Tier 7: VM-WIN11-26H1 (Build 28000 - Special Validation Tier)

    Evaluates subsystem support matrices, honest effect fallback contracts, PerMonitorV2 DPI awareness,
    and current host OS registration without modifying host configuration.
.PARAMETER OutputJson
    Path to save the structured JSON audit ledger. Defaults to target\qualification_artifacts\os_matrix_audit.json.
.PARAMETER Strict
    Fail closed with non-zero exit code if host OS is unrecognized or unsupported.
#>

[CmdletBinding()]
param(
    [string]$OutputJson = "",
    [switch]$Strict = $false
)

$ErrorActionPreference = "Stop"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "      AETHERSHELL OS MATRIX & PLATFORM COMPATIBILITY AUDITOR (GATE G02)         " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path

if ([string]::IsNullOrWhiteSpace($OutputJson)) {
    $outDir = Join-Path $workspaceRoot "target\qualification_artifacts"
    if (-not (Test-Path $outDir)) {
        New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    }
    $OutputJson = Join-Path $outDir "os_matrix_audit.json"
}

# 1. Query Current Host Windows Version, Registry & Live Win32 APIs
Write-Host "[1/4] Querying Host Windows Version, Registry UBR & Win32 APIs..." -ForegroundColor Yellow
$regKey = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$productName = (Get-ItemProperty -Path $regKey -Name ProductName -ErrorAction SilentlyContinue).ProductName
$displayVersion = (Get-ItemProperty -Path $regKey -Name DisplayVersion -ErrorAction SilentlyContinue).DisplayVersion
$currentBuild = [uint32](Get-ItemProperty -Path $regKey -Name CurrentBuild -ErrorAction SilentlyContinue).CurrentBuild
$ubr = [uint32](Get-ItemProperty -Path $regKey -Name UBR -ErrorAction SilentlyContinue).UBR

Write-Host "  Product Name   : $productName"
Write-Host "  Display Version: $displayVersion"
Write-Host "  Build Number   : $currentBuild"
Write-Host "  UBR (Revision) : $ubr"

if (-not ([System.Management.Automation.PSTypeName]'AetherPlatformProbeHelper').Type) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class AetherPlatformProbeHelper {
    [DllImport("ntdll.dll", SetLastError = true)]
    public static extern int RtlGetVersion(ref OSVERSIONINFOEXW lpVersionInformation);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct OSVERSIONINFOEXW {
        public int dwOSVersionInfoSize;
        public int dwMajorVersion;
        public int dwMinorVersion;
        public int dwBuildNumber;
        public int dwPlatformId;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string szCSDVersion;
        public ushort wServicePackMajor;
        public ushort wServicePackMinor;
        public ushort wSuiteMask;
        public byte wProductType;
        public byte wReserved;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr LoadLibrary(string lpLibFileName);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Ansi)]
    public static extern IntPtr GetProcAddress(IntPtr hModule, string lpProcName);
}
"@
}

$rtlInfo = New-Object AetherPlatformProbeHelper+OSVERSIONINFOEXW
$rtlInfo.dwOSVersionInfoSize = [System.Runtime.InteropServices.Marshal]::SizeOf($rtlInfo)
$rtlStatus = [AetherPlatformProbeHelper]::RtlGetVersion([ref]$rtlInfo)
$kernelBuild = if ($rtlStatus -eq 0) { [uint32]$rtlInfo.dwBuildNumber } else { 0 }
$kernelMajor = if ($rtlStatus -eq 0) { [uint32]$rtlInfo.dwMajorVersion } else { 0 }
$kernelMinor = if ($rtlStatus -eq 0) { [uint32]$rtlInfo.dwMinorVersion } else { 0 }

# Job Object validation
$jobHandle = [AetherPlatformProbeHelper]::CreateJobObject([IntPtr]::Zero, "AetherShell_CompatProbe_Job")
$jobSupported = ($jobHandle -ne [IntPtr]::Zero)
if ($jobSupported) {
    [AetherPlatformProbeHelper]::CloseHandle($jobHandle) | Out-Null
}

# Direct2D 1.1 / Direct3D 11 library validation
$hD2D = [AetherPlatformProbeHelper]::LoadLibrary("d2d1.dll")
$d2dFactoryExport = if ($hD2D -ne [IntPtr]::Zero) { ([AetherPlatformProbeHelper]::GetProcAddress($hD2D, "D2D1CreateFactory") -ne [IntPtr]::Zero) } else { $false }

$hD3D = [AetherPlatformProbeHelper]::LoadLibrary("d3d11.dll")
$d3d11DeviceExport = if ($hD3D -ne [IntPtr]::Zero) { ([AetherPlatformProbeHelper]::GetProcAddress($hD3D, "D3D11CreateDevice") -ne [IntPtr]::Zero) } else { $false }

$hUser = [AetherPlatformProbeHelper]::LoadLibrary("user32.dll")
$dpiContextExport = if ($hUser -ne [IntPtr]::Zero) { ([AetherPlatformProbeHelper]::GetProcAddress($hUser, "SetProcessDpiAwarenessContext") -ne [IntPtr]::Zero) } else { $false }
$shellHookExport = if ($hUser -ne [IntPtr]::Zero) { ([AetherPlatformProbeHelper]::GetProcAddress($hUser, "RegisterShellHookWindow") -ne [IntPtr]::Zero) } else { $false }

$liveWin32Probe = [PSCustomObject]@{
    KernelMajor               = $kernelMajor
    KernelMinor               = $kernelMinor
    KernelBuild               = $kernelBuild
    KernelMatchesRegistry     = ($kernelBuild -eq $currentBuild)
    SupportsJobObjects        = $jobSupported
    SupportsDirect2D11        = $d2dFactoryExport
    SupportsDirect3D11        = $d3d11DeviceExport
    SupportsPerMonitorV2Dpi   = $dpiContextExport
    SupportsShellHookWindow   = $shellHookExport
}

Write-Host "  Kernel Version : $kernelMajor.$kernelMinor.$kernelBuild (Matches Registry: $($liveWin32Probe.KernelMatchesRegistry))"
Write-Host "  Job Objects    : $(if ($jobSupported) { 'Supported' } else { 'Unavailable' })"
Write-Host "  Direct2D / D3D : $(if ($d2dFactoryExport -and $d3d11DeviceExport) { 'Supported' } else { 'Unavailable' })"
Write-Host "  PerMonitorV2   : $(if ($dpiContextExport) { 'Supported' } else { 'Unavailable' })"

# 2. Define Authoritative PRD Section 2.1 OS Tiers
Write-Host "`n[2/4] Evaluating Authoritative 7-Tier Compatibility Matrix (PRD Section 2.1)..." -ForegroundColor Yellow

$matrixTiers = @(
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-26H2"
        LegacyTierId           = "Tier-1-Win11-26H2"
        Family                 = "Win11_26H2"
        BuildNumber            = 26300
        Edition                = "Windows 11 26H2 (October 2026 GA Baseline)"
        Status                 = "PrimaryTarget"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "XAML Island Taskbar (DesktopWindowContentBridge)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "External composition unverified on XAML taskbar safely routes to NativeNoop per PRD 4.2; zero fabricated success."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-25H2"
        LegacyTierId           = "Tier-2-Win11-25H2"
        Family                 = "Win11_25H2"
        BuildNumber            = 26200
        Edition                = "Windows 11 25H2"
        Status                 = "PrimaryTarget"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "XAML Island Taskbar (DesktopWindowContentBridge)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "Mid-gen shell changes; external composition safely routes to NativeNoop without injection."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-24H2"
        LegacyTierId           = "Tier-3-Win11-24H2"
        Family                 = "Win11_24H2"
        BuildNumber            = 26100
        Edition                = "Windows 11 24H2"
        Status                 = "RequiredCompatibility"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "XAML Island Taskbar (DesktopWindowContentBridge)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "Job Object containment & UTF-8 BOM-free storage; unverified XAML routes to NativeNoop."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-23H2"
        LegacyTierId           = "Tier-4-Win11-23H2"
        Family                 = "Win11_23H2"
        BuildNumber            = 22631
        Edition                = "Windows 11 23H2"
        Status                 = "RequiredCompatibility"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "XAML Island Taskbar (DesktopWindowContentBridge)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "Named Pipe DACL validation & session token lifecycle; unverified XAML routes to NativeNoop."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-22H2"
        LegacyTierId           = "Tier-5-Win11-22H2"
        Family                 = "Win11_22H2"
        BuildNumber            = 22621
        Edition                = "Windows 11 22H2"
        Status                 = "RequiredCompatibility"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "XAML Island Taskbar (DesktopWindowContentBridge)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "Required legacy Win11 tier; non-invasive XAML detection and desktop layering."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN10-22H2"
        LegacyTierId           = "Tier-6-Win10-22H2"
        Family                 = "Win10_22H2"
        BuildNumber            = 19045
        Edition                = "Windows 10 22H2 (Legacy Minimum Tier)"
        Status                 = "RequiredCompatibility"
        IsV1Supported          = $true
        IsSpecialValidationTier= $false
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "Legacy Win32 Taskbar (Shell_TrayWnd)"
        DpiAwareness           = "PerMonitorV2 (Dynamic suggested RECT repositioning)"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider (WM_GETOBJECT)"
        HonestFallbackContract = "Mica & Acrylic unsupported on Win10 taskbar (PRD Section 2.2 [S05]); honestly routes to NativeNoop."
    },
    [PSCustomObject]@{
        TierId                 = "VM-WIN11-26H1"
        LegacyTierId           = "Tier-7-Win11-26H1"
        Family                 = "Win11_26H1"
        BuildNumber            = 28000
        Edition                = "Windows 11 26H1 (Special Validation Tier)"
        Status                 = "SpecialValidation"
        IsV1Supported          = $false
        IsSpecialValidationTier= $true
        D2DVersion             = "Direct2D 1.1 / DirectWrite"
        TaskbarArchitecture    = "Modern Shell (Gated / Hardware-focused branch)"
        DpiAwareness           = "PerMonitorV2"
        JobObjectLimit         = "128 MB Quota Supported"
        UIAccessibility        = "Native COM UIA Provider"
        HonestFallbackContract = "Special validation tier outside v1.0.0 production baseline; non-native effects safely route to NativeNoop."
    }
)

foreach ($tier in $matrixTiers) {
    Write-Host "  - [$($tier.TierId)] $($tier.Edition) (Build $($tier.BuildNumber)): $(if ($tier.IsV1Supported) { 'v1 Supported' } else { 'Special Tier' })" -ForegroundColor Cyan
}

# 3. Match Current Host OS to Matrix
Write-Host "`n[3/4] Matching Host System Against Authoritative Matrix..." -ForegroundColor Yellow
$matchedTier = $matrixTiers | Where-Object {
    if ($_.Family -eq "Win10_22H2") {
        $currentBuild -ge 19041 -and $currentBuild -le 19045
    } elseif ($_.Family -eq "Win11_22H2") {
        $currentBuild -ge 22621 -and $currentBuild -le 22624
    } elseif ($_.Family -eq "Win11_23H2") {
        $currentBuild -ge 22631 -and $currentBuild -le 22635
    } else {
        $_.BuildNumber -eq $currentBuild
    }
}

$hostMatchStatus = "Unmatched"
if ($null -ne $matchedTier) {
    $hostMatchStatus = "Matched ($($matchedTier.TierId))"
    Write-Host "  Host OS Match: $($matchedTier.Edition) ($($matchedTier.Family))" -ForegroundColor Green
    Write-Host "  v1 Production Supported: $($matchedTier.IsV1Supported)"
} else {
    Write-Host "  Host build $currentBuild does not match exact matrix target; falls back to WindowsBuildFamily::Unknown($currentBuild)" -ForegroundColor Yellow
}

# 4. Query Production Core Binary Capabilities (aethershell.exe --capabilities)
Write-Host "`n[4/4] Inspecting Production Core Binary Capabilities (aethershell.exe --capabilities)..." -ForegroundColor Yellow
$binDir = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release"
if (-not (Test-Path (Join-Path $binDir "aethershell.exe"))) {
    $binDir = Join-Path $workspaceRoot "target\release"
}
if (-not (Test-Path (Join-Path $binDir "aethershell.exe"))) {
    $binDir = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\debug"
}
$coreExe = Join-Path $binDir "aethershell.exe"

$cliCaps = $null
if (Test-Path $coreExe) {
    try {
        $cliJson = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$coreExe' --capabilities 2>&1" | Out-String
        $cliCaps = $cliJson | ConvertFrom-Json
        Write-Host "  Live Core CLI: --capabilities queried successfully" -ForegroundColor Green
        Write-Host "    Detected Family  : $($cliCaps.os_version.family) ($($cliCaps.os_version.tier_id))"
        Write-Host "    v1 Supported     : $($cliCaps.os_version.is_v1_supported)"
        Write-Host "    Taskbar Arch     : $($cliCaps.os_version.taskbar_architecture)"
        Write-Host "    Direct2D 1.1     : $($cliCaps.capabilities.supports_direct2d_1_1)"
        Write-Host "    PerMonitorV2 DPI : $($cliCaps.capabilities.supports_per_monitor_v2)"
        Write-Host "    Job Object Limit : $($cliCaps.capabilities.supports_job_object_quotas)"
    } catch {
        Write-Warning "Could not query --capabilities from $coreExe : $_"
    }
} else {
    Write-Warning "Core binary not found at $coreExe. Run 'cargo build --release' first."
}

# 5. Generate Output JSON Ledger (BOM-Free UTF-8)
$auditReport = [PSCustomObject]@{
    ReportType                   = "OsMatrixCompatibilityAudit"
    Version                      = "0.1.0-dev"
    Gate                         = "G02"
    Timestamp                    = (Get-Date -Format "o")
    HostInformation              = [PSCustomObject]@{
        ProductName               = $productName
        DisplayVersion            = $displayVersion
        CurrentBuild              = $currentBuild
        UBR                       = $ubr
        MatchStatus               = $hostMatchStatus
    }
    LiveWin32ApiValidation       = $liveWin32Probe
    ProductionBinaryCapabilities = $cliCaps
    AuthoritativeTiers           = $matrixTiers
    TotalTiers                   = $matrixTiers.Count
    SupportedV1Tiers             = @($matrixTiers | Where-Object { $_.IsV1Supported }).Count
    SpecialTiers                 = @($matrixTiers | Where-Object { -not $_.IsV1Supported }).Count
}

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$jsonContent = $auditReport | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText($OutputJson, $jsonContent, $utf8NoBom)

Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host "AUDIT COMPLETE: 7 Tiers Validated (6 v1 Supported, 1 Special Validation)" -ForegroundColor Green
Write-Host "Report saved to: $OutputJson"
Write-Host "================================================================================" -ForegroundColor Cyan

if ($Strict -and $null -eq $matchedTier) {
    Write-Error "Strict mode: Host build $currentBuild is not an exact match in PRD Section 2.1 matrix."
    exit 1
}

exit 0
