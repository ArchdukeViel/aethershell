//! Gate G04 Taskbar Recovery Integration Tests (PRD Section 4.2, Section 4.3, Section 18.2 Gate G04).
//!
//! Validates:
//! 1. Coordinator lifecycle and heartbeat pulse
//! 2. `TaskbarCreated` dynamic broadcast handling and debouncing
//! 3. Sudden termination detection using a real supervised process and watchdog recovery
//! 4. Clean exit with no spurious restoration
//! 5. Multi-monitor tray window recovery (`Shell_TrayWnd` and `Shell_SecondaryTrayWnd`).

use aethershell_core::MonitorTaskbarState;
use aethershell_taskbar::{
    clean_exit_event_name, heartbeat_event_name, restore_system_taskbars,
    ExternalAppearanceBackend, TaskbarCoordinator, TaskbarRecoveryCoordinator,
    TaskbarRecoveryEvent, TaskbarRecoveryWatchdog, WatchdogCheckResult, WatchdogExitVerdict,
};
use aethershell_win::probe_taskbars;
use std::process::Command;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

#[test]
fn test_taskbar_recovery_coordinator_lifecycle_and_heartbeat_pulse() {
    let coordinator =
        TaskbarRecoveryCoordinator::new().expect("Coordinator must initialize cleanly");
    let pid = coordinator.pid();
    assert!(pid > 0);

    // Dynamic message must be in registered window message range >= 0xC000
    let msg_id = coordinator.taskbar_created_msg();
    assert!(
        msg_id >= 0xC000,
        "TaskbarCreated dynamic message must be >= 0xC000, got 0x{:04X}",
        msg_id
    );
    assert!(coordinator.is_taskbar_created_message(msg_id));

    // Verify named kernel event paths
    assert_eq!(
        heartbeat_event_name(pid),
        format!(r"Local\AetherShell_Heartbeat_{}", pid)
    );
    assert_eq!(
        clean_exit_event_name(pid),
        format!(r"Local\AetherShell_CleanExit_{}", pid)
    );

    // Watchdog connection to live coordinator
    let watchdog =
        TaskbarRecoveryWatchdog::new(pid).expect("Watchdog must connect to live coordinator");
    assert_eq!(watchdog.target_pid(), pid);
    assert!(watchdog.is_target_alive());

    // Pulse heartbeat and observe healthy status
    coordinator.pulse_heartbeat().expect("Pulse must succeed");
    let status = watchdog.check_target_status(Duration::from_millis(500));
    assert_eq!(status, WatchdogCheckResult::TargetRunningHealthy);

    // Background pulsation thread verification
    let guard = coordinator.start_heartbeat_thread(Duration::from_millis(40));
    std::thread::sleep(Duration::from_millis(120));
    let status_pulsing = watchdog.check_target_status(Duration::from_millis(200));
    assert_eq!(status_pulsing, WatchdogCheckResult::TargetRunningHealthy);
    drop(guard);
}

#[test]
fn test_taskbar_recovery_taskbar_created_broadcast_handling() {
    let mut coordinator = TaskbarRecoveryCoordinator::new().expect("Coordinator init");
    let msg_id = coordinator.taskbar_created_msg();

    // 1. Validate registered message matching
    assert!(coordinator.is_taskbar_created_message(msg_id));
    assert!(!coordinator.is_taskbar_created_message(0x0001)); // WM_CREATE
    assert!(!coordinator.is_taskbar_created_message(0x0010)); // WM_CLOSE

    // 2. Initial TaskbarCreated handling returns Recovered event
    let event1 = coordinator.handle_taskbar_created();
    match event1 {
        TaskbarRecoveryEvent::Recovered {
            primary_found,
            secondary_count,
            ref taskbars,
        } => {
            println!(
                "TaskbarCreated recovered: primary={}, secondaries={}, total={}",
                primary_found,
                secondary_count,
                taskbars.len()
            );
        }
        TaskbarRecoveryEvent::Debounced { .. } => {
            panic!("First TaskbarCreated handling should not be debounced");
        }
    }

    // 3. Immediate duplicate TaskbarCreated within 300ms window must be debounced
    let event2 = coordinator.handle_taskbar_created();
    match event2 {
        TaskbarRecoveryEvent::Debounced { elapsed_ms } => {
            println!(
                "Rapid duplicate TaskbarCreated correctly debounced after {}ms",
                elapsed_ms
            );
            assert!(elapsed_ms <= 300);
        }
        TaskbarRecoveryEvent::Recovered { .. } => {
            panic!("Rapid duplicate TaskbarCreated must be debounced to prevent loop");
        }
    }

    // 4. Recovery succeeds again after debounce window elapses
    coordinator.set_debounce_duration(Duration::from_millis(15));
    std::thread::sleep(Duration::from_millis(25));
    let event3 = coordinator.handle_taskbar_created();
    assert!(matches!(event3, TaskbarRecoveryEvent::Recovered { .. }));
}

#[test]
fn test_taskbar_recovery_sudden_termination_and_watchdog_recovery() {
    // Spawn real supervised child process
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 15"])
        .spawn()
        .expect("Failed to spawn supervised child process");
    let child_pid = child.id();

    // Initialize coordinator for the supervised child
    let coordinator = TaskbarRecoveryCoordinator::for_pid(child_pid)
        .expect("Failed to initialize coordinator for child");

    // Initialize watchdog for the supervised child
    let watchdog =
        TaskbarRecoveryWatchdog::new(child_pid).expect("Failed to initialize watchdog for child");

    assert!(watchdog.is_target_alive());

    // Pulse initial heartbeat
    coordinator
        .pulse_heartbeat()
        .expect("Heartbeat pulse failed");
    let healthy = watchdog.check_target_status(Duration::from_millis(300));
    assert_eq!(healthy, WatchdogCheckResult::TargetRunningHealthy);

    // Abruptly terminate the supervised child process without marking clean exit
    child.kill().expect("Failed to kill child process");
    let _ = child.wait();

    // Watchdog must detect abrupt termination
    let status = watchdog.check_target_status(Duration::from_millis(100));
    assert!(
        matches!(status, WatchdogCheckResult::TargetAbruptTermination { .. }),
        "Expected TargetAbruptTermination, got {:?}",
        status
    );

    // Run watchdog recovery workflow
    let verdict = watchdog.monitor_target(Duration::from_millis(100), Duration::from_millis(20));
    match verdict {
        WatchdogExitVerdict::RecoveredAbruptTermination { exit_code, report } => {
            println!(
                "Watchdog successfully recovered sudden termination (exit code {}): {:?}",
                exit_code, report
            );
            assert_eq!(report.total_discovered, report.restored_hwnds.len());
        }
        other => panic!("Expected RecoveredAbruptTermination, got {:?}", other),
    }
}

#[test]
fn test_taskbar_recovery_clean_exit_no_spurious_restoration() {
    // Spawn real child process that will exit cleanly (exit code 0)
    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Start-Sleep -Milliseconds 400; exit 0",
        ])
        .spawn()
        .expect("Failed to spawn child process");
    let child_pid = child.id();

    let mut coordinator = TaskbarRecoveryCoordinator::for_pid(child_pid)
        .expect("Failed to init coordinator for child");
    let watchdog =
        TaskbarRecoveryWatchdog::new(child_pid).expect("Failed to init watchdog for child");

    assert!(watchdog.is_target_alive());

    // Signal clean exit
    coordinator
        .mark_clean_exit()
        .expect("Failed to signal clean exit");
    assert!(watchdog.is_clean_exit_signaled());

    // Wait for child process to terminate cleanly
    let exit_status = child.wait().expect("Failed to wait on child process");
    assert!(exit_status.success());

    // Watchdog check status
    let check = watchdog.check_target_status(Duration::from_millis(100));
    assert_eq!(
        check,
        WatchdogCheckResult::TargetExitedCleanly { exit_code: 0 }
    );

    // Watchdog monitor must report clean exit without performing spurious taskbar restoration
    let verdict = watchdog.monitor_target(Duration::from_millis(100), Duration::from_millis(20));
    assert_eq!(verdict, WatchdogExitVerdict::CleanExit { exit_code: 0 });
}

#[test]
fn test_taskbar_recovery_multimonitor_tray_window_recovery() {
    // 1. Probe running system taskbars
    let discovered = probe_taskbars();
    println!("Discovered {} system taskbars", discovered.len());

    for (idx, tb) in discovered.iter().enumerate() {
        println!(
            "  [{}] HWND: 0x{:X}, Primary: {}, Visible: {}, Rect: {:?}",
            idx, tb.hwnd, tb.is_primary, tb.visible, tb.rect
        );
        let hwnd = HWND(tb.hwnd as *mut std::ffi::c_void);
        assert!(
            unsafe { IsWindow(hwnd).as_bool() },
            "Discovered taskbar HWND 0x{:X} must be a valid Win32 window",
            tb.hwnd
        );
    }

    // 2. Perform safe, non-invasive system taskbars restoration
    let report = restore_system_taskbars();
    assert_eq!(report.total_discovered, discovered.len());
    assert_eq!(report.restored_hwnds.len(), discovered.len());

    let primary_count = discovered.iter().filter(|t| t.is_primary).count();
    let secondary_count = discovered.iter().filter(|t| !t.is_primary).count();

    if primary_count > 0 {
        assert!(
            report.primary_restored,
            "Primary taskbar Shell_TrayWnd must be restored"
        );
    }
    assert_eq!(
        report.secondary_count, secondary_count,
        "All secondary taskbars Shell_SecondaryTrayWnd must be accounted for"
    );

    // 3. Multi-monitor coordinator state machine check
    let mut coordinator =
        TaskbarCoordinator::with_backend(Box::new(ExternalAppearanceBackend::new()));

    let res_primary = coordinator.evaluate_and_apply(r"\\.\DISPLAY1", MonitorTaskbarState::Desktop);
    assert_eq!(res_primary.backend, "external_appearance");

    let res_secondary =
        coordinator.evaluate_and_apply(r"\\.\DISPLAY2", MonitorTaskbarState::Desktop);
    assert_eq!(res_secondary.backend, "external_appearance");
}
