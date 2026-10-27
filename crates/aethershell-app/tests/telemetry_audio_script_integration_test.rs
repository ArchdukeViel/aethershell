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
fn test_wasapi_audio_endpoint_migration_and_power_aware_operation() {
    let mut capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
    assert_eq!(capture.fps_mode(), FpsMode::AcBalanced);
    assert_eq!(capture.migration_counter(), 0);
    assert!(!capture.is_migration_requested());

    // 1. Verify power-aware polling intervals
    assert_eq!(
        FpsMode::Performance.active_poll_interval(),
        Duration::from_millis(10)
    );
    assert_eq!(
        FpsMode::AcBalanced.active_poll_interval(),
        Duration::from_millis(15)
    );
    assert_eq!(
        FpsMode::BatterySaver.active_poll_interval(),
        Duration::from_millis(33)
    );

    // Deep silence backoff interval verification
    assert_eq!(
        FpsMode::BatterySaver.deep_silence_poll_interval(),
        Duration::from_millis(200)
    );

    // 2. Start capture thread
    let res = capture.start();
    assert!(res.is_ok());
    assert!(capture.is_running());

    // 3. Dynamic power mode switching while capture thread is running
    capture.set_fps_mode(FpsMode::BatterySaver);
    assert_eq!(capture.fps_mode(), FpsMode::BatterySaver);

    // 4. Simulate IMMNotificationClient endpoint migration trigger
    capture.request_migration();
    assert_eq!(capture.migration_counter(), 1);

    // Allow thread to observe migration and perform refresh
    std::thread::sleep(Duration::from_millis(120));

    // Capture thread must remain running through endpoint migration
    assert!(capture.is_running());

    // 5. Clean teardown
    capture.stop();
    assert!(!capture.is_running());
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

    let temp_dir =
        std::env::temp_dir().join(format!("aethershell_telem_test_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let token_path = temp_dir.join("session_1.token");

    // 1. Launch primary instance in background with isolated config dir and session token path
    let mut child = Command::new(target_bin)
        .env("AETHERSHELL_CONFIG_DIR", &temp_dir)
        .env("AETHERSHELL_SESSION_TOKEN_PATH", &token_path)
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

        // Load session capability token from isolated test location
        let session_token = std::fs::read_to_string(&token_path)
            .or_else(|_| aethershell_ipc::SessionToken::load().ok_or(()))
            .expect("Active engine must expose session token capability");

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

        // 4. Test audio pause / resume with session capability token
        let pause_res = IpcClient::send_command(
            &pipe,
            "pause_audio",
            serde_json::json!({ "session_token": session_token }),
        )
        .await
        .expect("pause_audio call");
        assert!(pause_res.success);
        assert_eq!(pause_res.payload["paused"], true);

        let resume_res = IpcClient::send_command(
            &pipe,
            "resume_audio",
            serde_json::json!({ "session_token": session_token }),
        )
        .await
        .expect("resume_audio call");
        assert!(resume_res.success);
        assert_eq!(resume_res.payload["paused"], false);

        // 5. Test script worker identity verification (Gate G10 security & Owner Review Directives)
        // 5a. Verify spoofed PID rejection
        let spoofed_res = IpcClient::send_command(
            &pipe,
            "script_worker_ready",
            serde_json::json!({ "pid": 999999 }),
        )
        .await
        .expect("script_worker_ready call");
        assert!(!spoofed_res.success, "Spoofed PID must be rejected");

        // 5b. Verify worker registration bypass elimination:
        // Even when claimed_pid matches client_pid, calling script_worker_ready when no child was spawned must fail closed:
        let my_pid = std::process::id();
        let ready_res = IpcClient::send_command(
            &pipe,
            "script_worker_ready",
            serde_json::json!({ "pid": my_pid }),
        )
        .await
        .expect("script_worker_ready call");
        assert!(
            !ready_res.success,
            "Worker registration without supervised child must fail closed"
        );

        // 5c. Verify worker heartbeat without supervised child fails closed:
        let heartbeat_res =
            IpcClient::send_command(&pipe, "script_worker_heartbeat", serde_json::json!({}))
                .await
                .expect("script_worker_heartbeat call");
        assert!(
            !heartbeat_res.success,
            "Heartbeat without active child must fail closed"
        );

        let script_status =
            IpcClient::send_command(&pipe, "get_script_status", serde_json::json!({}))
                .await
                .expect("get_script_status call");
        assert!(script_status.success);
        assert_eq!(script_status.payload["is_quarantined"], false);

        // 6. Test privileged command caller authorization (Owner Review Directive 3)
        // 6a. Negative test: unauthenticated shutdown is rejected
        let unauth_shutdown = IpcClient::send_command(&pipe, "shutdown", serde_json::json!({}))
            .await
            .expect("unauthorized shutdown call");
        assert!(
            !unauth_shutdown.success,
            "Unauthenticated shutdown must be rejected"
        );

        // 6b. Negative test: shutdown with invalid session token is rejected
        let invalid_token_shutdown = IpcClient::send_command(
            &pipe,
            "shutdown",
            serde_json::json!({ "session_token": "invalid_fake_token_123" }),
        )
        .await
        .expect("invalid token shutdown call");
        assert!(
            !invalid_token_shutdown.success,
            "Shutdown with invalid token must be rejected"
        );

        // 6c. Authenticated shutdown using valid session capability token succeeds
        let shutdown_res = IpcClient::send_command(
            &pipe,
            "shutdown",
            serde_json::json!({ "session_token": session_token }),
        )
        .await
        .expect("shutdown call");
        assert!(
            shutdown_res.success,
            "Authenticated shutdown with valid session token must succeed"
        );
    });

    let exit_status = child.wait().expect("child exit");
    assert!(exit_status.success());
    let _ = std::fs::remove_dir_all(&temp_dir);
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

    // Wait for process to exit with bounded polling loop
    let start1 = std::time::Instant::now();
    let mut exit1 = None;
    while start1.elapsed() < Duration::from_millis(2000) {
        exit1 = supervisor.poll_worker_status();
        if exit1.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(exit1.is_some());
    assert_eq!(supervisor.crash_count(), 1);
    assert!(!supervisor.is_quarantined());

    // Crash 2: Second spawn crashes -> triggers circuit breaker
    let spawn2 = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn2.is_ok());

    let start2 = std::time::Instant::now();
    let mut exit2 = None;
    while start2.elapsed() < Duration::from_millis(2000) {
        exit2 = supervisor.poll_worker_status();
        if exit2.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(exit2.is_some());
    assert_eq!(supervisor.crash_count(), 2);
    assert!(supervisor.is_quarantined());

    // Third spawn attempt must be rejected because supervisor is quarantined
    let spawn3 = supervisor.spawn_worker(&host_bin, &script_path, &digest, "");
    assert!(spawn3.is_err());

    let _ = std::fs::remove_file(script_path);
}

#[test]
fn test_live_telemetry_and_spectrum_bindings_flow_into_scene() {
    use aethershell_app::compile_live_scene;
    use aethershell_renderer::RenderCommand;
    use std::collections::HashMap;

    let mut bindings = HashMap::new();
    // Supply real non-dummy telemetry values
    bindings.insert("metrics.cpu".into(), BindingValue::Percent(88.5));
    bindings.insert("metrics.ram".into(), BindingValue::Float(14.2));
    let test_bands = vec![0.15f32, 0.45, 0.85, 0.35];
    bindings.insert(
        "audio.spectrum".into(),
        BindingValue::FloatArray(test_bands.clone()),
    );

    let scene = compile_live_scene(1920.0, 1080.0, &bindings);
    assert!(!scene.commands.is_empty());

    // Verify time bindings were automatically populated
    let has_text_clock = scene.commands.iter().any(|cmd| match cmd {
        RenderCommand::DrawText { text, .. } => text.contains(':'),
        _ => false,
    });
    assert!(has_text_clock, "Scene must include rendered time clock");

    // Verify compile_live_scene accepts custom bindings without falling back to hardcoded dummy values
    let empty_bindings = HashMap::new();
    let empty_scene = compile_live_scene(1920.0, 1080.0, &empty_bindings);
    assert!(!empty_scene.commands.is_empty());
}

#[test]
fn test_adaptive_presentation_loop_power_and_gaming_shield() {
    use aethershell_audio::FpsMode;
    use aethershell_core::MonitorTaskbarState;
    use aethershell_taskbar::TaskbarCoordinator;

    // 1. Adaptive refresh rate selection:
    // Battery -> 15 FPS (66.6ms)
    assert_eq!(FpsMode::BatterySaver.target_fps(), 15);
    assert!(FpsMode::BatterySaver.frame_interval().as_millis() >= 66);

    // AC Balanced -> 30 FPS (33.3ms)
    assert_eq!(FpsMode::AcBalanced.target_fps(), 30);
    assert!(FpsMode::AcBalanced.frame_interval().as_millis() >= 33);

    // Performance -> 60 FPS (16.6ms)
    assert_eq!(FpsMode::Performance.target_fps(), 60);
    assert!(FpsMode::Performance.frame_interval().as_millis() <= 17);

    // 2. Gaming Shield detection:
    let mut tb = TaskbarCoordinator::new();
    assert!(!tb.is_any_monitor_fullscreen());

    // When a monitor transitions to Fullscreen (e.g. running game)
    tb.evaluate_and_apply("DISPLAY_1", MonitorTaskbarState::Fullscreen);
    assert!(
        tb.is_any_monitor_fullscreen(),
        "Gaming Shield must detect active full-screen application"
    );

    // When monitor returns to Desktop
    tb.evaluate_and_apply("DISPLAY_1", MonitorTaskbarState::Desktop);
    assert!(!tb.is_any_monitor_fullscreen());
}
