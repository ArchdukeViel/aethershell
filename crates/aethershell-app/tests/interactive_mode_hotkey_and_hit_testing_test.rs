use aethershell_app::{compile_live_scene_and_regions, extract_interactive_regions};
use aethershell_core::{InteractionMode, NodeType};
use aethershell_desktop::{
    DesktopHostWindow, DesktopWindowManager, DismissalReason, InteractionController,
    InteractiveRegion, KeyboardNavigationResult, PointerHitResult,
};
use aethershell_renderer::ResolvedNode;
use aethershell_win::DesktopHierarchyProbe;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_WIN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SendMessageW, GWL_EXSTYLE, WM_HOTKEY, WM_LBUTTONDOWN, WS_EX_NOACTIVATE,
    WS_EX_TRANSPARENT,
};

static HOTKEY_TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_global_hotkey_registration_and_toggle() {
    let _lock = HOTKEY_TEST_MUTEX.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let probe = DesktopHierarchyProbe::probe();
    let _ = win_mgr.initialize(&probe);

    // 1. Register global hotkey (Win+Ctrl+A)
    let reg_result = win_mgr.register_global_hotkey(MOD_WIN | MOD_CONTROL, 0x41);
    assert!(
        reg_result.is_ok(),
        "Global hotkey registration must succeed"
    );
    assert!(
        win_mgr.is_hotkey_registered(),
        "Hotkey must be flagged as registered"
    );

    let hotkey_toggles = Arc::new(Mutex::new(Vec::new()));
    let toggles_clone = Arc::clone(&hotkey_toggles);
    win_mgr.set_on_hotkey(move |active| {
        toggles_clone.lock().unwrap().push(active);
    });

    let primary_hwnd = win_mgr
        .primary_window()
        .map(|w| w.hwnd)
        .expect("Primary window should exist");

    // 2. Simulate first WM_HOTKEY -> Enter interactive mode
    unsafe {
        SendMessageW(
            primary_hwnd,
            WM_HOTKEY,
            WPARAM(0xAE01), // HOTKEY_ID_INTERACTIVE_MODE
            LPARAM(0),
        );
    }

    assert!(
        win_mgr.primary_window().unwrap().is_interactive(),
        "Window must enter interactive mode on first hotkey trigger"
    );
    let styles = unsafe { GetWindowLongPtrW(primary_hwnd, GWL_EXSTYLE) as u32 };
    assert_eq!(
        styles & WS_EX_TRANSPARENT.0,
        0,
        "Interactive mode must clear WS_EX_TRANSPARENT to receive input"
    );

    // 3. Simulate second WM_HOTKEY -> Exit to passive mode
    unsafe {
        SendMessageW(primary_hwnd, WM_HOTKEY, WPARAM(0xAE01), LPARAM(0));
    }

    assert!(
        !win_mgr.primary_window().unwrap().is_interactive(),
        "Window must exit interactive mode on second hotkey trigger"
    );
    let passive_styles = unsafe { GetWindowLongPtrW(primary_hwnd, GWL_EXSTYLE) as u32 };
    assert!(
        (passive_styles & WS_EX_TRANSPARENT.0) != 0,
        "Passive mode must restore WS_EX_TRANSPARENT"
    );

    let recorded = hotkey_toggles.lock().unwrap().clone();
    assert_eq!(recorded, vec![true, false]);

    // 4. Unregister global hotkey
    let unreg_result = win_mgr.unregister_global_hotkey();
    assert!(unreg_result.is_ok());
    assert!(!win_mgr.is_hotkey_registered());
}

#[test]
fn test_pointer_hit_testing_and_action_dispatch() {
    let mut win = DesktopHostWindow::create("test-hit-mon", 0, 0, 800, 600, None, None)
        .expect("Failed to create test window");

    let button_region = InteractiveRegion::new(
        "media_play_pause".to_string(),
        100.0,
        100.0,
        120.0,
        40.0,
        Some("media.play_pause".to_string()),
    );
    win.set_interactive_regions(vec![button_region]);

    let invoked_actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&invoked_actions);
    win.set_on_action(move |region, px, py| {
        actions_clone
            .lock()
            .unwrap()
            .push((region.id.clone(), region.action.clone(), px, py));
    });

    // Enter interactive mode
    win.set_interactive(true);
    assert!(win.is_interactive());

    // 1. Click directly inside the button region at (150, 120)
    let lparam_hit = LPARAM(((120i16 as u32) << 16 | (150i16 as u32 & 0xFFFF)) as isize);
    unsafe {
        SendMessageW(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_hit);
    }

    let actions = invoked_actions.lock().unwrap().clone();
    assert_eq!(actions.len(), 1, "Action callback should have fired once");
    assert_eq!(actions[0].0, "media_play_pause");
    assert_eq!(actions[0].1, Some("media.play_pause".to_string()));
    assert_eq!(actions[0].2, 150.0);
    assert_eq!(actions[0].3, 120.0);
    assert!(
        win.is_interactive(),
        "Window must remain interactive after hitting a valid widget region"
    );
}

#[test]
fn test_click_outside_dismisses_to_passive_mode() {
    let mut win = DesktopHostWindow::create("test-dismiss-mon", 0, 0, 800, 600, None, None)
        .expect("Failed to create test window");

    let button_region = InteractiveRegion::new(
        "test_button".to_string(),
        50.0,
        50.0,
        100.0,
        30.0,
        Some("test.action".to_string()),
    );
    win.set_interactive_regions(vec![button_region]);

    let dismissals = Arc::new(Mutex::new(Vec::new()));
    let dismiss_clone = Arc::clone(&dismissals);
    win.set_on_dismiss(move |reason| {
        dismiss_clone.lock().unwrap().push(reason);
    });

    win.set_interactive(true);
    assert!(win.is_interactive());

    // Click outside any interactive region at (400, 400)
    let lparam_miss = LPARAM(((400i16 as u32) << 16 | (400i16 as u32 & 0xFFFF)) as isize);
    unsafe {
        SendMessageW(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_miss);
    }

    assert!(
        !win.is_interactive(),
        "Window must immediately revert to passive mode on click outside"
    );
    let styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert!(
        (styles & WS_EX_TRANSPARENT.0) != 0,
        "Click-outside dismissal must immediately restore WS_EX_TRANSPARENT"
    );
    assert!(
        (styles & WS_EX_NOACTIVATE.0) != 0,
        "Click-outside dismissal must immediately restore WS_EX_NOACTIVATE"
    );

    let recorded_dismissals = dismissals.lock().unwrap().clone();
    assert_eq!(recorded_dismissals, vec![DismissalReason::ClickOutside]);
}

#[test]
fn test_inactivity_timeout_and_escape_dismissal() {
    let mut ctrl = InteractionController::new();
    assert_eq!(ctrl.current_mode(), InteractionMode::LockedPassive);

    // Enter temporary mode with 8-second timeout
    ctrl.enter_temporary_mode(8);
    match ctrl.current_mode() {
        InteractionMode::TemporaryInteractive { timeout_seconds } => {
            assert_eq!(timeout_seconds, 8);
        }
        _ => panic!("Expected TemporaryInteractive mode"),
    }

    // Reset activity
    ctrl.reset_activity();

    // Escape key dismissal
    assert!(ctrl.handle_key_down(0x1B));
    assert_eq!(ctrl.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(ctrl.last_dismissal(), Some(DismissalReason::EscapeKey));

    // Pointer hit-test logic on InteractionController
    let regions = vec![InteractiveRegion::new(
        "btn1".to_string(),
        10.0,
        10.0,
        50.0,
        20.0,
        Some("act1".to_string()),
    )];
    ctrl.enter_temporary_mode(8);

    // Hit test inside
    let hit_res = ctrl.handle_pointer_down(20.0, 15.0, &regions);
    match hit_res {
        PointerHitResult::Hit { region } => {
            assert_eq!(region.id, "btn1");
        }
        _ => panic!("Expected Hit result"),
    }
    assert_eq!(ctrl.last_dismissal(), None);

    // Hit test outside -> ClickOutside
    let miss_res = ctrl.handle_pointer_down(100.0, 100.0, &regions);
    assert_eq!(miss_res, PointerHitResult::MissClickOutside);
    assert_eq!(ctrl.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(ctrl.last_dismissal(), Some(DismissalReason::ClickOutside));
}

#[test]
fn test_explorer_restart_preserves_hotkey_and_interactive_regions() {
    let _lock = HOTKEY_TEST_MUTEX.lock().unwrap();

    let mut win_mgr = DesktopWindowManager::new();
    let probe = DesktopHierarchyProbe::probe();
    let _ = win_mgr.initialize(&probe);

    let reg = InteractiveRegion::new(
        "preserved_button".to_string(),
        10.0,
        20.0,
        80.0,
        30.0,
        Some("media.play_pause".to_string()),
    );
    win_mgr.set_interactive_regions(vec![reg.clone()]);
    let reg_result = win_mgr.register_global_hotkey(MOD_WIN | MOD_CONTROL, 0x41);
    assert!(reg_result.is_ok(), "Hotkey registration should succeed");

    assert!(win_mgr.is_hotkey_registered());
    assert_eq!(win_mgr.interactive_regions(), std::slice::from_ref(&reg));

    // Simulate Explorer restart
    let _ = win_mgr.handle_explorer_restart();

    // Verify preservation
    assert!(
        win_mgr.is_hotkey_registered(),
        "Hotkey must remain registered across Explorer restart"
    );
    assert_eq!(
        win_mgr.interactive_regions(),
        std::slice::from_ref(&reg),
        "Interactive regions must be preserved across Explorer restart"
    );
    let primary = win_mgr
        .primary_window()
        .expect("Primary window must exist after Explorer restart");
    assert_eq!(primary.get_interactive_regions().len(), 1);

    let _ = win_mgr.unregister_global_hotkey();
}

#[test]
fn test_compile_live_scene_and_regions_extraction() {
    let bindings = HashMap::new();
    let (scene, regions) = compile_live_scene_and_regions(1920.0, 1080.0, &bindings);

    assert!(
        !scene.commands.is_empty(),
        "Live scene should have compiled render commands"
    );
    assert!(
        !regions.is_empty(),
        "Live scene must yield at least one interactive region"
    );

    let button = regions
        .iter()
        .find(|r| r.id == "play_pause_btn")
        .expect("Expected 'play_pause_btn' interactive region in signature layout");

    assert_eq!(button.action, Some("media.play_pause".into()));
    assert_eq!(button.width, 110.0);
    assert_eq!(button.height, 28.0);

    // Test extract_interactive_regions helper directly
    let test_nodes = vec![
        ResolvedNode {
            id: "bg".into(),
            node_type: NodeType::Container,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            background: None,
            border_color: None,
            corner_radius: 0.0,
            text: None,
            text_color: None,
            font_size_dip: 0.0,
            progress: None,
            fill_color: None,
            bands: None,
            action: None,
        },
        ResolvedNode {
            id: "custom_btn".into(),
            node_type: NodeType::Button,
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 25.0,
            background: None,
            border_color: None,
            corner_radius: 4.0,
            text: Some("Click Me".into()),
            text_color: None,
            font_size_dip: 12.0,
            progress: None,
            fill_color: None,
            bands: None,
            action: Some("custom.click".into()),
        },
    ];

    let extracted = extract_interactive_regions(&test_nodes);
    assert_eq!(extracted.len(), 1);
    assert_eq!(extracted[0].id, "custom_btn");
    assert_eq!(extracted[0].action, Some("custom.click".into()));
    assert_eq!(extracted[0].x, 10.0);
    assert_eq!(extracted[0].y, 10.0);
    assert_eq!(extracted[0].width, 80.0);
    assert_eq!(extracted[0].height, 25.0);
    assert_eq!(
        extracted[0].accessible_description(),
        "Click Me button, action: custom.click"
    );
}

#[test]
fn test_keyboard_navigation_and_accessible_widget_extraction() {
    let bindings = HashMap::new();
    let (_, regions) = compile_live_scene_and_regions(1920.0, 1080.0, &bindings);

    assert!(
        !regions.is_empty(),
        "Extracted regions must contain at least one interactive widget"
    );

    // Verify accessibility descriptions exist for extracted regions
    for r in &regions {
        let desc = r.accessible_description();
        assert!(
            !desc.is_empty(),
            "Accessible description must not be empty for region {}",
            r.id
        );
    }

    let mut controller = InteractionController::new();
    controller.enter_temporary_mode(10);

    // Navigate with Tab
    let nav_res = controller.handle_keyboard_navigation(0x09, false, &regions);
    match nav_res {
        KeyboardNavigationResult::FocusChanged { index, description } => {
            assert_eq!(index, 0);
            assert_eq!(description, regions[0].accessible_description());
        }
        other => panic!("Expected FocusChanged, got {:?}", other),
    }

    // Activate focused widget with Enter (0x0D)
    let act_res = controller.handle_keyboard_navigation(0x0D, false, &regions);
    match act_res {
        KeyboardNavigationResult::Activated { action } => {
            assert_eq!(Some(action), regions[0].action);
        }
        other => panic!("Expected Activated, got {:?}", other),
    }

    // Dismiss with Escape (0x1B)
    let dis_res = controller.handle_keyboard_navigation(0x1B, false, &regions);
    assert_eq!(
        dis_res,
        KeyboardNavigationResult::Dismissed(DismissalReason::EscapeKey)
    );
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
}
