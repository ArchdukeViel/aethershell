use aethershell_app::AppEngine;
use aethershell_core::InteractionMode;
use aethershell_ipc::{IpcClient, IpcClientIdentity, IpcRequest, IpcServer};
use aethershell_script_host::SupervisedChild;
use std::os::windows::io::AsRawHandle;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Helper to spin up a real named pipe server hosting the actual AppEngine production handler.
struct LiveEnginePipeTestHarness {
    engine: Arc<AppEngine>,
    pipe_name: String,
    session_token: String,
    shutdown_tx: watch::Sender<bool>,
    server_handle: tokio::task::JoinHandle<()>,
}

impl LiveEnginePipeTestHarness {
    async fn start(session_id: u32, session_token: &str) -> Self {
        let engine = Arc::new(AppEngine::with_token(session_token));
        let pipe_name = format!(r"\\.\pipe\aethershell-auth-test-{}", uuid::Uuid::new_v4());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handler = engine.create_ipc_handler(session_id, shutdown_tx.clone());
        let server = IpcServer::new(&pipe_name, handler, shutdown_rx);

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give pipe server a moment to initialize the named pipe
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            engine,
            pipe_name,
            session_token: session_token.to_string(),
            shutdown_tx,
            server_handle,
        }
    }

    async fn stop(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.server_handle.await;
    }
}

#[tokio::test]
async fn test_real_server_privileged_management_and_live_state_changes() {
    let harness = LiveEnginePipeTestHarness::start(1, "valid_secret_token_live").await;

    // 1. Send pause_audio with valid session_token -> verifies live audio state changes
    let pause_res = IpcClient::send_command(
        &harness.pipe_name,
        "pause_audio",
        serde_json::json!({ "session_token": &harness.session_token }),
    )
    .await
    .expect("send pause_audio");

    assert!(pause_res.success);
    assert_eq!(pause_res.payload["paused"], true);
    {
        let audio_arc = harness.engine.audio();
        let aud = audio_arc.lock().await;
        assert!(aud.is_paused());
    }

    // 2. Send resume_audio with valid session_token -> verifies live audio state unpaused
    let resume_res = IpcClient::send_command(
        &harness.pipe_name,
        "resume_audio",
        serde_json::json!({ "session_token": &harness.session_token }),
    )
    .await
    .expect("send resume_audio");

    assert!(resume_res.success);
    assert_eq!(resume_res.payload["paused"], false);
    {
        let audio_arc = harness.engine.audio();
        let aud = audio_arc.lock().await;
        assert!(!aud.is_paused());
    }

    // 3. Send enter_interactive_mode -> verifies live interaction controller state
    let interactive_res = IpcClient::send_command(
        &harness.pipe_name,
        "enter_interactive_mode",
        serde_json::json!({ "session_token": &harness.session_token, "timeout_seconds": 12 }),
    )
    .await
    .expect("send enter_interactive_mode");

    assert!(interactive_res.success);
    assert_eq!(interactive_res.payload["active"], true);
    {
        let ctrl_arc = harness.engine.interaction();
        let ctrl = ctrl_arc.lock().await;
        assert_ne!(ctrl.current_mode(), InteractionMode::LockedPassive);
    }

    // 4. Send exit_interactive_mode -> verifies live return to LockedPassive
    let exit_res = IpcClient::send_command(
        &harness.pipe_name,
        "exit_interactive_mode",
        serde_json::json!({ "session_token": &harness.session_token }),
    )
    .await
    .expect("send exit_interactive_mode");

    assert!(exit_res.success);
    assert_eq!(exit_res.payload["active"], false);
    {
        let ctrl_arc = harness.engine.interaction();
        let ctrl = ctrl_arc.lock().await;
        assert_eq!(ctrl.current_mode(), InteractionMode::LockedPassive);
    }

    // 5. Send shutdown with valid session_token -> verifies live engine shutdown flag
    assert!(!harness.engine.is_shutdown_requested());
    let shutdown_res = IpcClient::send_command(
        &harness.pipe_name,
        "shutdown",
        serde_json::json!({ "session_token": &harness.session_token }),
    )
    .await
    .expect("send shutdown");

    assert!(shutdown_res.success);
    assert!(harness.engine.is_shutdown_requested());

    harness.stop().await;
}

#[tokio::test]
async fn test_negative_missing_and_invalid_tokens_rejected_on_real_server() {
    let harness = LiveEnginePipeTestHarness::start(1, "legit_token_999").await;

    // To test out-of-process caller security where PID != engine PID:
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = harness.engine.create_ipc_handler(1, shutdown_tx);

    let external_identity = IpcClientIdentity {
        process_id: 88888,
        session_id: 1,
    };

    // Case 1a: Out-of-process caller with NO token
    let req = IpcRequest::new("shutdown", serde_json::json!({}));
    let resp = (handler)(req, external_identity).await;
    assert!(
        !resp.success,
        "Out-of-process caller with missing token must be rejected"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1b: Out-of-process caller with INVALID token
    let req = IpcRequest::new(
        "shutdown",
        serde_json::json!({ "session_token": "wrong_token_xyz" }),
    );
    let resp = (handler)(req, external_identity).await;
    assert!(
        !resp.success,
        "Out-of-process caller with invalid token must be rejected"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1c: Out-of-process caller with empty string token
    let req = IpcRequest::new("pause_audio", serde_json::json!({ "session_token": "" }));
    let resp = (handler)(req, external_identity).await;
    assert!(!resp.success, "Empty token must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1d: Valid token from out-of-process caller -> Accepted
    let req = IpcRequest::new(
        "pause_audio",
        serde_json::json!({ "session_token": "legit_token_999" }),
    );
    let resp = (handler)(req, external_identity).await;
    assert!(
        resp.success,
        "Valid token from out-of-process caller must succeed"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_negative_cross_session_caller_rejected_by_production_handler() {
    let engine = AppEngine::with_token("token_sess_1");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = engine.create_ipc_handler(1, shutdown_tx);

    // Caller from Session 2 connecting to Session 1 engine
    let cross_session_identity = IpcClientIdentity {
        process_id: 1234,
        session_id: 2,
    };

    let req = IpcRequest::new("get_status", serde_json::json!({}));
    let resp = (handler)(req, cross_session_identity).await;
    assert!(!resp.success, "Cross-session caller must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("cross-session caller rejected"));
}

#[tokio::test]
async fn test_negative_dead_worker_and_wrong_pids_rejected() {
    let engine = AppEngine::with_token("token_worker_test");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = engine.create_ipc_handler(1, shutdown_tx);

    // 1. No child worker exists in supervisor -> registration fails closed
    let identity = IpcClientIdentity {
        process_id: 4000,
        session_id: 1,
    };
    let req = IpcRequest::new("script_worker_ready", serde_json::json!({ "pid": 4000 }));
    let resp = (handler)(req, identity).await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .unwrap()
        .contains("no supervised worker child is currently expected"));

    // 2. Claimed PID does not match caller PID -> rejected
    let req = IpcRequest::new("script_worker_ready", serde_json::json!({ "pid": 9999 }));
    let resp = (handler)(req, identity).await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .unwrap()
        .contains("claimed PID does not match IPC caller PID"));

    // 3. Supervised child is dead -> rejected
    let mut dummy_proc = Command::new("cmd.exe")
        .args(["/c", "exit", "0"])
        .spawn()
        .expect("spawn short-lived cmd");
    let dead_pid = dummy_proc.id();
    let _ = dummy_proc.wait(); // Process exits immediately, now DEAD

    {
        let sup_arc = engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        let dead_handle = windows::Win32::Foundation::HANDLE(dummy_proc.as_raw_handle() as *mut _);
        let dead_child = SupervisedChild::new(dead_handle, dead_pid);
        sup.assign_supervised_child(dead_child);
    }

    let dead_identity = IpcClientIdentity {
        process_id: dead_pid,
        session_id: 1,
    };

    let req = IpcRequest::new(
        "script_worker_ready",
        serde_json::json!({ "pid": dead_pid }),
    );
    let resp = (handler)(req, dead_identity).await;
    assert!(!resp.success, "Dead worker registration must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("worker process is no longer alive"));

    // Dead worker heartbeat must also be rejected
    let hb_req = IpcRequest::new("script_worker_heartbeat", serde_json::json!({}));
    let hb_resp = (handler)(hb_req, dead_identity).await;
    assert!(!hb_resp.success, "Dead worker heartbeat must be rejected");
    assert!(hb_resp
        .error_message
        .unwrap()
        .contains("no supervised worker child exists to heartbeat"));
}

#[tokio::test]
async fn test_negative_worker_role_boundary_and_process_replacement() {
    let engine = AppEngine::with_token("token_role_boundary");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = engine.create_ipc_handler(1, shutdown_tx);

    // Spawn live dummy process Worker A
    let mut worker_a = Command::new("cmd.exe")
        .args(["/c", "ping", "-n", "5", "127.0.0.1"])
        .spawn()
        .expect("spawn worker A");
    let pid_a = worker_a.id();

    {
        let sup_arc = engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        let handle_a = windows::Win32::Foundation::HANDLE(worker_a.as_raw_handle() as *mut _);
        sup.assign_supervised_child(SupervisedChild::new(handle_a, pid_a));
    }

    let identity_a = IpcClientIdentity {
        process_id: pid_a,
        session_id: 1,
    };

    // 1. Worker A tries privileged management commands -> strictly rejected even if token passed
    for cmd in &[
        "shutdown",
        "enter_interactive_mode",
        "pause_audio",
        "reload_profile",
    ] {
        let req = IpcRequest::new(
            *cmd,
            serde_json::json!({ "session_token": "token_role_boundary" }),
        );
        let resp = (handler)(req, identity_a).await;
        assert!(!resp.success, "Worker A must be rejected from {}", cmd);
        assert!(resp
            .error_message
            .unwrap()
            .contains("script worker is not authorized to invoke privileged management commands"));
    }

    // 2. Worker A queries allowed telemetry -> permitted
    let telem_req = IpcRequest::new("get_telemetry", serde_json::json!({}));
    let telem_resp = (handler)(telem_req, identity_a).await;
    assert!(
        telem_resp.success,
        "Worker A querying telemetry must succeed"
    );

    // 3. Worker Process Replacement:
    // Terminate Worker A and spawn Worker B
    let _ = worker_a.kill();
    let _ = worker_a.wait();

    let mut worker_b = Command::new("cmd.exe")
        .args(["/c", "ping", "-n", "5", "127.0.0.1"])
        .spawn()
        .expect("spawn worker B");
    let pid_b = worker_b.id();

    {
        let sup_arc = engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        let handle_b = windows::Win32::Foundation::HANDLE(worker_b.as_raw_handle() as *mut _);
        sup.assign_supervised_child(SupervisedChild::new(handle_b, pid_b));
    }

    let identity_b = IpcClientIdentity {
        process_id: pid_b,
        session_id: 1,
    };

    // Now old Worker A attempts heartbeat -> rejected
    let old_hb_req = IpcRequest::new("script_worker_heartbeat", serde_json::json!({}));
    let old_hb_resp = (handler)(old_hb_req, identity_a).await;
    assert!(
        !old_hb_resp.success,
        "Replaced old worker PID must be rejected"
    );

    // New Worker B heartbeat -> accepted
    let new_hb_req = IpcRequest::new("script_worker_heartbeat", serde_json::json!({}));
    let new_hb_resp = (handler)(new_hb_req, identity_b).await;
    assert!(
        new_hb_resp.success,
        "Newly assigned worker B heartbeat must succeed"
    );

    let _ = worker_b.kill();
    let _ = worker_b.wait();
}

#[tokio::test]
async fn test_negative_quarantined_supervisor_rejects_worker_actions() {
    let engine = AppEngine::with_token("token_quarantine");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = engine.create_ipc_handler(1, shutdown_tx);

    // Force supervisor into quarantined state by exceeding max crashes
    {
        let sup_arc = engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.record_start(5001);
        sup.record_exit(1);
        sup.record_start(5002);
        sup.record_exit(1);
        assert!(sup.is_quarantined(), "Supervisor must be quarantined");
    }

    let identity = IpcClientIdentity {
        process_id: 5003,
        session_id: 1,
    };

    // Attempt ready -> rejected
    let ready_req = IpcRequest::new("script_worker_ready", serde_json::json!({ "pid": 5003 }));
    let ready_resp = (handler)(ready_req, identity).await;
    assert!(
        !ready_resp.success,
        "Quarantined supervisor must reject ready"
    );
    assert!(ready_resp
        .error_message
        .unwrap()
        .contains("supervisor is quarantined"));

    // Attempt heartbeat -> rejected
    let hb_req = IpcRequest::new("script_worker_heartbeat", serde_json::json!({}));
    let hb_resp = (handler)(hb_req, identity).await;
    assert!(
        !hb_resp.success,
        "Quarantined supervisor must reject heartbeat"
    );
    assert!(hb_resp
        .error_message
        .unwrap()
        .contains("supervisor is quarantined"));
}
