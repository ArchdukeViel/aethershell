use aethershell_app::compile_first_run_scene;
use aethershell_desktop::{DesktopHost, DesktopHostWindow, DesktopWindowManager, DismissalReason};
use aethershell_renderer::{
    CanvasRenderer, ColorRgba, D2D1CanvasRenderer, RenderCommand, SceneGraph,
};
use aethershell_win::DesktopHierarchyProbe;
use std::sync::Arc;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GWL_EXSTYLE, WM_ACTIVATE, WM_KEYDOWN, WM_KILLFOCUS, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

#[test]
fn test_desktop_host_window_passive_extended_styles_and_stacking() {
    let passive_styles = DesktopHost::surface_extended_styles(false);
    assert!(
        (passive_styles & WS_EX_TRANSPARENT.0) != 0,
        "Passive surface must have WS_EX_TRANSPARENT for click fall-through to desktop icons"
    );
    assert!(
        (passive_styles & WS_EX_NOACTIVATE.0) != 0,
        "Passive surface must have WS_EX_NOACTIVATE to prevent stealing focus"
    );
    assert!(
        (passive_styles & WS_EX_TOOLWINDOW.0) != 0,
        "Passive surface must be a tool window"
    );
    assert_eq!(
        passive_styles & WS_EX_TOPMOST.0,
        0,
        "Passive surface must never be topmost"
    );

    let win = DesktopHostWindow::create("test-passive-mon", 0, 0, 400, 300, None, None)
        .expect("Failed to create passive overlay window");

    assert!(!win.is_interactive());
    assert!(!win.hwnd.0.is_null());

    let actual_styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert!(
        (actual_styles & WS_EX_TRANSPARENT.0) != 0,
        "Window actual style must include WS_EX_TRANSPARENT"
    );
    assert!(
        (actual_styles & WS_EX_NOACTIVATE.0) != 0,
        "Window actual style must include WS_EX_NOACTIVATE"
    );
    assert_eq!(
        actual_styles & WS_EX_TOPMOST.0,
        0,
        "Window actual style must NOT include WS_EX_TOPMOST"
    );
}

#[test]
fn test_temporary_interactive_mode_stacking_eliminates_topmost_defect() {
    // PRD Section 5.3 & Q31:A: Eliminate WS_EX_TOPMOST defect
    let interactive_styles = DesktopHost::surface_extended_styles(true);
    assert_eq!(
        interactive_styles & WS_EX_TOPMOST.0,
        0,
        "Defect violation: Interactive surface must NOT have WS_EX_TOPMOST per PRD 5.3 and Q31:A"
    );
    assert_eq!(
        interactive_styles & WS_EX_TRANSPARENT.0,
        0,
        "Interactive surface must take pointer clicks (no WS_EX_TRANSPARENT)"
    );
    assert_eq!(
        interactive_styles & WS_EX_NOACTIVATE.0,
        0,
        "Interactive surface must accept activation (no WS_EX_NOACTIVATE)"
    );
    assert!(
        (interactive_styles & WS_EX_TOOLWINDOW.0) != 0,
        "Interactive surface must be a tool window"
    );

    let mut win = DesktopHostWindow::create("test-interactive-mon", 100, 100, 500, 400, None, None)
        .expect("Failed to create host window");

    let dismiss_reasons = Arc::new(std::sync::Mutex::new(Vec::new()));
    let dismiss_clone = Arc::clone(&dismiss_reasons);

    win.set_on_dismiss(move |reason| {
        dismiss_clone.lock().unwrap().push(reason);
    });

    // 1. Enter interactive mode
    win.set_interactive(true);
    assert!(win.is_interactive());
    let actual_styles = unsafe { GetWindowLongPtrW(win.hwnd, GWL_EXSTYLE) as u32 };
    assert_eq!(
        actual_styles & WS_EX_TOPMOST.0,
        0,
        "Live interactive overlay must NOT have WS_EX_TOPMOST in Win32 styles"
    );

    // 2. Auto-dismissal on Escape key (0x1B)
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            win.hwnd,
            WM_KEYDOWN,
            WPARAM(0x1B),
            LPARAM(0),
        );
    }
    assert!(
        !win.is_interactive(),
        "Window must auto-dismiss to passive on Escape key"
    );
    {
        let reasons = dismiss_reasons.lock().unwrap();
        assert_eq!(reasons.last(), Some(&DismissalReason::EscapeKey));
    }

    // 3. Re-enter and auto-dismiss on Focus Loss (WM_ACTIVATE WA_INACTIVE)
    win.set_interactive(true);
    assert!(win.is_interactive());
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            win.hwnd,
            WM_ACTIVATE,
            WPARAM(0), // WA_INACTIVE
            LPARAM(0),
        );
    }
    assert!(
        !win.is_interactive(),
        "Window must auto-dismiss to passive on focus loss"
    );
    {
        let reasons = dismiss_reasons.lock().unwrap();
        assert_eq!(reasons.last(), Some(&DismissalReason::FocusLost));
    }

    // 4. Re-enter and auto-dismiss on WM_KILLFOCUS
    win.set_interactive(true);
    assert!(win.is_interactive());
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            win.hwnd,
            WM_KILLFOCUS,
            WPARAM(0),
            LPARAM(0),
        );
    }
    assert!(
        !win.is_interactive(),
        "Window must auto-dismiss to passive on kill focus"
    );
}

#[test]
fn test_direct2d_canvas_renderer_live_overlay_drawing() {
    let win = DesktopHostWindow::create("test-d2d-mon", 0, 0, 600, 400, None, None)
        .expect("Failed to create host window for D2D");

    let mut renderer = D2D1CanvasRenderer::new_for_hwnd(win.hwnd, 600, 400)
        .expect("Direct2D hardware/software render target creation failed");

    assert!(renderer.is_attached());

    // Clear with semi-transparent background
    renderer.set_clear_color(ColorRgba::new(10, 15, 25, 180));

    // Compile comprehensive render scene graph
    let scene = SceneGraph {
        commands: vec![
            // 1. Solid rectangle with border stroke
            RenderCommand::DrawRect {
                x: 20.0,
                y: 20.0,
                width: 200.0,
                height: 100.0,
                color: ColorRgba::new(30, 35, 45, 240),
                radius: 0.0,
                border_color: Some(ColorRgba::new(100, 110, 130, 255)),
            },
            // 2. Rounded rectangle with border
            RenderCommand::DrawRect {
                x: 240.0,
                y: 20.0,
                width: 200.0,
                height: 100.0,
                color: ColorRgba::new(40, 50, 70, 220),
                radius: 12.0,
                border_color: Some(ColorRgba::new(80, 150, 240, 255)),
            },
            // 3. DirectWrite text layout
            RenderCommand::DrawText {
                text: "AetherShell Direct2D 1.1 Live".into(),
                x: 35.0,
                y: 35.0,
                font_size_dip: 15.0,
                color: ColorRgba::WHITE,
            },
            // 4. Progress bar with track, fill, and radius
            RenderCommand::DrawProgressBar {
                x: 35.0,
                y: 70.0,
                width: 170.0,
                height: 10.0,
                progress: 0.65,
                fill: ColorRgba::new(76, 175, 80, 255),
                track: ColorRgba::new(50, 50, 50, 200),
                radius: 4.0,
            },
            // 5. Audio spectrum frequency visualizer
            RenderCommand::DrawSpectrum {
                x: 20.0,
                y: 140.0,
                width: 420.0,
                height: 60.0,
                bands: vec![0.2, 0.5, 0.85, 0.95, 0.7, 0.45, 0.3, 0.15],
                color: ColorRgba::new(0, 190, 255, 255),
            },
        ],
    };

    let rendered = renderer
        .render_scene(&scene)
        .expect("Direct2D render_scene failed");
    assert_eq!(
        rendered, 5,
        "All 5 render commands should execute on Direct2D target"
    );

    // Test resize handling
    renderer.resize(800, 600).expect("Direct2D resize failed");
}

#[test]
fn test_multi_monitor_window_manager_and_display_change() {
    let probe = DesktopHierarchyProbe {
        progman_hwnd: None,
        worker_w_hwnd: None,
        shelldll_defview_hwnd: None,
        syslistview32_hwnd: None,
    };

    let mut manager = DesktopWindowManager::new();
    let count = manager
        .initialize(&probe)
        .expect("DesktopWindowManager initialize failed");
    assert!(
        count >= 1,
        "At least 1 display monitor overlay window should be created"
    );

    // Multi-monitor interactive mode toggle
    manager.enter_interactive_mode();
    for win in manager.windows() {
        assert!(win.is_interactive());
    }

    manager.exit_interactive_mode();
    for win in manager.windows() {
        assert!(!win.is_interactive());
    }

    // Refresh monitor geometry
    manager.refresh_monitors();
}

#[test]
fn test_engine_first_run_scene_resolution() {
    let scene = compile_first_run_scene(1920.0, 1080.0);
    assert!(
        !scene.commands.is_empty(),
        "First-run scene compilation must produce render commands"
    );

    // Verify clock text command exists
    let has_clock_text = scene.commands.iter().any(|cmd| match cmd {
        RenderCommand::DrawText {
            text,
            font_size_dip,
            ..
        } => *font_size_dip >= 40.0 && text.contains(':'),
        _ => false,
    });
    assert!(
        has_clock_text,
        "Scene must include clock text rendered with DIP font size"
    );
}
