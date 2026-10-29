<#
.SYNOPSIS
    Automated Regression Tests for scripts/audit_package_artifacts.ps1 (Gate G03, G16, G20)
.DESCRIPTION
    Validates correctness, error handling, strict validation, and non-injection detection
    of scripts/audit_package_artifacts.ps1 using synthetic package layouts and release binaries.
#>

[CmdletBinding()]
param(
    [switch]$Json
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$auditScript = Join-Path $workspaceRoot "scripts\audit_package_artifacts.ps1"
if (-not (Test-Path $auditScript)) {
    throw "Auditor script not found at $auditScript"
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

Write-Host "Running package_isolation_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

# Test 1: Clean synthetic package passes audit in -Strict mode
Run-TestCase "Clean package layout passes audit in -Strict mode" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_clean_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        # Create minimal clean layout
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllText((Join-Path $sandbox "NOTICE.md"), "AetherShell Notices", (New-Object System.Text.UTF8Encoding($false)))
        # Create dummy clean binary
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "aethershell.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN_BINARY_PAYLOAD_NO_HOOKS"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -ne 0) {
            throw "Expected exit code 0 for clean layout, got $code. Output: $outputStr"
        }
        if ($outputStr -notmatch "Audited 1 binaries across package layout") {
            throw "Expected 1 binary audited in output: $outputStr"
        }
        if ($outputStr -notmatch "Errors Found\s*:\s*0") {
            throw "Expected 0 errors found: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 2: Injected Explorer extension marker fails closed (Gate G03)
Run-TestCase "Injected Explorer extension marker fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_hook_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllText((Join-Path $sandbox "NOTICE.md"), "AetherShell Notices", (New-Object System.Text.UTF8Encoding($false)))
        # Injected prohibited marker
        $dirtyPayload = "MZ_HEADER_TRICKY_PAYLOAD_IShellIconOverlayIdentifier_INJECTION"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "injected_module.dll"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for injected module, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'injected_module.dll': Detected IShellIconOverlayIdentifier") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 3: Portable marker file fails closed (Gate G20 / PRD Q38:B)
Run-TestCase "Prohibited aethershell.portable marker fails closed (Gate G20)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_marker_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllText((Join-Path $sandbox "aethershell.portable"), "marker", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "aethershell.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for portable marker, got 0"
        }
        if ($outputStr -notmatch "Gate G20 Violation: Prohibited 'aethershell.portable' marker found") {
            throw "Expected Gate G20 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 4: Missing LICENSE fails closed in -Strict mode (Gate G16)
Run-TestCase "Missing LICENSE file fails closed in -Strict mode (Gate G16)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_nolicense_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "aethershell.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for missing LICENSE, got 0"
        }
        if ($outputStr -notmatch "Gate G16 Violation: Missing Apache-2.0 LICENSE file in package root") {
            throw "Expected Gate G16 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 5: Direct ZIP archive auditing via -ZipArchive
Run-TestCase "Direct ZIP archive auditing via -ZipArchive succeeds" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_zip_" + [System.Guid]::NewGuid().ToString())
    $zipDir = Join-Path $sandbox "content"
    $zipFile = Join-Path $sandbox "test_pkg.zip"
    New-Item -ItemType Directory -Path $zipDir -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $zipDir "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllText((Join-Path $zipDir "NOTICE.md"), "AetherShell Notices", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllBytes((Join-Path $zipDir "app.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_ZIP_CLEAN_PAYLOAD"))

        Compress-Archive -Path "$zipDir\*" -DestinationPath $zipFile -Force

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -ZipArchive '$zipFile' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -ne 0) {
            throw "Expected exit code 0 for ZIP archive, got $code. Output: $outputStr"
        }
        if ($outputStr -notmatch "Audited 1 binaries across package layout") {
            throw "Expected 1 binary in ZIP output: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 6: Output manifest generation produces valid BOM-free JSON
Run-TestCase "Output manifest parameter produces valid BOM-free JSON" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_manifest_" + [System.Guid]::NewGuid().ToString())
    $manifestPath = Join-Path $sandbox "manifest.json"
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "aethershell.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -OutputManifestPath '$manifestPath' 2>&1"
        $code = $LASTEXITCODE

        if ($code -ne 0) {
            throw "Expected exit code 0, got $code"
        }
        if (-not (Test-Path $manifestPath)) {
            throw "Manifest file was not created"
        }

        # Check BOM
        $bytes = [System.IO.File]::ReadAllBytes($manifestPath)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            throw "Manifest file contains illegal UTF-8 Byte Order Mark (BOM)"
        }

        $parsed = Get-Content -Raw $manifestPath | ConvertFrom-Json
        if ($parsed.Verdict -ne "PASSED") {
            throw "Expected Verdict PASSED, got $($parsed.Verdict)"
        }
        if ($parsed.TotalBinaries -ne 1) {
            throw "Expected 1 binary in manifest, got $($parsed.TotalBinaries)"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 7: Prohibited COM in-process server registration export fails closed (Gate G03)
Run-TestCase "Prohibited COM in-process server registration export fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_com_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_DllRegisterServer_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "com_server.dll"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for COM in-process server export, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'com_server.dll': Detected DllRegisterServer") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 8: Prohibited detour library import fails closed (Gate G03)
Run-TestCase "Prohibited detour library import fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_detour_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_detours.dll_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "hooked_app.exe"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for detour import, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'hooked_app.exe': Detected detours.dll") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 9: Prohibited cross-process thread injection API fails closed (Gate G03)
Run-TestCase "Prohibited cross-process thread injection API fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_inject_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_CreateRemoteThread_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "injector.exe"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for thread injection API, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'injector.exe': Detected CreateRemoteThread") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 10: Prohibited Browser Helper Object (BHO) marker fails closed (Gate G03)
Run-TestCase "Prohibited Browser Helper Object (BHO) marker fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_bho_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_IBrowserHelperObject_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "bho_extension.dll"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for BHO marker, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'bho_extension.dll': Detected IBrowserHelperObject") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 11: Prohibited DeskBand extension marker fails closed (Gate G03)
Run-TestCase "Prohibited DeskBand extension marker fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_deskband_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_IDeskBand_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "deskband.dll"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for DeskBand marker, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'deskband.dll': Detected IDeskBand") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 12: Real compiled workspace release binaries audit passes cleanly (Gate G03, G16, G20)
Run-TestCase "Real compiled workspace release binaries audit passes cleanly (Gate G03, G16, G20)" {
    $relDir = Join-Path $workspaceRoot "target\x86_64-pc-windows-msvc\release"
    if (-not (Test-Path $relDir)) {
        $relDir = Join-Path $workspaceRoot "target\release"
    }

    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_realbins_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllText((Join-Path $sandbox "NOTICE.md"), "AetherShell Notices", (New-Object System.Text.UTF8Encoding($false)))

        $binsToCopy = @("aethershell.exe", "aethershell-script-host.exe", "aethershell-settings.exe")
        $copiedCount = 0
        foreach ($b in $binsToCopy) {
            $src = Join-Path $relDir $b
            if (Test-Path $src) {
                Copy-Item $src (Join-Path $sandbox $b)
                $copiedCount++
            }
        }

        if ($copiedCount -eq 0) {
            throw "No release binaries found to audit in $relDir"
        }

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -ne 0) {
            throw "Expected exit code 0 for compiled release binaries, got $code. Output: $outputStr"
        }
        if ($outputStr -notmatch "Errors Found\s*:\s*0") {
            throw "Expected 0 errors found: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 13: Live running Explorer process module scan confirms 0 AetherShell modules (Gate G03)
Run-TestCase "Live running Explorer process module scan confirms 0 AetherShell modules (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_running_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "dummy.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -ScanRunningProcesses -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -ne 0) {
            throw "Expected exit code 0 for running process audit, got $code. Output: $outputStr"
        }
        if ($outputStr -notmatch "Running Explorer Scan: Clean \(0 modules\)") {
            throw "Expected clean Explorer scan confirmation, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 14: Prohibited subhook detour library import fails closed (Gate G03)
Run-TestCase "Prohibited subhook detour library import fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_subhook_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_subhook.dll_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "hooked_app.exe"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for subhook import, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'hooked_app.exe': Detected subhook.dll") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 15: Prohibited QueueUserAPC thread injection API fails closed (Gate G03)
Run-TestCase "Prohibited QueueUserAPC thread injection API fails closed (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_apc_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        $dirtyPayload = "MZ_HEADER_QueueUserAPC_PAYLOAD"
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "apc_injector.exe"), [System.Text.Encoding]::ASCII.GetBytes($dirtyPayload))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -eq 0) {
            throw "Expected non-zero exit code for QueueUserAPC API, got 0"
        }
        if ($outputStr -notmatch "Gate G03 Violation in 'apc_injector.exe': Detected QueueUserAPC") {
            throw "Expected Gate G03 violation message, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Test 16: Multi-process critical protected process scan confirms 0 AetherShell modules (Gate G03)
Run-TestCase "Multi-process critical protected process scan confirms 0 AetherShell modules (Gate G03)" {
    $sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pkg_test_multi_proc_" + [System.Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
    try {
        [System.IO.File]::WriteAllText((Join-Path $sandbox "LICENSE"), "Apache License 2.0", (New-Object System.Text.UTF8Encoding($false)))
        [System.IO.File]::WriteAllBytes((Join-Path $sandbox "dummy.exe"), [System.Text.Encoding]::ASCII.GetBytes("MZ_CLEAN"))

        $output = powershell -NoProfile -ExecutionPolicy Bypass -Command "& '$auditScript' -PackageDir '$sandbox' -ScanRunningProcesses -Strict 2>&1"
        $code = $LASTEXITCODE
        $outputStr = ($output | Out-String)

        if ($code -ne 0) {
            throw "Expected exit code 0 for multi-process audit, got $code. Output: $outputStr"
        }
        if ($outputStr -notmatch "Running Protected Scan: Clean \(0 modules\)") {
            throw "Expected clean Protected Scan confirmation, got: $outputStr"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $sandbox -ErrorAction SilentlyContinue
    }
}

# Summary
$passedCount = ($testResults | Where-Object { $_.Status -eq "Passed" }).Count
$failedCount = ($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$totalCount = $testResults.Count

Write-Host "`nPackage Isolation & Shell Audit Test Summary: $passedCount/$totalCount passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($Json) {
    $jsonObj = [PSCustomObject]@{
        Gate = "G03_G16_G20"
        Timestamp = (Get-Date -Format "o")
        TotalTests = $totalCount
        PassedTests = $passedCount
        FailedTests = $failedCount
        Results = $testResults
    }
    $jsonObj | ConvertTo-Json -Depth 4
}

if ($failedCount -gt 0) {
    exit 1
} else {
    exit 0
}
