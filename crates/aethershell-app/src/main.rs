use aethershell_core::LifecycleState;
use aethershell_diagnostics::{redact_sensitive_info, SubsystemHealth, SystemDiagnosticReport};
use aethershell_win::{
    enumerate_monitors, probe_desktop_hierarchy, probe_taskbars, WindowsVersionInfo,
};
use std::env;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("AetherShell v{}", env!("CARGO_PKG_VERSION"));
        println!("Platform: Native Windows x86-64");
        println!("License: Apache-2.0");
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|a| a == "--diagnostics") {
        run_diagnostics();
        return ExitCode::SUCCESS;
    }

    let is_safe_mode = args.iter().any(|a| a == "--safe-mode");

    println!("Starting AetherShell v{}...", env!("CARGO_PKG_VERSION"));
    if is_safe_mode {
        println!("*** RUNNING IN SAFE MODE ***");
    }

    let os_info = WindowsVersionInfo::detect();
    println!("Operating System: {}", os_info.family);
    println!("Build: {}", os_info.build_number);

    let monitors = enumerate_monitors();
    println!("Connected Monitors: {}", monitors.len());

    let taskbars = probe_taskbars();
    println!("Detected Taskbar Windows: {}", taskbars.len());

    let desktop = probe_desktop_hierarchy();
    println!(
        "Desktop Stacking: Progman={:?}, ShellDllDefView={:?}, WorkerW={:?}",
        desktop.progman_hwnd.is_some(),
        desktop.shelldll_defview_hwnd.is_some(),
        desktop.worker_w_hwnd.is_some(),
    );

    println!("AetherShell core initialized successfully.");
    ExitCode::SUCCESS
}

fn run_diagnostics() {
    let os_info = WindowsVersionInfo::detect();
    let mut report = SystemDiagnosticReport::new(LifecycleState::Running, os_info.build_number);

    let taskbars = probe_taskbars();
    report.record_subsystem(SubsystemHealth {
        name: "taskbar_probe".into(),
        status: aethershell_core::VerificationStatus::Verified,
        healthy: !taskbars.is_empty(),
        last_heartbeat: chrono::Utc::now(),
        message: Some(format!("Found {} taskbars", taskbars.len())),
    });

    let desktop = probe_desktop_hierarchy();
    report.record_subsystem(SubsystemHealth {
        name: "desktop_host".into(),
        status: aethershell_core::VerificationStatus::Verified,
        healthy: desktop.shelldll_defview_hwnd.is_some(),
        last_heartbeat: chrono::Utc::now(),
        message: Some(format!(
            "Progman: {}, DefView: {}, WorkerW: {}",
            desktop.progman_hwnd.is_some(),
            desktop.shelldll_defview_hwnd.is_some(),
            desktop.worker_w_hwnd.is_some()
        )),
    });

    let json_str = serde_json::to_string_pretty(&report).unwrap_or_default();
    let sanitized = redact_sensitive_info(&json_str);
    println!("{}", sanitized);
}
