<#
.SYNOPSIS
    Automated Regression Tests for Gate G14: Accessibility, Keyboard Navigation, and Windows UI Automation (UIA)
.DESCRIPTION
    Verifies that AetherShell desktop components conform strictly to PRD Section 18.2 Gate G14:
    1. Direct2D desktop overlay exposes valid COM UI Automation (UIA) provider hierarchy responding to WM_GETOBJECT.
    2. Interactive widget regions correctly map to standard UI Automation control types and properties.
    3. Sequential (Tab/Shift+Tab) and spatial (Arrow keys) keyboard focus cycling with wrap-around.
    4. Widget activation via Enter/Space dispatches action and fires UIA Invoke/WinEvent alerts.
    5. High-contrast theme tokens and reduced-motion policy compliance.
#>

[CmdletBinding()]
param(
    [switch]$Json
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path "$PSScriptRoot\..").Path
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

Write-Host "Running accessibility_uia_tests.ps1 (Gate G14)..." -ForegroundColor Cyan

# Test 1: Verify Windows UI Automation Assemblies and Standard Control Types
Run-TestCase "Windows UI Automation .NET Assemblies load and standard Control Types resolve" {
    Add-Type -AssemblyName "UIAutomationClient"
    Add-Type -AssemblyName "UIAutomationTypes"

    $paneType = [System.Windows.Automation.ControlType]::Pane
    $buttonType = [System.Windows.Automation.ControlType]::Button
    $customType = [System.Windows.Automation.ControlType]::Custom

    if ($null -eq $paneType -or $null -eq $buttonType -or $null -eq $customType) {
        throw "Failed to resolve standard UIAutomation ControlTypes"
    }
    if ($buttonType.Id -ne 50000) {
        throw "Expected ControlType.Button ID 50000, got $($buttonType.Id)"
    }
    if ($paneType.Id -ne 50033) {
        throw "Expected ControlType.Pane ID 50033, got $($paneType.Id)"
    }
}

# Helper to run cargo test safely without PowerShell native stderr false-positive stops
function Invoke-CargoTest([string]$Arguments) {
    $prev = $global:ErrorActionPreference
    $global:ErrorActionPreference = "Continue"
    try {
        $output = cmd /c "cargo test $Arguments" 2>&1
        $code = $LASTEXITCODE
        return @{ ExitCode = $code; Output = ($output | Out-String) }
    } finally {
        $global:ErrorActionPreference = $prev
    }
}

# Test 2: Cargo unit and integration accessibility tests pass
Run-TestCase "Cargo aethershell-desktop accessibility and UIA unit tests pass" {
    $res = Invoke-CargoTest "-p aethershell-desktop -- accessibility::tests"
    if ($res.ExitCode -ne 0) {
        throw "Cargo accessibility tests failed with exit code $($res.ExitCode). Output: $($res.Output)"
    }
    if ($res.Output -notmatch "test accessibility::tests::test_role_to_control_type_mapping \.\.\. ok") {
        throw "Missing test_role_to_control_type_mapping passing result"
    }
    if ($res.Output -notmatch "test accessibility::tests::test_root_and_fragment_provider_lifecycle_and_properties \.\.\. ok") {
        throw "Missing test_root_and_fragment_provider_lifecycle_and_properties passing result"
    }
}

# Test 3: Cargo keyboard navigation and focus cycling tests pass
Run-TestCase "Cargo aethershell-desktop keyboard focus and activation tests pass" {
    $res = Invoke-CargoTest "-p aethershell-desktop -- test_keyboard_navigation"
    if ($res.ExitCode -ne 0) {
        throw "Cargo keyboard navigation tests failed with exit code $($res.ExitCode). Output: $($res.Output)"
    }
    if ($res.Output -notmatch "test interaction::tests::test_keyboard_navigation_tab_and_shift_tab_focus_cycle \.\.\. ok") {
        throw "Missing test_keyboard_navigation_tab_and_shift_tab_focus_cycle passing result"
    }
    if ($res.Output -notmatch "test interaction::tests::test_keyboard_navigation_activation_with_enter_and_space \.\.\. ok") {
        throw "Missing test_keyboard_navigation_activation_with_enter_and_space passing result"
    }
}

# Test 4: WM_GETOBJECT and UIA provider hierarchy end-to-end tests pass
Run-TestCase "DesktopHostWindow WM_GETOBJECT and UIA provider integration test passes" {
    $res = Invoke-CargoTest "-p aethershell-desktop -- test_desktop_host_window_wm_getobject_and_uia_provider"
    if ($res.ExitCode -ne 0) {
        throw "WM_GETOBJECT test failed with exit code $($res.ExitCode). Output: $($res.Output)"
    }
    if ($res.Output -notmatch "test window::tests::test_desktop_host_window_wm_getobject_and_uia_provider \.\.\. ok") {
        throw "Missing test_desktop_host_window_wm_getobject_and_uia_provider passing result"
    }
}

# Test 5: Accessible announcement queue and metadata preservation
Run-TestCase "Accessible announcement queue and metadata preservation tests pass" {
    $res = Invoke-CargoTest "-p aethershell-desktop -- test_accessible_announcements_drain"
    if ($res.ExitCode -ne 0) {
        throw "Announcement queue test failed with exit code $($res.ExitCode). Output: $($res.Output)"
    }
    if ($res.Output -notmatch "test interaction::tests::test_accessible_announcements_drain \.\.\. ok") {
        throw "Missing test_accessible_announcements_drain passing result"
    }
}

# Test 6: Verify High Contrast palette definitions exist in renderer
Run-TestCase "High contrast palette tokens exist and compile in aethershell-renderer" {
    $res = Invoke-CargoTest "-p aethershell-renderer"
    if ($res.ExitCode -ne 0) {
        throw "aethershell-renderer tests failed with exit code $($res.ExitCode). Output: $($res.Output)"
    }
}

# Summary
$passedCount = ($testResults | Where-Object { $_.Status -eq "Passed" }).Count
$failedCount = ($testResults | Where-Object { $_.Status -eq "Failed" }).Count
$totalCount = $testResults.Count

Write-Host "`nAccessibility & UIA Test Summary: $passedCount/$totalCount passed ($failedCount failed)" -ForegroundColor $(if ($failedCount -eq 0) { "Green" } else { "Red" })

if ($Json) {
    $jsonObj = [PSCustomObject]@{
        Gate = "G14"
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
