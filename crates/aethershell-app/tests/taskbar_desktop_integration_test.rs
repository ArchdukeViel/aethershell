use aethershell_core::{InteractionMode, MonitorTaskbarState, TaskbarEffect};
use aethershell_desktop::{DesktopHost, DesktopHostError, DismissalReason, InteractionController};
use aethershell_taskbar::{ExternalAppearanceBackend, TaskbarCoordinator};
use aethershell_win::{detect_taskbar_conflicts, ConflictInfo};
use chrono::{Duration, Utc};

#[test]
fn test_phase2_taskbar_state_machine_and_conflict_precedence() {
    let mut coordinator =
        TaskbarCoordinator::with_backend(Box::new(ExternalAppearanceBackend::new()));

    // 1. Initial desktop evaluation with unverified build falls back to native_noop per PRD 4.2
    let res = coordinator.evaluate_and_apply("DISPLAY1", MonitorTaskbarState::Desktop);
    assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
    assert_eq!(res.backend, "external_appearance");
    assert!(res.fallback_reason.unwrap().contains("safely routed"));

    // 2. Competing customizer conflict yield per Q21:A
    coordinator.set_conflict_info(Some(ConflictInfo {
        name: "TranslucentTB".into(),
        description: "Competing tool running".into(),
        recommendation: "Yield per Q21:A".into(),
    }));

    let res_conflict = coordinator.evaluate_and_apply("DISPLAY1", MonitorTaskbarState::Desktop);
    assert_eq!(res_conflict.applied_effect, TaskbarEffect::NativeNoop);
    assert_eq!(res_conflict.backend, "native_noop");
    assert!(res_conflict
        .fallback_reason
        .unwrap()
        .contains("Competing customizer"));

    // 3. Safe Mode precedence over conflict
    coordinator.set_safe_mode(true);
    let res_safe = coordinator.evaluate_and_apply("DISPLAY1", MonitorTaskbarState::WindowMaximized);
    assert_eq!(res_safe.applied_effect, TaskbarEffect::NativeNoop);
    assert_eq!(res_safe.backend, "safe_mode");
    assert!(res_safe
        .fallback_reason
        .unwrap()
        .contains("Safe mode active"));
}

#[test]
fn test_phase2_interaction_mode_dismissal_triggers() {
    let mut controller = InteractionController::new();
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);

    // Trigger A: Escape Key
    controller.enter_temporary_mode(10);
    assert!(controller.handle_key_down(0x1B));
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(
        controller.last_dismissal(),
        Some(DismissalReason::EscapeKey)
    );

    // Trigger B: Focus Loss
    controller.enter_temporary_mode(10);
    assert!(controller.handle_focus_loss());
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(
        controller.last_dismissal(),
        Some(DismissalReason::FocusLost)
    );

    // Trigger C: Session Lock
    controller.enter_temporary_mode(10);
    assert!(controller.handle_session_lock());
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(
        controller.last_dismissal(),
        Some(DismissalReason::SessionLocked)
    );

    // Trigger D: Timeout Expiration
    let start = Utc::now();
    controller.enter_temporary_mode(5);
    // +3s: not expired
    assert!(!controller.tick(start + Duration::seconds(3)));
    // +6s: expired
    assert!(controller.tick(start + Duration::seconds(6)));
    assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
    assert_eq!(controller.last_dismissal(), Some(DismissalReason::Timeout));
}

#[test]
fn test_phase2_desktop_stacking_validation_contract() {
    let host = DesktopHost::discover();
    // Verify probe discovery executes safely without crashing
    let _ = host.validate_monitor_stacking("DISPLAY1");

    // Negative case: when layering is not available, validate rejection
    let unviable_host = DesktopHost {
        probe: aethershell_win::DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        },
        target_layer: aethershell_core::DesktopLayer::AetherShellWidgets,
        is_layering_available: false,
    };

    let result = unviable_host.validate_monitor_stacking("DISPLAY1");
    assert!(matches!(
        result,
        Err(DesktopHostError::LayeringDisabled(..))
    ));
}

#[test]
fn test_phase2_non_invasive_conflict_detection_execution() {
    let detected = detect_taskbar_conflicts();
    if let Some(c) = detected {
        println!("Active customizer detected: {} ({})", c.name, c.description);
    }
}
