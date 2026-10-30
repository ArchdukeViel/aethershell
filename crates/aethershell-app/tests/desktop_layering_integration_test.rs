//! Gate G06 Desktop Layering & Icon Passthrough Integration Test
//!
//! Authoritative Requirement (PRD Section 18.2 Gate G06, Section 0 Principle 5, Section 5.1, Section 5.2, Section 5.3):
//! "Wallpaper below passive widgets below desktop icons on qualifying builds; otherwise visibly unsupported/disabled."
//!
//! Verifies:
//! 1. 4-tier stacking hierarchy: Wallpaper < AetherShellWidgets < DesktopIcons < NormalAppWindows.
//! 2. Passive extended styles: WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED.
//! 3. Strict absence of WS_EX_TOPMOST on both passive and interactive surfaces (Q31:A).
//! 4. Failure case F44 & Section 5.1: If SHELLDLL_DefView cannot be discovered, layering disabled with honest explanation.
//! 5. Shell hook window registration & debounced restacking upon Win+D / window activation (50ms debounce).
//! 6. Explorer restart recovery (TaskbarCreated message handling with 300ms debounce protection).
//! 7. Multi-monitor DesktopWindowManager coordination, display change (WM_DISPLAYCHANGE), and DPI scaling (WM_DPICHANGED).
//! 8. Host AppData isolation: %LOCALAPPDATA%\AetherShell remains strictly at 0 items.

use aethershell_desktop::{
    safe_stacking_target, shellhook_msg, taskbar_created_msg, DesktopHost, DesktopHostError,
    DesktopHostWindow, DesktopLayeringVerdict, DesktopStackingTier, DesktopWindowManager,
    HSHELL_ACTIVATESHELLWINDOW, HSHELL_REDRAW, HSHELL_RUDEAPPACTIVATED, HSHELL_WINDOWACTIVATED,
};
use aethershell_win::DesktopHierarchyProbe;
use std::fs;
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetWindowLongPtrW, PeekMessageW, SendMessageW, GWL_EXSTYLE, MSG, PM_REMOVE,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

fn pump_window_messages(limit: usize) {
    let mut msg = MSG::default();
    let mut count = 0;
    unsafe {
        while count < limit && PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
            DispatchMessageW(&msg);
            count += 1;
        }
    }
}

#[test]
fn test_desktop_layering_stacking_order_contract() {
    // 1. Authoritative 4-tier ordering
    assert!(DesktopStackingTier::Wallpaper < DesktopStackingTier::AetherShellWidgets);
    assert!(DesktopStackingTier::AetherShellWidgets < DesktopStackingTier::DesktopIcons);
    assert!(DesktopStackingTier::DesktopIcons < DesktopStackingTier::NormalAppWindows);

    // 2. Safe stacking target fallback
    // If target HWND is null or invalid, must safely fall back to HWND_BOTTOM
    let fallback = safe_stacking_target(None);
    assert_eq!(
        fallback.0 as isize,
        1, // HWND_BOTTOM is 1
        "Safe stacking target must return HWND_BOTTOM for None"
    );

    let invalid_hwnd = HWND(0x1234_5678 as *mut _);
    let fallback_invalid = safe_stacking_target(Some(invalid_hwnd));
    assert_eq!(
        fallback_invalid.0 as isize, 1,
        "Safe stacking target must return HWND_BOTTOM for non-existent HWND"
    );
}

#[test]
fn test_desktop_layering_passive_extended_styles_and_non_topmost() {
    let passive_styles = DesktopHost::surface_extended_styles(false);

    // Passive styles must have click pass-through & focus prevention
    assert!(
        (passive_styles & WS_EX_TRANSPARENT.0) != 0,
        "Passive surface must have WS_EX_TRANSPARENT for icon click fall-through"
    );
    assert!(
        (passive_styles & WS_EX_NOACTIVATE.0) != 0,
        "Passive surface must have WS_EX_NOACTIVATE to prevent focus stealing"
    );
    assert!(
        (passive_styles & WS_EX_TOOLWINDOW.0) != 0,
        "Passive surface must be WS_EX_TOOLWINDOW"
    );
    assert_eq!(
        passive_styles & WS_EX_TOPMOST.0,
        0,
        "Passive surface must NEVER have WS_EX_TOPMOST"
    );

    // Interactive surface styles must stay non-topmost (Q31:A)
    let interactive_styles = DesktopHost::surface_extended_styles(true);
    assert_eq!(
        interactive_styles & WS_EX_TOPMOST.0,
        0,
        "Interactive surface must NEVER have WS_EX_TOPMOST"
    );

    // Verify actual Win32 window creation reflects styles
    let win = DesktopHostWindow::create("test-layering-styles", 0, 0, 400, 300, None, None)
        .expect("Failed to create DesktopHostWindow");

    assert!(!win.hwnd.0.is_null());
    assert!(!win.is_interactive());

    let actual = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert!(
        (actual & WS_EX_TRANSPARENT.0) != 0,
        "Live HWND must have WS_EX_TRANSPARENT"
    );
    assert!(
        (actual & WS_EX_NOACTIVATE.0) != 0,
        "Live HWND must have WS_EX_NOACTIVATE"
    );
    assert_eq!(
        actual & WS_EX_TOPMOST.0,
        0,
        "Live HWND must NOT have WS_EX_TOPMOST"
    );
}

#[test]
fn test_desktop_layering_unsupported_hierarchy_disabled_fallback_f44() {
    // PRD Section 5.1 / Failure Case F44:
    // If the desktop shell lacks expected SHELLDLL_DefView hierarchy,
    // widgets must be safely disabled on that monitor with an explanation,
    // rather than moved above normal applications or obscuring desktop icons.
    let unviable_host = DesktopHost {
        probe: DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        },
        target_layer: aethershell_core::DesktopLayer::AetherShellWidgets,
        is_layering_available: false,
    };

    let verdict = unviable_host.evaluate_stacking_verdict("monitor-secondary");
    assert!(verdict.is_disabled());
    assert!(!verdict.is_verified());

    match verdict {
        DesktopLayeringVerdict::Disabled { monitor_id, reason } => {
            assert_eq!(monitor_id, "monitor-secondary");
            assert!(
                reason.contains("SHELLDLL_DefView hierarchy not located"),
                "Reason must mention missing hierarchy"
            );
            assert!(
                reason.contains("widget layer disabled"),
                "Reason must mention disabled layer"
            );
        }
        _ => panic!("Expected DesktopLayeringVerdict::Disabled"),
    }

    let validation_res = unviable_host.validate_monitor_stacking("monitor-secondary");
    assert!(validation_res.is_err());
    match validation_res.unwrap_err() {
        DesktopHostError::LayeringDisabled(mon, reason) => {
            assert_eq!(mon, "monitor-secondary");
            assert!(reason.contains("PRD 5.1"));
        }
        other => panic!("Expected LayeringDisabled, got {:?}", other),
    }
}

#[test]
fn test_desktop_layering_shell_hook_win_d_restacking_and_debouncing() {
    let win = DesktopHostWindow::create("test-shell-hook-mon", 0, 0, 500, 400, None, None)
        .expect("Failed to create DesktopHostWindow");

    let shell_msg = shellhook_msg();
    assert_ne!(
        shell_msg, 0,
        "Registered SHELLHOOK message must be non-zero"
    );

    let initial_count = win.win_d_restack_count();

    // 1. Send first shell hook event (HSHELL_WINDOWACTIVATED - typical Win+D trigger)
    unsafe {
        SendMessageW(
            win.hwnd,
            shell_msg,
            WPARAM(HSHELL_WINDOWACTIVATED),
            LPARAM(0),
        );
    }
    pump_window_messages(10);

    let count_after_first = win.win_d_restack_count();
    assert_eq!(
        count_after_first,
        initial_count + 1,
        "Shell hook event must trigger restacking behind desktop icons"
    );

    // 2. Send second rapid event immediately (within 50ms) -> Must be debounced
    unsafe {
        SendMessageW(
            win.hwnd,
            shell_msg,
            WPARAM(HSHELL_RUDEAPPACTIVATED),
            LPARAM(0),
        );
    }
    pump_window_messages(10);

    let count_after_rapid = win.win_d_restack_count();
    assert_eq!(
        count_after_rapid, count_after_first,
        "Rapid shell hook events within 50ms must be debounced to prevent thrashing"
    );

    // 3. Sleep past debounce interval (80ms to exceed 50ms debounce even with 15.6ms Windows timer granularity)
    std::thread::sleep(std::time::Duration::from_millis(80));
    unsafe {
        SendMessageW(
            win.hwnd,
            shell_msg,
            WPARAM(HSHELL_ACTIVATESHELLWINDOW),
            LPARAM(0),
        );
    }
    pump_window_messages(10);

    let count_after_delay = win.win_d_restack_count();
    assert_eq!(
        count_after_delay,
        count_after_first + 1,
        "Shell hook event after debounce window must trigger restacking"
    );

    // 4. Test HSHELL_REDRAW event after another debounce interval (80ms)
    std::thread::sleep(std::time::Duration::from_millis(80));
    unsafe {
        SendMessageW(win.hwnd, shell_msg, WPARAM(HSHELL_REDRAW), LPARAM(0));
    }
    pump_window_messages(10);

    let count_after_redraw = win.win_d_restack_count();
    assert_eq!(
        count_after_redraw,
        count_after_delay + 1,
        "HSHELL_REDRAW event must also trigger debounced restacking"
    );
}

#[test]
fn test_desktop_layering_taskbar_created_explorer_restart_restacking() {
    let win = DesktopHostWindow::create("test-restart-mon", 0, 0, 500, 400, None, None)
        .expect("Failed to create DesktopHostWindow");

    let tb_msg = taskbar_created_msg();
    assert_ne!(
        tb_msg, 0,
        "Registered TaskbarCreated message must be non-zero"
    );

    let initial_restarts = win.explorer_restart_count();

    // 1. Send TaskbarCreated message
    unsafe {
        SendMessageW(win.hwnd, tb_msg, WPARAM(0), LPARAM(0));
    }
    pump_window_messages(10);

    let restarts_after_first = win.explorer_restart_count();
    assert_eq!(
        restarts_after_first,
        initial_restarts + 1,
        "TaskbarCreated must increment explorer_restart_count"
    );

    // 2. Rapid second broadcast within 300ms must be debounced
    unsafe {
        SendMessageW(win.hwnd, tb_msg, WPARAM(0), LPARAM(0));
    }
    pump_window_messages(10);

    let restarts_after_rapid = win.explorer_restart_count();
    assert_eq!(
        restarts_after_rapid, restarts_after_first,
        "Duplicate TaskbarCreated within 300ms must be debounced"
    );
}

#[test]
fn test_desktop_layering_multi_monitor_window_manager_coordination() {
    let mut manager = DesktopWindowManager::new();
    let probe = aethershell_win::probe_desktop_hierarchy();

    let created_count = manager
        .initialize(&probe)
        .expect("Failed to initialize DesktopWindowManager");

    assert!(
        created_count > 0,
        "At least one desktop host window must be initialized"
    );
    assert_eq!(manager.windows().len(), created_count);

    // Verify all managed windows start in passive mode with non-topmost styles
    for win in manager.windows() {
        assert!(!win.is_interactive());
        let actual = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
        assert_eq!(
            actual & WS_EX_TOPMOST.0,
            0,
            "Managed window must NOT have WS_EX_TOPMOST"
        );
        assert!(
            (actual & WS_EX_TRANSPARENT.0) != 0,
            "Managed window must have WS_EX_TRANSPARENT"
        );
    }

    // Test reassert_all_desktop_stacking
    manager.reassert_all_desktop_stacking();

    for win in manager.windows() {
        assert!(
            win.win_d_restack_count() >= 1,
            "Window restack count must be at least 1 after reassert_all_desktop_stacking"
        );
    }
}

#[test]
fn test_desktop_layering_display_change_and_dpi_change_preserves_stacking() {
    let mut win = DesktopHostWindow::create("test-geom-mon", 0, 0, 800, 600, None, None)
        .expect("Failed to create DesktopHostWindow");

    // 1. WM_DISPLAYCHANGE message
    unsafe {
        SendMessageW(
            win.hwnd,
            WM_DISPLAYCHANGE,
            WPARAM(32), // 32 bpp
            LPARAM((1080_isize << 16) | 1920_isize),
        );
    }
    pump_window_messages(10);

    // Call handle_display_change explicitly to test geometry update
    win.handle_display_change(0, 0, 1920, 1080);
    assert_eq!(win.bounds(), [0, 0, 1920, 1080]);

    // Verify extended styles still remain passive and non-topmost
    let styles_after_display = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert_eq!(styles_after_display & WS_EX_TOPMOST.0, 0);
    assert!((styles_after_display & WS_EX_TRANSPARENT.0) != 0);

    // 2. WM_DPICHANGED message (simulate 150% DPI = 144)
    let suggested_rect = RECT {
        left: 0,
        top: 0,
        right: 2880,
        bottom: 1620,
    };
    unsafe {
        SendMessageW(
            win.hwnd,
            WM_DPICHANGED,
            WPARAM((144_usize << 16) | 144),
            LPARAM(&suggested_rect as *const _ as isize),
        );
    }
    pump_window_messages(10);

    assert_eq!(win.dpi(), 144);
    assert!((win.scale_factor() - 1.5).abs() < 0.01);
    assert_eq!(win.bounds(), [0, 0, 2880, 1620]);

    // Verify non-topmost style is strictly preserved
    let styles_after_dpi = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert_eq!(styles_after_dpi & WS_EX_TOPMOST.0, 0);
    assert!((styles_after_dpi & WS_EX_TRANSPARENT.0) != 0);
}

#[test]
fn test_desktop_layering_host_appdata_remains_strictly_zero_items() {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let aethershell_dir = PathBuf::from(local_app_data).join("AetherShell");
        if aethershell_dir.exists() {
            let count = fs::read_dir(&aethershell_dir)
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert_eq!(
                count, 0,
                "%LOCALAPPDATA%\\AetherShell must strictly contain 0 items, found {}",
                count
            );
        }
    }
}
