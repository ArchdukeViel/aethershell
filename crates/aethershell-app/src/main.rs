use aethershell_app::AppEngine;
use aethershell_core::LifecycleState;
use aethershell_diagnostics::{redact_sensitive_info, SubsystemHealth, SystemDiagnosticReport};
use aethershell_ipc::IpcClient;
use aethershell_win::{
    probe_desktop_hierarchy, probe_taskbars, SessionSingletonGuard, WindowsVersionInfo,
};
use std::env;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Declare PerMonitorV2 DPI awareness per PRD Section 5.4 & Q01:B
    let _ = aethershell_win::set_per_monitor_v2_dpi_awareness();

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

    if args.iter().any(|a| a == "--settings") {
        let singleton = SessionSingletonGuard::acquire();
        let session_id = singleton.as_ref().map(|s| s.session_id()).unwrap_or(1);
        let pipe_name = format!(r"\\.\pipe\aethershell-ipc-{}", session_id);

        println!("Requesting active AetherShell instance to open Settings...");
        match IpcClient::send_command(&pipe_name, "open_settings", serde_json::json!({})).await {
            Ok(res) => {
                println!("Settings activation requested: success={}", res.success);
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("No active AetherShell instance found or IPC failed: {}", e);
                return ExitCode::FAILURE;
            }
        }
    }

    if let Some(pos) = args.iter().position(|a| a == "--benchmark") {
        let duration: u32 = args.get(pos + 1).and_then(|s| s.parse().ok()).unwrap_or(60);
        println!(
            "Running AetherShell standardized performance benchmark ({} seconds)...",
            duration
        );
        let report = aethershell_diagnostics::run_benchmark(duration);
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return if report.is_within_budget {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let is_safe_mode = args.iter().any(|a| a == "--safe-mode");

    println!("Starting AetherShell v{}...", env!("CARGO_PKG_VERSION"));
    if is_safe_mode {
        println!("*** RUNNING IN SAFE MODE ***");
    }

    let engine = AppEngine::new();
    if let Err(e) = engine.run(is_safe_mode).await {
        eprintln!("AetherShell engine error: {}", e);
        return ExitCode::FAILURE;
    }

    println!("AetherShell core terminated cleanly.");
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
