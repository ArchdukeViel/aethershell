use aethershell_app::{
    compile_live_scene, compile_live_scene_and_regions, compile_live_scene_with_scale,
};
use aethershell_core::BindingValue;
use aethershell_desktop::{DesktopHostWindow, DesktopWindowManager};
use aethershell_renderer::D2D1CanvasRenderer;
use aethershell_win::{
    dpi_to_scale_factor, scale_dip_to_pixels, scale_pixels_to_dip,
    set_per_monitor_v2_dpi_awareness, DesktopHierarchyProbe, DEFAULT_DPI,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_DISPLAYCHANGE, WM_DPICHANGED};

#[test]
fn test_set_per_monitor_v2_dpi_awareness_and_scaling_helpers() {
    let result = set_per_monitor_v2_dpi_awareness();
    assert!(
        result.is_ok(),
        "PerMonitorV2 DPI awareness declaration must succeed or report valid OS status"
    );

    assert_eq!(dpi_to_scale_factor(DEFAULT_DPI), 1.0);
    assert_eq!(dpi_to_scale_factor(120), 1.25);
    assert_eq!(dpi_to_scale_factor(144), 1.50);
    assert_eq!(dpi_to_scale_factor(192), 2.00);

    assert!((scale_dip_to_pixels(100.0, 144) - 150.0).abs() < f32::EPSILON);
    assert!((scale_pixels_to_dip(150.0, 144) - 100.0).abs() < f32::EPSILON);
}

#[test]
fn test_window_dpi_and_bounds_queries() {
    let win = DesktopHostWindow::create("test-dpi-mon", 100, 200, 800, 600, None, None)
        .expect("Failed to create test window");

    assert!(win.dpi() >= 96, "Window DPI must be at least standard 96");
    assert!(
        win.scale_factor() >= 1.0,
        "Scale factor must be at least 1.0"
    );
    assert_eq!(win.bounds(), [100, 200, 800, 600]);
}

#[test]
fn test_wm_dpichanged_message_handling_and_callback() {
    let mut win = DesktopHostWindow::create("test-dpi-change-mon", 0, 0, 800, 600, None, None)
        .expect("Failed to create test window");

    let dpi_received = Arc::new(Mutex::new(Vec::new()));
    let dpi_clone = Arc::clone(&dpi_received);
    win.set_on_dpi_changed(move |_hwnd, dpi, rect| {
        dpi_clone.lock().unwrap().push((
            dpi,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        ));
    });

    let new_dpi = 144u32; // 150% scaling
    let suggested_rect = RECT {
        left: 50,
        top: 60,
        right: 1250,
        bottom: 960,
    };

    unsafe {
        SendMessageW(
            win.hwnd,
            WM_DPICHANGED,
            WPARAM(((new_dpi << 16) | new_dpi) as usize),
            LPARAM(&suggested_rect as *const RECT as isize),
        );
    }

    let records = dpi_received.lock().unwrap().clone();
    assert_eq!(records.len(), 1, "Expected 1 DPI change event");
    assert_eq!(records[0], (144, 50, 60, 1200, 900));

    assert_eq!(win.dpi(), 144);
    assert!((win.scale_factor() - 1.5).abs() < 0.01);
    assert_eq!(win.bounds(), [50, 60, 1200, 900]);
}

#[test]
fn test_wm_displaychange_message_handling_and_callback() {
    let mut win = DesktopHostWindow::create("test-disp-mon", 0, 0, 1024, 768, None, None)
        .expect("Failed to create test window");

    let display_change_count = Arc::new(Mutex::new(0));
    let count_clone = Arc::clone(&display_change_count);
    win.set_on_display_change(move || {
        let mut count = count_clone.lock().unwrap();
        *count += 1;
    });

    unsafe {
        SendMessageW(win.hwnd, WM_DISPLAYCHANGE, WPARAM(32), LPARAM(0));
    }

    assert_eq!(
        *display_change_count.lock().unwrap(),
        1,
        "WM_DISPLAYCHANGE should trigger display change callback"
    );

    win.handle_display_change(0, 0, 1920, 1080);
    assert_eq!(win.bounds(), [0, 0, 1920, 1080]);
}

#[test]
fn test_window_manager_dpi_and_topology_refresh() {
    let mut win_mgr = DesktopWindowManager::new();
    let probe = DesktopHierarchyProbe::probe();
    let res = win_mgr.initialize(&probe);
    assert!(res.is_ok());

    assert!(win_mgr.primary_dpi() >= 96);
    assert!(win_mgr.primary_scale_factor() >= 1.0);

    let display_notified = Arc::new(Mutex::new(false));
    let notif_clone = Arc::clone(&display_notified);
    win_mgr.set_on_display_change(move || {
        *notif_clone.lock().unwrap() = true;
    });

    win_mgr.refresh_monitors();
    assert!(
        *display_notified.lock().unwrap(),
        "refresh_monitors must notify display change listeners"
    );
}

#[test]
fn test_compile_live_scene_with_scale_and_layout_scaling() {
    let mut bindings = HashMap::new();
    bindings.insert("time.clock".into(), BindingValue::String("14:30".into()));
    bindings.insert(
        "time.date".into(),
        BindingValue::String("Tuesday, October 27".into()),
    );

    // 1. Compile at 1.0 scale
    let (scene_1x, regions_1x) = compile_live_scene_with_scale(1920.0, 1080.0, &bindings, 1.0);
    assert!(
        !scene_1x.commands.is_empty(),
        "Scene graph commands must not be empty at 1.0x"
    );
    assert!(
        !regions_1x.is_empty(),
        "Interactive regions must not be empty at 1.0x"
    );

    let region_1x = &regions_1x[0];
    let width_1x = region_1x.width;
    let height_1x = region_1x.height;

    // 2. Compile at 1.5 scale (144 DPI)
    let (scene_1_5x, regions_1_5x) = compile_live_scene_with_scale(1920.0, 1080.0, &bindings, 1.5);
    assert!(!scene_1_5x.commands.is_empty());
    assert_eq!(regions_1_5x.len(), regions_1x.len());

    let region_1_5x = &regions_1_5x[0];
    assert!(
        (region_1_5x.width - width_1x * 1.5).abs() < 1.0,
        "Interactive region width should scale by 1.5x (expected: {}, actual: {})",
        width_1x * 1.5,
        region_1_5x.width
    );
    assert!(
        (region_1_5x.height - height_1x * 1.5).abs() < 1.0,
        "Interactive region height should scale by 1.5x (expected: {}, actual: {})",
        height_1x * 1.5,
        region_1_5x.height
    );

    // 3. Compile at 2.0 scale (192 DPI)
    let (_scene_2x, regions_2x) = compile_live_scene_with_scale(1920.0, 1080.0, &bindings, 2.0);
    let region_2x = &regions_2x[0];
    assert!(
        (region_2x.width - width_1x * 2.0).abs() < 1.0,
        "Interactive region width should scale by 2.0x (expected: {}, actual: {})",
        width_1x * 2.0,
        region_2x.width
    );

    // Verify default compile_live_scene matches 1.0 scale
    let scene_default = compile_live_scene(1920.0, 1080.0, &bindings);
    let (scene_and_reg, _) = compile_live_scene_and_regions(1920.0, 1080.0, &bindings);
    assert_eq!(scene_default.commands.len(), scene_1x.commands.len());
    assert_eq!(scene_and_reg.commands.len(), scene_1x.commands.len());
}

#[test]
fn test_d2d1_renderer_set_dpi_and_resize() {
    let win = DesktopHostWindow::create("test-d2d-dpi", 0, 0, 800, 600, None, None)
        .expect("Failed to create test window");

    match D2D1CanvasRenderer::new_for_hwnd(win.hwnd, 800, 600) {
        Ok(mut renderer) => {
            let (initial_x, initial_y) = renderer.dpi();
            assert!(initial_x > 0.0 && initial_y > 0.0);

            renderer.set_dpi(144.0, 144.0);
            let (new_x, new_y) = renderer.dpi();
            assert_eq!(new_x, 144.0);
            assert_eq!(new_y, 144.0);

            let resize_res = renderer.resize(1200, 900);
            assert!(resize_res.is_ok(), "Direct2D surface resize must succeed");
        }
        Err(e) => {
            eprintln!(
                "Direct2D hardware renderer not supported in this headless environment ({}), skipping D2D surface test",
                e
            );
        }
    }
}
