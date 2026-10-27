use aethershell_app::AppEngine;
use aethershell_core::InteractionMode;
use aethershell_ipc::{IpcClient, IpcClientIdentity, IpcRequest, IpcServer};
use aethershell_script_host::SupervisedChild;
use std::os::windows::io::AsRawHandle;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

static ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn find_cpp_settings_binary() -> Option<std::path::PathBuf> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent()?.parent()?;
    let candidates = [
        repo_root.join(r"apps\aethershell-settings\x64\Release\aethershell-settings.exe"),
        repo_root.join(r"apps/aethershell-settings/x64/Release/aethershell-settings.exe"),
        std::path::PathBuf::from(r"apps\aethershell-settings\x64\Release\aethershell-settings.exe"),
        std::path::PathBuf::from(
            r"..\apps\aethershell-settings\x64\Release\aethershell-settings.exe",
        ),
        std::path::PathBuf::from(
            r"..\..\apps\aethershell-settings\x64\Release\aethershell-settings.exe",
        ),
    ];
    candidates.into_iter().find(|p| p.exists())
}

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

    async fn start_with_pipe_and_session(
        session_token: &str,
        pipe_name: &str,
        session_id: u32,
    ) -> Self {
        let engine = Arc::new(AppEngine::with_token(session_token));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handler = engine.create_ipc_handler(session_id, shutdown_tx.clone());
        let server = IpcServer::new(pipe_name, handler, shutdown_rx);

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            engine,
            pipe_name: pipe_name.to_string(),
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

#[tokio::test]
async fn test_runtime_token_rotation_through_production_ipc_handler() {
    let _env_lock = ENV_MUTEX.lock().await;
    let custom_dir = std::env::temp_dir().join(format!("test_rot_live_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&custom_dir);
    let token_file = custom_dir.join("session_live.token");
    std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &token_file);

    let initial_token = "initial_secret_token_12345".to_string();
    let mut guard = aethershell_ipc::SessionTokenGuard::with_token(&initial_token)
        .expect("create initial guard");

    let harness = LiveEnginePipeTestHarness::start(&initial_token).await;

    // 1. Authenticate with initial token over live Named Pipe -> succeeds
    let initial_res = IpcClient::send_command(
        &harness.pipe_name,
        "pause_audio",
        serde_json::json!({ "session_token": &initial_token }),
    )
    .await
    .expect("send pause with initial token");
    assert!(initial_res.success, "Initial token must be accepted");
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(aud, "Audio must be paused");
    }

    // 2. Perform live runtime token rotation on engine
    let new_token = harness
        .engine
        .rotate_session_token(&mut guard)
        .expect("rotate token");
    assert_ne!(new_token, initial_token);
    assert_eq!(harness.engine.session_token(), new_token);

    // 3. Attempt command with old/stale token -> must be REJECTED
    let stale_res = IpcClient::send_command(
        &harness.pipe_name,
        "resume_audio",
        serde_json::json!({ "session_token": &initial_token }),
    )
    .await
    .expect("send resume with stale token");
    assert!(
        !stale_res.success,
        "Stale token must be rejected after rotation"
    );
    assert!(stale_res
        .error_message
        .unwrap()
        .contains("valid session capability token required"));

    // 4. Client reloads rotated token from disk (mirroring client behavior)
    let reloaded_token =
        aethershell_ipc::SessionToken::load().expect("load rotated token from disk");
    assert_eq!(reloaded_token, new_token);

    // 5. Send command with newly rotated token -> SUCCEEDS without engine restart!
    let rotated_res = IpcClient::send_command(
        &harness.pipe_name,
        "resume_audio",
        serde_json::json!({ "session_token": &reloaded_token }),
    )
    .await
    .expect("send resume with rotated token");
    assert!(
        rotated_res.success,
        "Newly rotated token must be accepted immediately"
    );
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(!aud, "Audio must be resumed");
    }

    harness.stop().await;
    drop(guard);
    std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
    let _ = std::fs::remove_dir_all(custom_dir);
}

#[tokio::test]
async fn test_token_rotation_poisoned_lock_fails_closed_without_disk_divergence() {
    let _env_lock = ENV_MUTEX.lock().await;
    let custom_dir = std::env::temp_dir().join(format!("test_rot_poison_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&custom_dir);
    let token_file = custom_dir.join("session_poison.token");
    std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &token_file);

    let initial_token = "initial_token_before_poison_99".to_string();
    let mut guard = aethershell_ipc::SessionTokenGuard::with_token(&initial_token)
        .expect("create initial guard");

    let engine = Arc::new(AppEngine::with_token(&initial_token));

    // Verify initial consistency
    assert_eq!(engine.session_token(), initial_token);
    assert_eq!(
        aethershell_ipc::SessionToken::load().expect("load initial from disk"),
        initial_token
    );

    // 1. Poison runtime session token write lock
    engine.poison_session_token_lock_for_test();

    // 2. Attempt token rotation on engine
    let rotate_result = engine.rotate_session_token(&mut guard);

    // 3. Must return an error (lock failure reported)
    assert!(
        rotate_result.is_err(),
        "rotate_session_token must fail and report error when lock is poisoned"
    );
    let err_msg = rotate_result.err().unwrap().to_string();
    assert!(
        err_msg.contains("Failed to acquire session token write lock")
            || err_msg.contains("poisoned"),
        "Error message must indicate lock failure: {}",
        err_msg
    );

    // 4. Verify disk and runtime states did NOT silently diverge:
    // Disk MUST still contain the initial token, NOT a newly generated token!
    let disk_token =
        aethershell_ipc::SessionToken::load().expect("load from disk after failed rotation");
    assert_eq!(
        disk_token, initial_token,
        "Disk token must remain unchanged when lock acquisition fails"
    );
    assert_eq!(
        guard.token(),
        initial_token,
        "Guard token must remain unchanged when lock acquisition fails"
    );

    drop(guard);
    std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
    let _ = std::fs::remove_dir_all(custom_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rust_cli_settings_client_interoperability() {
    let _env_lock = ENV_MUTEX.lock().await;
    let custom_dir = std::env::temp_dir().join(format!("test_cli_iso_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&custom_dir);
    let token_file = custom_dir.join("cli_session.token");
    std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &token_file);

    let pipe_name = format!(
        r"\\.\pipe\aethershell-rust-cli-test-{}",
        uuid::Uuid::new_v4()
    );
    let token = format!("rust_cli_token_{}", uuid::Uuid::new_v4());

    let guard = aethershell_ipc::SessionTokenGuard::with_token(&token)
        .expect("persist session token for Rust CLI client");

    let harness = LiveEnginePipeTestHarness::start_with_pipe_and_session(
        &token,
        &pipe_name,
        current_session_id(),
    )
    .await;

    // Locate or build cargo-built aethershell-settings binary
    let target_dir_bin = std::path::Path::new(env!("CARGO_BIN_EXE_aethershell"))
        .with_file_name("aethershell-settings.exe");
    let bin_path = if target_dir_bin.exists() {
        target_dir_bin
    } else {
        let mut cmd = std::process::Command::new("cargo");
        cmd.args([
            "build",
            "-p",
            "aethershell-settings",
            "--bin",
            "aethershell-settings",
        ]);
        let status = cmd.status().expect("cargo build aethershell-settings");
        assert!(status.success());
        target_dir_bin
    };

    // 1. Invoke settings client binary to execute privileged 'pause_audio' over isolated test pipe
    let output_pause = tokio::process::Command::new(&bin_path)
        .args(["--pipe", &pipe_name, "--pause-audio"])
        .output()
        .await
        .expect("execute settings client pause_audio");

    assert!(
        output_pause.status.success(),
        "Settings client pause_audio must succeed: stderr={}",
        String::from_utf8_lossy(&output_pause.stderr)
    );

    // Verify live core state changed
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(aud, "Audio must be paused by settings client");
    }

    // 2. Invoke settings client binary to execute privileged 'resume_audio' over isolated test pipe
    let output_resume = tokio::process::Command::new(&bin_path)
        .args(["--pipe", &pipe_name, "--resume-audio"])
        .output()
        .await
        .expect("execute settings client resume_audio");

    assert!(
        output_resume.status.success(),
        "Settings client resume_audio must succeed: stderr={}",
        String::from_utf8_lossy(&output_resume.stderr)
    );

    // Verify live core state changed
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(!aud, "Audio must be resumed by settings client");
    }

    harness.stop().await;
    drop(guard);
    std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
    let _ = std::fs::remove_dir_all(custom_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cpp_winui_settings_client_authenticates_privileged_command() {
    let cpp_bin = find_cpp_settings_binary();
    if cpp_bin.is_none() {
        if std::env::var("REQUIRE_CPP_SETTINGS_TEST").is_ok() {
            panic!("REQUIRED_CPP_BIN_MISSING: C++/WinUI 3 settings client binary not found.");
        } else {
            eprintln!("STATUS_CPP_BIN_UNAVAILABLE: Real C++ Settings executable not found; skipping C++ verification without falling back to Rust CLI.");
            return;
        }
    }
    let bin_path = cpp_bin.unwrap();

    let _env_lock = ENV_MUTEX.lock().await;
    let custom_dir = std::env::temp_dir().join(format!("test_cpp_auth_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&custom_dir);
    let token_file = custom_dir.join("cpp_session.token");
    std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &token_file);

    let pipe_name = format!(
        r"\\.\pipe\aethershell-cpp-auth-test-{}",
        uuid::Uuid::new_v4()
    );
    let token = format!("cpp_winui_token_{}", uuid::Uuid::new_v4());

    let guard = aethershell_ipc::SessionTokenGuard::with_token(&token)
        .expect("persist session token for C++ client");

    let harness = LiveEnginePipeTestHarness::start_with_pipe_and_session(
        &token,
        &pipe_name,
        current_session_id(),
    )
    .await;

    // 1. Invoke C++/WinUI Settings client binary to execute privileged 'pause_audio' over isolated test pipe
    let output_pause = tokio::process::Command::new(&bin_path)
        .args([
            "--test-ipc-pipe",
            &pipe_name,
            "--test-ipc-command",
            "pause_audio",
        ])
        .output()
        .await
        .expect("execute C++ settings client pause_audio");

    assert!(
        output_pause.status.success(),
        "C++ Settings client pause_audio must succeed: stderr={}",
        String::from_utf8_lossy(&output_pause.stderr)
    );
    let stdout_pause = String::from_utf8_lossy(&output_pause.stdout);
    assert!(
        stdout_pause.contains("IPC_TEST_SUCCESS:"),
        "C++ Settings client must report IPC success: {}",
        stdout_pause
    );

    // Verify live core state changed
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(aud, "Audio must be paused by C++ settings client");
    }

    // 2. Invoke C++/WinUI Settings client binary to execute privileged 'resume_audio' over isolated test pipe
    let output_resume = tokio::process::Command::new(&bin_path)
        .args([
            "--test-ipc-pipe",
            &pipe_name,
            "--test-ipc-command",
            "resume_audio",
        ])
        .output()
        .await
        .expect("execute C++ settings client resume_audio");

    assert!(
        output_resume.status.success(),
        "C++ Settings client resume_audio must succeed: stderr={}",
        String::from_utf8_lossy(&output_resume.stderr)
    );
    let stdout_resume = String::from_utf8_lossy(&output_resume.stdout);
    assert!(
        stdout_resume.contains("IPC_TEST_SUCCESS:"),
        "C++ Settings client must report IPC success: {}",
        stdout_resume
    );

    // Verify live core state changed
    {
        let aud = harness.engine.audio().lock().await.is_paused();
        assert!(!aud, "Audio must be resumed by C++ settings client");
    }

    harness.stop().await;
    drop(guard);
    std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
    let _ = std::fs::remove_dir_all(custom_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cpp_winui_settings_client_rejects_unauthorized_and_invalid_commands() {
    let cpp_bin = find_cpp_settings_binary();
    if cpp_bin.is_none() {
        if std::env::var("REQUIRE_CPP_SETTINGS_TEST").is_ok() {
            panic!("REQUIRED_CPP_BIN_MISSING: C++/WinUI 3 settings client binary not found.");
        } else {
            eprintln!("STATUS_CPP_BIN_UNAVAILABLE: Real C++ Settings executable not found; skipping negative C++ tests.");
            return;
        }
    }
    let bin_path = cpp_bin.unwrap();

    let _env_lock = ENV_MUTEX.lock().await;

    // Case 1: Valid connection to real server, but with invalid/stale session token
    // The server reports { "success": false, "error_message": "Security violation: ..." }
    // The C++ client must parse JSON, respect success=false, propagate error_message, and exit with code 1.
    {
        let custom_dir =
            std::env::temp_dir().join(format!("test_cpp_rej_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&custom_dir);
        let token_file = custom_dir.join("cpp_bad_session.token");
        // Write invalid token into isolated token file
        std::fs::write(&token_file, "INVALID_STALE_TOKEN_XYZ").unwrap();
        std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &token_file);

        let pipe_name = format!(
            r"\\.\pipe\aethershell-cpp-rej-test-{}",
            uuid::Uuid::new_v4()
        );
        let server_valid_token = "LEGIT_SERVER_TOKEN_123";

        let harness = LiveEnginePipeTestHarness::start_with_pipe_and_session(
            server_valid_token,
            &pipe_name,
            current_session_id(),
        )
        .await;

        let output = tokio::process::Command::new(&bin_path)
            .args([
                "--test-ipc-pipe",
                &pipe_name,
                "--test-ipc-command",
                "pause_audio",
            ])
            .output()
            .await
            .expect("execute C++ settings client pause_audio with bad token");

        // Process must fail (exit code 1)
        assert!(
            !output.status.success(),
            "C++ client must exit with failure when server rejects command"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("IPC_TEST_ERROR:"),
            "C++ client must report IPC_TEST_ERROR: {}",
            stdout
        );
        assert!(
            stdout.contains("valid session capability token required"),
            "C++ client must propagate server error message: {}",
            stdout
        );

        // Core engine state must NOT have changed
        {
            let aud = harness.engine.audio().lock().await.is_paused();
            assert!(!aud, "Audio must NOT be paused when command was rejected");
        }

        harness.stop().await;
        std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
        let _ = std::fs::remove_dir_all(custom_dir);
    }

    // Case 2: Server returns malformed response (invalid JSON)
    // C++ client must reject it, output IPC_TEST_ERROR with JSON syntax notice, and exit with code 1.
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe_name = format!(
            r"\\.\pipe\aethershell-cpp-malformed-{}",
            uuid::Uuid::new_v4()
        );
        let mut pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("create mock malformed pipe server");

        let server_task = tokio::spawn(async move {
            pipe_server.connect().await.expect("client connect");
            let mut buf = [0u8; 1024];
            let _ = pipe_server.read(&mut buf).await;

            // Send malformed response: 4-byte length prefix + non-JSON body
            let malformed_body = b"NOT_A_VALID_JSON_STRING";
            let len_prefix = (malformed_body.len() as u32).to_le_bytes();
            let mut resp = Vec::new();
            resp.extend_from_slice(&len_prefix);
            resp.extend_from_slice(malformed_body);
            let _ = pipe_server.write_all(&resp).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let output = tokio::process::Command::new(&bin_path)
            .args([
                "--test-ipc-pipe",
                &pipe_name,
                "--test-ipc-command",
                "test_cmd",
            ])
            .output()
            .await
            .expect("execute C++ settings client against malformed server");

        assert!(
            !output.status.success(),
            "C++ client must fail on malformed JSON response"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("IPC_TEST_ERROR:Malformed IPC response: invalid JSON syntax."),
            "C++ client must report malformed JSON error: {}",
            stdout
        );

        let _ = server_task.await;
    }

    // Case 3: Server returns response with missing success field
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;

        let pipe_name = format!(
            r"\\.\pipe\aethershell-cpp-nosuccess-{}",
            uuid::Uuid::new_v4()
        );
        let mut pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("create mock no-success pipe server");

        let server_task = tokio::spawn(async move {
            pipe_server.connect().await.expect("client connect");
            let mut buf = [0u8; 1024];
            let _ = pipe_server.read(&mut buf).await;

            // Missing success field
            let body = br#"{"protocol_version":1,"correlation_id":"test","payload":{}}"#;
            let len_prefix = (body.len() as u32).to_le_bytes();
            let mut resp = Vec::new();
            resp.extend_from_slice(&len_prefix);
            resp.extend_from_slice(body);
            let _ = pipe_server.write_all(&resp).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let output = tokio::process::Command::new(&bin_path)
            .args([
                "--test-ipc-pipe",
                &pipe_name,
                "--test-ipc-command",
                "test_cmd",
            ])
            .output()
            .await
            .expect("execute C++ settings client against no-success server");

        assert!(
            !output.status.success(),
            "C++ client must fail on missing success field"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("IPC_TEST_ERROR:Malformed IPC response: missing success field."),
            "C++ client must report missing success field: {}",
            stdout
        );

        let _ = server_task.await;
    }
}
