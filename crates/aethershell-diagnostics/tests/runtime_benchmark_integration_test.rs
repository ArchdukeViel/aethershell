use aethershell_diagnostics::{
    calculate_percentile, BenchmarkVerdict, EnvironmentMetadata, GpuMetricStatus, ProcessRole,
    RuntimeBenchmarkSession, WorkloadProfile,
};
use std::time::Duration;

#[test]
fn test_prd_section_14_budget_conformance() {
    // PRD Section 14.2 Resource Targets:
    // Minimal Idle: Core <= 0.1% CPU, <= 30 MB RAM
    let idle_budget = WorkloadProfile::MinimalIdle.target_budget();
    assert_eq!(idle_budget.max_core_cpu_pct, 0.1);
    assert_eq!(idle_budget.max_core_working_set_mb, 30.0);
    assert_eq!(idle_budget.max_combined_cpu_pct, 0.1);
    assert_eq!(idle_budget.max_combined_working_set_mb, 30.0);

    // Default Widgets: Core <= 0.5% CPU, <= 45 MB RAM
    let widgets_budget = WorkloadProfile::DefaultWidgets.target_budget();
    assert_eq!(widgets_budget.max_core_cpu_pct, 0.5);
    assert_eq!(widgets_budget.max_core_working_set_mb, 45.0);
    assert_eq!(widgets_budget.max_combined_cpu_pct, 0.5);
    assert_eq!(widgets_budget.max_combined_working_set_mb, 45.0);

    // Active Media Visualization: Core <= 1.5% CPU, <= 60 MB RAM
    let media_budget = WorkloadProfile::ActiveMediaVisualization.target_budget();
    assert_eq!(media_budget.max_core_cpu_pct, 1.5);
    assert_eq!(media_budget.max_core_working_set_mb, 60.0);
    assert_eq!(media_budget.max_combined_cpu_pct, 1.5);
    assert_eq!(media_budget.max_combined_working_set_mb, 60.0);

    // Battery Mode: Core <= 0.2% CPU, <= 35 MB RAM
    let battery_budget = WorkloadProfile::BatteryMode.target_budget();
    assert_eq!(battery_budget.max_core_cpu_pct, 0.2);
    assert_eq!(battery_budget.max_core_working_set_mb, 35.0);
    assert_eq!(battery_budget.max_combined_cpu_pct, 0.2);
    assert_eq!(battery_budget.max_combined_working_set_mb, 35.0);

    // Gaming Shield: Core <= 0.05% CPU, <= 30 MB RAM
    let gaming_budget = WorkloadProfile::GamingShield.target_budget();
    assert_eq!(gaming_budget.max_core_cpu_pct, 0.05);
    assert_eq!(gaming_budget.max_core_working_set_mb, 30.0);
    assert_eq!(gaming_budget.max_combined_cpu_pct, 0.05);
    assert_eq!(gaming_budget.max_combined_working_set_mb, 30.0);

    // Settings Open: Core + Settings <= 1.0% CPU, <= 80 MB combined RAM
    let settings_budget = WorkloadProfile::SettingsOpen.target_budget();
    assert_eq!(settings_budget.max_combined_cpu_pct, 1.0);
    assert_eq!(settings_budget.max_combined_working_set_mb, 80.0);

    // Lua Worker Active: Core + Worker <= 1.0% CPU, <= 70 MB combined RAM
    let lua_budget = WorkloadProfile::LuaWorkerActive.target_budget();
    assert_eq!(lua_budget.max_combined_cpu_pct, 1.0);
    assert_eq!(lua_budget.max_combined_working_set_mb, 70.0);

    // Mixed Monitors: Core <= 1.0% CPU, <= 55 MB RAM
    let monitors_budget = WorkloadProfile::MixedMonitors.target_budget();
    assert_eq!(monitors_budget.max_core_cpu_pct, 1.0);
    assert_eq!(monitors_budget.max_core_working_set_mb, 55.0);
    assert_eq!(monitors_budget.max_combined_cpu_pct, 1.0);
    assert_eq!(monitors_budget.max_combined_working_set_mb, 55.0);
}

#[test]
fn test_environment_metadata_and_gpu_honesty() {
    let env = EnvironmentMetadata::detect();
    assert!(!env.os_family.is_empty());
    assert!(env.os_build > 0);
    assert!(env.logical_processor_count >= 1);
    assert!(env.total_physical_memory_mb > 100.0);

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
            assert!(
                (0.0..=100.0).contains(&usage_pct),
                "Measured GPU percentage must be within 0..=100"
            );
        }
    }
}

#[test]
fn test_multi_process_live_sampling_and_ledger_generation() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::LuaWorkerActive);

    // Spawn a lightweight child worker process to exercise genuine multi-process sampling
    let mut child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
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
        "Combined working set must sum process resident working sets"
    );
    assert!(
        rep.combined_private_bytes_median_mb > 0.0,
        "Combined private bytes must sum process commit charges"
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
    println!(
        "Executed ledger overall verdict: {:?}",
        ledger.overall_verdict
    );
    assert!(matches!(
        ledger.overall_verdict,
        BenchmarkVerdict::Passed
            | BenchmarkVerdict::PartiallyVerified { .. }
            | BenchmarkVerdict::Failed { .. }
    ));

    // Verify JSON serialization round-trip
    let json = serde_json::to_string_pretty(&ledger).expect("serialize ledger to JSON");
    assert!(json.contains("lua_worker_active"));
    assert!(json.contains("combined_working_set_median_mb"));

    let deserialized: aethershell_diagnostics::RuntimeBenchmarkLedger =
        serde_json::from_str(&json).expect("deserialize ledger from JSON");
    assert_eq!(deserialized.profile, ledger.profile);
    assert_eq!(deserialized.repetitions.len(), 2);

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_verdict_evaluation_within_budget() {
    let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::SettingsOpen);
    // Overwrite target budget to large threshold to test passing evaluation
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

    // Since GPU is unavailable in un-elevated context, honest verdict must be PartiallyVerified
    assert!(matches!(
        rep.verdict,
        BenchmarkVerdict::PartiallyVerified { .. }
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
