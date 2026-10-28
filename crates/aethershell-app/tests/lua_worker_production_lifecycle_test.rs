use aethershell_app::{compile_live_scene_with_scale, find_script_host_binary, AppEngine};
use aethershell_config::AppConfig;
use aethershell_core::BindingValue;
use aethershell_ipc::{IpcClient, IpcServer};
use aethershell_renderer::RenderCommand;
use aethershell_script_host::{sha256_hex, ScriptSupervisor};
use std::collections::HashMap;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{watch, Mutex};

fn current_session_id() -> u32 {
    let mut session_id = 0u32;
    unsafe {
        let _ = windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            std::process::id(),
            &mut session_id,
        );
    }
    session_id
}

/// Real Named Pipe test harness hosting the production AppEngine IPC router and lifecycle coordinator.
#[allow(dead_code)]
struct LuaTestHarness {
    engine: Arc<AppEngine>,
    pipe_name: String,
    session_token: String,
    temp_dir: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
    watchdog_handle: Option<tokio::task::JoinHandle<()>>,
}

impl LuaTestHarness {
    async fn start(session_token: &str) -> Self {
        Self::start_with_watchdog_interval(session_token, None).await
    }

    async fn start_with_watchdog_interval(
        session_token: &str,
        watchdog_interval: Option<Duration>,
    ) -> Self {
        let temp_dir =
            std::env::temp_dir().join(format!("test_aether_lua_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let engine = Arc::new(AppEngine::with_token_and_config_dir(
            session_token,
            &temp_dir,
        ));
        let pipe_name = format!(r"\\.\pipe\aethershell-lua-test-{}", uuid::Uuid::new_v4());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handler = engine.create_ipc_handler(current_session_id(), shutdown_tx.clone());
        let server = IpcServer::new(&pipe_name, handler, shutdown_rx.clone());

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        let watchdog_handle = if let Some(interval) = watchdog_interval {
            let engine_clone = Arc::clone(&engine);
            let mut rx = shutdown_rx.clone();
            Some(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            engine_clone.tick_script_worker_watchdog();
                        }
                        _ = rx.changed() => {
                            if *rx.borrow() {
                                break;
                            }
                        }
                    }
                }
            }))
        } else {
            None
        };

        tokio::time::sleep(Duration::from_millis(60)).await;

        Self {
            engine,
            pipe_name,
            session_token: session_token.to_string(),
            temp_dir,
            shutdown_tx,
            server_handle: Some(server_handle),
            watchdog_handle,
        }
    }

    #[allow(dead_code)]
    fn spawn_watchdog(&mut self, interval: Duration) {
        let engine_clone = Arc::clone(&self.engine);
        let mut rx = self.shutdown_tx.subscribe();
        self.watchdog_handle = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        engine_clone.tick_script_worker_watchdog();
                    }
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            break;
                        }
                    }
                }
            }
        }));
    }

    fn active_config(&self) -> Arc<RwLock<AppConfig>> {
        self.engine.active_config()
    }

    fn cached_bindings(&self) -> Arc<RwLock<HashMap<String, BindingValue>>> {
        self.engine.cached_bindings()
    }

    fn script_supervisor(&self) -> Arc<Mutex<ScriptSupervisor>> {
        self.engine.script_supervisor()
    }

    async fn stop(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(h) = self.server_handle.take() {
            let _ = h.await;
        }
        if let Some(h) = self.watchdog_handle.take() {
            let _ = h.await;
        }
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

impl Drop for LuaTestHarness {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

#[tokio::test]
async fn test_production_lua_worker_on_demand_launch_and_binding_render() {
    let harness = LuaTestHarness::start("lua_test_secret_token_1").await;

    // 1. Verify script host binary is available
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            eprintln!("STATUS_LUA_BIN_UNAVAILABLE: aethershell-script-host.exe not found; skipping out-of-process test.");
            harness.stop().await;
            return;
        }
    }

    // 2. Prepare isolated trusted Lua script
    let script_file = harness.temp_dir.join("worker_summary.lua");
    let script_content = r#"
function init()
    aether.set_binding("script.status", "Active: Lua 5.4")
    aether.set_binding("script.custom_metric", 42.0)
    aether.publish_bindings({
        ["script.status"] = "Active: Lua 5.4",
        ["script.custom_metric"] = 42.0
    })
end

function update()
    aether.publish_bindings({
        ["script.status"] = "Active: Lua 5.4",
        ["script.custom_metric"] = 42.0
    })
end
"#;
    std::fs::write(&script_file, script_content).expect("write script");
    let digest = sha256_hex(script_content.as_bytes());

    // 3. Configure AppEngine with active script and explicit trust grant
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest);
    }

    // 4. Synchronize script worker: launches child process on demand
    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync_script_worker must succeed");

    assert!(
        launch_res.is_some(),
        "Expected worker child process to be spawned"
    );
    let worker_pid = launch_res.unwrap();
    assert!(worker_pid > 0, "Worker PID must be positive");

    // 5. Poll for worker IPC handshake and binding publication (within 4 seconds)
    let start = std::time::Instant::now();
    let mut received_binding = false;
    let bindings_arc = harness.cached_bindings();
    while start.elapsed() < Duration::from_secs(4) {
        {
            let cache = bindings_arc.read().unwrap();
            if let Some(BindingValue::String(val)) = cache.get("script.status") {
                if val == "Active: Lua 5.4" {
                    received_binding = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        received_binding,
        "Engine cached_bindings must contain 'script.status' published by Lua worker"
    );

    // 6. Direct2D Scene Graph render verification:
    // Verify that the script-provided binding is directly rendered into scene graph nodes
    let bindings_snapshot = bindings_arc.read().unwrap().clone();
    let (scene, _regions) = compile_live_scene_with_scale(1920.0, 1080.0, &bindings_snapshot, 1.0);

    let found_rendered_script_text = scene.commands.iter().any(|cmd| match cmd {
        RenderCommand::DrawText { text, .. } => text == "Active: Lua 5.4",
        _ => false,
    });
    assert!(
        found_rendered_script_text,
        "SceneGraph must contain DrawText command rendering the script-provided binding 'Active: Lua 5.4'"
    );

    // 7. Test on-demand worker shutdown when script becomes inactive (PRD Section 13.2)
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.active_script = None;
    }
    let stop_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync_script_worker stop");
    assert_eq!(
        stop_res, None,
        "Worker must be stopped when no script is active"
    );

    // Confirm bindings are purged
    {
        let cache = bindings_arc.read().unwrap();
        assert!(
            !cache.contains_key("script.status"),
            "Script bindings must be purged upon worker shutdown"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_untrusted_script_rejection_fail_closed() {
    let harness = LuaTestHarness::start("lua_test_secret_token_2").await;

    let script_file = harness.temp_dir.join("untrusted_script.lua");
    let script_content = "function init() end\n";
    std::fs::write(&script_file, script_content).expect("write script");

    // Configure with script but WITHOUT trusted digest grant
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting.trusted_digests.clear(); // Empty trust list
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;

    assert!(
        launch_res.is_err(),
        "Untrusted script must fail closed and NOT launch a worker"
    );

    let sup_arc = harness.script_supervisor();
    let sup = sup_arc.lock().await;
    assert!(
        !sup.has_active_child(),
        "Supervisor must not have any active child for untrusted script"
    );

    drop(sup);
    harness.stop().await;
}

#[tokio::test]
async fn test_safe_mode_terminates_worker_and_purges_bindings() {
    let harness = LuaTestHarness::start("lua_test_secret_token_3").await;

    let bindings_arc = harness.cached_bindings();
    // Simulate pre-existing script bindings
    {
        let mut cache = bindings_arc.write().unwrap();
        cache.insert(
            "script.telemetry_custom".into(),
            BindingValue::String("Running".into()),
        );
        cache.insert(
            "user.theme_preference".into(),
            BindingValue::String("Dark".into()),
        );
        cache.insert("cpu.usage_percent".into(), BindingValue::Float(12.5));
    }

    // Call sync_script_worker with is_safe_mode = true
    let res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, true)
        .await
        .expect("sync_script_worker in safe mode");

    assert_eq!(res, None, "Safe mode must not launch worker");

    // Script bindings must be purged, while native system bindings remain intact
    {
        let cache = bindings_arc.read().unwrap();
        assert!(
            !cache.contains_key("script.telemetry_custom"),
            "script.* bindings must be purged in safe mode"
        );
        assert!(
            cache.contains_key("cpu.usage_percent"),
            "System telemetry bindings must be preserved in safe mode"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_script_crash_circuit_breaker_and_quarantine_isolation() {
    let harness = LuaTestHarness::start("lua_test_secret_token_4").await;

    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            eprintln!("STATUS_LUA_BIN_UNAVAILABLE: aethershell-script-host.exe not found; skipping out-of-process test.");
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("crashing_worker.lua");
    let script_content = "error('intentional crash for quarantine test')\n";
    std::fs::write(&script_file, script_content).expect("write script");
    let digest = sha256_hex(script_content.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest);
    }

    // Spawn 1
    let pid1 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("spawn 1");
    assert!(pid1.is_some());

    let sup_arc = harness.script_supervisor();

    // Wait for crash 1 to be polled
    let start1 = std::time::Instant::now();
    while start1.elapsed() < Duration::from_secs(3) {
        let mut sup = sup_arc.lock().await;
        let exit = sup.poll_worker_status();
        if exit.is_some() {
            break;
        }
        drop(sup);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    {
        let sup = sup_arc.lock().await;
        assert_eq!(sup.crash_count(), 1);
        assert!(!sup.is_quarantined());
    }

    // Spawn 2 (triggers quarantine limit of 2)
    let pid2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("spawn 2");
    assert!(pid2.is_some());

    let start2 = std::time::Instant::now();
    while start2.elapsed() < Duration::from_secs(3) {
        let mut sup = sup_arc.lock().await;
        let exit = sup.poll_worker_status();
        if exit.is_some() {
            break;
        }
        drop(sup);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Confirm supervisor transitioned to Quarantined state
    {
        let sup = sup_arc.lock().await;
        assert!(
            sup.is_quarantined(),
            "Supervisor must be quarantined after exceeding crash limit"
        );
    }

    // Further launch attempts fail closed while quarantined
    let launch_after_quarantine = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;
    assert!(
        launch_after_quarantine.is_err(),
        "Spawn must be blocked while supervisor is quarantined"
    );

    // Verify Core engine remains completely healthy and responsive
    let status_resp =
        IpcClient::send_command(&harness.pipe_name, "get_status", serde_json::json!({}))
            .await
            .expect("core get_status must succeed despite worker quarantine");
    assert!(
        status_resp.success,
        "Core Engine IPC must remain healthy and operational"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_dynamic_script_switching() {
    let harness = LuaTestHarness::start("lua_test_secret_token_5").await;

    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            eprintln!("STATUS_LUA_BIN_UNAVAILABLE: aethershell-script-host.exe not found; skipping out-of-process test.");
            harness.stop().await;
            return;
        }
    }

    let script1 = harness.temp_dir.join("worker_1.lua");
    let script1_content = "function init() aether.set_binding('script.status', 'Worker 1') end\n";
    std::fs::write(&script1, script1_content).expect("write script1");
    let digest1 = sha256_hex(script1_content.as_bytes());

    let script2 = harness.temp_dir.join("worker_2.lua");
    let script2_content = "function init() aether.set_binding('script.status', 'Worker 2') end\n";
    std::fs::write(&script2, script2_content).expect("write script2");
    let digest2 = sha256_hex(script2_content.as_bytes());

    // Configure Script 1
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script1.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script1.to_string_lossy().to_string(), digest1);
        cfg.scripting
            .trusted_digests
            .insert(script2.to_string_lossy().to_string(), digest2);
    }

    let pid1 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync script 1")
        .expect("pid1");

    assert!(pid1 > 0);

    // Switch to Script 2
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.active_script = Some(script2.to_string_lossy().to_string());
    }

    let pid2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync script 2")
        .expect("pid2");

    assert!(pid2 > 0);
    assert_ne!(
        pid1, pid2,
        "Switching script must terminate previous worker and spawn a fresh worker PID"
    );

    // Clean up
    {
        let sup_arc = harness.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.terminate_worker();
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_trust_rejection_modified_script_same_path_and_filename() {
    let harness = LuaTestHarness::start("lua_test_secret_token_6").await;

    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            eprintln!("STATUS_LUA_BIN_UNAVAILABLE: aethershell-script-host.exe not found; skipping out-of-process test.");
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("worker.lua");
    let original_content = "function init() aether.set_binding('script.status', 'Orig') end\n";
    std::fs::write(&script_file, original_content).expect("write original");
    let original_digest = sha256_hex(original_content.as_bytes());

    // 1. Authorize original digest
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), original_digest);
    }

    // 2. Modify script content on disk (same filename and path, but different bytes/digest)
    let modified_content = "function init() aether.set_binding('script.status', 'Tampered') end\n";
    std::fs::write(&script_file, modified_content).expect("write modified");

    // 3. Attempt launch: MUST fail closed due to digest mismatch
    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;

    assert!(
        launch_res.is_err(),
        "Modified script with changed content MUST be rejected fail-closed despite identical filename and path"
    );

    let sup_arc = harness.script_supervisor();
    let sup = sup_arc.lock().await;
    assert!(
        !sup.has_active_child(),
        "Supervisor must not have spawned a child process for modified script"
    );

    drop(sup);
    harness.stop().await;
}

#[tokio::test]
async fn test_trust_rejection_distinct_paths_identical_filename() {
    let harness = LuaTestHarness::start("lua_test_secret_token_7").await;

    let dir_a = harness.temp_dir.join("dir_a");
    let dir_b = harness.temp_dir.join("dir_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let script_a = dir_a.join("module.lua");
    let script_b = dir_b.join("module.lua");

    let content_a = "function init() aether.set_binding('script.status', 'A') end\n";
    let content_b = "function init() aether.set_binding('script.status', 'B') end\n";
    std::fs::write(&script_a, content_a).unwrap();
    std::fs::write(&script_b, content_b).unwrap();

    let digest_a = sha256_hex(content_a.as_bytes());

    // Trust only script A
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_b.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_a.to_string_lossy().to_string(), digest_a);
    }

    // Attempt to launch script B (which has the same filename 'module.lua')
    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;

    assert!(
        launch_res.is_err(),
        "Script B must not inherit trust approval from Script A despite identical filenames"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_trust_reauthorization_with_new_digest() {
    let harness = LuaTestHarness::start("lua_test_secret_token_8").await;

    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            eprintln!("STATUS_LUA_BIN_UNAVAILABLE: aethershell-script-host.exe not found; skipping out-of-process test.");
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("evolving_worker.lua");
    let content_v1 = "function init() aether.set_binding('script.status', 'V1') end\n";
    std::fs::write(&script_file, content_v1).unwrap();
    let digest_v1 = sha256_hex(content_v1.as_bytes());

    // 1. Authorize V1
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    let pid_v1 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("spawn v1");
    assert!(pid_v1.is_some(), "V1 should spawn successfully");

    // Terminate V1
    {
        let sup_arc = harness.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.terminate_worker();
    }

    // 2. Modify to V2 on disk
    let content_v2 = "function init() aether.set_binding('script.status', 'V2') end\n";
    std::fs::write(&script_file, content_v2).unwrap();
    let digest_v2 = sha256_hex(content_v2.as_bytes());

    // 3. Before reauthorization: launch attempt fails closed
    let reject_v2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;
    assert!(
        reject_v2.is_err(),
        "V2 must be rejected before explicit reauthorization"
    );

    // 4. Explicitly reauthorize V2 digest
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v2);
    }

    // 5. After reauthorization: launch succeeds!
    let pid_v2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("spawn v2");
    assert!(
        pid_v2.is_some(),
        "V2 should spawn successfully after explicit digest reauthorization"
    );

    // Clean up
    {
        let sup_arc = harness.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.terminate_worker();
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_trust_rejection_conflicting_digests() {
    let harness = LuaTestHarness::start("lua_test_secret_token_9").await;

    let script_file = harness.temp_dir.join("conflict.lua");
    let content = "function init() end\n";
    std::fs::write(&script_file, content).unwrap();
    let digest = sha256_hex(content.as_bytes());

    // Insert conflicting trust records for the same canonical file
    let canon_path = dunce_canonical(&script_file);
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest);
        cfg.scripting.trusted_digests.insert(
            canon_path,
            "0000000000000000000000000000000000000000000000000000000000000000".into(),
        );
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;

    assert!(
        launch_res.is_err(),
        "Conflicting trust records must fail closed"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_trust_rejection_malformed_digest() {
    let harness = LuaTestHarness::start("lua_test_secret_token_10").await;

    let script_file = harness.temp_dir.join("malformed.lua");
    let content = "function init() end\n";
    std::fs::write(&script_file, content).unwrap();

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting.trusted_digests.insert(
            script_file.to_string_lossy().to_string(),
            "not_a_hex_digest".into(),
        );
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;

    assert!(
        launch_res.is_err(),
        "Malformed trust digest must fail closed"
    );

    harness.stop().await;
}

fn dunce_canonical(p: &std::path::Path) -> String {
    let canon = std::fs::canonicalize(p).unwrap();
    let s = canon.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        s.to_string()
    }
}

#[tokio::test]
async fn test_trust_revocation_terminates_running_worker_and_purges_bindings() {
    let harness = LuaTestHarness::start("lua_test_secret_token_11").await;
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("revocation_test.lua");
    let script_content = r#"
function init()
    aether.set_binding("script.status", "Active: Lua 5.4")
    aether.publish_bindings({ ["script.status"] = "Active: Lua 5.4" })
end
function update()
    aether.publish_bindings({ ["script.status"] = "Active: Lua 5.4" })
end
"#;
    std::fs::write(&script_file, script_content).expect("write script");
    let digest = sha256_hex(script_content.as_bytes());

    // Insert native telemetry binding alongside script binding
    {
        let cached_arc = harness.cached_bindings();
        let mut bindings = cached_arc.write().unwrap();
        bindings.insert("cpu.usage_percent".into(), BindingValue::Float(18.5));
    }

    // Configure and launch worker
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest.clone());
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync_script_worker must succeed initially");
    assert!(launch_res.is_some(), "Worker child should be spawned");
    let worker_pid = launch_res.unwrap();

    // Verify worker is running and child is alive
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(sup.is_running(), "Supervisor must report worker running");
        assert_eq!(sup.active_child_pid(), Some(worker_pid));
    }

    // Wait for binding to appear
    let start = std::time::Instant::now();
    let mut received = false;
    let cached_arc = harness.cached_bindings();
    while start.elapsed() < Duration::from_secs(4) {
        if let Some(val) = cached_arc.read().unwrap().get("script.status") {
            if val == &BindingValue::String("Active: Lua 5.4".into()) {
                received = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(received, "Script status binding must be published");

    // Revoke trust by clearing trusted digests
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.trusted_digests.clear();
    }

    // Call sync_script_worker: must fail closed and terminate worker
    let sync_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;
    assert!(
        sync_res.is_err(),
        "Sync must fail closed when trust is revoked"
    );

    // Verify supervisor terminated the worker
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.is_running(),
            "Worker must be terminated upon trust revocation"
        );
        assert!(
            !sup.has_active_child(),
            "Child process must be cleared upon revocation"
        );
    }

    // Verify bindings: script.* must be purged, cpu.* must be preserved
    {
        let cached_arc = harness.cached_bindings();
        let bindings = cached_arc.read().unwrap();
        assert!(
            !bindings.contains_key("script.status"),
            "script.status must be purged upon trust revocation"
        );
        assert!(
            bindings.contains_key("cpu.usage_percent"),
            "Native telemetry bindings must remain intact"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_unapproved_script_modification_terminates_running_worker() {
    let harness = LuaTestHarness::start("lua_test_secret_token_12").await;
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("modification_test.lua");
    let script_content_v1 = r#"
function init()
    aether.set_binding("script.version", "v1")
    aether.publish_bindings({ ["script.version"] = "v1" })
end
function update()
    aether.publish_bindings({ ["script.version"] = "v1" })
end
"#;
    std::fs::write(&script_file, script_content_v1).expect("write script v1");
    let digest_v1 = sha256_hex(script_content_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("sync_script_worker must succeed for v1");
    assert!(launch_res.is_some());

    // Modify script file on disk without explicit reauthorization
    let script_content_tampered = r#"
function init()
    aether.set_binding("script.version", "tampered")
    aether.publish_bindings({ ["script.version"] = "tampered" })
end
"#;
    std::fs::write(&script_file, script_content_tampered).expect("write tampered script");

    // Sync must fail closed and terminate the running worker
    let sync_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await;
    assert!(
        sync_res.is_err(),
        "Sync must fail closed when script file content is modified without reauthorization"
    );

    // Verify worker terminated
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.is_running(),
            "Worker must be terminated when content changes without trust"
        );
        assert!(!sup.has_active_child());
    }

    // Verify script bindings purged
    {
        let cached_arc = harness.cached_bindings();
        let bindings = cached_arc.read().unwrap();
        assert!(!bindings.contains_key("script.version"));
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_same_path_replacement_with_approved_digest_restarts_worker() {
    let harness = LuaTestHarness::start("lua_test_secret_token_13").await;
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("versioned_worker.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.version", "v1")
    aether.publish_bindings({ ["script.version"] = "v1" })
end
function update()
    aether.publish_bindings({ ["script.version"] = "v1" })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    let pid1 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch v1")
        .expect("pid1");

    // Overwrite the same path with approved v2 script
    let script_v2 = r#"
function init()
    aether.set_binding("script.version", "v2_updated")
    aether.publish_bindings({ ["script.version"] = "v2_updated" })
end
function update()
    aether.publish_bindings({ ["script.version"] = "v2_updated" })
end
"#;
    std::fs::write(&script_file, script_v2).expect("write v2");
    let digest_v2 = sha256_hex(script_v2.as_bytes());

    // Explicitly approve the new digest at the same path
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v2);
    }

    // Sync script worker: must restart worker with the new approved digest
    let pid2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch v2")
        .expect("pid2");

    assert_ne!(
        pid1, pid2,
        "Same-path replacement with new approved digest must spawn a new worker process"
    );

    // Wait for v2 binding update
    let start = std::time::Instant::now();
    let mut updated = false;
    while start.elapsed() < Duration::from_secs(4) {
        if let Some(val) = harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.version")
        {
            if val == &BindingValue::String("v2_updated".into()) {
                updated = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(updated, "Binding must update to v2_updated value");

    harness.stop().await;
}

#[tokio::test]
async fn test_unchanged_path_and_digest_avoids_unnecessary_restart() {
    let harness = LuaTestHarness::start("lua_test_secret_token_14").await;
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            harness.stop().await;
            return;
        }
    }

    let script_file = harness.temp_dir.join("stable_worker.lua");
    let script_content = r#"
function init()
    aether.set_binding("script.status", "stable")
    aether.publish_bindings({ ["script.status"] = "stable" })
end
function update()
    aether.publish_bindings({ ["script.status"] = "stable" })
end
"#;
    std::fs::write(&script_file, script_content).expect("write script");
    let digest = sha256_hex(script_content.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest);
    }

    let pid1 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("first sync")
        .expect("pid1");

    // Second sync with identical configuration, path, and digest
    let pid2 = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("second sync")
        .expect("pid2");

    assert_eq!(
        pid1, pid2,
        "Worker process must be reused when path and digest remain unchanged; unnecessary restart must be avoided"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_config_sync_reports_unapplied_state_when_script_worker_fails() {
    let harness = LuaTestHarness::start("lua_test_secret_token_15").await;

    let script_file = harness.temp_dir.join("unapproved_config.lua");
    std::fs::write(&script_file, "function init() end\n").expect("write script");

    // Send set_config with an untrusted script
    let mut bad_config = aethershell_config::AppConfig::default();
    bad_config.scripting.enabled = true;
    bad_config.scripting.active_script = Some(script_file.to_string_lossy().to_string());
    bad_config
        .scripting
        .require_explicit_trust_for_imported_scripts = true;
    bad_config.scripting.trusted_digests.clear(); // No trust record!

    let resp = IpcClient::send_command(
        &harness.pipe_name,
        "set_config",
        serde_json::json!({
            "session_token": "lua_test_secret_token_15",
            "config": bad_config,
        }),
    )
    .await
    .expect("send set_config");

    assert!(resp.success, "IPC response was received");
    assert_eq!(
        resp.payload["applied"], false,
        "Configuration update must NOT report applied: true when script worker synchronization fails"
    );
    assert!(
        resp.payload["worker_sync_error"].is_string(),
        "worker_sync_error must be present in response"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_external_modification_terminates_within_watchdog_interval() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            return;
        }
    }

    // Launch harness with a 500ms continuous watchdog tick
    let harness = LuaTestHarness::start_with_watchdog_interval(
        "lua_test_watchdog_mod",
        Some(Duration::from_millis(500)),
    )
    .await;

    let script_file = harness.temp_dir.join("continuous_mod.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.status", "Active: Lua 5.4")
    aether.publish_bindings({ ["script.status"] = "Active: Lua 5.4" })
end
function update()
    aether.publish_bindings({ ["script.status"] = "Active: Lua 5.4" })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    // Insert native telemetry binding alongside script binding
    {
        let cached_arc = harness.cached_bindings();
        let mut bindings = cached_arc.write().unwrap();
        bindings.insert("cpu.usage_percent".into(), BindingValue::Float(22.5));
    }

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    let launch_res = harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");
    assert!(launch_res.is_some());

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if let Some(val) = harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.status")
        {
            if val == &BindingValue::String("Active: Lua 5.4".into()) {
                bound = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Externally modify script file on disk WITHOUT ANY IPC REQUEST
    let start_mod = std::time::Instant::now();
    let script_tampered = r#"
function init()
    aether.set_binding("script.status", "HACKED")
    aether.publish_bindings({ ["script.status"] = "HACKED" })
end
"#;
    std::fs::write(&script_file, script_tampered).expect("write tampered content");

    // Wait for the continuous watchdog to detect modification and terminate worker
    let mut terminated = false;
    while start_mod.elapsed() < Duration::from_secs(3) {
        let is_running = {
            let sup_arc = harness.script_supervisor();
            let sup = sup_arc.lock().await;
            sup.is_running()
        };
        if !is_running {
            terminated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let elapsed = start_mod.elapsed();
    assert!(
        terminated,
        "Watchdog must detect file modification without IPC requests within bounded latency"
    );
    // Bounded latency: <= 2 seconds (+ slight scheduling slack)
    assert!(
        elapsed <= Duration::from_millis(2500),
        "Modification termination latency ({:?}) must be bounded within <= 2s",
        elapsed
    );

    // Verify supervisor state
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(!sup.is_running());
        assert!(!sup.has_active_child());
    }

    // Verify bindings: script.* purged, native telemetry cpu.* intact
    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.status"),
            "script.* bindings must be purged"
        );
        assert!(
            bindings.contains_key("cpu.usage_percent"),
            "Native telemetry bindings must be preserved"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_external_deletion_terminates_worker() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            return;
        }
    }

    let harness = LuaTestHarness::start_with_watchdog_interval(
        "lua_test_watchdog_del",
        Some(Duration::from_millis(400)),
    )
    .await;

    let script_file = harness.temp_dir.join("continuous_del.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.del_metric", 99.0)
    aether.publish_bindings({ ["script.del_metric"] = 99.0 })
end
function update()
    aether.publish_bindings({ ["script.del_metric"] = 99.0 })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let cached_arc = harness.cached_bindings();
        let mut bindings = cached_arc.write().unwrap();
        bindings.insert("ram.usage_mb".into(), BindingValue::Float(4096.0));
    }

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.del_metric")
            .is_some()
        {
            bound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Externally DELETE script file on disk WITHOUT ANY IPC REQUEST
    let start_del = std::time::Instant::now();
    let _ = std::fs::remove_file(&script_file);

    // Watchdog should reap worker within bounded latency
    let mut terminated = false;
    while start_del.elapsed() < Duration::from_secs(3) {
        let is_running = {
            let sup_arc = harness.script_supervisor();
            let sup = sup_arc.lock().await;
            sup.is_running()
        };
        if !is_running {
            terminated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        terminated,
        "Watchdog must detect file deletion and terminate worker"
    );

    // Verify supervisor state and bindings
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(!sup.is_running());
        assert!(!sup.has_active_child());
    }

    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.del_metric"),
            "script.del_metric must be purged"
        );
        assert!(
            bindings.contains_key("ram.usage_mb"),
            "Native telemetry ram.usage_mb must remain intact"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_rapid_repeated_modifications() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            return;
        }
    }

    let harness = LuaTestHarness::start("lua_test_rapid_mods").await;

    let script_file = harness.temp_dir.join("rapid_mods.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.rapid_status", "original")
    aether.publish_bindings({ ["script.rapid_status"] = "original" })
end
function update()
    aether.publish_bindings({ ["script.rapid_status"] = "original" })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Rapidly rewrite the script file 10 times with different contents
    for i in 0..10 {
        let tampered_content = format!(
            r#"
function init()
    aether.set_binding("script.rapid_status", "rapid_mod_{}")
    aether.publish_bindings({{ ["script.rapid_status"] = "rapid_mod_{}" }})
end
"#,
            i, i
        );
        std::fs::write(&script_file, tampered_content).expect("rewrite script");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Trigger watchdog tick
    let ticked = harness.engine.tick_script_worker_watchdog();
    assert!(ticked, "Watchdog tick must acquire supervisor lock");

    // Verify worker was invalidated immediately upon rapid modification
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.is_running(),
            "Supervisor must report worker not running after rapid modifications"
        );
        assert!(
            !sup.has_active_child(),
            "Worker process handle must be cleared and terminated"
        );
    }

    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.rapid_status"),
            "Bindings must be purged"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_unreadable_script_fails_closed() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            return;
        }
    }

    let harness = LuaTestHarness::start("lua_test_unreadable").await;

    let script_file = harness.temp_dir.join("unreadable.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.lock_test", "running")
    aether.publish_bindings({ ["script.lock_test"] = "running" })
end
function update()
    aether.publish_bindings({ ["script.lock_test"] = "running" })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.lock_test")
            .is_some()
        {
            bound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Lock file with exclusive sharing mode (dwShareMode = 0) so reading fails
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&script_file)
        .expect("exclusive open");

    // Trigger watchdog tick
    let ticked = harness.engine.tick_script_worker_watchdog();
    assert!(ticked, "Watchdog tick must succeed");

    // Supervisor must detect unreadable file, fail closed, terminate worker
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.is_running(),
            "Supervisor must terminate worker when script is unreadable (fail closed)"
        );
        assert!(!sup.has_active_child());
    }

    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.lock_test"),
            "Bindings must be purged on unreadable script"
        );
    }

    drop(lock_file);
    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_trust_revocation_without_ipc_terminates_worker() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        if std::env::var("REQUIRE_LUA_WORKER_TEST").is_ok() {
            panic!("REQUIRED_LUA_BIN_MISSING: aethershell-script-host.exe not found");
        } else {
            return;
        }
    }

    // Launch harness with a 400ms continuous watchdog tick
    let harness = LuaTestHarness::start_with_watchdog_interval(
        "lua_test_watchdog_revoke",
        Some(Duration::from_millis(400)),
    )
    .await;

    let script_file = harness.temp_dir.join("continuous_revoke.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.rev_status", "valid")
    aether.publish_bindings({ ["script.rev_status"] = "valid" })
end
function update()
    aether.publish_bindings({ ["script.rev_status"] = "valid" })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write v1");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.rev_status")
            .is_some()
        {
            bound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Explicitly revoke trust in active_config WITHOUT CALLING ANY IPC FUNCTION OR SYNC
    let start_revoke = std::time::Instant::now();
    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.trusted_digests.clear();
    }

    // Watchdog should detect trust revocation and terminate worker
    let mut terminated = false;
    while start_revoke.elapsed() < Duration::from_secs(3) {
        let is_running = {
            let sup_arc = harness.script_supervisor();
            let sup = sup_arc.lock().await;
            sup.is_running()
        };
        if !is_running {
            terminated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        terminated,
        "Watchdog must detect trust revocation in active_config and terminate worker"
    );

    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(!sup.is_running());
        assert!(!sup.has_active_child());
    }

    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.rev_status"),
            "script.rev_status must be purged upon trust revocation"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_poisoned_config_lock_fails_closed() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        return;
    }

    let harness = LuaTestHarness::start_with_watchdog_interval("lua_poison_lock", None).await;

    let script_file = harness.temp_dir.join("poison_test.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.poison_metric", 42.0)
    aether.publish_bindings({ ["script.poison_metric"] = 42.0 })
end
function update()
    aether.publish_bindings({ ["script.poison_metric"] = 42.0 })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write script");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.poison_metric")
            .is_some()
        {
            bound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Poison the active_config RwLock
    let config_arc_clone = harness.active_config();
    let handle = std::thread::spawn(move || {
        let _guard = config_arc_clone.write().unwrap();
        panic!("Simulated lock poisoning in configuration subsystem");
    });
    let _ = handle.join();

    // Verify lock is indeed poisoned
    assert!(harness.active_config().read().is_err());

    // Trigger watchdog tick
    let ticked = harness.engine.tick_script_worker_watchdog();
    assert!(ticked, "Watchdog tick should succeed");

    // Verify worker terminated fail-closed
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.has_active_child(),
            "Active worker must be terminated fail-closed when config lock is poisoned"
        );
    }

    // Verify script bindings purged
    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.poison_metric"),
            "script.* bindings must be purged when trust ledger is inaccessible"
        );
    }

    harness.stop().await;
}

#[tokio::test]
async fn test_continuous_revalidation_active_child_missing_metadata_fails_closed() {
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        return;
    }

    let harness = LuaTestHarness::start_with_watchdog_interval("lua_missing_meta", None).await;

    let script_file = harness.temp_dir.join("missing_meta.lua");
    let script_v1 = r#"
function init()
    aether.set_binding("script.meta_metric", 100.0)
    aether.publish_bindings({ ["script.meta_metric"] = 100.0 })
end
function update()
    aether.publish_bindings({ ["script.meta_metric"] = 100.0 })
end
"#;
    std::fs::write(&script_file, script_v1).expect("write script");
    let digest_v1 = sha256_hex(script_v1.as_bytes());

    {
        let config_arc = harness.active_config();
        let mut cfg = config_arc.write().unwrap();
        cfg.scripting.enabled = true;
        cfg.scripting.active_script = Some(script_file.to_string_lossy().to_string());
        cfg.scripting.require_explicit_trust_for_imported_scripts = true;
        cfg.scripting
            .trusted_digests
            .insert(script_file.to_string_lossy().to_string(), digest_v1);
    }

    harness
        .engine
        .sync_script_worker(&harness.pipe_name, false)
        .await
        .expect("launch worker");

    // Wait for initial binding to appear
    let wait_start = std::time::Instant::now();
    let mut bound = false;
    while wait_start.elapsed() < Duration::from_secs(4) {
        if harness
            .cached_bindings()
            .read()
            .unwrap()
            .get("script.meta_metric")
            .is_some()
        {
            bound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(bound, "Initial binding must be published");

    // Corrupt supervisor state by clearing current_script while child is still active
    {
        let sup_arc = harness.script_supervisor();
        let mut sup = sup_arc.lock().await;
        assert!(sup.has_active_child());
        sup.clear_current_script_for_test();
        assert!(sup.current_script().is_none());
    }

    // Trigger watchdog tick - should detect missing metadata and fail-closed
    let ticked = harness.engine.tick_script_worker_watchdog();
    assert!(ticked, "Watchdog tick should succeed");

    // Verify worker terminated fail-closed
    {
        let sup_arc = harness.script_supervisor();
        let sup = sup_arc.lock().await;
        assert!(
            !sup.has_active_child(),
            "Active worker must be terminated fail-closed when script metadata is missing"
        );
    }

    // Verify script bindings purged
    {
        let cached = harness.cached_bindings();
        let bindings = cached.read().unwrap();
        assert!(
            !bindings.contains_key("script.meta_metric"),
            "script.* bindings must be purged when worker metadata is invalid"
        );
    }

    harness.stop().await;
}
