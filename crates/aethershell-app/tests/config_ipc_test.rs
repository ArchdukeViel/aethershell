use aethershell_app::AppEngine;
use aethershell_audio::FpsMode;
use aethershell_config::{AppConfig, ConfigManager, CURRENT_SCHEMA_VERSION};
use aethershell_ipc::{IpcClient, IpcResponse};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

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

struct ConfigTestHarness {
    engine: Arc<AppEngine>,
    pipe_name: String,
    #[allow(dead_code)]
    session_token: String,
    config_dir: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    server_handle: tokio::task::JoinHandle<()>,
}

impl ConfigTestHarness {
    #[allow(dead_code)]
    fn session_token(&self) -> &str {
        &self.session_token
    }
    async fn start(token: &str) -> Self {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_config_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        let manager = ConfigManager::new(&temp_dir);
        let initial_cfg = AppConfig::default();
        let engine = Arc::new(AppEngine::with_token_and_config(
            token,
            manager,
            initial_cfg,
        ));

        let pipe_name = format!(r"\\.\pipe\aethershell-config-test-{}", uuid::Uuid::new_v4());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handler = engine.create_ipc_handler(current_session_id(), shutdown_tx.clone());
        let server = aethershell_ipc::IpcServer::new(&pipe_name, handler, shutdown_rx);

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(60)).await;

        Self {
            engine,
            pipe_name,
            session_token: token.to_string(),
            config_dir: temp_dir,
            shutdown_tx,
            server_handle,
        }
    }

    async fn send(&self, command: &str, payload: serde_json::Value) -> IpcResponse {
        IpcClient::send_command(&self.pipe_name, command, payload)
            .await
            .expect("IPC send_command failed")
    }

    async fn stop(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.server_handle.await;
        let _ = fs::remove_dir_all(&self.config_dir);
    }
}

#[tokio::test]
async fn test_get_config_unauthenticated_rejected() {
    let harness = ConfigTestHarness::start("secret_test_token_001").await;

    // 1. Missing session capability token
    let resp = harness.send("get_config", serde_json::json!({})).await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("unauthorized caller"));

    // 2. Empty session capability token
    let resp = harness
        .send(
            "get_config",
            serde_json::json!({
                "session_token": ""
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("unauthorized caller"));

    // 3. Wrong session capability token
    let resp = harness
        .send(
            "get_config",
            serde_json::json!({
                "session_token": "wrong_token_xyz"
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("unauthorized caller"));

    harness.stop().await;
}

#[tokio::test]
async fn test_get_config_authenticated_full_and_sections() {
    let harness = ConfigTestHarness::start("secret_test_token_002").await;

    // 1. Full configuration query
    let resp = harness
        .send(
            "get_config",
            serde_json::json!({
                "session_token": "secret_test_token_002"
            }),
        )
        .await;
    assert!(resp.success, "get_config with valid token must succeed");
    assert_eq!(
        resp.payload["schema_version"], CURRENT_SCHEMA_VERSION,
        "Returned schema version must match current version"
    );
    assert_eq!(resp.payload["config"]["profile"], "signature_minimal");

    // 2. Specific valid section queries
    let sections = [
        "general",
        "taskbar",
        "desktop",
        "audio",
        "telemetry",
        "power",
        "gaming",
        "scripting",
        "privacy",
        "updates",
    ];
    for sec in &sections {
        let resp = harness
            .send(
                "get_config",
                serde_json::json!({
                    "session_token": "secret_test_token_002",
                    "section": sec
                }),
            )
            .await;
        assert!(resp.success, "Querying section '{}' must succeed", sec);
        assert_eq!(resp.payload["section"], *sec);
        assert!(resp.payload.get(*sec).is_some());
    }

    // 3. Unknown section query rejected with informative error
    let resp = harness
        .send(
            "get_config",
            serde_json::json!({
                "session_token": "secret_test_token_002",
                "section": "unsupported_extra_section"
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Unknown configuration section"));

    harness.stop().await;
}

#[tokio::test]
async fn test_set_config_unauthenticated_rejected() {
    let harness = ConfigTestHarness::start("secret_test_token_003").await;

    let valid_cfg = AppConfig::default();

    // 1. Missing token
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "config": valid_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("unauthorized caller"));

    // 2. Invalid token
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "invalid_forged_token",
                "config": valid_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("unauthorized caller"));

    // Verify config file was not written to disk
    assert!(!harness.config_dir.join("config.toml").exists());

    harness.stop().await;
}

#[tokio::test]
async fn test_set_config_schema_validation_rejections() {
    let harness = ConfigTestHarness::start("secret_test_token_004").await;

    // 1. Mismatched / unsupported schema version rejected
    let invalid_version_cfg = AppConfig {
        schema_version: "99.0.0".into(),
        ..Default::default()
    };
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_004",
                "config": invalid_version_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Configuration validation failed"));

    // 2. Invalid audio render FPS (0 fps) rejected
    let invalid_fps_cfg = AppConfig {
        audio: aethershell_config::AudioConfig {
            render_fps: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_004",
                "config": invalid_fps_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Audio render FPS must be between 1 and 240"));

    // 3. Unsafe profile path traversal rejected
    let invalid_path_cfg = AppConfig {
        profile: r"..\..\system32\malicious".into(),
        ..Default::default()
    };
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_004",
                "config": invalid_path_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Profile identifier cannot contain path separators"));

    // 4. Excessive scripting crash limit rejected
    let invalid_script_cfg = AppConfig {
        scripting: aethershell_config::schema::ScriptingConfig {
            restart_after_crash_limit: 50,
            ..Default::default()
        },
        ..Default::default()
    };
    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_004",
                "config": invalid_script_cfg
            }),
        )
        .await;
    assert!(!resp.success);
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Scripting crash restart limit cannot exceed 10"));

    // Verify engine state remains on defaults
    assert_eq!(harness.engine.config().profile, "signature_minimal");
    assert!(!harness.config_dir.join("config.toml").exists());

    harness.stop().await;
}

#[tokio::test]
async fn test_set_config_atomic_persistence_and_subsystem_propagation() {
    let harness = ConfigTestHarness::start("secret_test_token_005").await;

    // 1. Initial valid configuration mutation
    let updated_cfg = AppConfig {
        profile: "modern_cyberpunk".into(),
        general: aethershell_config::GeneralConfig {
            language: "ja-JP".into(),
            ..Default::default()
        },
        audio: aethershell_config::AudioConfig {
            render_fps: 60,
            ..Default::default()
        },
        power: aethershell_config::PowerConfig {
            profile: "performance".into(),
            ..Default::default()
        },
        desktop: aethershell_config::DesktopConfig {
            interactive_timeout_seconds: 16,
            ..Default::default()
        },
        ..Default::default()
    };

    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_005",
                "config": updated_cfg
            }),
        )
        .await;
    assert!(resp.success, "set_config with valid changes must succeed");
    assert_eq!(resp.payload["applied"], true);

    // Verify in-memory engine state updated
    assert_eq!(harness.engine.config().profile, "modern_cyberpunk");
    assert_eq!(harness.engine.config().general.language, "ja-JP");
    assert_eq!(harness.engine.config().audio.render_fps, 60);
    assert_eq!(
        harness.engine.audio().lock().await.fps_mode(),
        FpsMode::Performance
    );

    // Verify disk persistence
    let disk_file = harness.config_dir.join("config.toml");
    assert!(disk_file.exists(), "config.toml must be written to disk");
    let disk_content = fs::read_to_string(&disk_file).expect("read config.toml");
    assert!(disk_content.contains("modern_cyberpunk"));
    assert!(disk_content.contains("ja-JP"));

    // Verify subsequent get_config returns updated values
    let get_resp = harness
        .send(
            "get_config",
            serde_json::json!({
                "session_token": "secret_test_token_005"
            }),
        )
        .await;
    assert!(get_resp.success);
    assert_eq!(get_resp.payload["config"]["profile"], "modern_cyberpunk");

    // 2. Second configuration mutation to test .bak file creation
    let mut second_cfg = updated_cfg.clone();
    second_cfg.profile = "nordic_minimal".into();
    second_cfg.power.profile = "power_saver".into();
    second_cfg.audio.render_fps = 15;

    let resp2 = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_005",
                "config": second_cfg
            }),
        )
        .await;
    assert!(resp2.success);

    // Verify backup file was created preserving the first version
    let bak_file = harness.config_dir.join("config.toml.bak");
    assert!(bak_file.exists(), "config.toml.bak must exist after update");
    let bak_content = fs::read_to_string(&bak_file).expect("read config.toml.bak");
    assert!(
        bak_content.contains("modern_cyberpunk"),
        "Backup must preserve prior configuration version"
    );

    // Verify live audio mode transitioned to BatterySaver
    assert_eq!(
        harness.engine.audio().lock().await.fps_mode(),
        FpsMode::BatterySaver
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_set_config_with_raw_toml_string() {
    let harness = ConfigTestHarness::start("secret_test_token_006").await;

    let raw_toml = r#"
schema_version = "1.0.0"
profile = "solarized_dark"

[general]
language = "de-DE"

[audio]
render_fps = 60
spectrum_bands = 64
"#;

    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_006",
                "toml": raw_toml
            }),
        )
        .await;
    assert!(resp.success, "set_config with raw toml string must succeed");

    assert_eq!(harness.engine.config().profile, "solarized_dark");
    assert_eq!(harness.engine.config().general.language, "de-DE");

    harness.stop().await;
}

#[tokio::test]
async fn test_set_config_transactional_rollback_on_disk_persistence_failure() {
    let harness = ConfigTestHarness::start("secret_test_token_007").await;

    // Record original in-memory profile
    let original_profile = harness.engine.config().profile.clone();

    // Lock destination file by creating a directory with the exact name 'config.toml'
    // On Windows, ReplaceFileW/MoveFileExW into a directory fails with Access Denied
    let blocked_path = harness.config_dir.join("config.toml");
    fs::create_dir_all(&blocked_path).expect("create blocking directory");

    let candidate_cfg = AppConfig {
        profile: "should_never_be_saved".into(),
        ..Default::default()
    };

    let resp = harness
        .send(
            "set_config",
            serde_json::json!({
                "session_token": "secret_test_token_007",
                "config": candidate_cfg
            }),
        )
        .await;
    assert!(
        !resp.success,
        "set_config must fail when disk persistence fails"
    );
    assert!(resp
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("Transactional disk persistence failed"));

    // Critical assertion: In-memory engine state was NOT modified (transactional rollback)
    assert_eq!(
        harness.engine.config().profile,
        original_profile,
        "Engine state must not be mutated when disk persistence fails"
    );

    let _ = fs::remove_dir_all(&blocked_path);
    harness.stop().await;
}
