//! Gate G07 Temporary Interactive Mode Integration Tests (PRD Section 5.3, Section 18.2 Gate G07, Decision Q31:A).
//!
//! Validates:
//! 1. Global hotkey toggle (Win+Ctrl+A) and extended window styles (passive vs interactive, zero WS_EX_TOPMOST)
//! 2. Tray IPC invocation (`enter_interactive_mode` and `exit_interactive_mode`)
//! 3. Spatial pointer hit-testing and action dispatch
//! 4. Click-outside immediate safe dismissal restoring passive desktop stacking
//! 5. Escape key (VK_ESCAPE) safe dismissal
//! 6. Inactivity timeout automatic dismissal with configurable timeout duration
//! 7. Focus loss (WM_KILLFOCUS / WA_INACTIVE) safe dismissal
//! 8. Session lock (WM_WTSSESSION_CHANGE / WTS_SESSION_LOCK) safe dismissal
//! 9. Full-screen Gaming Shield safe dismissal
//! 10. Multi-monitor desktop window manager coordination
//! 11. Explorer restart (TaskbarCreated) preservation of hotkeys and interactive regions
//! 12. Host AppData zero-pollution verification (%LOCALAPPDATA%\AetherShell == 0 items)

use aethershell_desktop::{
    DesktopHostWindow, DesktopWindowManager, DismissalReason, InteractiveRegion,
    HOTKEY_ID_INTERACTIVE_MODE, TIMER_ID_INTERACTIVE_TIMEOUT,
};
use aethershell_win::DesktopHierarchyProbe;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_WIN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SendMessageW, GWL_EXSTYLE, WM_ACTIVATE, WM_HOTKEY, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// Test lock to serialize desktop window tests across concurrent integration runner threads.
static G07_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_temporary_interactive_mode_hotkey_toggle_and_extended_styles() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let probe = DesktopHierarchyProbe::probe();
    let _ = win_mgr.initialize(&probe);

    // Register global hotkey (Win+Ctrl+A per PRD Section 5.3 & Q31:A)
    let reg_result = win_mgr.register_global_hotkey(MOD_WIN | MOD_CONTROL, 0x41);
    assert!(
        reg_result.is_ok(),
        "Global hotkey registration must succeed"
    );
    assert!(win_mgr.is_hotkey_registered());

    let hotkey_toggles = Arc::new(Mutex::new(Vec::new()));
    let toggles_clone = Arc::clone(&hotkey_toggles);
    win_mgr.set_on_hotkey(move |active| {
        toggles_clone.lock().unwrap().push(active);
    });

    let primary_hwnd = win_mgr
        .primary_window()
        .map(|w| w.hwnd)
        .expect("Primary desktop overlay window must exist");

    // Initial passive state
    assert!(!win_mgr.primary_window().unwrap().is_interactive());
    let initial_styles = unsafe { GetWindowLongPtrW(primary_hwnd, GWL_EXSTYLE) as u32 };
    assert_ne!(
        initial_styles & WS_EX_TRANSPARENT.0,
        0,
        "Passive mode must have WS_EX_TRANSPARENT so pointer events pass through to icons"
    );
    assert_ne!(
        initial_styles & WS_EX_NOACTIVATE.0,
        0,
        "Passive mode must have WS_EX_NOACTIVATE"
    );
    assert_eq!(
        initial_styles & WS_EX_TOPMOST.0,
        0,
        "Passive mode must strictly possess ZERO WS_EX_TOPMOST"
    );

    // 1. Simulate first WM_HOTKEY -> Enter temporary interactive mode
    unsafe {
        SendMessageW(
            primary_hwnd,
            WM_HOTKEY,
            WPARAM(HOTKEY_ID_INTERACTIVE_MODE as usize),
            LPARAM(0),
        );
    }

    assert!(
        win_mgr.primary_window().unwrap().is_interactive(),
        "Primary window must enter interactive mode on first hotkey"
    );
    let interactive_styles = unsafe { GetWindowLongPtrW(primary_hwnd, GWL_EXSTYLE) as u32 };
    assert_eq!(
        interactive_styles & WS_EX_TRANSPARENT.0,
        0,
        "Interactive mode must clear WS_EX_TRANSPARENT to capture pointer input"
    );
    assert_eq!(
        interactive_styles & WS_EX_TOPMOST.0,
        0,
        "Interactive mode must strictly remain beneath normal applications (ZERO WS_EX_TOPMOST)"
    );

    // 2. Simulate second WM_HOTKEY -> Exit to passive mode
    unsafe {
        SendMessageW(
            primary_hwnd,
            WM_HOTKEY,
            WPARAM(HOTKEY_ID_INTERACTIVE_MODE as usize),
            LPARAM(0),
        );
    }

    assert!(
        !win_mgr.primary_window().unwrap().is_interactive(),
        "Primary window must return to passive mode on second hotkey"
    );
    let reverted_styles = unsafe { GetWindowLongPtrW(primary_hwnd, GWL_EXSTYLE) as u32 };
    assert_ne!(
        reverted_styles & WS_EX_TRANSPARENT.0,
        0,
        "Passive mode must restore WS_EX_TRANSPARENT"
    );
    assert_ne!(
        reverted_styles & WS_EX_NOACTIVATE.0,
        0,
        "Passive mode must restore WS_EX_NOACTIVATE"
    );
    assert_eq!(
        reverted_styles & WS_EX_TOPMOST.0,
        0,
        "Must remain non-topmost after dismissal"
    );

    let recorded = hotkey_toggles.lock().unwrap().clone();
    assert_eq!(recorded, vec![true, false]);

    let _ = win_mgr.unregister_global_hotkey();
}

#[test]
fn test_temporary_interactive_mode_spatial_hit_testing_and_action_dispatch() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-hit", 0, 0, 1024, 768, None, None)
        .expect("create test window");

    let button = InteractiveRegion::new(
        "media_next_btn",
        150.0,
        150.0,
        80.0,
        35.0,
        Some("media.next".into()),
    );
    win.set_interactive_regions(vec![button]);

    let action_events = Arc::new(Mutex::new(Vec::new()));
    let ae_clone = Arc::clone(&action_events);
    win.set_on_action(move |region, px, py| {
        ae_clone
            .lock()
            .unwrap()
            .push((region.id.clone(), region.action.clone(), px, py));
    });

    // Enter interactive mode
    win.set_interactive(true);
    assert!(win.is_interactive());

    // Click inside button at (180, 165)
    let lparam_hit = LPARAM(((165i16 as u32) << 16 | (180i16 as u32 & 0xFFFF)) as isize);
    unsafe {
        SendMessageW(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_hit);
    }

    let events = action_events.lock().unwrap().clone();
    assert_eq!(
        events.len(),
        1,
        "Action callback must be triggered on valid hit"
    );
    assert_eq!(events[0].0, "media_next_btn");
    assert_eq!(events[0].1, Some("media.next".into()));
    assert_eq!(events[0].2, 180.0);
    assert_eq!(events[0].3, 165.0);
    assert!(
        win.is_interactive(),
        "Window must remain in interactive mode after clicking a valid widget button"
    );
}

#[test]
fn test_temporary_interactive_mode_click_outside_dismissal() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-outside", 0, 0, 1024, 768, None, None)
        .expect("create test window");

    let button = InteractiveRegion::new(
        "media_prev_btn",
        50.0,
        50.0,
        60.0,
        30.0,
        Some("media.prev".into()),
    );
    win.set_interactive_regions(vec![button]);

    let dismissals = Arc::new(Mutex::new(Vec::new()));
    let dis_clone = Arc::clone(&dismissals);
    win.set_on_dismiss(move |reason| {
        dis_clone.lock().unwrap().push(reason);
    });

    win.set_interactive(true);
    assert!(win.is_interactive());

    // Click outside any interactive region at (500, 500)
    let lparam_miss = LPARAM(((500i16 as u32) << 16 | (500i16 as u32 & 0xFFFF)) as isize);
    unsafe {
        SendMessageW(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_miss);
    }

    assert!(
        !win.is_interactive(),
        "Click outside interactive regions must immediately dismiss interactive mode"
    );
    assert_eq!(
        win.last_dismissal_reason(),
        Some(DismissalReason::ClickOutside)
    );
    assert_eq!(
        *dismissals.lock().unwrap(),
        vec![DismissalReason::ClickOutside]
    );

    let styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert_ne!(
        styles & WS_EX_TRANSPARENT.0,
        0,
        "Click outside must restore WS_EX_TRANSPARENT"
    );
    assert_ne!(
        styles & WS_EX_NOACTIVATE.0,
        0,
        "Click outside must restore WS_EX_NOACTIVATE"
    );
}

#[test]
fn test_temporary_interactive_mode_escape_key_dismissal() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-escape", 0, 0, 800, 600, None, None)
        .expect("create test window");

    let dismissals = Arc::new(Mutex::new(Vec::new()));
    let dis_clone = Arc::clone(&dismissals);
    win.set_on_dismiss(move |reason| {
        dis_clone.lock().unwrap().push(reason);
    });

    win.set_interactive(true);
    assert!(win.is_interactive());

    // Send VK_ESCAPE (0x1B) via WM_KEYDOWN
    unsafe {
        SendMessageW(win.hwnd, WM_KEYDOWN, WPARAM(0x1B), LPARAM(0));
    }

    assert!(
        !win.is_interactive(),
        "Escape key must dismiss interactive mode back to passive desktop icons"
    );
    assert_eq!(
        win.last_dismissal_reason(),
        Some(DismissalReason::EscapeKey)
    );
    assert_eq!(
        *dismissals.lock().unwrap(),
        vec![DismissalReason::EscapeKey]
    );
}

#[test]
fn test_temporary_interactive_mode_inactivity_timeout_lifecycle() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-timeout", 0, 0, 800, 600, None, None)
        .expect("create test window");

    // Configure 1-second timeout
    win.set_interactive_timeout(1);
    assert_eq!(win.interactive_timeout(), 1);

    win.set_interactive(true);
    assert!(win.is_interactive());

    // Check inactivity before timeout (500ms elapsed) -> must NOT dismiss
    let last_tick = win.last_activity_tick();
    let dismissed_early = win.check_inactivity_timeout(last_tick + 500);
    assert!(!dismissed_early);
    assert!(win.is_interactive());

    // Check inactivity after timeout (1100ms elapsed) -> must dismiss
    let dismissed = win.check_inactivity_timeout(last_tick + 1100);
    assert!(dismissed);
    assert!(!win.is_interactive());
    assert_eq!(win.last_dismissal_reason(), Some(DismissalReason::Timeout));

    // Verify WM_TIMER message handler operates identically
    win.set_interactive(true);
    assert!(win.is_interactive());
    let timer_tick = win.last_activity_tick();
    win.set_last_activity_tick_for_test(timer_tick.saturating_sub(2000));
    unsafe {
        let res = SendMessageW(
            win.hwnd,
            WM_TIMER,
            WPARAM(TIMER_ID_INTERACTIVE_TIMEOUT),
            LPARAM(0),
        );
        assert_eq!(res.0, 0);
    }
    assert!(!win.is_interactive());
    assert_eq!(win.last_dismissal_reason(), Some(DismissalReason::Timeout));
}

#[test]
fn test_temporary_interactive_mode_focus_loss_dismissal() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-focus", 0, 0, 800, 600, None, None)
        .expect("create test window");

    // 1. WM_KILLFOCUS
    win.set_interactive(true);
    assert!(win.is_interactive());

    unsafe {
        SendMessageW(win.hwnd, WM_KILLFOCUS, WPARAM(0), LPARAM(0));
    }

    assert!(
        !win.is_interactive(),
        "WM_KILLFOCUS must dismiss interactive mode on focus loss"
    );
    assert_eq!(
        win.last_dismissal_reason(),
        Some(DismissalReason::FocusLost)
    );

    // 2. WM_ACTIVATE with WA_INACTIVE
    win.set_interactive(true);
    assert!(win.is_interactive());

    unsafe {
        SendMessageW(win.hwnd, WM_ACTIVATE, WPARAM(0), LPARAM(0));
    }

    assert!(
        !win.is_interactive(),
        "WM_ACTIVATE (WA_INACTIVE) must dismiss interactive mode on focus loss"
    );
    assert_eq!(
        win.last_dismissal_reason(),
        Some(DismissalReason::FocusLost)
    );
}

#[test]
fn test_temporary_interactive_mode_session_lock_dismissal() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win = DesktopHostWindow::create("test-g07-lock", 0, 0, 800, 600, None, None)
        .expect("create test window");

    win.set_interactive(true);
    assert!(win.is_interactive());

    // WM_WTSSESSION_CHANGE with WTS_SESSION_LOCK (7)
    unsafe {
        SendMessageW(win.hwnd, 0x02B1, WPARAM(7), LPARAM(0));
    }

    assert!(
        !win.is_interactive(),
        "Session lock (Win+L) must dismiss interactive mode"
    );
    assert_eq!(
        win.last_dismissal_reason(),
        Some(DismissalReason::SessionLocked)
    );
}

#[test]
fn test_temporary_interactive_mode_fullscreen_shield_dismissal() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let win1 = DesktopHostWindow::create("mon-shield-1", 0, 0, 1920, 1080, None, None).unwrap();
    let win2 = DesktopHostWindow::create("mon-shield-2", 1920, 0, 1920, 1080, None, None).unwrap();

    win_mgr.push_window_for_test(win1);
    win_mgr.push_window_for_test(win2);

    win_mgr.enter_interactive_mode();
    for win in win_mgr.windows() {
        assert!(win.is_interactive());
    }

    // Gaming shield fullscreen detection triggers safe dismissal
    win_mgr.handle_fullscreen_shield();

    for win in win_mgr.windows() {
        assert!(!win.is_interactive());
        assert_eq!(
            win.last_dismissal_reason(),
            Some(DismissalReason::FullScreenShield)
        );
    }
    assert_eq!(
        win_mgr.last_dismissal_reason(),
        Some(DismissalReason::FullScreenShield)
    );
}

#[test]
fn test_temporary_interactive_mode_multimonitor_coordination() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let win_a = DesktopHostWindow::create("disp-a", 0, 0, 1920, 1080, None, None).unwrap();
    let win_b = DesktopHostWindow::create("disp-b", 1920, 0, 2560, 1440, None, None).unwrap();

    win_mgr.push_window_for_test(win_a);
    win_mgr.push_window_for_test(win_b);

    // 1. Enter interactive mode across all monitors
    win_mgr.enter_interactive_mode();
    for win in win_mgr.windows() {
        assert!(win.is_interactive());
        let styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
        assert_eq!(styles & WS_EX_TOPMOST.0, 0);
    }

    // 2. Exit interactive mode across all monitors
    win_mgr.exit_interactive_mode();
    for win in win_mgr.windows() {
        assert!(!win.is_interactive());
        let styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
        assert_ne!(styles & WS_EX_TRANSPARENT.0, 0);
        assert_ne!(styles & WS_EX_NOACTIVATE.0, 0);
    }
}

#[test]
fn test_temporary_interactive_mode_explorer_restart_preservation() {
    let _lock = G07_TEST_LOCK.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let probe = DesktopHierarchyProbe::probe();
    let _ = win_mgr.initialize(&probe);

    let test_region = InteractiveRegion::new(
        "test_restart_region",
        20.0,
        30.0,
        100.0,
        40.0,
        Some("media.play_pause".into()),
    );
    win_mgr.set_interactive_regions(vec![test_region.clone()]);
    let reg_result = win_mgr.register_global_hotkey(MOD_WIN | MOD_CONTROL, 0x41);
    assert!(reg_result.is_ok());

    // Explorer restart recovery
    let _ = win_mgr.handle_explorer_restart();

    assert!(
        win_mgr.is_hotkey_registered(),
        "Global hotkey must remain registered after Explorer restart"
    );
    assert_eq!(
        win_mgr.interactive_regions(),
        std::slice::from_ref(&test_region),
        "Interactive regions must remain preserved after Explorer restart"
    );

    let _ = win_mgr.unregister_global_hotkey();
}

#[test]
fn test_temporary_interactive_mode_host_appdata_remains_strictly_zero_items() {
    let app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if !app_data.is_empty() {
        let aether_dir = std::path::Path::new(&app_data).join("AetherShell");
        if aether_dir.exists() {
            let count = std::fs::read_dir(&aether_dir)
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert_eq!(
                count, 0,
                "Host %LOCALAPPDATA%\\AetherShell must remain strictly at 0 items (zero pollution)"
            );
        }
    }
}
