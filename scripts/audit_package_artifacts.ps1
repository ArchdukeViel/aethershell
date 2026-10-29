<#
.SYNOPSIS
    AetherShell Package Artifact & Shell Isolation Auditor (Gate G03, G16, G20)
.DESCRIPTION
    Audits packaged distribution artifacts (directories or ZIP archives) to verify:
    1. Gate G03 Shell Isolation: Exactly zero in-process Explorer extensions, zero BHOs, zero deskbands,
       zero COM in-process server exports, zero detour libraries, and zero hooks into protected processes.
    2. Gate G16 License & Privacy: Presence of Apache-2.0 LICENSE, NOTICE.md, and verified path remapping (no developer paths in binaries).
    3. Gate G20 Packaging Identity: Adherence to PRD Q38:B (zero portable marker files overriding canonical %LOCALAPPDATA%\AetherShell).
    4. Computes cryptographic SHA-256 manifest across all packaged binaries with deep PE header, import, and export analysis.
#>

[CmdletBinding()]
param(
    [string]$PackageDir,
    [string]$ZipArchive,
    [switch]$Strict,
    [switch]$Json,
    [string]$OutputManifestPath,
    [switch]$ScanRunningProcesses
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditErrors = [System.Collections.Generic.List[string]]::new()
$auditWarnings = [System.Collections.Generic.List[string]]::new()
$binaryManifest = [System.Collections.Generic.List[object]]::new()

# Resolve target directory
$targetDir = $null
$isTempExtracted = $false

if (-not [string]::IsNullOrWhiteSpace($ZipArchive)) {
    if (-not (Test-Path $ZipArchive)) {
        throw "Specified ZIP archive does not exist: $ZipArchive"
    }
    $targetDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aethershell_pkg_audit_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Expand-Archive -Path $ZipArchive -DestinationPath $targetDir -Force
    $isTempExtracted = $true
} elseif (-not [string]::IsNullOrWhiteSpace($PackageDir)) {
    if (-not (Test-Path $PackageDir)) {
        throw "Specified package directory does not exist: $PackageDir"
    }
    $targetDir = (Resolve-Path $PackageDir).Path
} else {
    # Default to portable staging or release directory
    $candidateDirs = @(
        "$workspaceRoot\packaging\portable\stage",
        "$workspaceRoot\packaging\wix\stage",
        "$workspaceRoot\target\dist\portable",
        "$workspaceRoot\target\x86_64-pc-windows-msvc\release",
        "$workspaceRoot\target\release"
    )
    foreach ($cand in $candidateDirs) {
        if (Test-Path $cand) {
            $targetDir = $cand
            break
        }
    }
    if ($null -eq $targetDir) {
        $targetDir = "$workspaceRoot\target\release"
    }
}

# Register C# PE Inspector if not already registered
if (-not ([System.Management.Automation.PSTypeName]'AetherPeInspectorV3').Type) {
    Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Text;
using System.Collections.Generic;

public class AetherPeSection {
    public string Name;
    public uint VirtualSize;
    public uint VirtualAddress;
    public uint SizeOfRawData;
    public uint PointerToRawData;
}

public class AetherPeAnalysisResult {
    public bool IsPe;
    public string Architecture = "Unknown";
    public bool IsDll;
    public List<string> ExportedFunctions = new List<string>();
    public List<string> ImportedDlls = new List<string>();
    public List<string> ImportedFunctions = new List<string>();
    public List<string> ProhibitedImports = new List<string>();
    public List<string> ProhibitedExports = new List<string>();
    public List<string> Violations = new List<string>();
}

public static class AetherPeInspectorV3 {
    private static readonly HashSet<string> ForbiddenExportNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
        "DllRegisterServer",
        "DllUnregisterServer",
        "DllInstall"
    };

    private static readonly HashSet<string> ForbiddenDlls = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
        "detours.dll",
        "minhook.dll",
        "minhook.x64.dll",
        "minhook.x86.dll",
        "easyhook.dll",
        "easyhook64.dll",
        "easyhook32.dll",
        "mhook.dll",
        "subhook.dll",
        "polyhook.dll",
        "plthook.dll"
    };

    private static readonly HashSet<string> ForbiddenApis = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
        "CreateRemoteThread",
        "CreateRemoteThreadEx",
        "NtCreateThreadEx",
        "RtlCreateUserThread",
        "SetWindowsHookA",
        "SetWindowsHookW",
        "SetWindowsHookExA",
        "SetWindowsHookExW",
        "QueueUserAPC",
        "QueueUserAPC2",
        "NtQueueApcThread",
        "ZwQueueApcThread"
    };

    private static int RvaToOffset(uint rva, AetherPeSection[] sections) {
        if (rva == 0 || sections == null) return 0;
        for (int i = 0; i < sections.Length; i++) {
            if (sections[i] == null) continue;
            uint vStart = sections[i].VirtualAddress;
            uint vSize = Math.Max(sections[i].VirtualSize, sections[i].SizeOfRawData);
            if (rva >= vStart && rva < vStart + vSize) {
                return (int)((rva - vStart) + sections[i].PointerToRawData);
            }
        }
        return 0;
    }

    private static string ReadAsciiString(byte[] data, int offset) {
        if (offset <= 0 || offset >= data.Length) return string.Empty;
        int end = offset;
        while (end < data.Length && data[end] != 0) {
            end++;
        }
        return Encoding.ASCII.GetString(data, offset, end - offset);
    }

    public static AetherPeAnalysisResult AnalyzePeBytes(byte[] data) {
        var res = new AetherPeAnalysisResult();
        if (data == null || data.Length < 64) return res;

        if (data[0] != 0x4D || data[1] != 0x5A) return res; // MZ
        int peOffset = BitConverter.ToInt32(data, 0x3C);
        if (peOffset <= 0 || peOffset + 248 > data.Length) return res;

        uint peSig = BitConverter.ToUInt32(data, peOffset);
        if (peSig != 0x00004550) return res; // PE\0\0

        res.IsPe = true;
        ushort machine = BitConverter.ToUInt16(data, peOffset + 4);
        ushort numSections = BitConverter.ToUInt16(data, peOffset + 6);
        ushort characteristics = BitConverter.ToUInt16(data, peOffset + 22);
        res.IsDll = (characteristics & 0x2000) != 0;

        int optHeaderOffset = peOffset + 24;
        ushort optMagic = BitConverter.ToUInt16(data, optHeaderOffset);
        bool is64 = (optMagic == 0x20B);

        if (machine == 0x8664) res.Architecture = "x64";
        else if (machine == 0x014C) res.Architecture = "x86";
        else if (machine == 0xAA64) res.Architecture = "ARM64";
        else res.Architecture = "0x" + machine.ToString("X4");

        int sizeOfOptHeader = BitConverter.ToUInt16(data, peOffset + 20);
        int sectionsOffset = optHeaderOffset + sizeOfOptHeader;

        var sections = new AetherPeSection[numSections];
        for (int i = 0; i < numSections; i++) {
            int sOff = sectionsOffset + i * 40;
            if (sOff + 40 > data.Length) break;
            string sName = Encoding.ASCII.GetString(data, sOff, 8).TrimEnd('\0', ' ');
            uint vSize = BitConverter.ToUInt32(data, sOff + 8);
            uint vAddr = BitConverter.ToUInt32(data, sOff + 12);
            uint rawSize = BitConverter.ToUInt32(data, sOff + 16);
            uint rawPtr = BitConverter.ToUInt32(data, sOff + 20);
            sections[i] = new AetherPeSection {
                Name = sName,
                VirtualSize = vSize,
                VirtualAddress = vAddr,
                SizeOfRawData = rawSize,
                PointerToRawData = rawPtr
            };
        }

        uint exportRva = 0;
        uint exportSize = 0;
        uint importRva = 0;
        if (is64) {
            if (optHeaderOffset + 128 <= data.Length) {
                exportRva = BitConverter.ToUInt32(data, optHeaderOffset + 112);
                exportSize = BitConverter.ToUInt32(data, optHeaderOffset + 116);
                importRva = BitConverter.ToUInt32(data, optHeaderOffset + 120);
            }
        } else {
            if (optHeaderOffset + 112 <= data.Length) {
                exportRva = BitConverter.ToUInt32(data, optHeaderOffset + 96);
                exportSize = BitConverter.ToUInt32(data, optHeaderOffset + 100);
                importRva = BitConverter.ToUInt32(data, optHeaderOffset + 104);
            }
        }

        // Parse Exports
        if (exportRva != 0 && exportSize > 0) {
            int expFileOffset = RvaToOffset(exportRva, sections);
            if (expFileOffset > 0 && expFileOffset + 40 <= data.Length) {
                uint numNames = BitConverter.ToUInt32(data, expFileOffset + 24);
                uint namesRva = BitConverter.ToUInt32(data, expFileOffset + 32);
                int namesOffset = RvaToOffset(namesRva, sections);

                for (int i = 0; i < numNames && i < 4096; i++) {
                    if (namesOffset + i * 4 + 4 > data.Length) break;
                    uint fnNameRva = BitConverter.ToUInt32(data, namesOffset + i * 4);
                    string fnName = ReadAsciiString(data, RvaToOffset(fnNameRva, sections));
                    if (!string.IsNullOrEmpty(fnName)) {
                        res.ExportedFunctions.Add(fnName);
                        if (ForbiddenExportNames.Contains(fnName)) {
                            res.ProhibitedExports.Add(fnName);
                            res.Violations.Add("Prohibited COM In-Process Server export: '" + fnName + "'");
                        }
                    }
                }
            }
        }

        // Parse Imports
        if (importRva != 0) {
            int impFileOffset = RvaToOffset(importRva, sections);
            while (impFileOffset > 0 && impFileOffset + 20 <= data.Length) {
                uint origFirstThunk = BitConverter.ToUInt32(data, impFileOffset);
                uint nameRva = BitConverter.ToUInt32(data, impFileOffset + 12);
                uint firstThunk = BitConverter.ToUInt32(data, impFileOffset + 16);

                if (origFirstThunk == 0 && nameRva == 0 && firstThunk == 0) {
                    break; // End of import descriptors
                }

                string dllName = ReadAsciiString(data, RvaToOffset(nameRva, sections));
                if (!string.IsNullOrEmpty(dllName)) {
                    res.ImportedDlls.Add(dllName);
                    if (ForbiddenDlls.Contains(dllName)) {
                        res.ProhibitedImports.Add(dllName);
                        res.Violations.Add("Prohibited detour/injection library import: '" + dllName + "'");
                    }

                    // Enumerate thunks
                    uint thunkRva = origFirstThunk != 0 ? origFirstThunk : firstThunk;
                    int thunkOffset = RvaToOffset(thunkRva, sections);

                    while (thunkOffset > 0) {
                        if (is64) {
                            if (thunkOffset + 8 > data.Length) break;
                            ulong val = BitConverter.ToUInt64(data, thunkOffset);
                            if (val == 0) break;
                            thunkOffset += 8;
                            if ((val & 0x8000000000000000UL) == 0) {
                                uint hintNameRva = (uint)(val & 0xFFFFFFFF);
                                int fnOffset = RvaToOffset(hintNameRva, sections) + 2; // skip Hint
                                string fnName = ReadAsciiString(data, fnOffset);
                                if (!string.IsNullOrEmpty(fnName)) {
                                    res.ImportedFunctions.Add(fnName);
                                    if (ForbiddenApis.Contains(fnName)) {
                                        res.ProhibitedImports.Add(dllName + "!" + fnName);
                                        res.Violations.Add("Prohibited injection API import: '" + dllName + "!" + fnName + "'");
                                    }
                                }
                            }
                        } else {
                            if (thunkOffset + 4 > data.Length) break;
                            uint val = BitConverter.ToUInt32(data, thunkOffset);
                            if (val == 0) break;
                            thunkOffset += 4;
                            if ((val & 0x80000000) == 0) {
                                int fnOffset = RvaToOffset(val, sections) + 2;
                                string fnName = ReadAsciiString(data, fnOffset);
                                if (!string.IsNullOrEmpty(fnName)) {
                                    res.ImportedFunctions.Add(fnName);
                                    if (ForbiddenApis.Contains(fnName)) {
                                        res.ProhibitedImports.Add(dllName + "!" + fnName);
                                        res.Violations.Add("Prohibited injection API import: '" + dllName + "!" + fnName + "'");
                                    }
                                }
                            }
                        }
                    }
                }
                impFileOffset += 20;
            }
        }

        return res;
    }
}
"@
}

try {
    Write-Host "=== AetherShell Package Artifact & Shell Isolation Audit ===" -ForegroundColor Cyan
    Write-Host "Audit Target Directory: $targetDir" -ForegroundColor Gray

    # 1. Audit mandatory documentation and schema files (Gate G16)
    $hasLicense = (Test-Path (Join-Path $targetDir "LICENSE")) -or (Test-Path (Join-Path $targetDir "LICENSE.txt"))
    if (-not $hasLicense) {
        $auditErrors.Add("Gate G16 Violation: Missing Apache-2.0 LICENSE file in package root.")
    }

    $hasNotice = (Test-Path (Join-Path $targetDir "NOTICE.md")) -or (Test-Path (Join-Path $targetDir "NOTICE"))
    if (-not $hasNotice) {
        $auditWarnings.Add("Gate G16 Warning: NOTICE.md not found in package root.")
    }

    # 2. Audit PRD Q38:B Portable Marker Violation (Gate G20)
    $portableMarker = Join-Path $targetDir "aethershell.portable"
    if (Test-Path $portableMarker) {
        $auditErrors.Add("Gate G20 Violation: Prohibited 'aethershell.portable' marker found. PRD Q38:B mandates all modes share canonical AppData (%LOCALAPPDATA%\AetherShell).")
    }

    $localDataDir = Join-Path $targetDir "data"
    if (Test-Path $localDataDir) {
        $auditWarnings.Add("Gate G20 Warning: Local 'data' folder found in package layout. All runtime configuration must reside in canonical AppData.")
    }

    # 3. Discover and inspect all binary files (.exe, .dll)
    $binFiles = Get-ChildItem -Path $targetDir -Recurse -Include @("*.exe", "*.dll") -File

    if ($binFiles.Count -eq 0) {
        $msg = "No executable or DLL binaries found in audit target directory ($targetDir)."
        if ($Strict) {
            $auditErrors.Add($msg)
        } else {
            $auditWarnings.Add($msg)
        }
    }

    $forbiddenStrings = @(
        "IShellIconOverlayIdentifier",
        "IContextMenu",
        "IContextMenu2",
        "IContextMenu3",
        "IExplorerCommand",
        "IColumnProvider",
        "IShellExecuteHook",
        "IShellFolder",
        "IDeskBand",
        "IDeskBand2",
        "IBrowserHelperObject",
        "DllRegisterServer",
        "CreateRemoteThread",
        "SetWindowsHookExW",
        "detours.dll",
        "minhook.x64.dll",
        "minhook.dll",
        "subhook.dll",
        "QueueUserAPC",
        "{00021492-0000-0000-C000-000000000046}",
        "{06407553-9630-484F-A586-30CA07135045}"
    )

    $devUserPattern = "C:\\Users\\"

    foreach ($file in $binFiles) {
        $relPath = $file.FullName.Substring($targetDir.Length).TrimStart('\', '/')
        $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
        $sha256 = (Get-FileHash -Path $file.FullName -Algorithm SHA256).Hash

        $violations = [System.Collections.Generic.List[string]]::new()

        # A. Deep PE Header, Import, and Export Inspection (Gate G03)
        $peResult = [AetherPeInspectorV3]::AnalyzePeBytes($bytes)
        $isPe = $peResult.IsPe
        $arch = $peResult.Architecture

        if ($isPe) {
            # Invariant: AetherShell core and script host binaries must export exactly zero functions
            if (($file.Name -eq 'aethershell.exe' -or $file.Name -eq 'aethershell-script-host.exe') -and $peResult.ExportedFunctions.Count -gt 0) {
                $violations.Add("Prohibited exports in AetherShell binary: $($peResult.ExportedFunctions.Count) exports found")
                $msg = "Gate G03 Violation in '$relPath': AetherShell binary must export 0 functions, found $($peResult.ExportedFunctions.Count)"
                if (-not $auditErrors.Contains($msg)) {
                    $auditErrors.Add($msg)
                }
            }

            # Invariant: AetherShell settings binary must strictly never export COM in-process server registration or class factory
            if ($file.Name -eq 'aethershell-settings.exe') {
                $forbiddenForSettings = @("DllRegisterServer", "DllUnregisterServer", "DllInstall", "DllGetClassObject")
                foreach ($f in $peResult.ExportedFunctions) {
                    if ($forbiddenForSettings -contains $f) {
                        $violations.Add("Prohibited COM server export in settings binary: '$f'")
                        $msg = "Gate G03 Violation in '$relPath': Detected prohibited COM server export '$f' in settings application"
                        if (-not $auditErrors.Contains($msg)) {
                            $auditErrors.Add($msg)
                        }
                    }
                }
            }
            # Check for prohibited COM in-process server exports
            foreach ($prohExp in $peResult.ProhibitedExports) {
                $violations.Add("Prohibited COM In-Process Server export: '$prohExp'")
                $msg = "Gate G03 Violation in '$relPath': Detected COM In-Process Server export '$prohExp'"
                if (-not $auditErrors.Contains($msg)) {
                    $auditErrors.Add($msg)
                }
            }

            # Check for prohibited detour / hook imports
            foreach ($prohImp in $peResult.ProhibitedImports) {
                $violations.Add("Prohibited detour/injection library or API: '$prohImp'")
                $msg = "Gate G03 Violation in '$relPath': Detected $prohImp"
                if (-not $auditErrors.Contains($msg)) {
                    $auditErrors.Add($msg)
                }
            }
        }

        # B. String extraction for static symbol / pattern scanning
        $contentAscii = [System.Text.Encoding]::ASCII.GetString($bytes)

        foreach ($pattern in $forbiddenStrings) {
            if ($contentAscii.Contains($pattern)) {
                $violations.Add("Prohibited Explorer extension / hook marker: '$pattern'")
                $msg = "Gate G03 Violation in '$relPath': Detected $pattern"
                if (-not $auditErrors.Contains($msg)) {
                    $auditErrors.Add($msg)
                }
            }
        }

        # C. Check for non-remapped developer usernames in release binaries (Gate G16)
        $hasDevPathLeak = $false
        if ($contentAscii -match 'C:\\Users\\([a-zA-Z0-9_\-\.]+)\\') {
            $detectedUser = $Matches[1]
            if ($detectedUser -notmatch '^(Default|Public|runneradmin|All Users)$') {
                $hasDevPathLeak = $true
                $auditWarnings.Add("Gate G16 Notice in '$relPath': Potential developer workstation path detected: '$($Matches[0])'")
            }
        }

        $binaryManifest.Add([PSCustomObject]@{
            RelativePath      = $relPath
            FileName          = $file.Name
            SizeBytes         = $file.Length
            SHA256            = $sha256
            IsPe              = $isPe
            Architecture      = $arch
            IsDll             = if ($isPe) { $peResult.IsDll } else { $false }
            ExportCount       = if ($isPe) { $peResult.ExportedFunctions.Count } else { 0 }
            ImportedDllCount  = if ($isPe) { $peResult.ImportedDlls.Count } else { 0 }
            ImportedDlls      = if ($isPe) { $peResult.ImportedDlls } else { @() }
            ExportedFunctions = if ($isPe) { $peResult.ExportedFunctions } else { @() }
            Violations        = $violations
            HasDevPathLeak    = $hasDevPathLeak
        })
    }

    # 4. Live Running Process Module Audit (Gate G03)
    $runningExplorerAudited = $false
    $runningExplorerClean = $true
    $runningProtectedProcsAudited = $false
    $runningProtectedProcsClean = $true
    $totalProtectedProcsScanned = 0

    if ($ScanRunningProcesses) {
        Write-Host "`nAuditing running system processes for injected modules (Gate G03)..." -ForegroundColor Cyan
        
        # 4a. Primary Explorer process scan
        $explorerProcs = @(Get-Process -Name explorer -ErrorAction SilentlyContinue)
        if ($explorerProcs.Count -gt 0) {
            $runningExplorerAudited = $true
            $loadedAetherModules = [System.Collections.Generic.List[string]]::new()
            foreach ($p in $explorerProcs) {
                try {
                    foreach ($mod in $p.Modules) {
                        if ($mod.ModuleName -match 'aethershell' -or $mod.FileName -match 'aethershell') {
                            $loadedAetherModules.Add("PID $($p.Id): $($mod.FileName)")
                        }
                    }
                } catch {
                    # Access denied on some modules in protected sessions
                }
            }
            if ($loadedAetherModules.Count -gt 0) {
                $runningExplorerClean = $false
                foreach ($m in $loadedAetherModules) {
                    $auditErrors.Add("Gate G03 Violation: Detected AetherShell module loaded in running Explorer process: $m")
                }
            } else {
                Write-Host "  [PASS] Verified 0 AetherShell modules loaded across $($explorerProcs.Count) Explorer instance(s)." -ForegroundColor Green
            }
        } else {
            Write-Host "  [INFO] No running explorer.exe process detected to audit." -ForegroundColor Yellow
        }

        # 4b. Critical protected processes scan (dwm, csrss, svchost, audiodg, msedge, chrome)
        $protectedTargetNames = @("dwm", "csrss", "svchost", "audiodg", "msedge", "chrome", "firefox")
        $protectedProcs = @(Get-Process -Name $protectedTargetNames -ErrorAction SilentlyContinue)
        if ($protectedProcs.Count -gt 0) {
            $runningProtectedProcsAudited = $true
            $loadedProtectedModules = [System.Collections.Generic.List[string]]::new()
            foreach ($p in $protectedProcs) {
                try {
                    foreach ($mod in $p.Modules) {
                        $totalProtectedProcsScanned++
                        if ($mod.ModuleName -match 'aethershell' -or $mod.FileName -match 'aethershell') {
                            $loadedProtectedModules.Add("$($p.ProcessName) (PID $($p.Id)): $($mod.FileName)")
                        }
                    }
                } catch {
                    # Standard access denied on protected processes (e.g. csrss, audiodg) when non-elevated
                }
            }
            if ($loadedProtectedModules.Count -gt 0) {
                $runningProtectedProcsClean = $false
                foreach ($m in $loadedProtectedModules) {
                    $auditErrors.Add("Gate G03 Violation: Detected AetherShell module loaded in protected process: $m")
                }
            } else {
                Write-Host "  [PASS] Verified 0 AetherShell modules loaded across $($protectedProcs.Count) protected process instances ($totalProtectedProcsScanned inspectable modules)." -ForegroundColor Green
            }
        }
    }

    # Print Report
    Write-Host "`nAudited $($binFiles.Count) binaries across package layout:" -ForegroundColor Cyan
    foreach ($item in $binaryManifest) {
        $statusStr = if ($item.Violations.Count -eq 0) { "[PASS]" } else { "[FAIL]" }
        $color = if ($item.Violations.Count -eq 0) { "Green" } else { "Red" }
        $peDesc = if ($item.IsPe) { "$($item.Architecture) $(if ($item.IsDll) { 'DLL' } else { 'EXE' }), $($item.ImportedDllCount) imports, $($item.ExportCount) exports" } else { "Non-PE" }
        Write-Host "  $statusStr $($item.RelativePath) ($([math]::Round($item.SizeBytes / 1KB, 1)) KB, $peDesc) - SHA256: $($item.SHA256.Substring(0, 16))..." -ForegroundColor $color
    }

    Write-Host "`nAudit Summary:" -ForegroundColor Cyan
    Write-Host "  License Verified     : $hasLicense"
    Write-Host "  PRD Q38:B Compliance : $(-not (Test-Path $portableMarker))"
    if ($ScanRunningProcesses) {
        Write-Host "  Running Explorer Scan: $(if ($runningExplorerClean) { 'Clean (0 modules)' } else { 'VIOLATION DETECTED' })" -ForegroundColor $(if ($runningExplorerClean) { "Green" } else { "Red" })
        Write-Host "  Running Protected Scan: $(if ($runningProtectedProcsClean) { 'Clean (0 modules)' } else { 'VIOLATION DETECTED' })" -ForegroundColor $(if ($runningProtectedProcsClean) { "Green" } else { "Red" })
    }
    Write-Host "  Errors Found         : $($auditErrors.Count)" -ForegroundColor $(if ($auditErrors.Count -eq 0) { "Green" } else { "Red" })
    Write-Host "  Warnings Found       : $($auditWarnings.Count)" -ForegroundColor $(if ($auditWarnings.Count -eq 0) { "Green" } else { "Yellow" })

    foreach ($err in $auditErrors) {
        Write-Host "  [ERROR] $err" -ForegroundColor Red
    }
    foreach ($warn in $auditWarnings) {
        Write-Host "  [WARN]  $warn" -ForegroundColor Yellow
    }

    # Write Manifest if requested
    if (-not [string]::IsNullOrWhiteSpace($OutputManifestPath)) {
        $manifestObj = [PSCustomObject]@{
            PackageTarget                    = $targetDir
            AuditTimestamp                   = (Get-Date -Format "o")
            TotalBinaries                    = $binFiles.Count
            RunningExplorerAudited           = $runningExplorerAudited
            RunningExplorerClean             = $runningExplorerClean
            RunningProtectedProcessesAudited = $runningProtectedProcsAudited
            RunningProtectedProcessesClean   = $runningProtectedProcsClean
            Binaries                         = $binaryManifest
            Errors                           = $auditErrors
            Warnings                         = $auditWarnings
            Verdict                          = if ($auditErrors.Count -eq 0) { "PASSED" } else { "FAILED" }
        }
        $manifestJson = $manifestObj | ConvertTo-Json -Depth 5
        [System.IO.File]::WriteAllText($OutputManifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host "Wrote manifest to: $OutputManifestPath" -ForegroundColor Green
    }

    if ($Json) {
        $manifestObj = [PSCustomObject]@{
            PackageTarget                    = $targetDir
            AuditTimestamp                   = (Get-Date -Format "o")
            TotalBinaries                    = $binFiles.Count
            RunningExplorerAudited           = $runningExplorerAudited
            RunningExplorerClean             = $runningExplorerClean
            RunningProtectedProcessesAudited = $runningProtectedProcsAudited
            RunningProtectedProcessesClean   = $runningProtectedProcsClean
            Binaries                         = $binaryManifest
            Errors                           = $auditErrors
            Warnings                         = $auditWarnings
            Verdict                          = if ($auditErrors.Count -eq 0) { "PASSED" } else { "FAILED" }
        }
        $manifestObj | ConvertTo-Json -Depth 5
    }

    if ($auditErrors.Count -gt 0) {
        exit 1
    } else {
        exit 0
    }
} finally {
    if ($isTempExtracted -and (Test-Path $targetDir)) {
        Remove-Item -Recurse -Force -Path $targetDir -ErrorAction SilentlyContinue
    }
}
