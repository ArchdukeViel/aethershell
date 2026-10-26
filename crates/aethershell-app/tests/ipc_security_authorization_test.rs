use aethershell_ipc::{IpcClientIdentity, IpcRequest, IpcResponse};
use aethershell_script_host::ScriptSupervisor;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

/// Helper harness creating the engine's command handler with controllable state.
#[allow(dead_code)]
struct TestIpcHarness {
    session_token: String,
    session_id: u32,
    supervisor: Arc<Mutex<ScriptSupervisor>>,
    shutdown_flag: Arc<AtomicBool>,
    handler: aethershell_ipc::RequestHandler,
}

impl TestIpcHarness {
    fn new(session_id: u32, session_token: &str) -> Self {
        let supervisor = Arc::new(Mutex::new(ScriptSupervisor::new(2)));
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);

        let sup_clone = Arc::clone(&supervisor);
        let flag_clone = Arc::clone(&shutdown_flag);
        let expected_token = session_token.to_string();
        let valid_session_id = session_id;

        let handler = Arc::new(move |req: IpcRequest, client_identity: IpcClientIdentity| {
            let supervisor = Arc::clone(&sup_clone);
            let shutdown_flag = Arc::clone(&flag_clone);
            let shutdown_tx = shutdown_tx.clone();
            let session_token = expected_token.clone();

            Box::pin(async move {
                // 1. Session Isolation
                if client_identity.session_id != valid_session_id {
                    return IpcResponse::error(
                        req.message_id,
                        "Security violation: cross-session caller rejected",
                    );
                }

                // 2. Worker Role Separation
                let is_script_worker = {
                    let sup = supervisor.lock().await;
                    sup.active_child_pid() == Some(client_identity.process_id)
                };

                if is_script_worker
                    && !matches!(
                        req.command.as_str(),
                        "script_worker_ready"
                            | "script_worker_heartbeat"
                            | "get_telemetry"
                            | "get_spectrum"
                            | "get_status"
                    )
                {
                    return IpcResponse::error(
                        req.message_id,
                        "Security violation: script worker is not authorized to invoke privileged management commands",
                    );
                }

                // 3. Privileged Management Commands Authorization
                let is_management_command = matches!(
                    req.command.as_str(),
                    "shutdown"
                        | "enter_interactive_mode"
                        | "exit_interactive_mode"
                        | "reload_profile"
                        | "update_layout"
                        | "pause_audio"
                        | "resume_audio"
                );

                if is_management_command {
                    let is_same_process = client_identity.process_id == std::process::id();
                    let has_valid_token = req
                        .payload
                        .get("session_token")
                        .or_else(|| req.payload.get("token"))
                        .and_then(|v| v.as_str())
                        .map(|tok| !tok.is_empty() && tok == session_token.as_str())
                        .unwrap_or(false);

                    if !is_same_process && !has_valid_token {
                        return IpcResponse::error(
                            req.message_id,
                            "Security violation: unauthorized caller; valid session capability token required",
                        );
                    }
                }

                match req.command.as_str() {
                    "get_status" | "get_telemetry" | "get_spectrum" => {
                        IpcResponse::ok(req.message_id, serde_json::json!({ "status": "ok" }))
                    }
                    "script_worker_ready" => {
                        let claimed_pid = req.payload["pid"].as_u64().unwrap_or(0) as u32;
                        let mut sup = supervisor.lock().await;

                        if sup.is_quarantined() {
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: supervisor is quarantined; worker registration forbidden",
                            )
                        } else if claimed_pid == 0 || claimed_pid != client_identity.process_id {
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: claimed PID does not match IPC caller PID",
                            )
                        } else if let Some(child_pid) = sup.active_child_pid() {
                            if child_pid != client_identity.process_id {
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: unexpected worker process identity",
                                )
                            } else if !sup.is_child_alive() {
                                sup.invalidate_worker();
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: worker process is no longer alive",
                                )
                            } else {
                                sup.record_start(client_identity.process_id);
                                IpcResponse::ok(
                                    req.message_id,
                                    serde_json::json!({ "registered": true, "pid": client_identity.process_id }),
                                )
                            }
                        } else {
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: no supervised worker child is currently expected",
                            )
                        }
                    }
                    "script_worker_heartbeat" => {
                        let mut sup = supervisor.lock().await;
                        if sup.is_quarantined() {
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: supervisor is quarantined",
                            )
                        } else if let Some(child_pid) = sup.active_child_pid() {
                            if child_pid != client_identity.process_id {
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: untrusted worker PID",
                                )
                            } else if !sup.is_child_alive() {
                                sup.invalidate_worker();
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: worker process is dead",
                                )
                            } else {
                                sup.record_heartbeat();
                                IpcResponse::ok(
                                    req.message_id,
                                    serde_json::json!({ "acknowledged": true }),
                                )
                            }
                        } else {
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: no supervised worker child exists to heartbeat",
                            )
                        }
                    }
                    "enter_interactive_mode" => {
                        IpcResponse::ok(req.message_id, serde_json::json!({ "active": true }))
                    }
                    "shutdown" => {
                        shutdown_flag.store(true, Ordering::SeqCst);
                        let _ = shutdown_tx.send(true);
                        IpcResponse::ok(req.message_id, serde_json::json!({ "quitting": true }))
                    }
                    _ => IpcResponse::error(req.message_id, "Unknown command"),
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = IpcResponse> + Send>>
        });

        Self {
            session_token: session_token.to_string(),
            session_id,
            supervisor,
            shutdown_flag,
            handler,
        }
    }

    async fn dispatch(
        &self,
        command: &str,
        payload: serde_json::Value,
        client_pid: u32,
        client_session: u32,
    ) -> IpcResponse {
        let req = IpcRequest::new(command, payload);
        let identity = IpcClientIdentity {
            process_id: client_pid,
            session_id: client_session,
        };
        (self.handler)(req, identity).await
    }
}

#[tokio::test]
async fn test_worker_registration_bypass_eliminated() {
    let harness = TestIpcHarness::new(1, "test_token_secret");

    // Case 1: Caller tries to register when NO child is supervised -> must fail closed
    let resp = harness
        .dispatch(
            "script_worker_ready",
            serde_json::json!({ "pid": 4242 }),
            4242,
            1,
        )
        .await;
    assert!(
        !resp.success,
        "Registration without active child must fail closed"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("no supervised worker child is currently expected"));

    // Case 2: Spoofed claimed PID -> must fail closed
    let resp = harness
        .dispatch(
            "script_worker_ready",
            serde_json::json!({ "pid": 9999 }),
            4242,
            1,
        )
        .await;
    assert!(!resp.success, "Spoofed claimed PID must fail closed");
    assert!(resp
        .error_message
        .unwrap()
        .contains("claimed PID does not match IPC caller PID"));
}

#[tokio::test]
async fn test_worker_heartbeat_bypass_eliminated() {
    let harness = TestIpcHarness::new(1, "test_token_secret");

    // Case 1: Heartbeat when NO child is supervised -> must fail closed
    let resp = harness
        .dispatch("script_worker_heartbeat", serde_json::json!({}), 4242, 1)
        .await;
    assert!(
        !resp.success,
        "Heartbeat without active child must fail closed"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("no supervised worker child exists to heartbeat"));
}

#[tokio::test]
async fn test_supervised_worker_role_boundary_enforcement() {
    let harness = TestIpcHarness::new(1, "test_token_secret");

    // Simulate spawning a worker child (PID 7777)
    // Spawn a dummy process so is_child_alive() is true
    let mut dummy_proc = std::process::Command::new("cmd.exe")
        .args(["/c", "ping", "-n", "3", "127.0.0.1"])
        .spawn()
        .expect("spawn dummy");
    let real_pid = dummy_proc.id();

    // Spawn worker in supervisor
    {
        let mut sup = harness.supervisor.lock().await;
        let fake_handle = windows::Win32::Foundation::HANDLE(dummy_proc.as_raw_handle() as *mut _);
        let supervised = aethershell_script_host::SupervisedChild::new(fake_handle, real_pid);
        sup.assign_supervised_child(supervised);
    }

    // 1. Worker tries to call shutdown -> MUST BE REJECTED
    let resp = harness
        .dispatch(
            "shutdown",
            serde_json::json!({ "session_token": "test_token_secret" }),
            real_pid,
            1,
        )
        .await;
    assert!(
        !resp.success,
        "Script worker must be strictly forbidden from invoking shutdown"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("script worker is not authorized to invoke privileged management commands"));

    // 2. Worker tries to call enter_interactive_mode -> MUST BE REJECTED
    let resp = harness
        .dispatch(
            "enter_interactive_mode",
            serde_json::json!({ "session_token": "test_token_secret" }),
            real_pid,
            1,
        )
        .await;
    assert!(
        !resp.success,
        "Script worker must be forbidden from interactive mode"
    );
    assert!(resp
        .error_message
        .unwrap()
        .contains("script worker is not authorized to invoke privileged management commands"));

    // 3. Worker queries telemetry -> ALLOWED
    let resp = harness
        .dispatch("get_telemetry", serde_json::json!({}), real_pid, 1)
        .await;
    assert!(resp.success, "Script worker reading telemetry must succeed");

    let _ = dummy_proc.kill();
}

#[tokio::test]
async fn test_privileged_commands_require_session_capability_token() {
    let harness = TestIpcHarness::new(1, "correct_secret_session_token_123");
    let external_client_pid = 99999; // Different from std::process::id()

    // 1. Unauthenticated shutdown (no token) -> Rejected
    let resp = harness
        .dispatch("shutdown", serde_json::json!({}), external_client_pid, 1)
        .await;
    assert!(!resp.success, "Shutdown without token must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // 2. Shutdown with wrong token -> Rejected
    let resp = harness
        .dispatch(
            "shutdown",
            serde_json::json!({ "session_token": "wrong_token_xyz" }),
            external_client_pid,
            1,
        )
        .await;
    assert!(!resp.success, "Shutdown with wrong token must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // 3. Shutdown with correct session token -> Accepted
    assert!(!harness.shutdown_flag.load(Ordering::SeqCst));
    let resp = harness
        .dispatch(
            "shutdown",
            serde_json::json!({ "session_token": "correct_secret_session_token_123" }),
            external_client_pid,
            1,
        )
        .await;
    assert!(
        resp.success,
        "Shutdown with valid session token must succeed"
    );
    assert!(harness.shutdown_flag.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cross_session_ipc_rejection() {
    let harness = TestIpcHarness::new(1, "token");
    let cross_session_id = 2; // Different from engine session 1

    let resp = harness
        .dispatch("get_status", serde_json::json!({}), 5555, cross_session_id)
        .await;
    assert!(!resp.success, "Cross-session client must be rejected");
    assert!(resp
        .error_message
        .unwrap()
        .contains("cross-session caller rejected"));
}
