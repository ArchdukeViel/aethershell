use aethershell_core::TaskbarEffect;
use aethershell_taskbar::{ExternalAppearanceBackend, TaskbarBackend};
use aethershell_win::{
    dpi_to_scale_factor, probe_desktop_hierarchy, scale_dip_to_pixels, scale_pixels_to_dip,
    set_per_monitor_v2_dpi_awareness, TaskbarArchitecture, WindowsBuildFamily, WindowsVersionInfo,
    DEFAULT_DPI,
};

fn make_test_version(family: WindowsBuildFamily) -> WindowsVersionInfo {
    let (major, minor, build) = match family {
        WindowsBuildFamily::Win10_22H2 => (10, 0, 19045),
        WindowsBuildFamily::Win11_21H2 => (10, 0, 22000),
        WindowsBuildFamily::Win11_22H2 => (10, 0, 22621),
        WindowsBuildFamily::Win11_23H2 => (10, 0, 22631),
        WindowsBuildFamily::Win11_24H2 => (10, 0, 26100),
        WindowsBuildFamily::Win11_25H2 => (10, 0, 26200),
        WindowsBuildFamily::Win11_26H2 => (10, 0, 26300),
        WindowsBuildFamily::Win11_26H1 => (10, 0, 28000),
        WindowsBuildFamily::Unknown(b) => (10, 0, b),
    };

    WindowsVersionInfo {
        major,
        minor,
        build_number: build,
        ubr: Some(1000),
        family,
        is_supported_v1: family.is_v1_supported(),
    }
}

#[test]
fn test_os_matrix_seven_tiers_capability_contracts() {
    let tiers = WindowsBuildFamily::all_authoritative_tiers();
    assert_eq!(
        tiers.len(),
        7,
        "Exactly 7 authoritative tiers per PRD Section 2.1"
    );

    let expected = [
        (
            "VM-WIN10-22H2",
            19045,
            TaskbarArchitecture::LegacyWin32,
            true,
            false,
        ),
        (
            "VM-WIN11-22H2",
            22621,
            TaskbarArchitecture::XamlIsland,
            true,
            false,
        ),
        (
            "VM-WIN11-23H2",
            22631,
            TaskbarArchitecture::XamlIsland,
            true,
            false,
        ),
        (
            "VM-WIN11-24H2",
            26100,
            TaskbarArchitecture::XamlIsland,
            true,
            false,
        ),
        (
            "VM-WIN11-25H2",
            26200,
            TaskbarArchitecture::XamlIsland,
            true,
            false,
        ),
        (
            "VM-WIN11-26H2",
            26300,
            TaskbarArchitecture::XamlIsland,
            true,
            false,
        ),
        (
            "VM-WIN11-26H1",
            28000,
            TaskbarArchitecture::XamlIsland,
            false,
            true,
        ),
    ];

    for (i, family) in tiers.iter().enumerate() {
        let (tier_id, build, arch, v1_supported, special) = expected[i];
        assert_eq!(family.tier_id(), tier_id);
        assert_eq!(family.build_number_or_default(), build);
        assert_eq!(family.taskbar_architecture(), arch);
        assert_eq!(family.is_v1_supported(), v1_supported);
        assert_eq!(family.is_special_validation_tier(), special);

        let caps = family.tier_capabilities();
        assert_eq!(caps.tier_id, tier_id);
        assert_eq!(caps.build_number, build);
        assert!(caps.supports_per_monitor_v2);
        assert!(caps.supports_direct2d_1_1);
        assert!(caps.supports_job_object_quotas);
        assert!(caps.supports_named_pipe_security);
        assert!(caps.supports_shell_hook_stacking);
    }
}

#[test]
fn test_honest_taskbar_effects_fallback_across_all_tiers() {
    // 1. Windows 10 22H2
    let win10 = make_test_version(WindowsBuildFamily::Win10_22H2);
    let mut backend_win10_unverified =
        ExternalAppearanceBackend::with_version_and_verification(win10.clone(), false);
    let res = backend_win10_unverified
        .apply_effect(
            "mon-1",
            aethershell_core::MonitorTaskbarState::Desktop,
            TaskbarEffect::Clear,
        )
        .unwrap();
    assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
    assert!(res
        .fallback_reason
        .unwrap()
        .contains("safely routed to native_noop"));

    let mut backend_win10_verified =
        ExternalAppearanceBackend::with_version_and_verification(win10, true);
    let res_acrylic = backend_win10_verified
        .apply_effect(
            "mon-1",
            aethershell_core::MonitorTaskbarState::Desktop,
            TaskbarEffect::AcrylicLike,
        )
        .unwrap();
    assert_eq!(res_acrylic.applied_effect, TaskbarEffect::NativeNoop);
    assert!(res_acrylic
        .fallback_reason
        .unwrap()
        .contains("Acrylic / Mica materials are not supported on Windows 10"));

    // 2. Windows 11 26H2 (GA Baseline)
    let win11 = make_test_version(WindowsBuildFamily::Win11_26H2);
    let mut backend_win11_unverified =
        ExternalAppearanceBackend::with_version_and_verification(win11.clone(), false);
    let res_win11 = backend_win11_unverified
        .apply_effect(
            "mon-1",
            aethershell_core::MonitorTaskbarState::Desktop,
            TaskbarEffect::Clear,
        )
        .unwrap();
    assert_eq!(res_win11.applied_effect, TaskbarEffect::NativeNoop);
    assert!(res_win11
        .fallback_reason
        .unwrap()
        .contains("External composition unverified on modern Windows 11 XAML taskbar"));

    let mut backend_win11_verified =
        ExternalAppearanceBackend::with_version_and_verification(win11, true);
    let res_win11_ok = backend_win11_verified
        .apply_effect(
            "mon-1",
            aethershell_core::MonitorTaskbarState::Desktop,
            TaskbarEffect::Clear,
        )
        .unwrap();
    assert_eq!(res_win11_ok.applied_effect, TaskbarEffect::Clear);
    assert!(res_win11_ok.fallback_reason.is_none());

    // 3. Windows 11 26H1 (Special validation tier)
    let win11_special = make_test_version(WindowsBuildFamily::Win11_26H1);
    let mut backend_special =
        ExternalAppearanceBackend::with_version_and_verification(win11_special, true);
    let res_special = backend_special
        .apply_effect(
            "mon-1",
            aethershell_core::MonitorTaskbarState::Desktop,
            TaskbarEffect::Clear,
        )
        .unwrap();
    assert_eq!(res_special.applied_effect, TaskbarEffect::NativeNoop);
    assert!(res_special
        .fallback_reason
        .unwrap()
        .contains("special validation tier outside v1.0.0 production baseline"));
}

#[test]
fn test_per_monitor_v2_dpi_awareness_and_scaling_conversions() {
    let awareness_res = set_per_monitor_v2_dpi_awareness();
    assert!(awareness_res.is_ok());

    assert_eq!(dpi_to_scale_factor(DEFAULT_DPI), 1.0);
    assert_eq!(dpi_to_scale_factor(120), 1.25);
    assert_eq!(dpi_to_scale_factor(144), 1.50);
    assert_eq!(dpi_to_scale_factor(192), 2.00);

    assert!((scale_dip_to_pixels(100.0, 144) - 150.0).abs() < f32::EPSILON);
    assert!((scale_pixels_to_dip(150.0, 144) - 100.0).abs() < f32::EPSILON);
}

#[test]
fn test_desktop_hierarchy_probe_contract() {
    let probe = probe_desktop_hierarchy();
    // Desktop probing must execute safely without panics on target platform
    println!(
        "Desktop probe result: progman={:?}, worker_w={:?}, shelldll={:?}, syslistview={:?}",
        probe.progman_hwnd,
        probe.worker_w_hwnd,
        probe.shelldll_defview_hwnd,
        probe.syslistview32_hwnd
    );
}

#[test]
fn test_host_appdata_isolation_clean_at_zero_items() {
    let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if !local_appdata.is_empty() {
        let appdata_dir = std::path::Path::new(&local_appdata).join("AetherShell");
        if appdata_dir.exists() {
            let entries = std::fs::read_dir(&appdata_dir)
                .map(|rd| rd.count())
                .unwrap_or(0);
            assert_eq!(
                entries, 0,
                "Host %LOCALAPPDATA%\\AetherShell must remain strictly clean at 0 items!"
            );
        }
    }
}
