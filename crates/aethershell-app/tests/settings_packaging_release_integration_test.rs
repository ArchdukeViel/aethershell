use aethershell_config::{resolve_app_data_dir_from_base, ConfigManager};
use aethershell_diagnostics::run_benchmark;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn test_settings_ipc_on_demand_lifecycle_and_exit_gate_g11() {
    let app_bin = env!("CARGO_BIN_EXE_aethershell");
    let settings_bin = Path::new(app_bin).with_file_name("aethershell-settings.exe");

    if !settings_bin.exists() {
        let status = Command::new("cargo")
            .args(["build", "--bin", "aethershell-settings"])
            .status()
            .expect("cargo build aethershell-settings");
        assert!(status.success());
    }

    // 1. Launch primary Core Engine in background
    let mut core_child = Command::new(app_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch core instance");

    std::thread::sleep(Duration::from_millis(700));

    // 2. Launch Settings with --status: connects over IPC, prints diagnostics, exits cleanly (Gate G11)
    let status_output = Command::new(&settings_bin)
        .arg("--status")
        .output()
        .expect("launch settings --status");

    assert!(
        status_output.status.success(),
        "Settings must succeed querying status over IPC"
    );
    let stdout_str = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        stdout_str.contains("core_engine"),
        "Status output must include core_engine subsystem"
    );

    // 3. Launch Settings with --capabilities
    let caps_output = Command::new(&settings_bin)
        .arg("--capabilities")
        .output()
        .expect("launch settings --capabilities");
    assert!(caps_output.status.success());
    let caps_str = String::from_utf8_lossy(&caps_output.stdout);
    assert!(caps_str.contains("taskbar_count"));

    // 4. Launch Settings with --telemetry
    let telem_output = Command::new(&settings_bin)
        .arg("--telemetry")
        .output()
        .expect("launch settings --telemetry");
    assert!(telem_output.status.success());

    // 5. Cleanly shut down Core Engine via Settings --shutdown-core
    let shutdown_output = Command::new(&settings_bin)
        .arg("--shutdown-core")
        .output()
        .expect("launch settings --shutdown-core");
    assert!(shutdown_output.status.success());

    let exit_status = core_child.wait().expect("core exit");
    assert!(exit_status.success());
}

#[test]
fn test_standardized_benchmark_budget_evaluation_gate_g13() {
    // Run short 2-second benchmark harness
    let report = run_benchmark(2);
    assert_eq!(report.duration_seconds, 2);

    // Verify metrics are truthful numbers
    assert!(report.idle_ram_mb > 0.0);
    assert!(report.active_ram_mb > 0.0);
    assert!(report.idle_cpu_avg_pct >= 0.0);
    assert!(report.active_cpu_avg_pct >= 0.0);

    // Verify budget constants conform to PRD Section 17 & Table 17-1
    assert_eq!(report.budgets.idle_cpu_limit_pct, 0.5);
    assert_eq!(report.budgets.active_cpu_limit_pct, 2.0);
    assert_eq!(report.budgets.idle_ram_limit_mb, 40.0);
    assert_eq!(report.budgets.active_ram_limit_mb, 65.0);

    // Verify verdict string is truthful
    if report.is_within_budget {
        assert_eq!(report.verdict, "passed");
    } else {
        assert_eq!(report.verdict, "exceeded");
    }
}

#[test]
fn test_portable_mode_packaging_identity_gate_g20() {
    let temp_dir = std::env::temp_dir().join(format!("aethershell_pkg_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();

    let fake_local_appdata = PathBuf::from(r"C:\Users\Test\AppData\Local");

    // Installed / standard mode: resolves to %LOCALAPPDATA%\AetherShell
    let installed_dir =
        resolve_app_data_dir_from_base(Some(temp_dir.clone()), Some(fake_local_appdata.clone()));
    assert_eq!(installed_dir, fake_local_appdata.join("AetherShell"));

    // Portable mode (PRD Q38:B / Gate G20): marker file redirects AppData to <exe_dir>\data
    let marker_file = temp_dir.join("aethershell.portable");
    fs::write(&marker_file, "").unwrap();

    let portable_dir =
        resolve_app_data_dir_from_base(Some(temp_dir.clone()), Some(fake_local_appdata));
    assert_eq!(portable_dir, temp_dir.join("data"));

    // Verify ConfigManager works identically inside portable data directory
    let manager = ConfigManager::new(&portable_dir);
    let cfg = manager.load_or_default().expect("load or default");
    assert_eq!(cfg.profile, "signature_minimal");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_five_channel_packaging_artifacts_gate_g15() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // 1. WiX MSI package source
    let wix_pkg = root.join("packaging/wix/Package.wxs");
    assert!(wix_pkg.exists(), "WiX Package.wxs must exist");
    let wix_content = fs::read_to_string(&wix_pkg).unwrap();
    assert!(wix_content.contains("UpgradeCode=\"d78b87c1-7c92-4f36-b631-0cf24e6c3821\""));
    assert!(wix_content.contains("aethershell.exe"));
    assert!(wix_content.contains("aethershell-settings.exe"));

    // 2. MSIX AppxManifest.xml
    let msix_manifest = root.join("packaging/msix/AppxManifest.xml");
    assert!(msix_manifest.exists(), "MSIX AppxManifest.xml must exist");
    let msix_content = fs::read_to_string(&msix_manifest).unwrap();
    assert!(msix_content.contains("Name=\"ArchdukeViel.AetherShell\""));
    assert!(msix_content.contains("runFullTrust"));

    // 3. Store Preflight Checklist
    let store_checklist = root.join("packaging/store/StorePreflight.md");
    assert!(
        store_checklist.exists(),
        "StorePreflight.md must be documented"
    );
    let store_content = fs::read_to_string(&store_checklist).unwrap();
    assert!(store_content.contains("10.5 Personal Data & Privacy"));
    assert!(store_content.contains("Audio Privacy Guarantee"));

    // 4. WinGet Manifests
    let winget_dir = root.join("packaging/winget/manifests/a/ArchdukeViel/AetherShell/1.0.0");
    assert!(winget_dir.join("ArchdukeViel.AetherShell.yaml").exists());
    assert!(winget_dir
        .join("ArchdukeViel.AetherShell.installer.yaml")
        .exists());
    assert!(winget_dir
        .join("ArchdukeViel.AetherShell.locale.en-US.yaml")
        .exists());

    let winget_locale =
        fs::read_to_string(winget_dir.join("ArchdukeViel.AetherShell.locale.en-US.yaml")).unwrap();
    assert!(winget_locale.contains("PackageIdentifier: ArchdukeViel.AetherShell"));
    assert!(winget_locale.contains("PackageVersion: 1.0.0"));

    // 5. Portable ZIP packaging script
    let portable_script = root.join("packaging/portable/build-portable.ps1");
    assert!(
        portable_script.exists(),
        "Portable packaging script must exist"
    );
    let portable_content = fs::read_to_string(&portable_script).unwrap();
    assert!(portable_content.contains("aethershell.portable"));
}
