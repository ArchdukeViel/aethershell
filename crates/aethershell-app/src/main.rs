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

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
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
            "Running AetherShell component-level microbenchmark ({} seconds)...",
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

    if let Some(pos) = args.iter().position(|a| a == "--benchmark-runtime") {
        let profile_str = args
            .get(pos + 1)
            .map(|s| s.as_str())
            .unwrap_or("minimal_idle");
        let profile = match profile_str {
            "minimal_idle" | "minimal-idle" => {
                aethershell_diagnostics::WorkloadProfile::MinimalIdle
            }
            "default_widgets" | "default-widgets" => {
                aethershell_diagnostics::WorkloadProfile::DefaultWidgets
            }
            "active_media" | "active-media" | "active_media_visualization" => {
                aethershell_diagnostics::WorkloadProfile::ActiveMediaVisualization
            }
            "battery" | "battery_mode" | "battery-mode" => {
                aethershell_diagnostics::WorkloadProfile::BatteryMode
            }
            "gaming" | "gaming_shield" | "gaming-shield" => {
                aethershell_diagnostics::WorkloadProfile::GamingShield
            }
            "settings_open" | "settings-open" | "settings" => {
                aethershell_diagnostics::WorkloadProfile::SettingsOpen
            }
            "lua_worker_active" | "lua-worker-active" | "lua" => {
                aethershell_diagnostics::WorkloadProfile::LuaWorkerActive
            }
            "mixed_monitors" | "mixed-monitors" => {
                aethershell_diagnostics::WorkloadProfile::MixedMonitors
            }
            "monitor_off" | "monitor_off_session_locked" | "session_locked" => {
                aethershell_diagnostics::WorkloadProfile::MonitorOffSessionLocked
            }
            _ => aethershell_diagnostics::WorkloadProfile::MinimalIdle,
        };

        let duration_secs: u64 = args
            .iter()
            .position(|a| a == "--duration")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        let repetitions: u32 = args
            .iter()
            .position(|a| a == "--repetitions")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let warmup_secs: u64 = args
            .iter()
            .position(|a| a == "--warmup")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let allow_self_pid = args.iter().any(|a| a == "--allow-self-pid");
        let is_formal = args
            .iter()
            .any(|a| a == "--formal" || a == "--qualification");

        if repetitions == 0 {
            eprintln!("Error: Benchmark repetitions must be at least 1 (got 0).");
            return ExitCode::FAILURE;
        }
        if duration_secs == 0 {
            eprintln!("Error: Benchmark recording duration must be positive (> 0 seconds).");
            return ExitCode::FAILURE;
        }
        if is_formal && allow_self_pid {
            eprintln!(
                "Error: Cannot run formal PRD Section 14 qualification with --allow-self-pid override."
            );
            return ExitCode::FAILURE;
        }
        if is_formal && repetitions < 5 {
            eprintln!(
                "Error: Formal PRD Section 14 qualification requires --repetitions >= 5 (got {}).",
                repetitions
            );
            return ExitCode::FAILURE;
        }
        if is_formal && duration_secs < 60 {
            eprintln!(
                "Error: Formal PRD Section 14 qualification requires --duration >= 60 (got {}).",
                duration_secs
            );
            return ExitCode::FAILURE;
        }
        if is_formal && profile != aethershell_diagnostics::WorkloadProfile::MinimalIdle {
            eprintln!(
                "Error: Formal PRD Section 14 qualification only applies to ratified MinimalIdle profile, but {:?} has provisional proposals.",
                profile
            );
            return ExitCode::FAILURE;
        }

        println!(
            "Running genuine multi-process runtime benchmark (Profile: {:?}, Repetitions: {}, Duration: {}s, Warmup: {}s, Formal: {})...",
            profile, repetitions, duration_secs, warmup_secs, is_formal
        );

        let mut session = aethershell_diagnostics::RuntimeBenchmarkSession::new(profile);
        session.set_allow_self_pid(allow_self_pid);
        session.set_formal_qualification(is_formal);

        let core_pid: u32 = match args
            .iter()
            .position(|a| a == "--pid-core")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
        {
            Some(pid) => pid,
            None => match aethershell_diagnostics::find_running_core_process() {
                Ok(pid) => {
                    println!(
                        "Discovered running Core 'aethershell.exe' process (PID: {})",
                        pid
                    );
                    pid
                }
                Err(err) => {
                    eprintln!(
                        "Error: Cannot resolve Core process: {}. A genuine runtime benchmark requires an explicit or running Core PID.",
                        err
                    );
                    return ExitCode::FAILURE;
                }
            },
        };

        if let Err(e) = session.track_process(core_pid, aethershell_diagnostics::ProcessRole::Core)
        {
            eprintln!(
                "Error: Failed to validate Core process PID {}: {}",
                core_pid, e
            );
            return ExitCode::FAILURE;
        }

        if profile == aethershell_diagnostics::WorkloadProfile::SettingsOpen {
            let settings_pid = args
                .iter()
                .position(|a| a == "--pid-settings")
                .and_then(|p| args.get(p + 1))
                .and_then(|s| s.parse().ok());

            match settings_pid {
                Some(pid) => {
                    if let Err(e) =
                        session.track_process(pid, aethershell_diagnostics::ProcessRole::Settings)
                    {
                        eprintln!(
                            "Error: Failed to validate Settings process PID {}: {}",
                            pid, e
                        );
                        return ExitCode::FAILURE;
                    }
                }
                None => {
                    eprintln!("Error: WorkloadProfile::SettingsOpen requires a running 'aethershell-settings.exe' via --pid-settings <PID>.");
                    return ExitCode::FAILURE;
                }
            }
        } else if let Some(settings_pid) = args
            .iter()
            .position(|a| a == "--pid-settings")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
        {
            if let Err(e) =
                session.track_process(settings_pid, aethershell_diagnostics::ProcessRole::Settings)
            {
                eprintln!(
                    "Warning: Optional Settings process PID {} rejected: {}",
                    settings_pid, e
                );
            }
        }

        if profile == aethershell_diagnostics::WorkloadProfile::LuaWorkerActive {
            let worker_pid = args
                .iter()
                .position(|a| a == "--pid-worker")
                .and_then(|p| args.get(p + 1))
                .and_then(|s| s.parse().ok());

            match worker_pid {
                Some(pid) => {
                    if let Err(e) =
                        session.track_process(pid, aethershell_diagnostics::ProcessRole::LuaWorker)
                    {
                        eprintln!(
                            "Error: Failed to validate Lua worker process PID {}: {}",
                            pid, e
                        );
                        return ExitCode::FAILURE;
                    }
                }
                None => {
                    eprintln!("Error: WorkloadProfile::LuaWorkerActive requires a running 'aethershell-script-host.exe' via --pid-worker <PID>.");
                    return ExitCode::FAILURE;
                }
            }
        } else if let Some(worker_pid) = args
            .iter()
            .position(|a| a == "--pid-worker")
            .and_then(|p| args.get(p + 1))
            .and_then(|s| s.parse().ok())
        {
            if let Err(e) =
                session.track_process(worker_pid, aethershell_diagnostics::ProcessRole::LuaWorker)
            {
                eprintln!(
                    "Warning: Optional Lua worker process PID {} rejected: {}",
                    worker_pid, e
                );
            }
        }

        if let Err(e) = session.validate_required_processes() {
            eprintln!("Runtime benchmark process validation failed: {}", e);
            return ExitCode::FAILURE;
        }

        let ledger = match session.execute_full_procedure(
            repetitions,
            std::time::Duration::from_secs(warmup_secs),
            std::time::Duration::from_secs(duration_secs),
            std::time::Duration::from_millis(500),
        ) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Runtime benchmark error: {}", e);
                return ExitCode::FAILURE;
            }
        };

        let json = serde_json::to_string_pretty(&ledger).unwrap_or_default();
        println!("{}", json);

        if let Some(out_pos) = args.iter().position(|a| a == "--output") {
            if let Some(path) = args.get(out_pos + 1) {
                let _ = std::fs::write(path, &json);
                println!("Ledger written to {}", path);
            }
        }

        return match ledger.overall_verdict {
            aethershell_diagnostics::BenchmarkVerdict::Passed
            | aethershell_diagnostics::BenchmarkVerdict::PartiallyVerified { .. }
            | aethershell_diagnostics::BenchmarkVerdict::ProvisionalDiagnostic { .. } => {
                ExitCode::SUCCESS
            }
            _ => ExitCode::FAILURE,
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

fn print_usage() {
    println!(
        "AetherShell v{} — Unified Windows Taskbar and Desktop Widget Customization Engine",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("    aethershell.exe [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help               Print help information");
    println!("    -v, --version            Print version information");
    println!("    --safe-mode              Launch in Safe Mode (forces NativeNoopBackend, bypasses hooks)");
    println!("    --diagnostics            Generate and print sanitized JSON diagnostics report");
    println!("    --settings               Request active instance to open Settings window");
    println!("    --benchmark <SECS>       Run component-level microbenchmark (default: 60s)");
    println!("    --benchmark-runtime <PROFILE> [OPTIONS]");
    println!("                             Run genuine multi-process runtime benchmark");
    println!(
        "                               Profiles: minimal_idle, default_widgets, active_media,"
    );
    println!("                                         battery, gaming, settings_open, lua_worker_active,");
    println!("                                         mixed_monitors, monitor_off");
    println!("                               Flags:    --repetitions <N>, --duration <SECS>, --warmup <SECS>,");
    println!("                                         --pid-core <PID>, --pid-settings <PID>, --pid-worker <PID>,");
    println!(
        "                                         --formal, --allow-self-pid, --output <PATH>"
    );
}
