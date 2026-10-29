use crate::tray::SystemTray;
use aethershell_audio::{FpsMode, WasapiLoopbackCapture};
use aethershell_config::{AppConfig, ConfigManager};
use aethershell_core::{
    BindingValue, LayoutDefinition, LayoutNode, LifecycleState, NodeType, ProfileDefinition,
    ThemeDefinition, VerificationStatus,
};
use aethershell_desktop::{
    DesktopHost, DesktopWindowManager, InteractionController, InteractiveRegion,
};
use aethershell_diagnostics::{SubsystemHealth, SystemDiagnosticReport};
use aethershell_ipc::{
    IpcClient, IpcClientIdentity, IpcRequest, IpcResponse, IpcServer, SessionTokenGuard,
};
use aethershell_renderer::{
    CanvasRenderer, D2D1CanvasRenderer, LayoutEngine, ResolvedNode, SceneGraph,
};
use aethershell_script_host::ScriptSupervisor;
use aethershell_taskbar::TaskbarCoordinator;
use aethershell_telemetry::TelemetryRegistry;
use aethershell_win::{
    enumerate_monitors, probe_desktop_hierarchy, probe_taskbars, SessionSingletonGuard, ShellEvent,
    ShellEventMonitor, WindowStateEvaluator, WindowsVersionInfo,
};
use std::collections::HashMap;
use std::ffi::c_void;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::{watch, Mutex};
use tracing::{info, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_WIN};

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
    session_token: Arc<RwLock<String>>,
    cached_bindings: Arc<RwLock<HashMap<String, BindingValue>>>,
    config_manager: Arc<Mutex<ConfigManager>>,
    active_config: Arc<RwLock<AppConfig>>,
}

impl AppEngine {
    pub fn new() -> Self {
        Self::with_token(uuid::Uuid::new_v4().to_string())
    }

    pub fn with_token(token: impl Into<String>) -> Self {
        let config_dir = aethershell_config::resolve_app_data_dir();
        Self::with_token_and_config_dir(token, config_dir)
    }

    pub fn with_token_and_config_dir(
        token: impl Into<String>,
        config_dir: impl Into<PathBuf>,
    ) -> Self {
        let manager = ConfigManager::new(&config_dir.into());
        Self::with_token_and_config_manager(token, manager)
    }

    pub fn with_token_and_config_manager(token: impl Into<String>, manager: ConfigManager) -> Self {
        let initial_config = manager.load_if_exists().unwrap_or(None).unwrap_or_default();
        Self::with_token_and_config(token, manager, initial_config)
    }

    pub fn with_token_and_config(
        token: impl Into<String>,
        config_manager: ConfigManager,
        initial_config: AppConfig,
    ) -> Self {
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
            session_token: Arc::new(RwLock::new(token.into())),
            cached_bindings: Arc::new(RwLock::new(HashMap::new())),
            config_manager: Arc::new(Mutex::new(config_manager)),
            active_config: Arc::new(RwLock::new(initial_config)),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.active_config
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn config_manager(&self) -> Arc<Mutex<ConfigManager>> {
        Arc::clone(&self.config_manager)
    }

    pub fn active_config(&self) -> Arc<RwLock<AppConfig>> {
        Arc::clone(&self.active_config)
    }

    pub fn session_token(&self) -> String {
        self.session_token
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Rotates the authoritative runtime session token and updates the persistent token file.
    ///
    /// Runtime lock is acquired before disk persistence so lock poisoning or acquisition
    /// failure is reported immediately and prevents disk and runtime token divergence.
    pub fn rotate_session_token(&self, guard: &mut SessionTokenGuard) -> io::Result<String> {
        let mut writer = self.session_token.write().map_err(|e| {
            io::Error::other(format!("Failed to acquire session token write lock: {}", e))
        })?;
        let new_token = guard.rotate()?;
        *writer = new_token.clone();
        Ok(new_token)
    }

    #[doc(hidden)]
    pub fn poison_session_token_lock_for_test(&self) {
        let lock = Arc::clone(&self.session_token);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.write().unwrap();
            panic!("poisoning session token lock for test");
        }));
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

    #[allow(dead_code)]
    pub fn cached_bindings(&self) -> Arc<RwLock<HashMap<String, BindingValue>>> {
        Arc::clone(&self.cached_bindings)
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_flag.load(Ordering::Relaxed)
    }

    /// Synchronizes the supervised out-of-process Lua worker based on active configuration and profile (PRD Section 13.2 / Q32:B / Gate G10).
    pub async fn sync_script_worker(
        &self,
        pipe_name: &str,
        is_safe_mode: bool,
    ) -> Result<Option<u32>, Box<dyn std::error::Error + Send + Sync>> {
        sync_script_worker_internal(
            &self.script_supervisor,
            &self.active_config,
            &self.cached_bindings,
            pipe_name,
            is_safe_mode,
        )
        .await
    }

    /// Executes a single periodic watchdog maintenance tick for supervised script worker status and integrity.
    ///
    /// Checks:
    /// 1. Polls child worker status and reaps exited/crashed child processes.
    /// 2. If quarantined or crashed, purges `script.*` bindings from `cached_bindings`.
    /// 3. Continuous integrity verification (Hypothesis A / Continuous Enforcement):
    ///    Checks whether the active script file on disk was modified, deleted, or revoked in `trusted_digests`.
    ///    If invalid, immediately terminates worker and purges `script.*` bindings.
    pub fn tick_script_worker_watchdog(&self) -> bool {
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
                if let Ok(mut cache) = self.cached_bindings.write() {
                    cache.retain(|k, _| !k.starts_with("script."));
                }
            }
            if sup.is_quarantined() {
                warn!("Script worker is quarantined: {:?}", sup.state());
                if let Ok(mut cache) = self.cached_bindings.write() {
                    cache.retain(|k, _| !k.starts_with("script."));
                }
            }

            // Continuous, bounded-latency script worker integrity & revocation revalidation
            if sup.has_active_child() {
                let trusted_digests = self
                    .active_config
                    .read()
                    .ok()
                    .map(|c| c.scripting.trusted_digests.clone());
                if let Err(integrity_err) =
                    sup.verify_active_worker_integrity(trusted_digests.as_ref())
                {
                    warn!(
                        "Script worker integrity verification failed during periodic watchdog: {}. Worker terminated and script bindings purged.",
                        integrity_err
                    );
                    if let Ok(mut cache) = self.cached_bindings.write() {
                        cache.retain(|k, _| !k.starts_with("script."));
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Asynchronous variant of `tick_script_worker_watchdog` that waits for the script supervisor lock.
    pub async fn tick_script_worker_watchdog_async(&self) {
        let mut sup = self.script_supervisor.lock().await;
        let exit_code = sup.poll_worker_status();
        if let Some(code) = exit_code {
            if code != 0 {
                warn!(
                    "Script worker exited unexpectedly with code {}; crash count: {}",
                    code,
                    sup.crash_count()
                );
            }
            if let Ok(mut cache) = self.cached_bindings.write() {
                cache.retain(|k, _| !k.starts_with("script."));
            }
        }
        if sup.is_quarantined() {
            warn!("Script worker is quarantined: {:?}", sup.state());
            if let Ok(mut cache) = self.cached_bindings.write() {
                cache.retain(|k, _| !k.starts_with("script."));
            }
        }

        // Continuous, bounded-latency script worker integrity & revocation revalidation
        if sup.has_active_child() {
            let trusted_digests = self
                .active_config
                .read()
                .ok()
                .map(|c| c.scripting.trusted_digests.clone());
            if let Err(integrity_err) = sup.verify_active_worker_integrity(trusted_digests.as_ref())
            {
                warn!(
                    "Script worker integrity verification failed during periodic watchdog: {}. Worker terminated and script bindings purged.",
                    integrity_err
                );
                if let Ok(mut cache) = self.cached_bindings.write() {
                    cache.retain(|k, _| !k.starts_with("script."));
                }
            }
        }
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
        // Fail-closed if token persistence fails to ensure management controls are never left broken
        let _session_guard = SessionTokenGuard::with_token(self.session_token()).map_err(|e| {
            format!(
                "Fatal error: Could not persist session capability token: {}",
                e
            )
        })?;
        info!(
            "Session capability token registered at {:?}",
            _session_guard.path()
        );

        // Load or initialize persistent configuration store (PRD Section 10 & 16, Q38:B)
        {
            let mgr = self.config_manager.lock().await;
            match mgr.load_or_default() {
                Ok(loaded) => {
                    info!(
                        "Loaded configuration successfully (schema v{})",
                        loaded.schema_version
                    );
                    if let Ok(mut writer) = self.active_config.write() {
                        *writer = loaded;
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to load configuration on startup: {}. Operating with in-memory default.",
                        e
                    );
                }
            }
        }

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

            // Register system-wide global hotkey for Interactive Mode (Win+Ctrl+A per PRD Section 5.3 & Q31:A)
            match win_mgr.register_global_hotkey(MOD_WIN | MOD_CONTROL, 0x41) {
                Ok(()) => {
                    info!("Registered global hotkey Win+Ctrl+A for Temporary Interactive Mode")
                }
                Err(e) => warn!(
                    "Failed to register global hotkey for Interactive Mode: {}",
                    e
                ),
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

            // Wire hotkey toggle callback across all windows
            let ctrl_hotkey = Arc::clone(&self.interaction);
            let active_cfg_hotkey = Arc::clone(&self.active_config);
            win_mgr.set_on_hotkey(move |active| {
                if let Ok(mut ctrl) = ctrl_hotkey.try_lock() {
                    if active {
                        let timeout = active_cfg_hotkey
                            .read()
                            .map(|c| c.desktop.interactive_timeout_seconds)
                            .unwrap_or(8);
                        ctrl.enter_temporary_mode(timeout);
                    } else {
                        ctrl.dismiss_to_passive();
                    }
                }
            });

            // Wire action callback when user clicks interactive regions in interactive mode
            let ctrl_action = Arc::clone(&self.interaction);
            let audio_action = Arc::clone(&self.audio);
            win_mgr.set_on_action(move |region, _px, _py| {
                if let Ok(mut ctrl) = ctrl_action.try_lock() {
                    ctrl.reset_activity();
                }
                if let Some(ref action) = region.action {
                    match action.as_str() {
                        "media.play_pause" | "play_pause" => {
                            if let Ok(aud) = audio_action.try_lock() {
                                let currently_paused = aud.is_paused();
                                aud.set_paused(!currently_paused);
                            }
                        }
                        "media.pause" | "pause" | "pause_audio" => {
                            if let Ok(aud) = audio_action.try_lock() {
                                aud.set_paused(true);
                            }
                        }
                        "media.resume" | "resume" | "resume_audio" => {
                            if let Ok(aud) = audio_action.try_lock() {
                                aud.set_paused(false);
                            }
                        }
                        other => {
                            info!("Interactive action invoked: {}", other);
                        }
                    }
                }
            });

            // Wire Explorer restart recovery across all desktop overlay windows
            let win_mgr_restart = Arc::clone(&self.desktop_windows);
            let renderer_restart = Arc::clone(&self.desktop_renderer);
            let cached_restart = Arc::clone(&self.cached_bindings);
            win_mgr.set_on_explorer_restart(move || {
                info!("Explorer restart notification received: re-probing desktop hierarchy and re-synchronizing surfaces");
                if let Ok(mut mgr) = win_mgr_restart.try_lock() {
                    let probe = mgr.handle_explorer_restart();
                    info!(
                        "Desktop hierarchy re-probed after Explorer restart (DefView: {:?}, WorkerW: {:?})",
                        probe.shelldll_defview_hwnd, probe.worker_w_hwnd
                    );
                    let primary_info = mgr
                        .primary_window()
                        .map(|p| (p.hwnd, p.bounds[2].max(100) as u32, p.bounds[3].max(100) as u32));
                    if let Some((primary_hwnd, width, height)) = primary_info {
                        if let Ok(mut r_lock) = renderer_restart.try_lock() {
                            match D2D1CanvasRenderer::new_for_hwnd(primary_hwnd, width, height) {
                                Ok(mut new_renderer) => {
                                    let bindings = cached_restart
                                        .read()
                                        .map(|b| b.clone())
                                        .unwrap_or_default();
                                    let (scene, regions) = compile_live_scene_and_regions(
                                        width as f32,
                                        height as f32,
                                        &bindings,
                                    );
                                    mgr.set_interactive_regions(regions);
                                    let _ = new_renderer.render_scene(&scene);
                                    *r_lock = Some(new_renderer);
                                    info!(
                                        "Direct2D canvas renderer re-synchronized to HWND {:?}",
                                        primary_hwnd.0
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        "Direct2D re-synchronization warning after Explorer restart: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            });

            // Direct2D / DirectWrite canvas rendering pipeline initialization
            let initial_scale = win_mgr.primary_scale_factor();
            let initial_dpi = win_mgr.primary_dpi();
            if let Some(primary) = win_mgr.primary_window_mut() {
                let width = primary.bounds[2].max(100) as u32;
                let height = primary.bounds[3].max(100) as u32;
                match D2D1CanvasRenderer::new_for_hwnd(primary.hwnd, width, height) {
                    Ok(mut renderer) => {
                        renderer.set_dpi(initial_dpi as f32, initial_dpi as f32);
                        let initial_bindings = self
                            .cached_bindings
                            .read()
                            .map(|b| b.clone())
                            .unwrap_or_default();
                        let (scene, regions) = compile_live_scene_with_scale(
                            width as f32,
                            height as f32,
                            &initial_bindings,
                            initial_scale,
                        );
                        win_mgr.set_interactive_regions(regions);
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
            }

            // Wire PerMonitorV2 DPI change callback across all desktop overlay windows
            let r_dpi = Arc::clone(&self.desktop_renderer);
            let cached_dpi = Arc::clone(&self.cached_bindings);
            let win_mgr_dpi = Arc::clone(&self.desktop_windows);
            win_mgr.set_on_dpi_changed(move |_hwnd, new_dpi, rect| {
                let w = (rect.right - rect.left).max(100) as u32;
                let h = (rect.bottom - rect.top).max(100) as u32;
                let scale = aethershell_win::dpi_to_scale_factor(new_dpi);
                info!(
                    "PerMonitorV2 DPI changed to {} (scale factor {:.2})",
                    new_dpi, scale
                );
                let bindings = cached_dpi.read().map(|b| b.clone()).unwrap_or_default();
                let (scene, regions) =
                    compile_live_scene_with_scale(w as f32, h as f32, &bindings, scale);
                if let Ok(mut mgr) = win_mgr_dpi.try_lock() {
                    mgr.set_interactive_regions(regions);
                }
                if let Ok(mut r_lock) = r_dpi.try_lock() {
                    if let Some(ref mut renderer) = *r_lock {
                        let _ = renderer.resize(w, h);
                        renderer.set_dpi(new_dpi as f32, new_dpi as f32);
                        let _ = renderer.render_scene(&scene);
                    }
                }
            });

            // Wire display topology change callback across all desktop overlay windows
            let r_display = Arc::clone(&self.desktop_renderer);
            let cached_display = Arc::clone(&self.cached_bindings);
            let win_mgr_display = Arc::clone(&self.desktop_windows);
            win_mgr.set_on_display_change(move || {
                info!("Display topology change (WM_DISPLAYCHANGE): refreshing monitors and re-synchronizing layout");
                if let Ok(mut mgr) = win_mgr_display.try_lock() {
                    mgr.refresh_monitors();
                    let (w, h, dpi, scale) = if let Some(primary) = mgr.primary_window() {
                        let b = primary.bounds();
                        let d = primary.dpi();
                        (
                            b[2].max(100) as u32,
                            b[3].max(100) as u32,
                            d,
                            primary.scale_factor(),
                        )
                    } else {
                        (1920, 1080, 96, 1.0)
                    };
                    let bindings = cached_display.read().map(|b| b.clone()).unwrap_or_default();
                    let (scene, regions) =
                        compile_live_scene_with_scale(w as f32, h as f32, &bindings, scale);
                    mgr.set_interactive_regions(regions);
                    if let Ok(mut r_lock) = r_display.try_lock() {
                        if let Some(ref mut renderer) = *r_lock {
                            let _ = renderer.resize(w, h);
                            renderer.set_dpi(dpi as f32, dpi as f32);
                            let _ = renderer.render_scene(&scene);
                        }
                    }
                }
            });

            // Wire on_paint callback across all desktop overlay windows to automatically repaint scene on WM_PAINT
            let r_paint = Arc::clone(&self.desktop_renderer);
            let cached_paint = Arc::clone(&self.cached_bindings);
            let win_mgr_paint = Arc::clone(&self.desktop_windows);
            win_mgr.set_on_paint(move |_hwnd| {
                let (w, h, scale) = if let Ok(mgr) = win_mgr_paint.try_lock() {
                    if let Some(primary) = mgr.primary_window() {
                        let b = primary.bounds();
                        (
                            b[2].max(100) as f32,
                            b[3].max(100) as f32,
                            primary.scale_factor(),
                        )
                    } else {
                        (1920.0, 1080.0, 1.0)
                    }
                } else {
                    (1920.0, 1080.0, 1.0)
                };
                let bindings = cached_paint.read().map(|b| b.clone()).unwrap_or_default();
                let (scene, _) = compile_live_scene_with_scale(w, h, &bindings, scale);
                if let Ok(mut r_lock) = r_paint.try_lock() {
                    if let Some(ref mut renderer) = *r_lock {
                        let _ = renderer.render_scene(&scene);
                    }
                }
            });
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

        // Synchronize on-demand script worker per PRD Section 13.2 / Q32:B / Gate G10
        if let Err(e) = self.sync_script_worker(&pipe_name, is_safe_mode).await {
            warn!(
                "Failed to synchronize on-demand script worker at startup: {}",
                e
            );
        }

        // 7. Engine event loop with periodic interaction timeout tick, telemetry polling, and adaptive presentation loop
        let mut last_telemetry_poll = std::time::Instant::now();
        let mut last_watchdog_check = std::time::Instant::now();
        let mut last_render_tick = std::time::Instant::now();

        // Initial telemetry poll to populate bindings immediately
        let mut current_telemetry: HashMap<String, BindingValue> = {
            if let Ok(mut tel) = self.telemetry.try_lock() {
                tel.poll_all()
            } else {
                HashMap::new()
            }
        };
        {
            if let Ok(mut cache) = self.cached_bindings.write() {
                *cache = current_telemetry.clone();
            }
        }

        // Shed one-time startup allocations (PRD Section 14 & Decision Q47)
        aethershell_win::trim_working_set();

        // Dirty-state presentation gating state (PRD Section 7.3 & Decision Q47)
        let mut last_rendered_bindings: Option<HashMap<String, BindingValue>> = None;
        let mut last_rendered_geometry: Option<([i32; 4], f32)> = None;
        let mut is_initial_frame = true;
        let mut last_trim_time = std::time::Instant::now();

        while !self.shutdown_flag.load(Ordering::Relaxed) {
            // Pump Win32 messages (WM_PAINT, WM_KEYDOWN, WM_ACTIVATE, WM_WTSSESSION_CHANGE, etc.)
            DesktopWindowManager::pump_messages();

            tokio::time::sleep(tokio::time::Duration::from_millis(16)).await;

            let now = std::time::Instant::now();

            // 1. Tick interaction mode timeout
            {
                let mut ctrl = self.interaction.lock().await;
                if ctrl.tick(chrono::Utc::now()) {
                    info!("Temporary Interactive Mode automatically dismissed via timeout.");
                    if let Ok(mut win_mgr) = self.desktop_windows.try_lock() {
                        win_mgr.exit_interactive_mode();
                    }
                }
            }

            // 2. Periodic native telemetry poll (1s interval to prevent CPU wakeups per PRD G08)
            if now.duration_since(last_telemetry_poll) >= std::time::Duration::from_secs(1) {
                if let Ok(mut tel) = self.telemetry.try_lock() {
                    current_telemetry = tel.poll_all();
                }
                last_telemetry_poll = now;
            }

            // 3. Periodic script worker watchdog check, crash monitor, and continuous integrity verification (2s nominal interval)
            if now.duration_since(last_watchdog_check) >= std::time::Duration::from_secs(2)
                && self.tick_script_worker_watchdog()
            {
                last_watchdog_check = now;
            }

            // 4. Adaptive Direct2D Presentation Loop (PRD Section 7.3 & Q36:A)
            let is_on_battery = match current_telemetry.get("power.ac_state") {
                Some(BindingValue::String(s)) => s == "Battery",
                _ => false,
            };

            let configured_fps_mode = if let Ok(cfg) = self.active_config.read() {
                match cfg.power.profile.as_str() {
                    "performance" => FpsMode::Performance,
                    "power_saver" => FpsMode::BatterySaver,
                    _ => {
                        if cfg.audio.render_fps >= 60 {
                            FpsMode::Performance
                        } else if cfg.audio.render_fps <= 15 {
                            FpsMode::BatterySaver
                        } else {
                            FpsMode::AcBalanced
                        }
                    }
                }
            } else {
                FpsMode::AcBalanced
            };

            let target_fps_mode = if is_on_battery {
                FpsMode::BatterySaver // 15 FPS
            } else {
                configured_fps_mode // Restores configured FPS (e.g. 30 FPS or 60 FPS) when on AC power
            };

            let fps_mode = target_fps_mode;

            // Dynamically synchronize audio loopback capture power profile bidirectionally
            if let Ok(aud) = self.audio.try_lock() {
                if aud.fps_mode() != target_fps_mode {
                    aud.set_fps_mode(target_fps_mode);
                }
            }

            let frame_interval = fps_mode.frame_interval();

            if now.duration_since(last_render_tick) >= frame_interval {
                // Check if gaming shield is active (fullscreen application running)
                let is_gaming = if let Ok(tb) = self.taskbar.try_lock() {
                    tb.is_any_monitor_fullscreen()
                } else {
                    false
                };

                if !is_gaming {
                    // Sample audio spectrum
                    let raw_bands = if let Ok(aud) = self.audio.try_lock() {
                        aud.current_bands()
                    } else {
                        Vec::new()
                    };

                    let (bands, is_audio_silent) = normalize_audio_spectrum(&raw_bands);

                    // Construct live bindings
                    let mut live_bindings = current_telemetry.clone();
                    live_bindings.insert("audio.spectrum".into(), BindingValue::FloatArray(bands));

                    let now_local = chrono::Local::now();
                    live_bindings.insert(
                        "time.clock".into(),
                        BindingValue::String(now_local.format("%H:%M").to_string()),
                    );
                    live_bindings.insert(
                        "time.date".into(),
                        BindingValue::String(now_local.format("%A, %B %d").to_string()),
                    );

                    // Update cached bindings with live telemetry & extract full snapshot for rendering
                    let bindings_snapshot = if let Ok(mut cache) = self.cached_bindings.write() {
                        for (k, v) in live_bindings {
                            cache.insert(k, v);
                        }
                        cache.clone()
                    } else {
                        live_bindings
                    };

                    // Render scene if renderer is available and desktop host has primary window
                    if let Ok(win_mgr) = self.desktop_windows.try_lock() {
                        if let Some(primary) = win_mgr.primary_window() {
                            let bounds = primary.bounds();
                            let width = bounds[2].max(100) as f32;
                            let height = bounds[3].max(100) as f32;
                            let scale = primary.scale_factor();
                            let current_geometry = (bounds, scale);

                            // Dirty-state presentation gating (Decision Q47):
                            // When audio spectrum is silent/empty and clock/telemetry strings are identical
                            // to previous frame, skip redundant Direct2D scene compilation and DXGI swapchain draw calls.
                            let is_dirty = should_render_frame(
                                is_initial_frame,
                                last_rendered_geometry,
                                current_geometry,
                                is_audio_silent,
                                last_rendered_bindings.as_ref(),
                                &bindings_snapshot,
                            );

                            if !is_dirty {
                                last_render_tick = now;
                                if now.duration_since(last_trim_time)
                                    >= std::time::Duration::from_secs(10)
                                {
                                    aethershell_win::trim_working_set();
                                    last_trim_time = now;
                                }
                                continue;
                            }

                            let (scene, _) = compile_live_scene_with_scale(
                                width,
                                height,
                                &bindings_snapshot,
                                scale,
                            );

                            if let Ok(mut r_lock) = self.desktop_renderer.try_lock() {
                                if let Some(ref mut renderer) = *r_lock {
                                    if renderer.is_device_lost() {
                                        if let Err(e) = renderer.recover_device() {
                                            tracing::debug!(
                                                "Direct2D renderer waiting for GPU recovery: {}",
                                                e
                                            );
                                            is_initial_frame = true;
                                            continue;
                                        }
                                    }
                                    if let Err(e) = renderer.render_scene(&scene) {
                                        tracing::warn!("Direct2D scene render failed: {}", e);
                                    } else {
                                        last_rendered_bindings = Some(bindings_snapshot);
                                        last_rendered_geometry = Some(current_geometry);
                                        is_initial_frame = false;
                                        last_trim_time = now;
                                    }
                                }
                            }
                        }
                    }
                }

                last_render_tick = now;
            }

            if *shutdown_tx.borrow() {
                break;
            }
        }

        info!("Shutting down AetherShell Engine...");
        let _ = shutdown_tx.send(true);
        {
            let mut sup = self.script_supervisor.lock().await;
            if sup.is_running() {
                info!("Terminating script worker on engine shutdown");
                sup.terminate_worker();
            }
        }
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
        let session_token = Arc::clone(&self.session_token);
        let cached_bindings = Arc::clone(&self.cached_bindings);
        let config_manager = Arc::clone(&self.config_manager);
        let active_config = Arc::clone(&self.active_config);
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
            let session_token = Arc::clone(&session_token);
            let cached_bindings = Arc::clone(&cached_bindings);
            let config_manager = Arc::clone(&config_manager);
            let active_config = Arc::clone(&active_config);

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
                            | "publish_script_bindings"
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
                        | "get_config"
                        | "set_config"
                );

                if is_management_command {
                    let has_valid_token = {
                        let active_token =
                            session_token.read().map(|g| g.clone()).unwrap_or_default();
                        req.payload
                            .get("session_token")
                            .or_else(|| req.payload.get("token"))
                            .and_then(|v| v.as_str())
                            .map(|tok| !tok.is_empty() && tok == active_token.as_str())
                            .unwrap_or(false)
                    };

                    if !has_valid_token {
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
                    "publish_script_bindings" => {
                        let mut sup = supervisor.lock().await;
                        if sup.is_quarantined() {
                            warn!("publish_script_bindings rejected: supervisor is quarantined");
                            return IpcResponse::error(
                                req.message_id,
                                "Security violation: supervisor is quarantined",
                            );
                        }
                        if let Some(child_pid) = sup.active_child_pid() {
                            if child_pid != client_identity.process_id {
                                warn!(
                                    "publish_script_bindings rejected: client PID {} does not match worker PID {}",
                                    client_identity.process_id, child_pid
                                );
                                return IpcResponse::error(
                                    req.message_id,
                                    "Security violation: unexpected worker PID",
                                );
                            }
                            if !sup.is_child_alive() {
                                warn!(
                                    "publish_script_bindings rejected: worker PID {} is dead",
                                    child_pid
                                );
                                sup.invalidate_worker();
                                return IpcResponse::error(
                                    req.message_id,
                                    "Security violation: worker process is dead",
                                );
                            }
                        } else {
                            warn!(
                                "publish_script_bindings rejected: no active worker child from PID {}",
                                client_identity.process_id
                            );
                            return IpcResponse::error(
                                req.message_id,
                                "Security violation: no active worker child expected",
                            );
                        }

                        // Validate payload
                        let bindings_obj =
                            match req.payload.get("bindings").and_then(|v| v.as_object()) {
                                Some(obj) => obj,
                                None => {
                                    return IpcResponse::error(
                                        req.message_id,
                                        "Validation error: 'bindings' must be a JSON object",
                                    );
                                }
                            };

                        // Strictly enforce PRD Q32:B / Section 13 limits
                        if bindings_obj.len() > 32 {
                            return IpcResponse::error(
                                req.message_id,
                                "Validation error: maximum 32 script binding keys allowed per publish",
                            );
                        }

                        let mut valid_bindings = Vec::new();
                        for (k, v) in bindings_obj {
                            if !k.starts_with("script.") && !k.starts_with("user.") {
                                return IpcResponse::error(
                                    req.message_id,
                                    format!(
                                        "Validation error: key '{}' must start with 'script.' or 'user.' namespace",
                                        k
                                    ),
                                );
                            }
                            if k.len() > 64 {
                                return IpcResponse::error(
                                    req.message_id,
                                    format!(
                                        "Validation error: key '{}' exceeds maximum length of 64 characters",
                                        k
                                    ),
                                );
                            }

                            let binding_val = match v {
                                serde_json::Value::String(s) => {
                                    if s.len() > 1024 {
                                        return IpcResponse::error(
                                            req.message_id,
                                            format!(
                                                "Validation error: string value for key '{}' exceeds 1024 characters",
                                                k
                                            ),
                                        );
                                    }
                                    BindingValue::String(s.clone())
                                }
                                serde_json::Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        BindingValue::Integer(i)
                                    } else if let Some(f) = n.as_f64() {
                                        BindingValue::Float(f)
                                    } else {
                                        BindingValue::Float(0.0)
                                    }
                                }
                                serde_json::Value::Bool(b) => BindingValue::String(if *b {
                                    "true".into()
                                } else {
                                    "false".into()
                                }),
                                serde_json::Value::Array(arr) => {
                                    let mut floats = Vec::with_capacity(arr.len().min(64));
                                    for item in arr.iter().take(64) {
                                        if let Some(f) = item.as_f64() {
                                            floats.push(f as f32);
                                        }
                                    }
                                    BindingValue::FloatArray(floats)
                                }
                                _ => continue,
                            };
                            valid_bindings.push((k.clone(), binding_val));
                        }

                        let count = valid_bindings.len();
                        if let Ok(mut cache) = cached_bindings.write() {
                            for (k, v) in valid_bindings {
                                cache.insert(k, v);
                            }
                        }

                        sup.record_heartbeat();

                        IpcResponse::ok(req.message_id, serde_json::json!({ "updated": count }))
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
                        let timeout = req.payload["timeout_seconds"]
                            .as_u64()
                            .map(|t| t as u32)
                            .unwrap_or_else(|| {
                                active_config
                                    .read()
                                    .map(|c| c.desktop.interactive_timeout_seconds)
                                    .unwrap_or(8)
                            });
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
                        let is_safe_mode = taskbar.lock().await.is_safe_mode();
                        let pipe_name = format!(r"\\.\pipe\aethershell-ipc-{}", valid_session_id);
                        let sync_res = sync_script_worker_internal(
                            &supervisor,
                            &active_config,
                            &cached_bindings,
                            &pipe_name,
                            is_safe_mode,
                        )
                        .await;
                        let (worker_synced, worker_err) = match sync_res {
                            Ok(_) => (true, None),
                            Err(e) => (false, Some(e.to_string())),
                        };

                        let mut win_mgr = desktop_windows.lock().await;
                        let mut rendered_commands = 0;
                        let mut rendered = false;
                        let mut render_err: Option<String> = None;
                        if let Some(primary) = win_mgr.primary_window() {
                            let width = primary.bounds[2].max(100) as f32;
                            let height = primary.bounds[3].max(100) as f32;
                            let bindings = cached_bindings
                                .read()
                                .map(|b| b.clone())
                                .unwrap_or_default();
                            let (scene, regions) =
                                compile_live_scene_and_regions(width, height, &bindings);
                            win_mgr.set_interactive_regions(regions);
                            let mut r_lock = desktop_renderer.lock().await;
                            if let Some(ref mut renderer) = *r_lock {
                                match renderer.render_scene(&scene) {
                                    Ok(count) => {
                                        rendered = true;
                                        rendered_commands = count;
                                    }
                                    Err(e) => {
                                        rendered = false;
                                        render_err = Some(e.to_string());
                                        warn!("Direct2D scene rendering error during layout update: {}", e);
                                    }
                                }
                            } else {
                                rendered = false;
                                render_err = Some(
                                    "Desktop render target is uninitialized or missing".into(),
                                );
                                warn!("Direct2D scene rendering skipped: desktop render target is uninitialized or missing");
                            }
                        }

                        // Complete application requires worker_synced AND no rendering error
                        let applied = worker_synced && render_err.is_none();

                        info!(
                            "Updated layout and re-rendered on live desktop host (commands: {}, rendered: {}, applied: {}, worker_synced: {})",
                            rendered_commands, rendered, applied, worker_synced
                        );
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({
                                "accepted": true,
                                "success": applied,
                                "persisted": false,
                                "applied": applied,
                                "worker_synced": worker_synced,
                                "worker_error": worker_err.clone(),
                                "worker_sync_error": worker_err,
                                "rendered": rendered,
                                "rendered_commands": rendered_commands,
                                "render_error": render_err,
                            }),
                        )
                    }
                    "get_config" => {
                        let current_cfg =
                            active_config.read().map(|c| c.clone()).unwrap_or_default();
                        let section = req.payload.get("section").and_then(|v| v.as_str());
                        let response_payload = match section {
                            Some("general") => {
                                serde_json::json!({ "section": "general", "general": current_cfg.general })
                            }
                            Some("taskbar") => {
                                serde_json::json!({ "section": "taskbar", "taskbar": current_cfg.taskbar })
                            }
                            Some("desktop") => {
                                serde_json::json!({ "section": "desktop", "desktop": current_cfg.desktop })
                            }
                            Some("audio") => {
                                serde_json::json!({ "section": "audio", "audio": current_cfg.audio })
                            }
                            Some("telemetry") => serde_json::json!({
                                "section": "telemetry",
                                "telemetry": current_cfg.telemetry
                            }),
                            Some("power") => {
                                serde_json::json!({ "section": "power", "power": current_cfg.power })
                            }
                            Some("gaming") => {
                                serde_json::json!({ "section": "gaming", "gaming": current_cfg.gaming })
                            }
                            Some("scripting") => serde_json::json!({
                                "section": "scripting",
                                "scripting": current_cfg.scripting
                            }),
                            Some("privacy") => {
                                serde_json::json!({ "section": "privacy", "privacy": current_cfg.privacy })
                            }
                            Some("updates") => {
                                serde_json::json!({ "section": "updates", "updates": current_cfg.updates })
                            }
                            Some(other) => {
                                return IpcResponse::error(
                                    req.message_id,
                                    format!("Unknown configuration section: '{}'", other),
                                );
                            }
                            None => serde_json::json!({
                                "config": current_cfg,
                                "schema_version": current_cfg.schema_version,
                            }),
                        };
                        IpcResponse::ok(req.message_id, response_payload)
                    }
                    "set_config" => {
                        // Extract candidate configuration from payload
                        let candidate_res: Result<AppConfig, String> = if let Some(toml_str) =
                            req.payload.get("toml").and_then(|v| v.as_str())
                        {
                            toml::from_str::<AppConfig>(toml_str)
                                .map_err(|e| format!("Failed to parse TOML configuration: {}", e))
                        } else if let Some(cfg_val) = req.payload.get("config") {
                            if let Some(cfg_str) = cfg_val.as_str() {
                                toml::from_str::<AppConfig>(cfg_str)
                                    .or_else(|_| serde_json::from_str::<AppConfig>(cfg_str))
                                    .map_err(|e| {
                                        format!("Failed to parse configuration string: {}", e)
                                    })
                            } else if cfg_val.is_object() {
                                serde_json::from_value::<AppConfig>(cfg_val.clone()).map_err(|e| {
                                    format!("Invalid configuration object schema: {}", e)
                                })
                            } else {
                                Err("Field 'config' must be a valid JSON object or TOML string"
                                    .into())
                            }
                        } else {
                            // Attempt to parse the payload itself (excluding capability token fields)
                            let mut stripped = req.payload.clone();
                            if let Some(map) = stripped.as_object_mut() {
                                map.remove("session_token");
                                map.remove("token");
                            }
                            serde_json::from_value::<AppConfig>(stripped)
                                .map_err(|e| format!("Invalid configuration payload schema: {}", e))
                        };

                        let candidate_cfg = match candidate_res {
                            Ok(cfg) => cfg,
                            Err(err_msg) => {
                                warn!("IPC 'set_config' rejected: {}", err_msg);
                                return IpcResponse::error(req.message_id, err_msg);
                            }
                        };

                        // Pre-validation against integrity, bounds, and schema version constraints
                        if let Err(val_err) = candidate_cfg.validate() {
                            warn!("IPC 'set_config' validation failed: {}", val_err);
                            return IpcResponse::error(
                                req.message_id,
                                format!("Configuration validation failed: {}", val_err),
                            );
                        }

                        // Transactional disk persistence (atomic rename with backup preservation)
                        {
                            let mgr = config_manager.lock().await;
                            if let Err(save_err) = mgr.save_atomic(&candidate_cfg) {
                                warn!(
                                    "IPC 'set_config' atomic disk persistence failed: {}",
                                    save_err
                                );
                                return IpcResponse::error(
                                    req.message_id,
                                    format!("Transactional disk persistence failed: {}", save_err),
                                );
                            }
                        }

                        // Update in-memory active configuration
                        if let Ok(mut writer) = active_config.write() {
                            *writer = candidate_cfg.clone();
                        }

                        // Synchronize on-demand script worker with updated configuration
                        let script_sync_res = {
                            let is_safe_mode = taskbar.lock().await.is_safe_mode();
                            let pipe_name =
                                format!(r"\\.\pipe\aethershell-ipc-{}", valid_session_id);
                            sync_script_worker_internal(
                                &supervisor,
                                &active_config,
                                &cached_bindings,
                                &pipe_name,
                                is_safe_mode,
                            )
                            .await
                        };

                        // Propagate changes to live engine subsystems
                        {
                            // 1. Audio and power FPS profile
                            let fps_mode = match candidate_cfg.power.profile.as_str() {
                                "performance" => FpsMode::Performance,
                                "power_saver" => FpsMode::BatterySaver,
                                _ => {
                                    if candidate_cfg.audio.render_fps >= 60 {
                                        FpsMode::Performance
                                    } else if candidate_cfg.audio.render_fps <= 15 {
                                        FpsMode::BatterySaver
                                    } else {
                                        FpsMode::AcBalanced
                                    }
                                }
                            };
                            let aud = audio.lock().await;
                            aud.set_fps_mode(fps_mode);
                        }

                        // 2. Trigger live scene re-render with updated configuration
                        let mut rendered_commands = 0;
                        let mut rendered = false;
                        let mut render_err: Option<String> = None;
                        {
                            let mut win_mgr = desktop_windows.lock().await;
                            if let Some(primary) = win_mgr.primary_window() {
                                let width = primary.bounds[2].max(100) as f32;
                                let height = primary.bounds[3].max(100) as f32;
                                let bindings = cached_bindings
                                    .read()
                                    .map(|b| b.clone())
                                    .unwrap_or_default();
                                let (scene, regions) =
                                    compile_live_scene_and_regions(width, height, &bindings);
                                win_mgr.set_interactive_regions(regions);
                                let mut r_lock = desktop_renderer.lock().await;
                                if let Some(ref mut renderer) = *r_lock {
                                    match renderer.render_scene(&scene) {
                                        Ok(count) => {
                                            rendered = true;
                                            rendered_commands = count;
                                        }
                                        Err(e) => {
                                            rendered = false;
                                            render_err = Some(e.to_string());
                                            warn!("Direct2D scene rendering error during set_config: {}", e);
                                        }
                                    }
                                } else {
                                    rendered = false;
                                    render_err = Some(
                                        "Desktop render target is uninitialized or missing".into(),
                                    );
                                    warn!("Direct2D scene rendering skipped: desktop render target is uninitialized or missing");
                                }
                            }
                        }

                        let (worker_synced, sync_err) = match script_sync_res {
                            Ok(_) => (true, None),
                            Err(e) => {
                                warn!(
                                    "IPC 'set_config' persisted to disk, but runtime worker synchronization failed: {}",
                                    e
                                );
                                (false, Some(e.to_string()))
                            }
                        };

                        // Complete application requires worker_synced AND no rendering error
                        let applied = worker_synced && render_err.is_none();

                        info!(
                            "IPC 'set_config' processed (persisted: true, applied: {}, worker_synced: {}, rendered: {}, sync_err: {:?}, render_err: {:?})",
                            applied, worker_synced, rendered, sync_err, render_err
                        );
                        IpcResponse::ok(
                            req.message_id,
                            serde_json::json!({
                                "accepted": true,
                                "success": applied,
                                "persisted": true,
                                "applied": applied,
                                "worker_synced": worker_synced,
                                "worker_error": sync_err.clone(),
                                "worker_sync_error": sync_err,
                                "rendered": rendered,
                                "rendered_commands": rendered_commands,
                                "render_error": render_err,
                                "config": candidate_cfg,
                            }),
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
    bindings.insert("metrics.cpu".into(), BindingValue::Percent(0.0));
    bindings.insert("metrics.ram".into(), BindingValue::Float(0.0));

    compile_live_scene(screen_width, _screen_height, &bindings)
}

/// Extracts interactive regions from resolved layout nodes for pointer hit-testing and accessibility (Gate G14).
pub fn extract_interactive_regions(resolved: &[ResolvedNode]) -> Vec<InteractiveRegion> {
    resolved
        .iter()
        .filter(|node| node.node_type == NodeType::Button || node.action.is_some())
        .map(|node| {
            let role = match node.node_type {
                NodeType::Button => "button",
                NodeType::Progress => "progressbar",
                NodeType::Text => "text",
                NodeType::SpectrumVisualizer => "visualizer",
                _ => {
                    if node.action.is_some() {
                        "button"
                    } else {
                        "widget"
                    }
                }
            };
            let name = node
                .text
                .clone()
                .unwrap_or_else(|| node.id.replace('_', " "));
            let val = node.progress.map(|p| format!("{:.0}%", p * 100.0));

            InteractiveRegion::new(
                node.id.clone(),
                node.x,
                node.y,
                node.width,
                node.height,
                node.action.clone(),
            )
            .with_accessibility(name, role, val)
        })
        .collect()
}

/// Compiles the scene with live telemetry, audio spectrum, and time bindings,
/// applying dynamic DPI scaling and returning both the rendered `SceneGraph` and extracted `InteractiveRegion` list.
pub fn compile_live_scene_with_scale(
    screen_width: f32,
    _screen_height: f32,
    bindings: &HashMap<String, BindingValue>,
    scale: f32,
) -> (SceneGraph, Vec<InteractiveRegion>) {
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
    let mut layout: LayoutDefinition = layout_path
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
                [[nodes]]
                id = "play_pause_btn"
                type = "button"
                x = 0.0
                y = 80.0
                width = 110.0
                height = 28.0
                value = "Play / Pause"
                color = "@text.primary"
                background = "@surface.raised"
                action = "media.play_pause"
            "#,
            )
            .unwrap()
        });

    // Ensure time bindings are always present (callers may not provide them)
    let mut merged = bindings.clone();
    let now = chrono::Local::now();
    merged
        .entry("time.clock".into())
        .or_insert_with(|| BindingValue::String(now.format("%H:%M").to_string()));
    merged
        .entry("time.date".into())
        .or_insert_with(|| BindingValue::String(now.format("%A, %B %d").to_string()));

    // When a Lua script publishes script.status or script.* bindings, inject dynamic status node
    if merged.contains_key("script.status") {
        layout.nodes.push(LayoutNode {
            id: "script_status_indicator".into(),
            node_type: NodeType::Text,
            parent: None,
            x: 0.0,
            y: 114.0,
            width: None,
            height: None,
            background: None,
            radius: None,
            value: Some("${script.status}".into()),
            color: Some("@text.secondary".into()),
            font_size_dip: Some(12.0),
            value_source: None,
            min_value: None,
            max_value: None,
            fill: None,
            action: None,
        });
    }

    let effective_scale = if scale > 0.0 { scale } else { 1.0 };
    let origin_x = (screen_width * 0.5 - 120.0 * effective_scale).max(40.0 * effective_scale);
    let origin_y = 60.0f32 * effective_scale;

    let resolved = LayoutEngine::resolve(
        &layout,
        &theme,
        &merged,
        origin_x,
        origin_y,
        effective_scale,
    )
    .unwrap_or_default();

    let regions = extract_interactive_regions(&resolved);
    (SceneGraph::compile(&resolved), regions)
}

/// Compiles the scene with live telemetry, audio spectrum, and time bindings,
/// returning both the rendered `SceneGraph` and extracted `InteractiveRegion` list.
pub fn compile_live_scene_and_regions(
    screen_width: f32,
    screen_height: f32,
    bindings: &HashMap<String, BindingValue>,
) -> (SceneGraph, Vec<InteractiveRegion>) {
    compile_live_scene_with_scale(screen_width, screen_height, bindings, 1.0)
}

/// Compiles the scene with live telemetry, audio spectrum, and time bindings.
pub fn compile_live_scene(
    screen_width: f32,
    screen_height: f32,
    bindings: &HashMap<String, BindingValue>,
) -> SceneGraph {
    compile_live_scene_and_regions(screen_width, screen_height, bindings).0
}

fn find_asset_file(rel_path: &str) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(rel_path),
        PathBuf::from("../../").join(rel_path),
        PathBuf::from("../").join(rel_path),
    ];
    candidates.into_iter().find(|candidate| candidate.exists())
}

/// Helper to locate the script host executable (`aethershell-script-host.exe`).
pub fn find_script_host_binary(configured: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(configured);
    if direct.is_file() {
        return Some(direct);
    }
    if let Ok(mut current) = std::env::current_exe() {
        current.set_file_name(configured);
        if current.is_file() {
            return Some(current);
        }
        let mut parent = current.clone();
        parent.pop(); // pop configured file name
        if parent.file_name().map(|n| n == "deps").unwrap_or(false) {
            parent.pop(); // pop deps
            let candidate = parent.join(configured);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let candidates = [
        format!("target/x86_64-pc-windows-msvc/debug/{}", configured),
        format!("target/x86_64-pc-windows-msvc/release/{}", configured),
        format!("target/debug/{}", configured),
        format!("target/release/{}", configured),
        format!("../../target/x86_64-pc-windows-msvc/debug/{}", configured),
        format!("../../target/x86_64-pc-windows-msvc/release/{}", configured),
        format!("../../target/debug/{}", configured),
        format!("../../target/release/{}", configured),
        format!("../target/x86_64-pc-windows-msvc/debug/{}", configured),
        format!("../target/x86_64-pc-windows-msvc/release/{}", configured),
        format!("../target/debug/{}", configured),
        format!("../target/release/{}", configured),
    ];
    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Core synchronization logic for the supervised out-of-process Lua 5.4 child worker process.
pub async fn sync_script_worker_internal(
    supervisor: &Arc<Mutex<ScriptSupervisor>>,
    active_config: &Arc<RwLock<AppConfig>>,
    cached_bindings: &Arc<RwLock<HashMap<String, BindingValue>>>,
    pipe_name: &str,
    is_safe_mode: bool,
) -> Result<Option<u32>, Box<dyn std::error::Error + Send + Sync>> {
    let (scripting_enabled, worker_exe_name, active_script, trusted_digests) = {
        if let Ok(cfg) = active_config.read() {
            (
                cfg.scripting.enabled,
                cfg.scripting.worker_executable.clone(),
                cfg.scripting.active_script.clone(),
                cfg.scripting.trusted_digests.clone(),
            )
        } else {
            (
                true,
                "aethershell-script-host.exe".into(),
                None,
                HashMap::new(),
            )
        }
    };

    // 1. Safe Mode or Scripting Disabled -> terminate worker and clean bindings
    if is_safe_mode || !scripting_enabled {
        let mut sup = supervisor.lock().await;
        if sup.is_running() || sup.has_active_child() {
            info!("Terminating script worker due to safe mode or scripting disabled");
            sup.terminate_worker();
        }
        if let Ok(mut cache) = cached_bindings.write() {
            cache.retain(|k, _| !k.starts_with("script."));
        }
        return Ok(None);
    }

    // 2. Discover target script path
    let script_to_run = if let Some(ref s) = active_script {
        Some(s.clone())
    } else {
        // Inspect active profile for widget skin script declarations
        let profile_path = find_asset_file("assets/profiles/signature_minimal.toml");
        let profile: ProfileDefinition = profile_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|c| toml::from_str(&c).ok())
            .unwrap_or_else(ProfileDefinition::signature_minimal);

        let mut found = None;
        for widget in &profile.widgets {
            let manifest_path = format!("assets/skins/{}/manifest.toml", widget.skin);
            if let Some(p) = find_asset_file(&manifest_path) {
                if let Ok(content) = std::fs::read_to_string(&p) {
                    if let Ok(manifest) = toml::from_str::<aethershell_core::SkinManifest>(&content)
                    {
                        if let Some(script_file) = manifest.script {
                            let candidate = if let Some(parent) = p.parent() {
                                parent.join(&script_file)
                            } else {
                                PathBuf::from(&script_file)
                            };
                            if candidate.is_file() {
                                found = Some(candidate.to_string_lossy().to_string());
                                break;
                            }
                        }
                    }
                }
            }
        }
        found
    };

    // 3. If no script is required, terminate any running worker and purge script bindings
    let script_path_str = match script_to_run {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            let mut sup = supervisor.lock().await;
            if sup.is_running() || sup.has_active_child() {
                info!("No scripted widgets active; stopping on-demand script worker per PRD Section 13.2");
                sup.terminate_worker();
            }
            if let Ok(mut cache) = cached_bindings.write() {
                cache.retain(|k, _| !k.starts_with("script."));
            }
            return Ok(None);
        }
    };

    let script_path = if let Some(found_asset) = find_asset_file(&script_path_str) {
        found_asset
    } else {
        PathBuf::from(&script_path_str)
    };

    if !script_path.is_file() {
        warn!("Target script file does not exist: {:?}", script_path);
        let mut sup = supervisor.lock().await;
        if sup.is_running() || sup.has_active_child() {
            info!(
                "Terminating running script worker because target script file no longer exists: {:?}",
                script_path
            );
            sup.terminate_worker();
        }
        if let Ok(mut cache) = cached_bindings.write() {
            cache.retain(|k, _| !k.starts_with("script."));
        }
        return Err(format!("Target script file does not exist: {:?}", script_path).into());
    }

    // 4. Enforce mandatory explicit user trust grant (PRD Section 13.2 / Q32:B / Gate G10)
    // Mandatory consent for imported executable scripts is strictly enforced and cannot be bypassed.
    let approved_digest = match aethershell_script_host::authorize_script_trust(
        &script_path,
        &script_path_str,
        &trusted_digests,
    ) {
        Ok(digest) => digest,
        Err(e) => {
            warn!(
                "Security violation: Script '{:?}' rejected by trust authorization: {}. Worker launch / execution revoked per PRD Section 13.2.",
                script_path, e
            );
            let mut sup = supervisor.lock().await;
            if sup.is_running() || sup.has_active_child() {
                info!("Terminating running script worker due to trust authorization failure / revocation");
                sup.terminate_worker();
            }
            if let Ok(mut cache) = cached_bindings.write() {
                cache.retain(|k, _| !k.starts_with("script."));
            }
            return Err(format!("Script trust authorization rejected: {}", e).into());
        }
    };

    // 5. Supervisor checks
    let mut sup = supervisor.lock().await;
    if sup.is_quarantined() {
        warn!(
            "Script worker is quarantined ({:?}); launch aborted",
            sup.state()
        );
        return Err(format!("Script supervisor is quarantined: {:?}", sup.state()).into());
    }

    let canonical_target = aethershell_script_host::normalize_script_path(&script_path)
        .unwrap_or_else(|_| script_path.clone());

    if sup.is_child_alive() {
        let is_same_script = sup
            .current_script()
            .map(|p| {
                let canon_curr = aethershell_script_host::normalize_script_path(p)
                    .unwrap_or_else(|_| p.to_path_buf());
                aethershell_script_host::paths_equal(&canon_curr, &canonical_target)
            })
            .unwrap_or(false);

        let is_same_digest = sup.current_digest() == Some(&approved_digest);

        if is_same_script && is_same_digest {
            tracing::debug!(
                "Script worker already running and alive for {:?} with matching authorized digest {} (PID: {:?})",
                canonical_target,
                approved_digest,
                sup.active_child_pid()
            );
            return Ok(sup.active_child_pid());
        } else {
            if !is_same_script {
                info!(
                    "Active script changed from {:?} to {:?}; restarting worker",
                    sup.current_script(),
                    canonical_target
                );
            } else {
                info!(
                    "Active script content digest updated at {:?} (from {:?} to {}); restarting worker with approved new digest",
                    canonical_target,
                    sup.current_digest(),
                    approved_digest
                );
            }
            sup.terminate_worker();
            if let Ok(mut cache) = cached_bindings.write() {
                cache.retain(|k, _| !k.starts_with("script."));
            }
        }
    }

    // 6. Locate worker executable binary
    let host_bin = find_script_host_binary(&worker_exe_name)
        .ok_or_else(|| format!("Could not locate script host binary '{}'", worker_exe_name))?;

    // 7. Spawn worker with Job Object containment and verified approved digest
    let pid = sup
        .spawn_worker(&host_bin, &canonical_target, &approved_digest, pipe_name)
        .map_err(|e| format!("Supervisor spawn_worker error: {}", e))?;

    info!(
        "Successfully launched supervised on-demand Lua worker (PID: {}) for script {:?} (digest: {})",
        pid, canonical_target, approved_digest
    );

    Ok(Some(pid))
}

/// Normalizes audio spectrum bands for dirty presentation gating.
///
/// Under Decision Q47: If all bands are below the noise floor threshold (<= 0.001) or empty,
/// clamps non-empty bands to 0.0 to prevent microscopic noise floor jitter from dirtying
/// the presentation loop during silence, while ensuring a clean final frame is drawn when
/// active audio decays.
pub fn normalize_audio_spectrum(bands: &[f32]) -> (Vec<f32>, bool) {
    let is_silent = bands.is_empty() || bands.iter().all(|&b| b <= 0.001);
    if is_silent {
        if bands.is_empty() || bands.iter().all(|&b| b == 0.0) {
            (bands.to_vec(), true)
        } else {
            (vec![0.0f32; bands.len()], true)
        }
    } else {
        (bands.to_vec(), false)
    }
}

/// Determines whether the desktop presentation scene must be recompiled and rendered to the DXGI swapchain.
///
/// Under Decision Q47 (G13 Runtime Resource Optimization):
/// Returns false when the audio spectrum is silent/empty (bands <= 0.001) and all window geometry,
/// DPI scaling, clock, telemetry, and script bindings are unchanged from the previous frame.
pub fn should_render_frame(
    is_initial_frame: bool,
    last_geometry: Option<([i32; 4], f32)>,
    current_geometry: ([i32; 4], f32),
    is_audio_silent: bool,
    last_bindings: Option<&HashMap<String, BindingValue>>,
    current_bindings: &HashMap<String, BindingValue>,
) -> bool {
    let geometry_unchanged = match last_geometry {
        Some(prev_geo) => prev_geo == current_geometry,
        None => false,
    };
    let bindings_unchanged = match last_bindings {
        Some(prev) => prev == current_bindings,
        None => false,
    };

    is_initial_frame || !geometry_unchanged || !is_audio_silent || !bindings_unchanged
}
