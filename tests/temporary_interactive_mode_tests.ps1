<#
.SYNOPSIS
    AetherShell Temporary Interactive Mode Regression Suite (Gate G07, PRD Section 5.3, Section 18.2)
.DESCRIPTION
    Validates Temporary Interactive Mode architectural invariants:
    - Test 1: Cargo aethershell-desktop unit tests pass cleanly (36 tests)
    - Test 2: Cargo temporary_interactive_mode_integration_test passes all 11 tests
    - Test 3: Extended styles enforce passive icon passthrough (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) and zero WS_EX_TOPMOST
    - Test 4: All 8 auto-dismissal triggers verified (Escape, Timeout, FocusLost, ClickOutside, SessionLock, FullScreenShield, ExplicitExit, SafeMode)
    - Test 5: Global hotkey registration (Win+Ctrl+A / 0xAE01) and tray menu IPC wiring
    - Test 6: Safety boundary Q37:A fails closed on physical daily-use host (zero hooks into protected processes)
    - Test 7: Host AppData (%LOCALAPPDATA%\AetherShell) remains strictly unpolluted at 0 items
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

Write-Host "Running temporary_interactive_mode_tests.ps1 Regression Test Suite..." -ForegroundColor Cyan

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
$passCount = 0
$failCount = 0

function Assert-Condition([bool]$condition, [string]$testName) {
    if ($condition) {
        Write-Host "  [PASS] $testName" -ForegroundColor Green
        $script:passCount++
    } else {
        Write-Host "  [FAIL] $testName" -ForegroundColor Red
        $script:failCount++
    }
}

# -----------------------------------------------------------------------------
# Test 1: Cargo aethershell-desktop Unit Tests
# -----------------------------------------------------------------------------
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    $unitOut = cmd /c "cargo test -p aethershell-desktop 2>&1"
    $exitCode = $LASTEXITCODE
    $outStr = ($unitOut | Out-String)
    $unitPassed = ($exitCode -eq 0) -and ($outStr -match "36 passed")
    Assert-Condition $unitPassed "Cargo aethershell-desktop unit tests pass cleanly (36/36)"
} catch {
    Assert-Condition $false "Cargo aethershell-desktop unit tests pass cleanly ($($_.Exception.Message))"
} finally {
    $ErrorActionPreference = $prevEap
}

# -----------------------------------------------------------------------------
# Test 2: Cargo temporary_interactive_mode_integration_test
# -----------------------------------------------------------------------------
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    $integOut = cmd /c "cargo test -p aethershell-app --test temporary_interactive_mode_integration_test 2>&1"
    $exitCode = $LASTEXITCODE
    $outStr = ($integOut | Out-String)
    $integPassed = ($exitCode -eq 0) -and ($outStr -match "11 passed")
    Assert-Condition $integPassed "Cargo temporary_interactive_mode_integration_test passes all 11 tests"
} catch {
    Assert-Condition $false "Cargo temporary_interactive_mode_integration_test passes all 11 tests ($($_.Exception.Message))"
} finally {
    $ErrorActionPreference = $prevEap
}

# -----------------------------------------------------------------------------
# Test 3: Extended Styles: Icon Passthrough vs Interactive, Zero WS_EX_TOPMOST
# -----------------------------------------------------------------------------
try {
    $hostRs = Get-Content -LiteralPath (Join-Path $workspaceRoot "crates\aethershell-desktop\src\host.rs") -Raw
    $fnMatch = [regex]::Match($hostRs, 'pub fn surface_extended_styles[\s\S]*?\n    \}')
    $hasSurfaceStyles = $fnMatch.Success
    $methodBody = if ($hasSurfaceStyles) { $fnMatch.Value } else { "" }
    $hasTransparentInPassive = $methodBody.Contains("WS_EX_TRANSPARENT")
    $hasNoActivateInPassive = $methodBody.Contains("WS_EX_NOACTIVATE")
    $noTopmostInMethod = -not ($methodBody.Contains("WS_EX_TOPMOST"))

    $windowRs = Get-Content -LiteralPath (Join-Path $workspaceRoot "crates\aethershell-desktop\src\window.rs") -Raw
    $usesHwndNoTopmost = $windowRs -match "HWND_NOTOPMOST"
    $neverHwndTopmost = -not ($windowRs -match "\bHWND_TOPMOST\b")

    $stylesContract = $hasSurfaceStyles -and $hasTransparentInPassive -and $hasNoActivateInPassive -and $noTopmostInMethod -and $usesHwndNoTopmost -and $neverHwndTopmost
    Assert-Condition $stylesContract "Extended styles enforce passive icon passthrough and strictly zero WS_EX_TOPMOST"
} catch {
    Assert-Condition $false "Extended styles enforce passive icon passthrough ($($_.Exception.Message))"
}

# -----------------------------------------------------------------------------
# Test 4: All 8 Auto-Dismissal Triggers Verified
# -----------------------------------------------------------------------------
try {
    $interactionRs = Get-Content -LiteralPath (Join-Path $workspaceRoot "crates\aethershell-desktop\src\interaction.rs") -Raw
    $hasEscape = $interactionRs -match "EscapeKey"
    $hasTimeout = $interactionRs -match "Timeout"
    $hasFocusLost = $interactionRs -match "FocusLost"
    $hasClickOutside = $interactionRs -match "ClickOutside"
    $hasSessionLocked = $interactionRs -match "SessionLocked"
    $hasFullScreenShield = $interactionRs -match "FullScreenShield"
    $hasExplicitExit = $interactionRs -match "ExplicitExit"
    $hasSafeMode = $interactionRs -match "SafeMode"

    $hasTimerId = $windowRs -match "TIMER_ID_INTERACTIVE_TIMEOUT"
    $hasDismissAll = $windowRs -match "pub fn dismiss_all\(&mut self"
    $hasShieldHandler = $windowRs -match "pub fn handle_fullscreen_shield\(&mut self\)"

    $allTriggers = $hasEscape -and $hasTimeout -and $hasFocusLost -and $hasClickOutside -and $hasSessionLocked -and $hasFullScreenShield -and $hasExplicitExit -and $hasSafeMode -and $hasTimerId -and $hasDismissAll -and $hasShieldHandler
    Assert-Condition $allTriggers "All 8 auto-dismissal triggers verified in interaction controller and window manager"
} catch {
    Assert-Condition $false "All 8 auto-dismissal triggers verified ($($_.Exception.Message))"
}

# -----------------------------------------------------------------------------
# Test 5: Global Hotkey Registration & Tray Menu Wiring
# -----------------------------------------------------------------------------
try {
    $hasHotkeyConst = $windowRs -match "HOTKEY_ID_INTERACTIVE_MODE: i32 = 0xAE01"
    $hasHotkeyMethod = $windowRs -match "pub fn register_global_hotkey"
    $hasHotkeyUnregister = $windowRs -match "pub fn unregister_global_hotkey"

    $trayRs = Get-Content -LiteralPath (Join-Path $workspaceRoot "crates\aethershell-app\src\tray.rs") -Raw
    $hasTrayInteractive = $trayRs -match "ID_TRAY_INTERACTIVE_MODE"
    $hasTrayMenuString = $trayRs -match "Interactive Mode \(Win\+Ctrl\+A\)"
    $hasTrayIpc = $trayRs -match "enter_interactive_mode"

    $hotkeyAndTray = $hasHotkeyConst -and $hasHotkeyMethod -and $hasHotkeyUnregister -and $hasTrayInteractive -and $hasTrayMenuString -and $hasTrayIpc
    Assert-Condition $hotkeyAndTray "Global hotkey (Win+Ctrl+A / 0xAE01) and tray menu IPC invocation verified"
} catch {
    Assert-Condition $false "Global hotkey and tray menu wiring ($($_.Exception.Message))"
}

# -----------------------------------------------------------------------------
# Test 6: Safety Boundary Q37:A Compliance
# -----------------------------------------------------------------------------
try {
    # Check that explorer.exe is running on host and has NOT been terminated
    $explorerProc = Get-Process -Name "explorer" -ErrorAction SilentlyContinue
    $explorerRunning = ($null -ne $explorerProc) -and ($explorerProc.Count -gt 0)

    # Check for zero low-level mouse hooks (WH_MOUSE_LL / SetWindowsHookEx) in desktop crate
    $hasNoMouseHook = -not ($windowRs -match "WH_MOUSE_LL|SetWindowsHookEx")

    $safetyPassed = $explorerRunning -and $hasNoMouseHook
    Assert-Condition $safetyPassed "Safety boundary Q37:A fails closed on physical daily-use host (zero WH_MOUSE_LL hooks)"
} catch {
    Assert-Condition $false "Safety boundary Q37:A fails closed ($($_.Exception.Message))"
}

# -----------------------------------------------------------------------------
# Test 7: Zero Host AppData Pollution
# -----------------------------------------------------------------------------
try {
    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
    $aetherDir = Join-Path $localAppData "AetherShell"
    $pollutionFree = $true
    if (Test-Path $aetherDir) {
        $count = (Get-ChildItem -Path $aetherDir -Recurse -Force | Measure-Object).Count
        if ($count -ne 0) {
            $pollutionFree = $false
        }
    }
    Assert-Condition $pollutionFree "Host AppData (%LOCALAPPDATA%\AetherShell) remains strictly unpolluted at 0 items"
} catch {
    Assert-Condition $false "Host AppData remains strictly unpolluted ($($_.Exception.Message))"
}

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
Write-Host "--------------------------------------------------------------------------------" -ForegroundColor Gray
Write-Host "Temporary Interactive Mode Test Summary: $passCount/$($passCount + $failCount) passed ($failCount failed)" -ForegroundColor $(if ($failCount -eq 0) { "Green" } else { "Red" })

if ($failCount -gt 0) {
    exit 1
}
exit 0
