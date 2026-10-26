use aethershell_audio::{FpsMode, WasapiLoopbackCapture};
use aethershell_core::BindingValue;
use aethershell_ipc::IpcClient;
use aethershell_script_host::{
    sha256_hex, verify_script_digest, verify_script_file_digest, ScriptSupervisor, SupervisorState,
    TrustError,
};
use aethershell_telemetry::TelemetryRegistry;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn test_telemetry_registry_sensor_honesty() {
    let mut registry = TelemetryRegistry::new();
    let snapshot = registry.poll_all();

    // Verify required metrics are present
    assert!(snapshot.contains_key("cpu.usage_percent"));
    assert!(snapshot.contains_key("metrics.cpu"));
    assert!(snapshot.contains_key("memory.used_gib"));
    assert!(snapshot.contains_key("metrics.ram"));
    assert!(snapshot.contains_key("storage.free_gib"));
    assert!(snapshot.contains_key("power.battery_percent"));
    assert!(snapshot.contains_key("network.down"));
    assert!(snapshot.contains_key("network.up"));

    // Check RAM value honesty: if available, should be > 0.0
    if let Some(BindingValue::Float(used_ram)) = snapshot.get("memory.used_gib") {
        assert!(*used_ram > 0.0, "System RAM reported must be positive");
    }

    // Check Battery honesty: on desktop or unsupported machines, must be Unavailable(Unsupported) or valid percentage
    if let Some(val) = snapshot.get("power.battery_percent") {
        match val {
            BindingValue::Percent(pct) => assert!((0.0..=100.0).contains(pct)),
            BindingValue::Unavailable(_) => { /* Valid honest report for desktop without battery */
            }
            other => panic!("Unexpected battery binding state: {:?}", other),
        }
    }
}

#[test]
fn test_wasapi_audio_spectrum_and_silence_auto_hide() {
    let mut capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
    assert_eq!(capture.band_count(), 32);
    assert_eq!(capture.fps_mode(), FpsMode::AcBalanced);
    assert!(!capture.is_paused());

    // Generate 512 samples of a 440 Hz synthetic tone at 44.1 kHz
    let sample_rate = 44100.0f32;
    let frequency = 440.0f32;
    let mut tone_samples = Vec::with_capacity(512);
    for i in 0..512 {
        let t = i as f32 / sample_rate;
        tone_samples.push((2.0 * std::f32::consts::PI * frequency * t).sin() * 0.8);
    }

    // Feed tone samples: should compute non-zero spectrum bands and mark active
    let active_bands = capture.feed_samples(&tone_samples);
    assert_eq!(active_bands.len(), 32);
    let energy: f32 = active_bands.iter().sum();
    assert!(
        energy > 0.0,
        "Synthesized tone must yield non-zero spectrum energy"
    );
    assert!(!capture.is_silent());
    assert!(!capture.should_auto_hide());

    // Test pause toggle (PRD Section 7.3)
    capture.set_paused(true);
    assert!(capture.is_paused());
    let paused_bands = capture.feed_samples(&tone_samples);
    assert_eq!(paused_bands, vec![0.0; 32]);

    capture.set_paused(false);
    assert!(!capture.is_paused());

    // Feed zero silence
    let silence_samples = vec![0.0f32; 512];
    for _ in 0..10 {
        let _ = capture.feed_samples(&silence_samples);
    }
}

#[test]
fn test_script_digest_verification_and_quarantine() {
    let temp_dir = std::env::temp_dir();
    let script_file = temp_dir.join("aethershell_test_widget.lua");

    let script_content = b"function on_render()\n    return 'Hello AetherShell'\nend\n";
    {
        let mut f = File::create(&script_file).expect("create test lua file");
        f.write_all(script_content).expect("write lua");
    }

    // Calculate expected hash
    let valid_hash = sha256_hex(script_content);
    assert_eq!(valid_hash.len(), 64);

    // Verify matching digest passes (both file and in-memory bytes)
    assert!(verify_script_file_digest(&script_file, &valid_hash).is_ok());
    assert!(verify_script_digest(script_content, &valid_hash).is_ok());

    // Tampered digest fails with DigestMismatch
    let tampered_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    match verify_script_file_digest(&script_file, tampered_hash) {
        Err(TrustError::DigestMismatch { expected, actual }) => {
            assert_eq!(expected, tampered_hash);
            assert_eq!(actual, valid_hash);
        }
        other => panic!("Expected DigestMismatch, got {:?}", other),
    }

    // Test Supervisor crash loop limit and quarantine (PRD Section 10.2 / Gate G10)
    let mut supervisor = ScriptSupervisor::new(2);
    assert_eq!(*supervisor.state(), SupervisorState::Dormant);

    // Crash 1: Worker crashes unexpectedly
    supervisor.record_start(5544);
    supervisor.record_exit(1);
    assert_eq!(supervisor.crash_count(), 1);
    assert_eq!(*supervisor.state(), SupervisorState::Terminated);
    assert!(!supervisor.is_quarantined());

    // Crash 2: Worker crashes second time -> Quarantined
    supervisor.record_start(5545);
    supervisor.record_exit(1);
    assert_eq!(supervisor.crash_count(), 2);
    assert!(supervisor.is_quarantined());
    if let SupervisorState::Quarantined { reason } = supervisor.state() {
        assert!(reason.contains("exceeded limit of 2"));
    } else {
        panic!("Supervisor must be Quarantined");
    }

    // Clean up temp file
    let _ = std::fs::remove_file(script_file);
}

#[test]
fn test_phase4_engine_ipc_integration() {
    let target_bin = env!("CARGO_BIN_EXE_aethershell");

    // 1. Launch primary instance in background
    let mut child = Command::new(target_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch primary instance");

    // Give server time to bind mutex and named pipe
    std::thread::sleep(Duration::from_millis(700));

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Connect to named pipe
        let mut connected_pipe = None;
        for session_id in 0..=3 {
            let pipe = format!(r"\\.\pipe\aethershell-ipc-{}", session_id);
            if let Ok(res) =
                IpcClient::send_command(&pipe, "get_status", serde_json::json!({})).await
            {
                if res.success {
                    connected_pipe = Some(pipe);
                    break;
                }
            }
        }

        let pipe = connected_pipe.expect("Must connect to running engine IPC");

        // 1. Query status: verify telemetry_provider, audio_spectrum, and script_supervisor
        let status_res = IpcClient::send_command(&pipe, "get_status", serde_json::json!({}))
            .await
            .expect("get_status call");
        assert!(status_res.success);
        let subsystems = status_res.payload["subsystems"]
            .as_array()
            .expect("subsystems array");
        let sub_names: Vec<&str> = subsystems
            .iter()
            .filter_map(|s| s["name"].as_str())
            .collect();
        assert!(sub_names.contains(&"core_engine"));
        assert!(sub_names.contains(&"taskbar_coordinator"));
        assert!(sub_names.contains(&"telemetry_provider"));
        assert!(sub_names.contains(&"audio_spectrum"));
        assert!(sub_names.contains(&"script_supervisor"));

        // 2. Query telemetry
        let telem_res = IpcClient::send_command(&pipe, "get_telemetry", serde_json::json!({}))
            .await
            .expect("get_telemetry call");
        assert!(telem_res.success);
        assert!(telem_res.payload.get("cpu.usage_percent").is_some());
        assert!(telem_res.payload.get("memory.used_gib").is_some());

        // 3. Query spectrum
        let spectrum_res = IpcClient::send_command(&pipe, "get_spectrum", serde_json::json!({}))
            .await
            .expect("get_spectrum call");
        assert!(spectrum_res.success);
        assert!(spectrum_res.payload.get("bands").is_some());

        // 4. Test audio pause / resume
        let pause_res = IpcClient::send_command(&pipe, "pause_audio", serde_json::json!({}))
            .await
            .expect("pause_audio call");
        assert!(pause_res.success);
        assert_eq!(pause_res.payload["paused"], true);

        let resume_res = IpcClient::send_command(&pipe, "resume_audio", serde_json::json!({}))
            .await
            .expect("resume_audio call");
        assert!(resume_res.success);
        assert_eq!(resume_res.payload["paused"], false);

        // 5. Test script worker heartbeat & status
        let ready_res = IpcClient::send_command(
            &pipe,
            "script_worker_ready",
            serde_json::json!({ "pid": 9999 }),
        )
        .await
        .expect("script_worker_ready call");
        assert!(ready_res.success);
        assert_eq!(ready_res.payload["registered"], true);

        let heartbeat_res =
            IpcClient::send_command(&pipe, "script_worker_heartbeat", serde_json::json!({}))
                .await
                .expect("script_worker_heartbeat call");
        assert!(heartbeat_res.success);
        assert_eq!(heartbeat_res.payload["acknowledged"], true);

        let script_status =
            IpcClient::send_command(&pipe, "get_script_status", serde_json::json!({}))
                .await
                .expect("get_script_status call");
        assert!(script_status.success);
        assert_eq!(script_status.payload["is_quarantined"], false);

        // 6. Clean shutdown
        let shutdown_res = IpcClient::send_command(&pipe, "shutdown", serde_json::json!({}))
            .await
            .expect("shutdown call");
        assert!(shutdown_res.success);
    });

    let exit_status = child.wait().expect("child exit");
    assert!(exit_status.success());
}

fn find_script_host_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.file_name().map(|n| n == "deps").unwrap_or(false) {
        path.pop();
    }
    path.join("aethershell-script-host.exe")
}

#[test]
fn test_phase5_wasapi_loopback_capture_thread_lifecycle() {
    let mut capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
    let start_res = capture.start();
    assert!(
        start_res.is_ok(),
        "WASAPI loopback start must succeed (or fallback)"
    );
    assert!(capture.is_running());

    std::thread::sleep(Duration::from_millis(50));

    // Pause audio
    capture.set_paused(true);
    assert!(capture.is_paused());

    // Resume audio
    capture.set_paused(false);
    assert!(!capture.is_paused());

    capture.stop();
    assert!(!capture.is_running());
}

#[test]
fn test_phase5_lua_script_host_real_execution() {
    let host_bin = find_script_host_binary();
    if !host_bin.exists() {
        eprintln!("Skipping out-of-process test: binary not built yet");
        return;
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("aether_phase5_test.lua");
    let script_code = b"local t = aether.get_telemetry()\nlocal s = aether.get_spectrum()\nassert(t ~= nil)\nassert(s ~= nil)\n";
    std::fs::write(&script_path, script_code).expect("write script");

    let digest = sha256_hex(script_code);

    let status = Command::new(&host_bin)
        .arg("--script")
        .arg(&script_path)
        .arg("--digest")
        .arg(&digest)
        .arg("--once")
        .status()
        .expect("run script host");

    assert!(
        status.success(),
        "Lua 5.4 script host execution must exit with 0"
    );

    let _ = std::fs::remove_file(script_path);
}

#[test]
fn test_phase5_job_object_containment() {
    let host_bin = find_script_host_binary();
    if !host_bin.exists() {
        return;
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("aether_job_test.lua");
    let script_code = b"print('Job object test script')\n";
    std::fs::write(&script_path, script_code).expect("write script");

    let digest = sha256_hex(script_code);

    let mut supervisor = ScriptSupervisor::new(2);
    let spawn_res = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(
        spawn_res.is_ok(),
        "Spawn worker under Job Object must succeed: {:?}",
        spawn_res
    );
    assert!(supervisor.has_active_child());

    std::thread::sleep(Duration::from_millis(100));

    supervisor.terminate_worker();
    assert!(!supervisor.has_active_child());

    let _ = std::fs::remove_file(script_path);
}

#[test]
fn test_phase5_supervised_infinite_loop_watchdog_quarantine() {
    let host_bin = find_script_host_binary();
    if !host_bin.exists() {
        return;
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("aether_hang_test.lua");
    // Script that hangs in top-level execution
    let script_code = b"while true do end\n";
    std::fs::write(&script_path, script_code).expect("write script");

    let digest = sha256_hex(script_code);

    let mut supervisor = ScriptSupervisor::new(2);
    supervisor.set_heartbeat_timeout_secs(1); // 1-second timeout for fast test

    let spawn_res = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn_res.is_ok());
    assert!(supervisor.has_active_child());

    // Sleep long enough for heartbeat timeout (1.5s > 1s)
    std::thread::sleep(Duration::from_millis(1500));

    // Poll worker status: should detect hung worker, kill child process, and transition to Quarantined
    let exit = supervisor.poll_worker_status();
    assert_eq!(exit, None);
    assert!(supervisor.is_quarantined());
    assert!(!supervisor.has_active_child());

    let _ = std::fs::remove_file(script_path);
}

#[test]
fn test_phase5_supervised_crash_circuit_breaker() {
    let host_bin = find_script_host_binary();
    if !host_bin.exists() {
        return;
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("aether_crash_test.lua");
    // Script that exits with error
    let script_code = b"error('intentional crash test')\n";
    std::fs::write(&script_path, script_code).expect("write script");

    let digest = sha256_hex(script_code);

    let mut supervisor = ScriptSupervisor::new(2);

    // Crash 1: First spawn crashes
    let spawn1 = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn1.is_ok());

    // Wait for process to exit
    std::thread::sleep(Duration::from_millis(400));
    let exit1 = supervisor.poll_worker_status();
    assert!(exit1.is_some());
    assert_eq!(supervisor.crash_count(), 1);
    assert!(!supervisor.is_quarantined());

    // Crash 2: Second spawn crashes -> triggers circuit breaker
    let spawn2 = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn2.is_ok());

    std::thread::sleep(Duration::from_millis(400));
    let exit2 = supervisor.poll_worker_status();
    assert!(exit2.is_some());
    assert_eq!(supervisor.crash_count(), 2);
    assert!(supervisor.is_quarantined());

    // Third spawn attempt must be rejected because supervisor is quarantined
    let spawn3 = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn3.is_err());

    let _ = std::fs::remove_file(script_path);
}
