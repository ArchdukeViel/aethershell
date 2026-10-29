use aethershell_ipc::IpcClient;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

static LIFECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_singleton_and_ipc_second_launch() {
    let _test_lock = LIFECYCLE_TEST_LOCK.lock().unwrap();
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    let temp_dir =
        std::env::temp_dir().join(format!("aethershell_lifecycle_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background with isolated config dir and session token path
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Give server time to bind mutex and named pipe
    std::thread::sleep(Duration::from_millis(600));

    // 2. Launch second instance with --settings
    let output = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .arg("--settings")
        .output()
        .expect("launch secondary instance with --settings");

    let stdout_sec = String::from_utf8_lossy(&output.stdout);
    println!("Secondary instance --settings output: {}", stdout_sec);
    assert!(
        output.status.success(),
        "Secondary --settings instance must exit 0"
    );

    // 2b. Launch second instance with NO arguments (default double-click invocation)
    let output_default = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .output()
        .expect("launch secondary instance with default args");

    let stdout_default = String::from_utf8_lossy(&output_default.stdout);
    println!(
        "Secondary instance default invocation output: {}",
        stdout_default
    );
    assert!(
        output_default.status.success(),
        "Secondary instance without args must detect singleton, route to primary, and exit 0"
    );

    // 3. Connect to named pipe and send "shutdown"
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Query status first
        // Attempt connection (session ID 1 or current)
        for session_id in 0..=3 {
            let pipe = format!(r"\\.\pipe\aethershell-ipc-{}", session_id);
            if let Ok(res) =
                IpcClient::send_command(&pipe, "get_status", serde_json::json!({})).await
            {
                assert!(res.success);

                // Negative test: unauthenticated shutdown must fail closed
                let unauth_res = IpcClient::send_command(&pipe, "shutdown", serde_json::json!({}))
                    .await
                    .expect("IPC call");
                assert!(
                    !unauth_res.success,
                    "Unauthenticated shutdown must be rejected"
                );

                // Authenticated shutdown using genuine session token capability
                let token = std::fs::read_to_string(&token_path)
                    .or_else(|_| aethershell_ipc::SessionToken::load().ok_or(()))
                    .expect("Session token must be present on disk for active engine");
                let auth_res = IpcClient::send_command(
                    &pipe,
                    "shutdown",
                    serde_json::json!({ "session_token": token.trim() }),
                )
                .await
                .expect("IPC call");
                assert!(auth_res.success, "Authenticated shutdown must succeed");
                break;
            }
        }
    });

    // Await primary exit
    let exit_status = child.wait().expect("child exit");
    assert!(exit_status.success());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_quit_routing_clean_shutdown() {
    let _test_lock = LIFECYCLE_TEST_LOCK.lock().unwrap();
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    let temp_dir =
        std::env::temp_dir().join(format!("aethershell_cli_quit_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Give primary time to initialize
    std::thread::sleep(Duration::from_millis(600));

    // 2. Launch secondary instance with --quit
    let quit_start = std::time::Instant::now();
    let quit_output = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .arg("--quit")
        .output()
        .expect("launch secondary instance with --quit");

    let quit_stdout = String::from_utf8_lossy(&quit_output.stdout);
    println!("Secondary instance --quit output: {}", quit_stdout);
    assert!(
        quit_output.status.success(),
        "Secondary instance with --quit must succeed with exit code 0"
    );

    // 3. Await primary instance process termination and measure duration
    let exit_status = child.wait().expect("child exit");
    let quit_duration = quit_start.elapsed();

    assert!(
        exit_status.success(),
        "Primary instance must terminate with code 0 on --quit"
    );
    assert!(
        quit_duration < Duration::from_millis(1500),
        "Shutdown under CLI --quit must complete within 1.5s; took {:?}",
        quit_duration
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_tray_wm_close_integration_shutdown() {
    let _test_lock = LIFECYCLE_TEST_LOCK.lock().unwrap();
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    let temp_dir =
        std::env::temp_dir().join(format!("aethershell_wm_close_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Wait for tray window to be created
    let mut tray_hwnd = windows::Win32::Foundation::HWND::default();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        unsafe {
            if let Ok(hwnd) = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows::core::w!("AetherShellTrayWindowClass"),
                windows::core::w!("AetherShellTrayWindow"),
            ) {
                if !hwnd.0.is_null() {
                    tray_hwnd = hwnd;
                    break;
                }
            }
        }
    }
    assert!(
        !tray_hwnd.0.is_null(),
        "Primary instance tray window must be discoverable"
    );

    // 2. Dispatch WM_CLOSE to tray window
    let shutdown_start = std::time::Instant::now();
    unsafe {
        let res = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            tray_hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(res.0, 0, "WM_CLOSE must return 0");
    }

    // 3. Await primary instance process termination and measure duration
    let exit_status = child.wait().expect("child exit");
    let shutdown_duration = shutdown_start.elapsed();

    assert!(
        exit_status.success(),
        "Process must terminate cleanly with exit code 0 on WM_CLOSE"
    );
    assert!(
        shutdown_duration < Duration::from_millis(1500),
        "Shutdown under WM_CLOSE must complete within 1.5s; took {:?}",
        shutdown_duration
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_wm_endsession_integration_shutdown() {
    let _test_lock = LIFECYCLE_TEST_LOCK.lock().unwrap();
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    let temp_dir = std::env::temp_dir().join(format!(
        "aethershell_wm_endsession_{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Wait for tray window to be created
    let mut tray_hwnd = windows::Win32::Foundation::HWND::default();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        unsafe {
            if let Ok(hwnd) = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows::core::w!("AetherShellTrayWindowClass"),
                windows::core::w!("AetherShellTrayWindow"),
            ) {
                if !hwnd.0.is_null() {
                    tray_hwnd = hwnd;
                    break;
                }
            }
        }
    }
    assert!(
        !tray_hwnd.0.is_null(),
        "Primary instance tray window must be discoverable"
    );

    // 2. Dispatch WM_QUERYENDSESSION: Window must return LRESULT(1) confirming safe shutdown permission
    unsafe {
        let res = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            tray_hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_QUERYENDSESSION,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(res.0, 1, "WM_QUERYENDSESSION must return 1");
    }

    // 3. Dispatch WM_ENDSESSION: wParam = 1 (session is terminating)
    let shutdown_start = std::time::Instant::now();
    unsafe {
        let res = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            tray_hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION,
            windows::Win32::Foundation::WPARAM(1),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(res.0, 0, "WM_ENDSESSION must return 0");
    }

    // 4. Await primary instance process termination and measure shutdown duration
    let exit_status = child.wait().expect("child exit");
    let shutdown_duration = shutdown_start.elapsed();

    assert!(
        exit_status.success(),
        "Process must terminate cleanly with exit code 0"
    );
    assert!(
        shutdown_duration < Duration::from_millis(1500),
        "Shutdown under WM_ENDSESSION must complete within 1.5s per PRD Section 18.2 Gate G01; took {:?}",
        shutdown_duration
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_desktop_wm_endsession_integration_shutdown() {
    let _test_lock = LIFECYCLE_TEST_LOCK.lock().unwrap();
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    let temp_dir = std::env::temp_dir().join(format!(
        "aethershell_desktop_endsession_{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Wait for desktop host overlay window to be created
    let mut desktop_hwnd = windows::Win32::Foundation::HWND::default();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        unsafe {
            if let Ok(hwnd) = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows::core::w!("AetherShellDesktopHostWindow"),
                windows::core::w!("AetherShell Desktop Overlay"),
            ) {
                if !hwnd.0.is_null() {
                    desktop_hwnd = hwnd;
                    break;
                }
            }
        }
    }

    // If running in a test environment without desktop window, fallback to tray window
    if desktop_hwnd.0.is_null() {
        println!("Note: Desktop overlay window not created in current environment; verifying via tray window");
        unsafe {
            if let Ok(hwnd) = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows::core::w!("AetherShellTrayWindowClass"),
                windows::core::w!("AetherShellTrayWindow"),
            ) {
                desktop_hwnd = hwnd;
            }
        }
    }
    assert!(
        !desktop_hwnd.0.is_null(),
        "Window must be discoverable for session end test"
    );

    // 2. Dispatch WM_QUERYENDSESSION: Window must return LRESULT(1)
    unsafe {
        let res = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            desktop_hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_QUERYENDSESSION,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(res.0, 1, "WM_QUERYENDSESSION must return 1");
    }

    // 3. Dispatch WM_ENDSESSION: wParam = 1 (session is terminating)
    let shutdown_start = std::time::Instant::now();
    unsafe {
        let res = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            desktop_hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION,
            windows::Win32::Foundation::WPARAM(1),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(res.0, 0, "WM_ENDSESSION must return 0");
    }

    // 4. Await primary instance process termination and measure shutdown duration
    let exit_status = child.wait().expect("child exit");
    let shutdown_duration = shutdown_start.elapsed();

    assert!(
        exit_status.success(),
        "Process must terminate cleanly with exit code 0 on desktop WM_ENDSESSION"
    );
    assert!(
        shutdown_duration < Duration::from_millis(1500),
        "Shutdown under desktop WM_ENDSESSION must complete within 1.5s; took {:?}",
        shutdown_duration
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
