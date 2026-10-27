use aethershell_app::{compile_live_scene_with_scale, find_script_host_binary, AppEngine};
use aethershell_config::AppConfig;
use aethershell_core::BindingValue;
use aethershell_ipc::{IpcClient, IpcServer};
use aethershell_renderer::RenderCommand;
use aethershell_script_host::{sha256_hex, ScriptSupervisor};
use std::collections::HashMap;
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
}

impl LuaTestHarness {
    async fn start(session_token: &str) -> Self {
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
        let server = IpcServer::new(&pipe_name, handler, shutdown_rx);

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(60)).await;

        Self {
            engine,
            pipe_name,
            session_token: session_token.to_string(),
            temp_dir,
            shutdown_tx,
            server_handle: Some(server_handle),
        }
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
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

impl Drop for LuaTestHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

#[tokio::test]
async fn test_production_lua_worker_on_demand_launch_and_binding_render() {
    let harness = LuaTestHarness::start("lua_test_secret_token_1").await;

    // 1. Verify script host binary is available
    let host_bin = find_script_host_binary("aethershell-script-host.exe");
    if host_bin.is_none() {
        eprintln!("Skipping test: aethershell-script-host.exe not yet built");
        harness.stop().await;
        return;
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
            .insert("worker_summary.lua".into(), digest);
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
        .await
        .expect("sync_script_worker call");

    assert_eq!(
        launch_res, None,
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
        harness.stop().await;
        return;
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
            .insert("crashing_worker.lua".into(), digest);
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
        .await
        .expect("launch attempt");
    assert_eq!(
        launch_after_quarantine, None,
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
        harness.stop().await;
        return;
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
            .insert("worker_1.lua".into(), digest1);
        cfg.scripting
            .trusted_digests
            .insert("worker_2.lua".into(), digest2);
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
