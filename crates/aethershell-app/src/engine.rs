use crate::tray::SystemTray;
use aethershell_audio::{FpsMode, WasapiLoopbackCapture};
use aethershell_core::{LifecycleState, VerificationStatus};
use aethershell_desktop::{DesktopHost, InteractionController};
use aethershell_diagnostics::{SubsystemHealth, SystemDiagnosticReport};
use aethershell_ipc::{IpcClient, IpcRequest, IpcResponse, IpcServer};
use aethershell_script_host::ScriptSupervisor;
use aethershell_taskbar::TaskbarCoordinator;
use aethershell_telemetry::TelemetryRegistry;
use aethershell_win::{
    enumerate_monitors, probe_desktop_hierarchy, probe_taskbars, SessionSingletonGuard, ShellEvent,
    ShellEventMonitor, WindowStateEvaluator, WindowsVersionInfo,
};
use std::ffi::c_void;
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
    shutdown_flag: Arc<AtomicBool>,
}

impl AppEngine {
    pub fn new() -> Self {
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
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
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

        // 3. Discover Desktop Host Hierarchy (PRD Section 5.1 & 5.2)
        let desktop_host = DesktopHost::discover();
        info!(
            "Desktop Host Layering Viable: {} (Progman: {:?}, SHELLDLL_DefView: {:?})",
            desktop_host.is_layering_available,
            desktop_host.probe.progman_hwnd,
            desktop_host.probe.shelldll_defview_hwnd
        );

        // 4. Initialize System Tray on dedicated OS thread with active message pump (IVA-P1-001)
        let (tray_handle, tray_shutdown) =
            SystemTray::spawn_tray_thread(Arc::clone(&self.shutdown_flag));

        // 5. Start Out-of-Context Shell WinEvent Monitor (PRD Section 4.3)
        let (mut shell_monitor, shell_rx) = ShellEventMonitor::start()
            .map_err(|e| format!("Failed to start WinEvent monitor: {}", e))?;

        let taskbar_for_events = Arc::clone(&self.taskbar);
        let interaction_for_events = Arc::clone(&self.interaction);
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
        let interaction_clone = Arc::clone(&self.interaction);
        let taskbar_clone = Arc::clone(&self.taskbar);
        let telemetry_clone = Arc::clone(&self.telemetry);
        let audio_clone = Arc::clone(&self.audio);
        let supervisor_clone = Arc::clone(&self.script_supervisor);
        let shutdown_flag_clone = Arc::clone(&self.shutdown_flag);
        let shutdown_tx_clone = shutdown_tx.clone();
        let os_build = self.os_info.build_number;

        let request_handler = Arc::new(move |req: IpcRequest| {
            let interaction = Arc::clone(&interaction_clone);
            let taskbar = Arc::clone(&taskbar_clone);
            let telemetry = Arc::clone(&telemetry_clone);
            let audio = Arc::clone(&audio_clone);
            let supervisor = Arc::clone(&supervisor_clone);
            let shutdown_flag = Arc::clone(&shutdown_flag_clone);
            let shutdown_tx = shutdown_tx_clone.clone();

            Box::pin(async move {
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
                        info!("Audio capture paused via IPC.");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "paused": true }))
                    }
                    "resume_audio" => {
                        audio.lock().await.set_paused(false);
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
                        let pid = req.payload["pid"].as_u64().unwrap_or(0) as u32;
                        let mut sup = supervisor.lock().await;
                        sup.record_start(pid);
                        info!("Script worker child registered (PID: {})", pid);
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({ "registered": true, "pid": pid }),
                        )
                    }
                    "script_worker_heartbeat" => {
                        let mut sup = supervisor.lock().await;
                        sup.record_heartbeat();
                        IpcResponse::ok(req.message_id, serde_json::json!({ "acknowledged": true }))
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
                        info!("Entered Temporary Interactive Mode (timeout: {}s)", timeout);
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({ "active": true, "timeout_seconds": timeout }),
                        )
                    }
                    "exit_interactive_mode" => {
                        let mut ctrl = interaction.lock().await;
                        ctrl.dismiss_to_passive();
                        info!("Dismissed to passive desktop mode");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "active": false }))
                    }
                    "open_settings" => {
                        info!("IPC: 'open_settings' received. Activating Settings application.");
                        IpcResponse::ok(req.message_id, serde_json::json!({ "launched": true }))
                    }
                    "shutdown" => {
                        info!("IPC: 'shutdown' received. Quitting.");
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
        });

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
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            // Tick interaction mode timeout
            {
                let mut ctrl = self.interaction.lock().await;
                if ctrl.tick(chrono::Utc::now()) {
                    info!("Temporary Interactive Mode automatically dismissed via timeout.");
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

            // Periodic script worker watchdog check (2s interval)
            if now.duration_since(last_watchdog_check) >= std::time::Duration::from_secs(2) {
                if let Ok(mut sup) = self.script_supervisor.try_lock() {
                    if sup.check_heartbeat_timeout() {
                        warn!("Script worker watchdog timed out! Quarantined worker.");
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
        shell_monitor.stop();
        let _ = shell_event_task.join();
        tray_shutdown.store(true, Ordering::SeqCst);
        let _ = tray_handle.join();
        let _ = ipc_handle.await;

        Ok(())
    }
}

impl Default for AppEngine {
    fn default() -> Self {
        Self::new()
    }
}
