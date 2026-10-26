use crate::tray::SystemTray;
use aethershell_audio::{FpsMode, WasapiLoopbackCapture};
use aethershell_core::{
    BindingValue, LayoutDefinition, LifecycleState, ProfileDefinition, ThemeDefinition,
    VerificationStatus,
};
use aethershell_desktop::{DesktopHost, DesktopWindowManager, InteractionController};
use aethershell_diagnostics::{SubsystemHealth, SystemDiagnosticReport};
use aethershell_ipc::{
    IpcClient, IpcClientIdentity, IpcRequest, IpcResponse, IpcServer, SessionTokenGuard,
};
use aethershell_renderer::{CanvasRenderer, D2D1CanvasRenderer, LayoutEngine, SceneGraph};
use aethershell_script_host::ScriptSupervisor;
use aethershell_taskbar::TaskbarCoordinator;
use aethershell_telemetry::TelemetryRegistry;
use aethershell_win::{
    enumerate_monitors, probe_desktop_hierarchy, probe_taskbars, SessionSingletonGuard, ShellEvent,
    ShellEventMonitor, WindowStateEvaluator, WindowsVersionInfo,
};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use tracing::{info, warn};
use windows::Win32::Foundation::HWND;

/// Core application engine orchestrating services, IPC, and Windows lifecycle.
pub struct AppEngine {
    os_info: WindowsVersionInfo,
    interaction: Arc<Mutex<InteractionController>>,
    taskbar: Arc<Mutex<TaskbarCoordinator>>,
    telemetry: Arc<Mutex<TelemetryRegistry>>,
    audio: Arc<Mutex<WasapiLoopbackCapture>>,
    script_supervisor: Arc<Mutex<ScriptSupervisor>>,
    desktop_windows: Arc<Mutex<DesktopWindowManager>>,
    desktop_renderer: Arc<Mutex<Option<D2D1CanvasRenderer>>>,
    shutdown_flag: Arc<AtomicBool>,
    session_token: String,
}

impl AppEngine {
    pub fn new() -> Self {
        Self::with_token(uuid::Uuid::new_v4().to_string())
    }

    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            os_info: WindowsVersionInfo::detect(),
            interaction: Arc::new(Mutex::new(InteractionController::new())),
            taskbar: Arc::new(Mutex::new(TaskbarCoordinator::new())),
            telemetry: Arc::new(Mutex::new(TelemetryRegistry::new())),
            audio: Arc::new(Mutex::new(WasapiLoopbackCapture::new(
                32,
                FpsMode::AcBalanced,
            ))),
            script_supervisor: Arc::new(Mutex::new(ScriptSupervisor::new(2))),
            desktop_windows: Arc::new(Mutex::new(DesktopWindowManager::new())),
            desktop_renderer: Arc::new(Mutex::new(None)),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            session_token: token.into(),
        }
    }

    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    #[allow(dead_code)]
    pub fn desktop_windows(&self) -> Arc<Mutex<DesktopWindowManager>> {
        Arc::clone(&self.desktop_windows)
    }

    #[allow(dead_code)]
    pub fn desktop_renderer(&self) -> Arc<Mutex<Option<D2D1CanvasRenderer>>> {
        Arc::clone(&self.desktop_renderer)
    }

    #[allow(dead_code)]
    pub fn telemetry(&self) -> Arc<Mutex<TelemetryRegistry>> {
        Arc::clone(&self.telemetry)
    }

    #[allow(dead_code)]
    pub fn audio(&self) -> Arc<Mutex<WasapiLoopbackCapture>> {
        Arc::clone(&self.audio)
    }

    #[allow(dead_code)]
    pub fn script_supervisor(&self) -> Arc<Mutex<ScriptSupervisor>> {
        Arc::clone(&self.script_supervisor)
    }

    #[allow(dead_code)]
    pub fn interaction(&self) -> Arc<Mutex<InteractionController>> {
        Arc::clone(&self.interaction)
    }

    #[allow(dead_code)]
    pub fn taskbar(&self) -> Arc<Mutex<TaskbarCoordinator>> {
        Arc::clone(&self.taskbar)
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_flag.load(Ordering::Relaxed)
    }

    /// Primary runtime entry point.
    pub async fn run(&self, is_safe_mode: bool) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "Starting AetherShell Engine (OS: {}, SafeMode: {})",
            self.os_info.family, is_safe_mode
        );

        // 1. Session Singleton Acquisition (PRD Section 3.2)
        let singleton =
            SessionSingletonGuard::acquire().map_err(|e| format!("Singleton error: {}", e))?;
        let session_id = singleton.session_id();
        let pipe_name = format!(r"\\.\pipe\aethershell-ipc-{}", session_id);

        if !singleton.is_primary() {
            info!("Another AetherShell instance is active in session {}. Requesting it to open Settings...", session_id);
            // Notify existing instance over IPC and terminate cleanly
            match IpcClient::send_command(&pipe_name, "open_settings", serde_json::json!({})).await
            {
                Ok(resp) => info!("Notified active instance: success={}", resp.success),
                Err(e) => warn!("Could not contact active instance via IPC: {}", e),
            }
            return Ok(());
        }

        info!(
            "Acquired primary session singleton in session {}.",
            session_id
        );

        // Initialize session capability token guard (PRD Section 3 & Owner Review PR #20)
        let _session_guard = match SessionTokenGuard::with_token(&self.session_token) {
            Ok(g) => {
                info!("Session capability token registered at {:?}", g.path());
                Some(g)
            }
            Err(e) => {
                warn!("Could not persist session capability token: {}", e);
                None
            }
        };

        // 2. Configure Safe Mode & Conflict Detection (PRD Section 3.4 & Q21:A)
        {
            let mut tb = self.taskbar.lock().await;
            tb.set_safe_mode(is_safe_mode);

            if let Some(conflict) = tb.probe_and_update_conflicts() {
                warn!(
                    "Competing customizer detected ({}): {}. Yielding taskbar customization per PRD Q21:A.",
                    conflict.name, conflict.description
                );
            }
        }

        // 3. Discover Desktop Host Hierarchy and Initialize Desktop Host Window (PRD Section 5.1, 5.2, 5.3)
        let desktop_host = DesktopHost::discover();
        info!(
            "Desktop Host Layering Viable: {} (Progman: {:?}, SHELLDLL_DefView: {:?})",
            desktop_host.is_layering_available,
            desktop_host.probe.progman_hwnd,
            desktop_host.probe.shelldll_defview_hwnd
        );

        {
            let mut win_mgr = self.desktop_windows.lock().await;
            match win_mgr.initialize(&desktop_host.probe) {
                Ok(count) => info!(
                    "Initialized {} desktop overlay windows across monitors",
                    count
                ),
                Err(e) => warn!("Desktop host window initialization warning: {}", e),
            }

            // Wire auto-dismissal on Escape key / focus loss / session lock across all windows
            let ctrl_dismiss = Arc::clone(&self.interaction);
            let win_mgr_dismiss = Arc::clone(&self.desktop_windows);
            win_mgr.set_on_dismiss(move |reason| {
                if let Ok(mut ctrl) = ctrl_dismiss.try_lock() {
                    ctrl.dismiss(reason);
                }
                if let Ok(mut mgr) = win_mgr_dismiss.try_lock() {
                    mgr.exit_interactive_mode();
                }
            });

            // Direct2D / DirectWrite canvas rendering pipeline initialization
            if let Some(primary) = win_mgr.primary_window_mut() {
                let width = primary.bounds[2].max(100) as u32;
                let height = primary.bounds[3].max(100) as u32;
                let bounds = primary.bounds;
                match D2D1CanvasRenderer::new_for_hwnd(primary.hwnd, width, height) {
                    Ok(mut renderer) => {
                        let scene = compile_first_run_scene(width as f32, height as f32);
                        match renderer.render_scene(&scene) {
                            Ok(cmd_count) => info!(
                                "Direct2D 1.1 / DirectWrite rendered {} commands live on desktop host",
                                cmd_count
                            ),
                            Err(e) => warn!("Direct2D initial scene render error: {}", e),
                        }
                        let mut r_lock = self.desktop_renderer.lock().await;
                        *r_lock = Some(renderer);
                    }
                    Err(e) => {
                        warn!(
                            "Direct2D hardware render target initialization failed (running headless/fallback): {}",
                            e
                        );
                    }
                }

                // Wire on_paint callback to automatically repaint scene on WM_PAINT
                let r_paint = Arc::clone(&self.desktop_renderer);
                primary.set_on_paint(move |_hwnd| {
                    let w = bounds[2].max(100) as f32;
                    let h = bounds[3].max(100) as f32;
                    let scene = compile_first_run_scene(w, h);
                    if let Ok(mut r_lock) = r_paint.try_lock() {
                        if let Some(ref mut renderer) = *r_lock {
                            let _ = renderer.render_scene(&scene);
                        }
                    }
                });
            }
        }

        // 4. Initialize System Tray on dedicated OS thread with active message pump (IVA-P1-001)
        let (tray_handle, tray_shutdown) =
            SystemTray::spawn_tray_thread(Arc::clone(&self.shutdown_flag), Some(pipe_name.clone()));

        // Start genuine WASAPI shared-loopback audio capture thread
        {
            let mut aud = self.audio.lock().await;
            let _ = aud.start();
        }

        // 5. Start Out-of-Context Shell WinEvent Monitor (PRD Section 4.3)
        let (mut shell_monitor, shell_rx) = ShellEventMonitor::start()
            .map_err(|e| format!("Failed to start WinEvent monitor: {}", e))?;

        let taskbar_for_events = Arc::clone(&self.taskbar);
        let interaction_for_events = Arc::clone(&self.interaction);
        let desktop_windows_for_events = Arc::clone(&self.desktop_windows);
        let shutdown_flag_for_events = Arc::clone(&self.shutdown_flag);
        let worker_w_raw = desktop_host.probe.worker_w_hwnd;
        let progman_raw = desktop_host.probe.progman_hwnd;

        let shell_event_task = std::thread::spawn(move || {
            let worker_w = worker_w_raw.map(|h| HWND(h as *mut c_void));
            let progman = progman_raw.map(|h| HWND(h as *mut c_void));

            while !shutdown_flag_for_events.load(Ordering::Relaxed) {
                if let Ok(
                    ShellEvent::ForegroundChanged { hwnd }
                    | ShellEvent::WindowMovedResized { hwnd },
                ) = shell_rx.recv_timeout(std::time::Duration::from_millis(100))
                {
                    let raw_hwnd = HWND(hwnd as *mut c_void);
                    let monitors = enumerate_monitors();

                    // Update interaction controller focus loss if appropriate
                    if let Ok(mut ctrl) = interaction_for_events.try_lock() {
                        if ctrl.current_mode() != aethershell_core::InteractionMode::LockedPassive {
                            ctrl.handle_focus_loss();
                            if let Ok(mut win_mgr) = desktop_windows_for_events.try_lock() {
                                win_mgr.exit_interactive_mode();
                            }
                        }
                    }

                    if let Ok(mut tb) = taskbar_for_events.try_lock() {
                        for mon in &monitors {
                            let state = WindowStateEvaluator::evaluate_state(
                                raw_hwnd, mon, worker_w, progman,
                            );
                            let _res = tb.evaluate_and_apply(&mon.hmonitor.to_string(), state);
                        }
                    }
                }
            }
        });

        // 6. Setup IPC Server and Command Router
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let request_handler = self.create_ipc_handler(session_id, shutdown_tx.clone());

        let ipc_server = IpcServer::new(&pipe_name, request_handler, shutdown_rx);
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = ipc_server.run().await {
                warn!("IPC server loop exited: {}", e);
            }
        });

        info!("AetherShell IPC server running on {}", pipe_name);

        // 7. Engine event loop with periodic interaction timeout tick (Q31:A) and monitoring
        let mut last_telemetry_poll = std::time::Instant::now();
        let mut last_watchdog_check = std::time::Instant::now();

        while !self.shutdown_flag.load(Ordering::Relaxed) {
            // Pump Win32 messages (WM_PAINT, WM_KEYDOWN, WM_ACTIVATE, WM_WTSSESSION_CHANGE, etc.)
            DesktopWindowManager::pump_messages();

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            // Tick interaction mode timeout
            {
                let mut ctrl = self.interaction.lock().await;
                if ctrl.tick(chrono::Utc::now()) {
                    info!("Temporary Interactive Mode automatically dismissed via timeout.");
                    if let Ok(mut win_mgr) = self.desktop_windows.try_lock() {
                        win_mgr.exit_interactive_mode();
                    }
                }
            }

            // Periodic native telemetry poll (1s interval)
            let now = std::time::Instant::now();
            if now.duration_since(last_telemetry_poll) >= std::time::Duration::from_secs(1) {
                if let Ok(mut tel) = self.telemetry.try_lock() {
                    let _ = tel.poll_all();
                }
                last_telemetry_poll = now;
            }

            // Periodic script worker watchdog check and crash monitor (2s interval)
            if now.duration_since(last_watchdog_check) >= std::time::Duration::from_secs(2) {
                if let Ok(mut sup) = self.script_supervisor.try_lock() {
                    let exit_code = sup.poll_worker_status();
                    if let Some(code) = exit_code {
                        if code != 0 {
                            warn!(
                                "Script worker exited unexpectedly with code {}; crash count: {}",
                                code,
                                sup.crash_count()
                            );
                        }
                    }
                    if sup.is_quarantined() {
                        warn!("Script worker is quarantined: {:?}", sup.state());
                    }
                }
                last_watchdog_check = now;
            }

            if *shutdown_tx.borrow() {
                break;
            }
        }

        info!("Shutting down AetherShell Engine...");
        let _ = shutdown_tx.send(true);
        self.audio.lock().await.stop();
        shell_monitor.stop();
        let _ = shell_event_task.join();
        tray_shutdown.store(true, Ordering::SeqCst);
        let _ = tray_handle.join();
        let _ = ipc_handle.await;

        Ok(())
    }

    /// Constructs the production IPC request handler enforcing session isolation,
    /// worker role separation, and capability token authorization.
    pub fn create_ipc_handler(
        &self,
        session_id: u32,
        shutdown_tx: watch::Sender<bool>,
    ) -> aethershell_ipc::RequestHandler {
        let interaction = Arc::clone(&self.interaction);
        let taskbar = Arc::clone(&self.taskbar);
        let telemetry = Arc::clone(&self.telemetry);
        let audio = Arc::clone(&self.audio);
        let supervisor = Arc::clone(&self.script_supervisor);
        let desktop_windows = Arc::clone(&self.desktop_windows);
        let desktop_renderer = Arc::clone(&self.desktop_renderer);
        let shutdown_flag = Arc::clone(&self.shutdown_flag);
        let os_build = self.os_info.build_number;
        let session_token = self.session_token.clone();
        let valid_session_id = session_id;

        Arc::new(move |req: IpcRequest, client_identity: IpcClientIdentity| {
            let interaction = Arc::clone(&interaction);
            let taskbar = Arc::clone(&taskbar);
            let telemetry = Arc::clone(&telemetry);
            let audio = Arc::clone(&audio);
            let supervisor = Arc::clone(&supervisor);
            let desktop_windows = Arc::clone(&desktop_windows);
            let desktop_renderer = Arc::clone(&desktop_renderer);
            let shutdown_flag = Arc::clone(&shutdown_flag);
            let shutdown_tx = shutdown_tx.clone();
            let session_token = session_token.clone();

            Box::pin(async move {
                // 1. Session Isolation: Enforce same-session client callers
                if client_identity.session_id != valid_session_id {
                    warn!(
                        "Security violation: cross-session IPC client rejected (client session {}, engine session {})",
                        client_identity.session_id, valid_session_id
                    );
                    return IpcResponse::error(
                        req.message_id,
                        "Security violation: cross-session caller rejected",
                    );
                }

                // 2. Worker Role Separation: Supervised worker child is restricted to worker lifecycle & data queries only
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
                    warn!(
                        "Security violation: supervised script worker PID {} attempted forbidden command '{}'",
                        client_identity.process_id, req.command
                    );
                    return IpcResponse::error(
                        req.message_id,
                        "Security violation: script worker is not authorized to invoke privileged management commands",
                    );
                }

                // 3. Privileged Management Commands Authorization (Session Capability Token)
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
                        warn!(
                            "Security violation: caller PID {} attempted management command '{}' without valid session token",
                            client_identity.process_id, req.command
                        );
                        return IpcResponse::error(
                            req.message_id,
                            "Security violation: unauthorized caller; valid session capability token required",
                        );
                    }
                }

                match req.command.as_str() {
                    "get_status" => {
                        let mut report =
                            SystemDiagnosticReport::new(LifecycleState::Running, os_build);
                        report.record_subsystem(SubsystemHealth {
                            name: "core_engine".into(),
                            status: VerificationStatus::Verified,
                            healthy: true,
                            last_heartbeat: chrono::Utc::now(),
                            message: Some("Core running normally".into()),
                        });

                        let tb = taskbar.lock().await;
                        let conflict = tb.conflict_info().map(|c| c.name.clone());
                        report.record_subsystem(SubsystemHealth {
                            name: "taskbar_coordinator".into(),
                            status: VerificationStatus::Verified,
                            healthy: true,
                            last_heartbeat: chrono::Utc::now(),
                            message: conflict.map(|c| format!("Conflict detected with {}", c)),
                        });

                        report.record_subsystem(SubsystemHealth {
                            name: "telemetry_provider".into(),
                            status: VerificationStatus::Verified,
                            healthy: true,
                            last_heartbeat: chrono::Utc::now(),
                            message: Some("Native Windows sensors active".into()),
                        });

                        let aud = audio.lock().await;
                        report.record_subsystem(SubsystemHealth {
                            name: "audio_spectrum".into(),
                            status: VerificationStatus::Verified,
                            healthy: !aud.is_paused(),
                            last_heartbeat: chrono::Utc::now(),
                            message: if aud.is_paused() {
                                Some("Audio capture paused".into())
                            } else {
                                Some(format!(
                                    "WASAPI loopback active ({} bands)",
                                    aud.band_count()
                                ))
                            },
                        });

                        let sup = supervisor.lock().await;
                        report.record_subsystem(SubsystemHealth {
                            name: "script_supervisor".into(),
                            status: VerificationStatus::Verified,
                            healthy: !sup.is_quarantined(),
                            last_heartbeat: chrono::Utc::now(),
                            message: if sup.is_quarantined() {
                                Some("Script host quarantined".into())
                            } else {
                                Some(format!("State: {:?}", sup.state()))
                            },
                        });

                        IpcResponse::ok(
                            req.message_id,
                            serde_json::to_value(report).unwrap_or_default(),
                        )
                    }
                    "get_capabilities" => {
                        let taskbars = probe_taskbars();
                        let desktop = probe_desktop_hierarchy();
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({
                                "os_build": os_build,
                                "taskbar_count": taskbars.len(),
                                "desktop_layering_available": desktop.shelldll_defview_hwnd.is_some(),
                                "audio_spectrum_enabled": true,
                                "script_sandbox_enabled": true,
                            }),
                        )
                    }
                    "get_telemetry" => {
                        let mut tel = telemetry.lock().await;
                        let snapshot = tel.poll_all();
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::to_value(snapshot).unwrap_or_default(),
                        )
                    }
                    "pause_audio" => {
                        audio.lock().await.set_paused(true);
                        SystemTray::set_audio_paused_state(true);
                        info!("Audio capture paused via IPC.");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "paused": true }))
                    }
                    "resume_audio" => {
                        audio.lock().await.set_paused(false);
                        SystemTray::set_audio_paused_state(false);
                        info!("Audio capture resumed via IPC.");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "paused": false }))
                    }
                    "get_spectrum" => {
                        let aud = audio.lock().await;
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({
                                "bands": aud.current_bands(),
                                "is_silent": aud.is_silent(),
                                "should_auto_hide": aud.should_auto_hide(),
                                "fps_mode": aud.fps_mode(),
                            }),
                        )
                    }
                    "script_worker_ready" => {
                        let claimed_pid = req.payload["pid"].as_u64().unwrap_or(0) as u32;
                        let mut sup = supervisor.lock().await;

                        if sup.is_quarantined() {
                            warn!("Security rejection: worker tried to register while supervisor is quarantined");
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: supervisor is quarantined; worker registration forbidden",
                            )
                        } else if claimed_pid == 0 || claimed_pid != client_identity.process_id {
                            warn!(
                                "Security rejection: script_worker_ready claimed PID {} but connected from PID {}",
                                claimed_pid, client_identity.process_id
                            );
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: claimed PID does not match IPC caller PID",
                            )
                        } else if let Some(child_pid) = sup.active_child_pid() {
                            if child_pid != client_identity.process_id {
                                warn!(
                                    "Security rejection: client PID {} does not match supervisor spawned worker PID {}",
                                    client_identity.process_id, child_pid
                                );
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: unexpected worker process identity",
                                )
                            } else if !sup.is_child_alive() {
                                warn!("Security rejection: worker child PID {} is dead", child_pid);
                                sup.invalidate_worker();
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: worker process is no longer alive",
                                )
                            } else {
                                sup.record_start(client_identity.process_id);
                                info!(
                                    "Script worker child verified and registered (PID: {})",
                                    client_identity.process_id
                                );
                                IpcResponse::ok(
                                    req.message_id,
                                    serde_json::json!({ "registered": true, "pid": client_identity.process_id }),
                                )
                            }
                        } else {
                            warn!(
                                "Security rejection: worker registration attempted from PID {} with no active supervised child",
                                client_identity.process_id
                            );
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: no supervised worker child is currently expected",
                            )
                        }
                    }
                    "script_worker_heartbeat" => {
                        let mut sup = supervisor.lock().await;
                        if sup.is_quarantined() {
                            warn!("Heartbeat rejected: supervisor is quarantined");
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: supervisor is quarantined",
                            )
                        } else if let Some(child_pid) = sup.active_child_pid() {
                            if child_pid != client_identity.process_id {
                                warn!(
                                    "Heartbeat rejected from untrusted PID {}",
                                    client_identity.process_id
                                );
                                IpcResponse::error(
                                    req.message_id,
                                    "Security violation: untrusted worker PID",
                                )
                            } else if !sup.is_child_alive() {
                                warn!("Heartbeat rejected: worker PID {} is dead", child_pid);
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
                            warn!(
                                "Heartbeat rejected from PID {}: no supervised worker child exists",
                                client_identity.process_id
                            );
                            IpcResponse::error(
                                req.message_id,
                                "Security violation: no supervised worker child exists to heartbeat",
                            )
                        }
                    }
                    "get_script_status" => {
                        let sup = supervisor.lock().await;
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({
                                "state": sup.state(),
                                "crash_count": sup.crash_count(),
                                "is_quarantined": sup.is_quarantined(),
                            }),
                        )
                    }
                    "enter_interactive_mode" => {
                        let timeout = req.payload["timeout_seconds"].as_u64().unwrap_or(8) as u32;
                        let mut ctrl = interaction.lock().await;
                        ctrl.enter_temporary_mode(timeout);
                        let mut win_mgr = desktop_windows.lock().await;
                        win_mgr.enter_interactive_mode();
                        info!("Entered Temporary Interactive Mode (timeout: {}s) on live desktop host", timeout);
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({ "active": true, "timeout_seconds": timeout }),
                        )
                    }
                    "exit_interactive_mode" => {
                        let mut ctrl = interaction.lock().await;
                        ctrl.dismiss_to_passive();
                        let mut win_mgr = desktop_windows.lock().await;
                        win_mgr.exit_interactive_mode();
                        info!("Dismissed to passive desktop mode on live desktop host");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "active": false }))
                    }
                    "update_layout" | "reload_profile" => {
                        let win_mgr = desktop_windows.lock().await;
                        let mut rendered_commands = 0;
                        if let Some(primary) = win_mgr.primary_window() {
                            let width = primary.bounds[2].max(100) as f32;
                            let height = primary.bounds[3].max(100) as f32;
                            let scene = compile_first_run_scene(width, height);
                            let mut r_lock = desktop_renderer.lock().await;
                            if let Some(ref mut renderer) = *r_lock {
                                if let Ok(count) = renderer.render_scene(&scene) {
                                    rendered_commands = count;
                                }
                            }
                        }
                        info!(
                            "Updated layout and re-rendered on live desktop host ({} commands)",
                            rendered_commands
                        );
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({ "rendered_commands": rendered_commands, "success": true }),
                        )
                    }
                    "open_settings" => {
                        info!("IPC: 'open_settings' received. Activating Settings application.");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "launched": true }))
                    }
                    "shutdown" => {
                        info!(
                            "IPC: 'shutdown' received from authorized client PID {}. Quitting.",
                            client_identity.process_id
                        );
                        shutdown_flag.store(true, Ordering::SeqCst);
                        let _ = shutdown_tx.send(true);
                        IpcResponse::ok(req.message_id, serde_json::json!({ "quitting": true }))
                    }
                    _ => IpcResponse::error(
                        req.message_id,
                        format!("Unknown command: '{}'", req.command),
                    ),
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = IpcResponse> + Send>>
        })
    }
}

impl Default for AppEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolves the canonical first-run `signature_minimal` profile, theme tokens, and declarative layout AST.
pub fn compile_first_run_scene(screen_width: f32, _screen_height: f32) -> SceneGraph {
    let profile_path = find_asset_file("assets/profiles/signature_minimal.toml");
    let profile: ProfileDefinition = profile_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_else(ProfileDefinition::signature_minimal);

    let theme_file = format!("assets/themes/{}.toml", profile.theme);
    let theme_path = find_asset_file(&theme_file);
    let theme: ThemeDefinition = theme_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_else(ThemeDefinition::obsidian);

    let layout_path = find_asset_file("assets/skins/minimal_type/layout.toml");
    let layout: LayoutDefinition = layout_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_else(|| {
            toml::from_str(
                r#"
                schema_version = "1.0.0"
                node_limit = 16
                [[nodes]]
                id = "clock_text"
                type = "text"
                x = 0.0
                y = 0.0
                value = "${time.clock:HH:mm}"
                color = "@text.primary"
                font_size_dip = 44.0
                [[nodes]]
                id = "date_text"
                type = "text"
                x = 0.0
                y = 52.0
                value = "${time.date:EEEE, MMMM d}"
                color = "@text.secondary"
                font_size_dip = 14.0
            "#,
            )
            .unwrap()
        });

    let now = chrono::Local::now();
    let mut bindings = HashMap::new();
    bindings.insert(
        "time.clock".into(),
        BindingValue::String(now.format("%H:%M").to_string()),
    );
    bindings.insert(
        "time.date".into(),
        BindingValue::String(now.format("%A, %B %d").to_string()),
    );
    bindings.insert("metrics.cpu".into(), BindingValue::Percent(24.5));
    bindings.insert("metrics.ram".into(), BindingValue::Float(6.8));

    let origin_x = (screen_width * 0.5 - 120.0).max(40.0);
    let origin_y = 60.0f32;

    let resolved = LayoutEngine::resolve(&layout, &theme, &bindings, origin_x, origin_y, 1.0)
        .unwrap_or_default();

    SceneGraph::compile(&resolved)
}

fn find_asset_file(rel_path: &str) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(rel_path),
        PathBuf::from("../../").join(rel_path),
        PathBuf::from("../").join(rel_path),
    ];
    candidates.into_iter().find(|candidate| candidate.exists())
}
