<#
.SYNOPSIS
    AetherShell Disposable VM & Sandbox Orchestrator (PRD Section 18.2, Q01:B, Q02:A, Q37:A, Q48:A)
.DESCRIPTION
    Orchestrates the entire disposable Windows Sandbox & VM qualification workflow:
    1. Host preflight & safety checks (2048 MB RAM, 40 GB disk, CPU < 50%, single-VM concurrency).
    2. Packages qualification bundle (release binaries, runtime DLLs, test scripts).
    3. Generates the isolated Windows Sandbox profile (.wsb) with air-gapped network containment.
    4. Supports non-disruptive host preflight (-PreflightOnly, -DryRun) and in-guest execution.
.PARAMETER PreflightOnly
    Verifies host hardware, resource envelopes, and AppData cleanliness without staging files.
.PARAMETER DryRun
    Stages artifacts, validates .wsb XML, and runs non-disruptive tests (-SkipExplorerKill) on host.
.PARAMETER OutputDir
    Directory for qualification artifacts. Defaults to target\qualification_artifacts.
.PARAMETER Strict
    Fail immediately with non-zero exit code if any step fails.
#>

[CmdletBinding()]
param(
    [switch]$PreflightOnly = $false,
    [switch]$DryRun = $false,
    [string]$OutputDir = "",
    [switch]$Strict = $false
)

$ErrorActionPreference = "Stop"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "     AETHERSHELL DISPOSABLE VM QUALIFICATION ORCHESTRATOR (Q02:A, Q48:A)        " -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $workspaceRoot "target\qualification_artifacts"
}
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

# 1. Run Host Resource Preflight (PRD Q01:B / Q48:A)
Write-Host "[1/4] Running Disposable VM Resource Preflight..." -ForegroundColor Yellow
$provisionScript = Join-Path $workspaceRoot "scripts\provision_disposable_vm.ps1"
if (-not (Test-Path $provisionScript)) {
    throw "Provisioning script not found at $provisionScript"
}

$preflightOut = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$provisionScript' -Mode PreflightOnly 2>&1"
$preflightExit = $LASTEXITCODE

if ($preflightExit -ne 0) {
    Write-Error "Preflight failed with exit code ${preflightExit}: $($preflightOut | Out-String)"
    exit $preflightExit
}
Write-Host "  Resource preflight: PASSED (RAM >= 2048 MB, Disk >= 40 GB, CPU < 50%, AppData clean)" -ForegroundColor Green

if ($PreflightOnly) {
    Write-Host "`nPreflight-only mode requested. Exiting cleanly." -ForegroundColor Green
    exit 0
}

# 2. Stage Guest Qualification Bundle
Write-Host "`n[2/4] Staging Guest Qualification Bundle..." -ForegroundColor Yellow
$bundleDir = Join-Path $OutputDir "guest_bundle"
if (-not (Test-Path $bundleDir)) {
    New-Item -ItemType Directory -Path $bundleDir -Force | Out-Null
}

# Locate binaries
$binSource = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release"
if (-not (Test-Path $binSource)) {
    $binSource = Join-Path $workspaceRoot "target\release"
}

$coreExe = Join-Path $binSource "aethershell.exe"
if (-not (Test-Path $coreExe)) {
    throw "Release binaries not found in $binSource. Build release binaries first."
}

# Copy executables to bundle
$binTarget = Join-Path $bundleDir "bin"
if (-not (Test-Path $binTarget)) {
    New-Item -ItemType Directory -Path $binTarget -Force | Out-Null
}

Copy-Item -LiteralPath $coreExe -Destination $binTarget -Force
Write-Host "  Staged aethershell.exe -> $binTarget"

$scriptHostExe = Join-Path $binSource "aethershell-script-host.exe"
if (Test-Path $scriptHostExe) {
    Copy-Item -LiteralPath $scriptHostExe -Destination $binTarget -Force
    Write-Host "  Staged aethershell-script-host.exe -> $binTarget"
}

$settingsExe = Join-Path $binSource "aethershell-settings.exe"
if (Test-Path $settingsExe) {
    Copy-Item -LiteralPath $settingsExe -Destination $binTarget -Force
    Write-Host "  Staged aethershell-settings.exe -> $binTarget"
}

# Copy guest harness script
$guestHarness = Join-Path $workspaceRoot "scripts\guest_sandbox_qualification.ps1"
Copy-Item -LiteralPath $guestHarness -Destination $bundleDir -Force
Write-Host "  Staged guest_sandbox_qualification.ps1 -> $bundleDir"

# Copy license and notice files
Copy-Item -LiteralPath (Join-Path $workspaceRoot "LICENSE") -Destination $bundleDir -Force
$noticePath = Join-Path $workspaceRoot "NOTICE.md"
if (-not (Test-Path $noticePath)) {
    $noticePath = Join-Path $workspaceRoot "NOTICE"
}
if (Test-Path $noticePath) {
    Copy-Item -LiteralPath $noticePath -Destination $bundleDir -Force
}

# 3. Generate Windows Sandbox Profile (.wsb)
Write-Host "`n[3/4] Generating Windows Sandbox Configuration (.wsb)..." -ForegroundColor Yellow
$wsbFile = Join-Path $OutputDir "AetherShell-Disruptive-Qualification.wsb"

$wsbXml = @"
<Configuration>
  <VGpu>Disable</VGpu>
  <Networking>Disable</Networking>
  <MemoryInMB>2048</MemoryInMB>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$OutputDir</HostFolder>
      <SandboxFolder>C:\AetherShellQualification</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -ExecutionPolicy Bypass -NoProfile -Command "Set-Location C:\AetherShellQualification\guest_bundle; .\guest_sandbox_qualification.ps1 -ArtifactsDir C:\AetherShellQualification -Strict"</Command>
  </LogonCommand>
</Configuration>
"@

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($wsbFile, $wsbXml, $utf8NoBom)
Write-Host "  Generated Windows Sandbox Profile: $wsbFile" -ForegroundColor Green
Write-Host "  Allocation: 2048 MB RAM (Q48:A), Network: Disabled (Airgap)"

# 4. Execute Qualification Run
Write-Host "`n[4/4] Executing Qualification Flow..." -ForegroundColor Yellow
if ($DryRun) {
    Write-Host "  [DryRun] Executing non-disruptive host validation suite (-SkipExplorerKill)..." -ForegroundColor Cyan
    $ledgerJson = Join-Path $OutputDir "host_validation_ledger.json"
    $guestRunOut = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$guestHarness' -ArtifactsDir '$OutputDir' -OutputLedgerJson '$ledgerJson' -SkipExplorerKill $(if ($Strict) { '-Strict' }) 2>&1"
    $guestRunExit = $LASTEXITCODE

    Write-Host ($guestRunOut | Out-String)

    if ($guestRunExit -ne 0) {
        Write-Error "Host validation failed with exit code $guestRunExit"
        exit $guestRunExit
    }
    Write-Host "  [DryRun] Qualification flow validated cleanly without host disruption." -ForegroundColor Green
} else {
    Write-Host "  Windows Sandbox profile generated and ready for launch:" -ForegroundColor Cyan
    Write-Host "    Start-Process `"$wsbFile`"" -ForegroundColor White
}

# Verify Host AppData Cleanliness
$localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
$prodDir = Join-Path $localAppData "AetherShell"
if (Test-Path $prodDir) {
    $count = (Get-ChildItem -Path $prodDir -Recurse -Force -ErrorAction SilentlyContinue).Count
    if ($count -ne 0) {
        Write-Warning "Host AppData contains $count items. Violates test isolation!"
    } else {
        Write-Host "`nHost %LOCALAPPDATA%\AetherShell verified clean (0 items)." -ForegroundColor Green
    }
} else {
    Write-Host "`nHost %LOCALAPPDATA%\AetherShell verified clean (directory not present)." -ForegroundColor Green
}

Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host "ORCHESTRATOR COMPLETE: Profile generated at $wsbFile" -ForegroundColor Green
Write-Host "================================================================================" -ForegroundColor Cyan
exit 0
