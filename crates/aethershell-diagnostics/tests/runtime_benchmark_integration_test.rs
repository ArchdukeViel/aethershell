use aethershell_diagnostics::{
    calculate_percentile, BenchmarkVerdict, BudgetClassification, EnvironmentMetadata,
    GpuMetricStatus, PresentationMetricStatus, ProcessRole, RuntimeBenchmarkSession,
    WakeupMetricStatus, WorkloadProfile,
};
use std::time::Duration;

#[test]
fn test_prd_section_14_budget_conformance() {
    // PRD Section 14.2 Resource Targets:
    // Minimal Idle: Ratified Target: Core <= 0.1% CPU, <= 30 MB RAM
    let idle_budget = WorkloadProfile::MinimalIdle.target_budget();
    assert_eq!(
        idle_budget.classification,
        BudgetClassification::RatifiedTarget
    );
    assert_eq!(idle_budget.max_core_cpu_pct, 0.1);
    assert_eq!(idle_budget.max_core_working_set_mb, 30.0);
    assert_eq!(idle_budget.max_combined_cpu_pct, 0.1);
    assert_eq!(idle_budget.max_combined_working_set_mb, 30.0);
    assert!(idle_budget.rationale.contains("Ratified PRD Section 14.2"));

    // Default Widgets: Provisional Proposal
    let widgets_budget = WorkloadProfile::DefaultWidgets.target_budget();
    assert_eq!(
        widgets_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(widgets_budget.max_core_cpu_pct, 0.5);
    assert_eq!(widgets_budget.max_core_working_set_mb, 45.0);

    // Active Media Visualization: Provisional Proposal
    let media_budget = WorkloadProfile::ActiveMediaVisualization.target_budget();
    assert_eq!(
        media_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(media_budget.max_core_cpu_pct, 1.5);
    assert_eq!(media_budget.max_core_working_set_mb, 60.0);

    // Battery Mode: Provisional Proposal
    let battery_budget = WorkloadProfile::BatteryMode.target_budget();
    assert_eq!(
        battery_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(battery_budget.max_core_cpu_pct, 0.2);
    assert_eq!(battery_budget.max_core_working_set_mb, 35.0);

    // Gaming Shield: Provisional Proposal
    let gaming_budget = WorkloadProfile::GamingShield.target_budget();
    assert_eq!(
        gaming_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(gaming_budget.max_core_cpu_pct, 0.05);
    assert_eq!(gaming_budget.max_core_working_set_mb, 30.0);

    // Settings Open: Provisional Proposal
    let settings_budget = WorkloadProfile::SettingsOpen.target_budget();
    assert_eq!(
        settings_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(settings_budget.max_combined_cpu_pct, 1.0);
    assert_eq!(settings_budget.max_combined_working_set_mb, 80.0);

    // Lua Worker Active: Provisional Proposal
    let lua_budget = WorkloadProfile::LuaWorkerActive.target_budget();
    assert_eq!(
        lua_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(lua_budget.max_combined_cpu_pct, 1.0);
    assert_eq!(lua_budget.max_combined_working_set_mb, 70.0);

    // Mixed Monitors: Provisional Proposal
    let monitors_budget = WorkloadProfile::MixedMonitors.target_budget();
    assert_eq!(
        monitors_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(monitors_budget.max_core_cpu_pct, 1.0);
    assert_eq!(monitors_budget.max_core_working_set_mb, 55.0);

    // Monitor Off / Session Locked: Provisional Proposal
    let lock_budget = WorkloadProfile::MonitorOffSessionLocked.target_budget();
    assert_eq!(
        lock_budget.classification,
        BudgetClassification::ProvisionalProposal
    );
    assert_eq!(lock_budget.max_core_cpu_pct, 0.05);
    assert_eq!(lock_budget.max_core_working_set_mb, 30.0);
    assert!(lock_budget.rationale.contains("Normal lock/sleep/resume"));
}

#[test]
fn test_environment_metadata_and_instrumentation_honesty() {
    let env = EnvironmentMetadata::detect();
    assert!(!env.os_family.is_empty());
    assert!(env.os_build > 0);
    assert!(env.logical_processor_count >= 1);
    assert!(env.total_physical_memory_mb > 100.0);
    assert!(!env.active_theme.is_empty());
    assert!(!env.power_source.is_empty());

    let session = RuntimeBenchmarkSession::new(WorkloadProfile::DefaultWidgets);
    let gpu_status = session.probe_gpu_metrics();
    match gpu_status {
        GpuMetricStatus::Unavailable { ref reason } => {
            assert!(
                !reason.is_empty(),
                "Unavailable GPU status must include an honest technical reason"
            );
        }
        GpuMetricStatus::Measured { usage_pct, .. } => {
            assert!((0.0..=100.0).contains(&usage_pct));
        }
    }

    let wakeup_status = session.probe_wakeup_metrics();
    assert!(matches!(
        wakeup_status,
        WakeupMetricStatus::Unavailable { .. }
    ));

    let presentation_status = session.probe_presentation_metrics();
    assert!(matches!(
        presentation_status,
        PresentationMetricStatus::Unavailable { .. }
    ));
}

#[test]
fn test_fail_closed_self_attachment_protection() {
    let current_pid = std::process::id();
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    // allow_self_pid is false by default
    let res = session.track_process(current_pid, ProcessRole::Core);
    assert!(
        res.is_err(),
        "Tracking current benchmark PID without allow_self must fail closed"
    );
    let err_str = res.unwrap_err();
    assert!(
        err_str.contains("SelfAttachmentDisallowed") || err_str.contains("ExecutableMismatch"),
        "Error message must cite SelfAttachmentDisallowed or ExecutableMismatch, got: {}",
        err_str
    );
}

#[test]
fn test_fail_closed_empty_sampler_rejection() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    let err = session
        .run_repetition(
            1,
            Duration::from_millis(10),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .unwrap_err();
    assert!(err.contains("No processes are registered"));
}

#[test]
fn test_fail_closed_missing_required_roles() {
    let current_pid = std::process::id();

    // 1. SettingsOpen missing Settings role
    let mut session_settings = RuntimeBenchmarkSession::new(WorkloadProfile::SettingsOpen);
    session_settings.set_allow_self_pid(true);
    session_settings
        .track_process(current_pid, ProcessRole::Core)
        .expect("track core");

    let err_settings = session_settings
        .run_repetition(
            1,
            Duration::from_millis(10),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .unwrap_err();
    assert!(
        err_settings.contains("Required process Settings (aethershell-settings.exe) is missing")
    );

    // 2. LuaWorkerActive missing LuaWorker role
    let mut session_lua = RuntimeBenchmarkSession::new(WorkloadProfile::LuaWorkerActive);
    session_lua.set_allow_self_pid(true);
    session_lua
        .track_process(current_pid, ProcessRole::Core)
        .expect("track core");

    let err_lua = session_lua
        .run_repetition(
            1,
            Duration::from_millis(10),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .unwrap_err();
    assert!(err_lua.contains("Required process LuaWorker (aethershell-script-host.exe) is missing"));
}

#[test]
fn test_fail_closed_executable_mismatch_without_override() {
    let mut child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
        .spawn()
        .expect("spawn powershell child");

    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    // allow_self_pid is false: powershell.exe must be rejected when claimed as Core (aethershell.exe)
    let res = session.track_process(child.id(), ProcessRole::Core);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Executable mismatch"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_multi_process_live_sampling_and_ledger_generation() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::LuaWorkerActive);
    session.set_allow_self_pid(true);

    let worker_bin = {
        let mut p = std::env::current_exe().unwrap();
        p.pop();
        if p.file_name().map(|n| n == "deps").unwrap_or(false) {
            p.pop();
        }
        let candidate = p.join("aethershell-script-host.exe");
        if candidate.is_file() {
            candidate
        } else {
            std::path::PathBuf::from(
                "target/x86_64-pc-windows-msvc/debug/aethershell-script-host.exe",
            )
        }
    };

    // Spawn a genuine child worker process to exercise multi-process sampling
    let mut child = std::process::Command::new(&worker_bin)
        .args(["--test-sleep-secs", "5"])
        .spawn()
        .expect("spawn test child process");

    let current_pid = std::process::id();
    let child_pid = child.id();

    session
        .track_process(current_pid, ProcessRole::Core)
        .expect("track core process");
    session
        .track_process(child_pid, ProcessRole::LuaWorker)
        .expect("track worker process");

    // Execute repetition with 50ms warmup, 200ms duration, 50ms interval
    let rep = session
        .run_repetition(
            1,
            Duration::from_millis(50),
            Duration::from_millis(200),
            Duration::from_millis(50),
        )
        .expect("run repetition");

    assert_eq!(rep.repetition_index, 1);
    assert_eq!(rep.process_metrics.len(), 2);

    for proc_m in &rep.process_metrics {
        assert!(proc_m.sample_count >= 2);
        assert!(
            proc_m.working_set_median_mb > 0.0,
            "Working set must be positive resident RAM"
        );
        assert!(
            proc_m.private_bytes_median_mb > 0.0,
            "Private commit charge must be positive"
        );
        assert!(
            proc_m.cpu_median_pct >= 0.0,
            "CPU usage must be non-negative"
        );
    }

    assert!(
        rep.combined_working_set_median_mb > 0.0,
        "Combined working set must be positive"
    );
    assert!(
        rep.combined_private_bytes_median_mb > 0.0,
        "Combined private bytes must be positive"
    );

    // Execute full 2-repetition procedure to generate complete ledger
    let ledger = session
        .execute_full_procedure(
            2,
            Duration::from_millis(50),
            Duration::from_millis(150),
            Duration::from_millis(50),
        )
        .expect("execute full procedure");

    assert_eq!(ledger.profile, WorkloadProfile::LuaWorkerActive);
    assert_eq!(ledger.repetitions.len(), 2);
    assert!(matches!(
        ledger.overall_verdict,
        BenchmarkVerdict::ProvisionalDiagnostic { .. }
            | BenchmarkVerdict::Passed
            | BenchmarkVerdict::PartiallyVerified { .. }
            | BenchmarkVerdict::Failed { .. }
    ));

    // Verify JSON serialization round-trip
    let json = serde_json::to_string_pretty(&ledger).expect("serialize ledger to JSON");
    assert!(json.contains("lua_worker_active"));
    assert!(json.contains("combined_working_set_median_mb"));
    assert!(json.contains("provisional_proposal"));

    let deserialized: aethershell_diagnostics::RuntimeBenchmarkLedger =
        serde_json::from_str(&json).expect("deserialize ledger from JSON");
    assert_eq!(deserialized.profile, ledger.profile);
    assert_eq!(deserialized.repetitions.len(), 2);

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_verdict_evaluation_within_budget() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);

    let budget = session.target_budget_mut();
    budget.max_combined_working_set_mb = 1000.0;
    budget.max_combined_cpu_pct = 100.0;
    budget.max_core_working_set_mb = 1000.0;
    budget.max_core_cpu_pct = 100.0;

    let current_pid = std::process::id();
    session
        .track_process(current_pid, ProcessRole::Core)
        .expect("track core");

    let rep = session
        .run_repetition(
            1,
            Duration::from_millis(10),
            Duration::from_millis(60),
            Duration::from_millis(30),
        )
        .expect("run repetition");

    // GPU, wakeups, or presentation are unavailable, so honest verdict must be PartiallyVerified
    assert!(matches!(
        rep.verdict,
        BenchmarkVerdict::PartiallyVerified { .. }
    ));
}

#[test]
fn test_process_anomaly_exit_detection() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);

    // Track current process as Core
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    // Spawn a quick process that terminates immediately
    let mut quick_child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Exit 0"])
        .spawn()
        .expect("spawn quick child");

    let child_pid = quick_child.id();
    // Wait for it to terminate
    let _ = quick_child.wait();

    // Opening a terminated process must be rejected with NotActive
    let res =
        aethershell_diagnostics::validate_process_identity(child_pid, ProcessRole::Auxiliary, true);
    assert!(matches!(
        res,
        Err(aethershell_diagnostics::ProcessValidationError::NotActive { .. })
    ));
}

#[test]
fn test_percentile_sorting_edge_cases() {
    assert_eq!(calculate_percentile(&[], 50.0), 0.0);
    assert_eq!(calculate_percentile(&[42.0], 50.0), 42.0);
    assert_eq!(calculate_percentile(&[42.0], 95.0), 42.0);

    let data = [5.0, 1.0, 4.0, 2.0, 3.0];
    assert_eq!(calculate_percentile(&data, 50.0), 3.0);
    assert_eq!(calculate_percentile(&data, 0.0), 1.0);
    assert_eq!(calculate_percentile(&data, 100.0), 5.0);
}

#[test]
fn test_zero_repetitions_rejected_fail_closed() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    let res = session.execute_full_procedure(
        0,
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::from_millis(10),
    );
    assert!(res.is_err(), "Zero repetitions must fail closed");
    let err = res.unwrap_err();
    assert!(
        err.contains("repetitions must be >= 1"),
        "Error should explain minimum repetition requirement: {}",
        err
    );
}

#[test]
fn test_zero_duration_rejected_fail_closed() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    let res = session.execute_full_procedure(
        1,
        Duration::from_millis(10),
        Duration::ZERO,
        Duration::from_millis(10),
    );
    assert!(res.is_err(), "Zero duration must fail closed");
    let err = res.unwrap_err();
    assert!(
        err.contains("recording duration must be positive"),
        "Error should explain positive duration requirement: {}",
        err
    );
}

#[test]
fn test_invalid_sample_interval_rejected_fail_closed() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    // Sample interval = 0
    let res1 = session.execute_full_procedure(
        1,
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::ZERO,
    );
    assert!(res1.is_err(), "Zero sample interval must fail closed");

    // Sample interval > recording duration
    let res2 = session.execute_full_procedure(
        1,
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::from_millis(100),
    );
    assert!(res2.is_err(), "Sample interval > duration must fail closed");
    let err = res2.unwrap_err();
    assert!(
        err.contains("cannot exceed recording duration"),
        "Error detail: {}",
        err
    );
}

#[test]
fn test_self_override_strictly_scoped_to_current_pid_and_rejects_external_pids() {
    let mut child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
        .spawn()
        .expect("spawn test child");

    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    // Even when allow_self_pid is true:
    session.set_allow_self_pid(true);

    // Tracking external PID as Core (which expects aethershell.exe) MUST fail with ExecutableMismatch
    let res_external = session.track_process(child.id(), ProcessRole::Core);
    assert!(
        res_external.is_err(),
        "allow_self_pid must NOT bypass executable name validation for external PIDs"
    );
    let err = res_external.unwrap_err();
    assert!(
        err.contains("Executable mismatch"),
        "Expected executable mismatch, got: {}",
        err
    );

    // But tracking the current process itself (which is std::process::id()) succeeds
    let res_self = session.track_process(std::process::id(), ProcessRole::Core);
    assert!(
        res_self.is_ok(),
        "allow_self_pid must permit attaching to std::process::id() for simulation tests"
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_diagnostic_simulation_labeling_on_self_override() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    let ledger = session
        .execute_full_procedure(
            1,
            Duration::from_millis(10),
            Duration::from_millis(50),
            Duration::from_millis(25),
        )
        .expect("execute procedure");

    assert_eq!(
        ledger.execution_mode,
        aethershell_diagnostics::BenchmarkExecutionMode::DiagnosticSimulation,
        "Ledger must be explicitly marked as DiagnosticSimulation when self-PID override is active"
    );
    assert!(
        ledger
            .notes
            .iter()
            .any(|n| n.contains("DIAGNOSTIC SIMULATION")),
        "Ledger notes must include explicit DIAGNOSTIC SIMULATION disclosure"
    );
}

#[test]
fn test_formal_qualification_enforcements() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session.set_formal_qualification(true);

    // 1. Self PID rejected for formal qualification
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    let res_self = session.execute_full_procedure(
        5,
        Duration::from_secs(5),
        Duration::from_secs(60),
        Duration::from_millis(500),
    );
    assert!(
        res_self.is_err(),
        "Formal qualification must reject self-PID attachment"
    );
    assert!(res_self.unwrap_err().contains("self-PID attachment"));

    // 2. Reject < 5 repetitions
    let mut session2 = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session2.set_formal_qualification(true);
    let res_reps = session2.execute_full_procedure(
        4,
        Duration::from_secs(5),
        Duration::from_secs(60),
        Duration::from_millis(500),
    );
    assert!(
        res_reps.is_err(),
        "Formal qualification must require at least 5 repetitions"
    );
    assert!(res_reps.unwrap_err().contains("at least 5"));

    // 3. Reject < 60s recording duration
    let mut session3 = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
    session3.set_formal_qualification(true);
    let res_dur = session3.execute_full_procedure(
        5,
        Duration::from_secs(5),
        Duration::from_secs(30),
        Duration::from_millis(500),
    );
    assert!(
        res_dur.is_err(),
        "Formal qualification must require duration >= 60s"
    );
    assert!(res_dur.unwrap_err().contains(">= 60 seconds"));

    // 4. Reject provisional profile for formal qualification
    let mut session4 = RuntimeBenchmarkSession::new(WorkloadProfile::DefaultWidgets);
    session4.set_formal_qualification(true);
    let res_prof = session4.execute_full_procedure(
        5,
        Duration::from_secs(5),
        Duration::from_secs(60),
        Duration::from_millis(500),
    );
    assert!(
        res_prof.is_err(),
        "Formal qualification must reject provisional profiles"
    );
    assert!(res_prof
        .unwrap_err()
        .contains("only applies to ratified target budgets"));
}

#[test]
fn test_provisional_profile_verdict_preserves_distinction() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::DefaultWidgets);
    session.set_allow_self_pid(true);
    session
        .track_process(std::process::id(), ProcessRole::Core)
        .expect("track core");

    let ledger = session
        .execute_full_procedure(
            1,
            Duration::from_millis(10),
            Duration::from_millis(50),
            Duration::from_millis(25),
        )
        .expect("execute procedure");

    assert!(
        matches!(
            ledger.overall_verdict,
            BenchmarkVerdict::ProvisionalDiagnostic { .. }
        ),
        "Provisional profile must produce ProvisionalDiagnostic verdict, not hard Pass/Fail: {:?}",
        ledger.overall_verdict
    );
}

#[test]
fn test_deserialize_ryzen5_5500u_benchmark_ledger_json() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ledger_path =
        manifest_dir.join("../../docs/qualification/ryzen5_5500u_benchmark_ledger.json");
    if !ledger_path.exists() {
        return;
    }
    let content =
        std::fs::read_to_string(&ledger_path).expect("read ryzen5_5500u_benchmark_ledger.json");
    let ledger: aethershell_diagnostics::RuntimeBenchmarkLedger =
        serde_json::from_str(&content).expect("deserialize ryzen5_5500u_benchmark_ledger.json");

    assert_eq!(ledger.profile, WorkloadProfile::MinimalIdle);
    assert_eq!(
        ledger.target_budget.classification,
        BudgetClassification::RatifiedTarget
    );
    assert_eq!(ledger.target_budget.max_core_working_set_mb, 30.0);
    assert_eq!(ledger.target_budget.max_core_cpu_pct, 0.1);
    assert_eq!(ledger.repetitions.len(), 5);
    for rep in &ledger.repetitions {
        assert!(matches!(
            rep.gpu_status,
            aethershell_diagnostics::GpuMetricStatus::Unavailable { .. }
        ));
        assert!(matches!(
            rep.wakeup_status,
            aethershell_diagnostics::WakeupMetricStatus::Unavailable { .. }
        ));
        assert!(matches!(
            rep.presentation_status,
            aethershell_diagnostics::PresentationMetricStatus::Unavailable { .. }
        ));
    }
}
