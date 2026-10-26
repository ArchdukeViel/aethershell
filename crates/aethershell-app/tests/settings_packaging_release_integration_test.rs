use aethershell_config::{resolve_app_data_dir_from_base, ConfigManager};
use aethershell_diagnostics::run_benchmark;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn test_settings_ipc_on_demand_lifecycle_and_exit_gate_g11() {
    let app_bin = env!("CARGO_BIN_EXE_aethershell");
    let settings_bin = Path::new(app_bin).with_file_name("aethershell-settings.exe");

    if !settings_bin.exists() {
        let mut cmd = Command::new("cargo");
        cmd.args([
            "build",
            "-p",
            "aethershell-settings",
            "--bin",
            "aethershell-settings",
        ]);
        if !cfg!(debug_assertions) || app_bin.contains("release") {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("cargo build aethershell-settings");
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

    let fake_local_appdata = temp_dir.join("FakeAppDataLocal");

    // Installed / standard mode: resolves to %LOCALAPPDATA%\AetherShell
    let installed_dir =
        resolve_app_data_dir_from_base(Some(temp_dir.clone()), Some(fake_local_appdata.clone()));
    assert_eq!(installed_dir, fake_local_appdata.join("AetherShell"));

    // Portable mode (PRD Q38:B / Gate G20): portable binaries consistently resolve to canonical AppData
    let marker_file = temp_dir.join("aethershell.portable");
    fs::write(&marker_file, "").unwrap();

    let portable_dir =
        resolve_app_data_dir_from_base(Some(temp_dir.clone()), Some(fake_local_appdata.clone()));
    assert_eq!(portable_dir, fake_local_appdata.join("AetherShell"));

    // Verify ConfigManager works identically inside canonical data directory
    let manager = ConfigManager::new(&portable_dir);
    let cfg = manager.load_or_default().expect("load or default");
    assert_eq!(cfg.profile, "signature_minimal");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_five_channel_packaging_artifacts_gate_g15() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // 1. WiX MSI package source & self-contained runtime deployment (PRD Table 16 / Line 1205)
    let wix_pkg = root.join("packaging/wix/Package.wxs");
    assert!(wix_pkg.exists(), "WiX Package.wxs must exist");
    let wix_content = fs::read_to_string(&wix_pkg).unwrap();
    assert!(wix_content.contains("UpgradeCode=\"d78b87c1-7c92-4f36-b631-0cf24e6c3821\""));
    assert!(wix_content.contains("Scope=\"perMachine\""));
    assert!(wix_content.contains("aethershell.exe"));
    assert!(wix_content.contains("aethershell-settings.exe"));
    assert!(wix_content.contains("SettingsRuntimeFiles"));
    assert!(wix_content.contains("Microsoft.ui.xaml.dll"));
    assert!(wix_content.contains("assets\\skins"));
    assert!(wix_content.contains("assets\\themes"));
    assert!(wix_content.contains("assets\\profiles"));
    assert!(wix_content.contains("schemas\\"));

    // 2. MSIX AppxManifest.xml, visual assets, and validated build script
    let msix_manifest = root.join("packaging/msix/AppxManifest.xml");
    assert!(msix_manifest.exists(), "MSIX AppxManifest.xml must exist");
    let msix_content = fs::read_to_string(&msix_manifest).unwrap();
    assert!(msix_content.contains("Name=\"ArchdukeViel.AetherShell\""));
    assert!(msix_content.contains("runFullTrust"));

    let msix_script = root.join("packaging/msix/build-msix.ps1");
    assert!(msix_script.exists(), "build-msix.ps1 must exist");
    let msix_script_content = fs::read_to_string(&msix_script).unwrap();
    assert!(
        !msix_script_content.contains("/nv"),
        "MSIX build script must enforce genuine semantic validation without /nv bypass"
    );

    let msix_assets_dir = root.join("packaging/msix/Assets");
    assert!(msix_assets_dir.exists(), "MSIX Assets directory must exist");
    for asset_name in &[
        "StoreLogo.png",
        "Square150x150Logo.png",
        "Square44x44Logo.png",
        "Wide310x150Logo.png",
        "Square71x71Logo.png",
        "SplashScreen.png",
    ] {
        let asset_file = msix_assets_dir.join(asset_name);
        assert!(asset_file.exists(), "Visual asset {asset_name} must exist");
        let meta = fs::metadata(&asset_file).expect("asset metadata");
        assert!(
            meta.len() > 100,
            "Visual asset {asset_name} must be authentic and non-empty"
        );
    }

    // 3. Store Preflight Checklist
    let store_checklist = root.join("packaging/store/StorePreflight.md");
    assert!(
        store_checklist.exists(),
        "StorePreflight.md must be documented"
    );
    let store_content = fs::read_to_string(&store_checklist).unwrap();
    assert!(store_content.contains("10.5 Personal Data & Privacy"));
    assert!(store_content.contains("Audio Privacy Guarantee"));
    assert!(store_content.contains("Five-Channel Packaging Matrix Audit"));

    // 4. WinGet Manifests & Genuine SHA-256 Computation Script
    let winget_dir = root.join("packaging/winget/manifests/a/ArchdukeViel/AetherShell/1.0.0");
    assert!(winget_dir.join("ArchdukeViel.AetherShell.yaml").exists());
    assert!(winget_dir
        .join("ArchdukeViel.AetherShell.installer.yaml")
        .exists());
    assert!(winget_dir
        .join("ArchdukeViel.AetherShell.locale.en-US.yaml")
        .exists());

    let winget_script = root.join("packaging/winget/compute-manifest-sha256.ps1");
    assert!(
        winget_script.exists(),
        "WinGet genuine SHA-256 computation script must exist"
    );

    let winget_installer =
        fs::read_to_string(winget_dir.join("ArchdukeViel.AetherShell.installer.yaml")).unwrap();
    assert!(winget_installer.contains("Scope: machine"));
    assert!(
        !winget_installer
            .contains("0000000000000000000000000000000000000000000000000000000000000000"),
        "WinGet InstallerSha256 must be computed"
    );

    let winget_locale =
        fs::read_to_string(winget_dir.join("ArchdukeViel.AetherShell.locale.en-US.yaml")).unwrap();
    assert!(winget_locale.contains("PackageIdentifier: ArchdukeViel.AetherShell"));
    assert!(winget_locale.contains("PackageVersion: 1.0.0"));

    // 5. Portable ZIP packaging script & self-contained runtime contract (Q38:B / PRD Table 16)
    let portable_script = root.join("packaging/portable/build-portable.ps1");
    assert!(
        portable_script.exists(),
        "Portable packaging script must exist"
    );
    let portable_content = fs::read_to_string(&portable_script).unwrap();
    assert!(portable_content.contains("AetherShell-v1.0.0-portable-win-x64.zip"));
    assert!(portable_content.contains("PRD Q38:B Storage Contract"));
    assert!(
        portable_content.contains(r#""$WinUiSettingsDir\*""#),
        "Portable packaging script must use wildcard path to stage runtime DLLs"
    );
}
