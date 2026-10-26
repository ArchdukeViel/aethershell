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

impl LiveEnginePipeTestHarness {
    async fn start(session_token: &str) -> Self {
        Self::start_with_session(session_token, current_session_id()).await
    }

    async fn start_with_session(session_token: &str, session_id: u32) -> Self {
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
    let harness = LiveEnginePipeTestHarness::start("valid_secret_token_live").await;

    // 1. Send pause_audio with valid session_token over real Named Pipe -> verifies live audio state changes
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

    // 2. Send resume_audio with valid session_token over real Named Pipe -> verifies live audio state unpaused
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

    // 3. Send enter_interactive_mode over real Named Pipe -> verifies live interaction controller state
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

    // 4. Send exit_interactive_mode over real Named Pipe -> verifies live return to LockedPassive
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

    // 5. Send shutdown with valid session_token over real Named Pipe -> verifies live engine shutdown flag
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
    let harness = LiveEnginePipeTestHarness::start("legit_token_999").await;

    // Case 1a: Caller over real Named Pipe with NO token -> rejected
    let resp = IpcClient::send_command(&harness.pipe_name, "shutdown", serde_json::json!({}))
        .await
        .expect("send over real pipe");
    assert!(
        !resp.success,
        "Caller with missing token over real Named Pipe must be rejected"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1b: Caller over real Named Pipe with INVALID token -> rejected
    let resp = IpcClient::send_command(
        &harness.pipe_name,
        "shutdown",
        serde_json::json!({ "session_token": "wrong_token_xyz" }),
    )
    .await
    .expect("send over real pipe");
    assert!(
        !resp.success,
        "Caller with invalid token over real Named Pipe must be rejected"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1c: Caller over real Named Pipe with empty string token -> rejected
    let resp = IpcClient::send_command(
        &harness.pipe_name,
        "pause_audio",
        serde_json::json!({ "session_token": "" }),
    )
    .await
    .expect("send over real pipe");
    assert!(
        !resp.success,
        "Caller with empty token over real Named Pipe must be rejected"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // Case 1d: Caller over real Named Pipe with valid token -> Accepted
    let resp = IpcClient::send_command(
        &harness.pipe_name,
        "pause_audio",
        serde_json::json!({ "session_token": "legit_token_999" }),
    )
    .await
    .expect("send over real pipe");
    assert!(
        resp.success,
        "Valid token over real Named Pipe must succeed"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_negative_cross_session_caller_rejected_by_production_handler() {
    let my_session = current_session_id();
    // 1. Test cross-session rejection over the REAL Named Pipe
    // Server is configured for a foreign session; kernel reports client's actual session
    let foreign_session_id = my_session + 99;
    let harness =
        LiveEnginePipeTestHarness::start_with_session("token_sess_1", foreign_session_id).await;

    let resp = IpcClient::send_command(&harness.pipe_name, "get_status", serde_json::json!({}))
        .await
        .expect("send across pipe");

    assert!(
        !resp.success,
        "Cross-session caller must be rejected over real Named Pipe"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("cross-session caller rejected"));

    harness.stop().await;

    // 2. Defense-in-depth: test production handler closure directly with cross-session identity
    let engine = AppEngine::with_token("token_sess_1");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = engine.create_ipc_handler(my_session, shutdown_tx);

    let cross_session_identity = IpcClientIdentity {
        process_id: 1234,
        session_id: my_session + 99,
    };

    let req = IpcRequest::new("get_status", serde_json::json!({}));
    let handler_resp = (handler)(req, cross_session_identity).await;
    assert!(!handler_resp.success);
    assert!(handler_resp
        .error_message
        .unwrap()
        .contains("cross-session caller rejected"));
}

#[tokio::test]
async fn test_negative_dead_worker_and_wrong_pids_rejected() {
    let harness = LiveEnginePipeTestHarness::start("token_worker_test").await;
    let my_session = current_session_id();
    let my_pid = std::process::id();

    // 1. No child worker exists in supervisor -> registration fails closed over real pipe
    let resp = IpcClient::send_command(
        &harness.pipe_name,
        "script_worker_ready",
        serde_json::json!({ "pid": my_pid }),
    )
    .await
    .expect("send over pipe");
    assert!(!resp.success);
    assert!(resp
        .error_message
        .unwrap()
        .contains("no supervised worker child is currently expected"));

    // 2. Claimed PID does not match caller PID -> rejected over real pipe
    let dummy_handle = windows::Win32::Foundation::HANDLE::default();
    {
        let sup_arc = harness.engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.assign_supervised_child(SupervisedChild::new(dummy_handle, my_pid));
    }

    let spoof_resp = IpcClient::send_command(
        &harness.pipe_name,
        "script_worker_ready",
        serde_json::json!({ "pid": 999999 }),
    )
    .await
    .expect("send spoofed PID over pipe");
    assert!(!spoof_resp.success);
    assert!(spoof_resp
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
        let sup_arc = harness.engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        let dead_handle = windows::Win32::Foundation::HANDLE(dummy_proc.as_raw_handle() as *mut _);
        let dead_child = SupervisedChild::new(dead_handle, dead_pid);
        sup.assign_supervised_child(dead_child);
    }

    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let handler = harness.engine.create_ipc_handler(my_session, shutdown_tx);
    let dead_identity = IpcClientIdentity {
        process_id: dead_pid,
        session_id: my_session,
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

    harness.stop().await;
}

#[tokio::test]
async fn test_negative_worker_role_boundary_and_process_replacement() {
    let harness = LiveEnginePipeTestHarness::start("token_role_boundary").await;
    let my_pid = std::process::id();

    // Assign caller PID as the supervised worker
    let my_handle = windows::Win32::Foundation::HANDLE::default();
    {
        let sup_arc = harness.engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.assign_supervised_child(SupervisedChild::new(my_handle, my_pid));
    }

    // 1. Worker tries privileged management commands over real Named Pipe -> strictly rejected even if token passed
    for cmd in &[
        "shutdown",
        "enter_interactive_mode",
        "pause_audio",
        "reload_profile",
    ] {
        let resp = IpcClient::send_command(
            &harness.pipe_name,
            *cmd,
            serde_json::json!({ "session_token": "token_role_boundary" }),
        )
        .await
        .expect("send command over pipe");
        assert!(!resp.success, "Worker must be rejected from {}", cmd);
        assert!(resp
            .error_message
            .unwrap()
            .contains("script worker is not authorized to invoke privileged management commands"));
    }

    // 2. Worker queries allowed telemetry over real Named Pipe -> permitted
    let telem_resp =
        IpcClient::send_command(&harness.pipe_name, "get_telemetry", serde_json::json!({}))
            .await
            .expect("query telemetry over pipe");
    assert!(
        telem_resp.success,
        "Worker querying telemetry over real pipe must succeed"
    );

    // 3. Worker Process Replacement:
    // Reassign supervisor to a new worker PID (simulating worker replacement)
    {
        let sup_arc = harness.engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.assign_supervised_child(SupervisedChild::new(
            windows::Win32::Foundation::HANDLE::default(),
            777777,
        ));
    }

    // Old caller PID attempts heartbeat over real Named Pipe -> rejected as untrusted
    let old_hb_resp = IpcClient::send_command(
        &harness.pipe_name,
        "script_worker_heartbeat",
        serde_json::json!({}),
    )
    .await
    .expect("send heartbeat over pipe");
    assert!(
        !old_hb_resp.success,
        "Replaced old worker PID must be rejected over real pipe"
    );
    assert!(old_hb_resp
        .error_message
        .unwrap()
        .contains("untrusted worker PID"));

    harness.stop().await;
}

#[tokio::test]
async fn test_negative_quarantined_supervisor_rejects_worker_actions() {
    let harness = LiveEnginePipeTestHarness::start("token_quarantine").await;
    let my_pid = std::process::id();

    // Assign caller PID as supervised child
    let my_handle = windows::Win32::Foundation::HANDLE::default();
    {
        let sup_arc = harness.engine.script_supervisor();
        let mut sup = sup_arc.lock().await;
        sup.assign_supervised_child(SupervisedChild::new(my_handle, my_pid));
        // Force supervisor into quarantined state by exceeding max crashes
        sup.record_start(5001);
        sup.record_exit(1);
        sup.record_start(5002);
        sup.record_exit(1);
        assert!(sup.is_quarantined(), "Supervisor must be quarantined");
    }

    // Attempt ready over real Named Pipe -> rejected
    let ready_resp = IpcClient::send_command(
        &harness.pipe_name,
        "script_worker_ready",
        serde_json::json!({ "pid": my_pid }),
    )
    .await
    .expect("send ready over pipe");
    assert!(
        !ready_resp.success,
        "Quarantined supervisor must reject ready over real pipe"
    );
    assert!(ready_resp
        .error_message
        .unwrap()
        .contains("supervisor is quarantined"));

    // Attempt heartbeat over real Named Pipe -> rejected
    let hb_resp = IpcClient::send_command(
        &harness.pipe_name,
        "script_worker_heartbeat",
        serde_json::json!({}),
    )
    .await
    .expect("send heartbeat over pipe");
    assert!(
        !hb_resp.success,
        "Quarantined supervisor must reject heartbeat over real pipe"
    );
    assert!(hb_resp
        .error_message
        .unwrap()
        .contains("supervisor is quarantined"));

    harness.stop().await;
}
