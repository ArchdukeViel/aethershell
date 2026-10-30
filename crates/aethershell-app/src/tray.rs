use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadIconW, PostMessageW,
    PostQuitMessage, PostThreadMessageW, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenuEx, TranslateMessage, GWLP_USERDATA,
    IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
    WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION,
    WM_QUIT, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

pub const WM_TRAYICON: u32 = WM_APP + 1;

pub const ID_TRAY_OPEN_SETTINGS: usize = 1001;
pub const ID_TRAY_UNLOCK_LAYOUT: usize = 1002;
pub const ID_TRAY_PAUSE_AUDIO: usize = 1003;
pub const ID_TRAY_SAFE_MODE: usize = 1004;
pub const ID_TRAY_DIAGNOSTICS: usize = 1005;
pub const ID_TRAY_EXIT: usize = 1006;
pub const ID_TRAY_RESUME_AUDIO: usize = 1007;
pub const ID_TRAY_INTERACTIVE_MODE: usize = 1008;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_IS_PAUSED: AtomicBool = AtomicBool::new(false);
static TRAY_PIPE_NAME: RwLock<Option<String>> = RwLock::new(None);

/// Registers or retrieves the dynamic `TaskbarCreated` message broadcast when Explorer restarts.
pub fn taskbar_created_msg() -> u32 {
    static CACHED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = CACHED.load(Ordering::Relaxed);
    if id != 0 {
        return id;
    }
    unsafe {
        let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        CACHED.store(msg, Ordering::Relaxed);
        msg
    }
}

pub type ExplorerRestartCallback = Arc<dyn Fn() + Send + Sync>;

/// State stored in GWLP_USERDATA of the hidden tray window.
pub struct TrayWindowState {
    pub nid: NOTIFYICONDATAW,
    pub is_active: Arc<AtomicBool>,
    pub reconnect_count: Arc<AtomicUsize>,
    pub last_reconnect_tick: Arc<AtomicU64>,
    pub on_explorer_restart: std::sync::Mutex<Option<ExplorerRestartCallback>>,
}

/// System Tray controller for managing Windows shell notification icons.
pub struct SystemTray {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    is_active: Arc<AtomicBool>,
    reconnect_count: Arc<AtomicUsize>,
    state_ptr: *mut TrayWindowState,
}

unsafe impl Send for SystemTray {}
unsafe impl Sync for SystemTray {}

impl SystemTray {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let class_name = w!("AetherShellTrayWindowClass");

            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(tray_window_proc),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                    .unwrap_or_default()
                    .into(),
                lpszClassName: class_name,
                ..Default::default()
            };

            let _ = RegisterClassW(&wnd_class);

            let hwnd = CreateWindowExW(
                Default::default(),
                class_name,
                w!("AetherShellTrayWindow"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                HWND::default(),
                None,
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default(),
                None,
            )
            .map_err(|e| format!("Failed to create hidden tray window: {}", e))?;

            // Register TaskbarCreated message dynamically
            let _ = taskbar_created_msg();

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
                ..Default::default()
            };

            // Set tooltip
            let tip = "AetherShell Engine";
            let mut tip_chars = [0u16; 128];
            for (i, c) in tip.encode_utf16().enumerate().take(127) {
                tip_chars[i] = c;
            }
            nid.szTip = tip_chars;

            let is_active = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
            let is_active_atomic = Arc::new(AtomicBool::new(is_active));
            let reconnect_count = Arc::new(AtomicUsize::new(0));
            let last_reconnect_tick = Arc::new(AtomicU64::new(0));

            let state = Box::new(TrayWindowState {
                nid,
                is_active: Arc::clone(&is_active_atomic),
                reconnect_count: Arc::clone(&reconnect_count),
                last_reconnect_tick,
                on_explorer_restart: std::sync::Mutex::new(None),
            });
            let state_ptr = Box::into_raw(state);

            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            Ok(Self {
                hwnd,
                nid,
                is_active: is_active_atomic,
                reconnect_count,
                state_ptr,
            })
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    pub fn reconnect_count(&self) -> usize {
        self.reconnect_count.load(Ordering::SeqCst)
    }

    /// Explicitly triggers tray icon re-registration upon Explorer restart.
    ///
    /// Safely cleans up any stale shell icon registration first to prevent duplicates,
    /// then registers the notification icon into the fresh taskbar.
    pub fn handle_taskbar_created(&mut self) -> bool {
        unsafe {
            let now_tick = GetTickCount64();
            if !self.state_ptr.is_null() {
                let prev_tick = (*self.state_ptr)
                    .last_reconnect_tick
                    .load(Ordering::Relaxed);
                if now_tick >= prev_tick && now_tick - prev_tick < 300 {
                    return self.is_active();
                }
                (*self.state_ptr)
                    .last_reconnect_tick
                    .store(now_tick, Ordering::Relaxed);
            }

            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            let mut added = Shell_NotifyIconW(NIM_ADD, &self.nid).as_bool();
            if !added {
                // Taskbar notification area might still be initializing during Explorer startup;
                // perform bounded retry (up to 3 attempts with 50ms interval)
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
                    added = Shell_NotifyIconW(NIM_ADD, &self.nid).as_bool();
                    if added {
                        break;
                    }
                }
            }
            self.is_active.store(added, Ordering::SeqCst);
            self.reconnect_count.fetch_add(1, Ordering::SeqCst);

            if !self.state_ptr.is_null() {
                (*self.state_ptr).is_active.store(added, Ordering::SeqCst);
            }
            added
        }
    }

    /// Sets a callback invoked when Explorer restarts (`TaskbarCreated`).
    pub fn set_on_explorer_restart<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_explorer_restart.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn is_exit_requested() -> bool {
        SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
    }

    pub fn set_audio_paused_state(paused: bool) {
        AUDIO_IS_PAUSED.store(paused, Ordering::SeqCst);
    }

    pub fn is_audio_paused() -> bool {
        AUDIO_IS_PAUSED.load(Ordering::Relaxed)
    }

    /// Signals the tray message window to close and exit.
    #[allow(dead_code)]
    pub fn request_close(&self) {
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    /// Spawns the system tray on a dedicated OS thread with an event-driven Win32 message pump.
    ///
    /// Resolves Finding IVA-P1-001 and Decision Q47: Uses blocking `GetMessageW` to consume
    /// strictly 0 CPU and 0 wakeups when idle, waking immediately upon shell messages or shutdown.
    pub fn spawn_tray_thread(
        shutdown_flag: Arc<AtomicBool>,
        pipe_name: Option<String>,
    ) -> (std::thread::JoinHandle<()>, TrayShutdownHandle) {
        SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        if let Some(pipe) = pipe_name {
            if let Ok(mut lock) = TRAY_PIPE_NAME.write() {
                *lock = Some(pipe);
            }
        }

        let tray_shutdown_flag = Arc::new(AtomicBool::new(false));
        let tray_shutdown_clone = Arc::clone(&tray_shutdown_flag);

        let (tx, rx) = std::sync::mpsc::sync_channel::<(u32, isize)>(1);

        let handle = std::thread::spawn(move || {
            let thread_id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };

            let tray = match SystemTray::new() {
                Ok(t) => {
                    let _ = tx.send((thread_id, t.hwnd().0 as isize));
                    t
                }
                Err(e) => {
                    let _ = tx.send((thread_id, 0));
                    tracing::warn!("Could not create system tray: {}", e);
                    return;
                }
            };

            tracing::info!(
                "System tray thread active with event-driven blocking message pump (HWND: {:?}, ThreadId: {})",
                tray.hwnd().0,
                thread_id
            );

            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);

                    if shutdown_flag.load(Ordering::Relaxed)
                        || tray_shutdown_clone.load(Ordering::Relaxed)
                        || Self::is_exit_requested()
                    {
                        break;
                    }
                }

                // Unconditionally signal shutdown_flag when the tray message loop terminates
                shutdown_flag.store(true, Ordering::SeqCst);
            }

            drop(tray);
            tracing::info!("System tray thread cleanly terminated.");
        });

        let (thread_id, hwnd_raw) = match rx.recv() {
            Ok((tid, h)) => (tid, h),
            Err(_) => (0, 0),
        };

        let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        let tray_shutdown = TrayShutdownHandle::new(tray_shutdown_flag, thread_id, hwnd);

        (handle, tray_shutdown)
    }
}

/// Thread-safe shutdown trigger handle for the dedicated system tray message thread.
///
/// Dispatches `WM_CLOSE` to the tray window and `WM_QUIT` to the thread message queue,
/// unblocking `GetMessageW` immediately while consuming 0 CPU and 0 wakeups when idle.
#[derive(Clone, Debug)]
pub struct TrayShutdownHandle {
    flag: Arc<AtomicBool>,
    thread_id: u32,
    hwnd: HWND,
}

unsafe impl Send for TrayShutdownHandle {}
unsafe impl Sync for TrayShutdownHandle {}

impl TrayShutdownHandle {
    pub fn new(flag: Arc<AtomicBool>, thread_id: u32, hwnd: HWND) -> Self {
        Self {
            flag,
            thread_id,
            hwnd,
        }
    }

    pub fn store(&self, val: bool, order: Ordering) {
        let prev = self.flag.swap(val, order);
        if val && !prev {
            unsafe {
                if !self.hwnd.0.is_null() {
                    let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                if self.thread_id != 0 {
                    let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
        }
    }

    pub fn load(&self, order: Ordering) -> bool {
        self.flag.load(order)
    }

    pub fn signal(&self) {
        self.store(true, Ordering::SeqCst);
    }

    pub fn flag(&self) -> &Arc<AtomicBool> {
        &self.flag
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn thread_id(&self) -> u32 {
        self.thread_id
    }
}

impl std::ops::Deref for TrayShutdownHandle {
    type Target = AtomicBool;
    fn deref(&self) -> &Self::Target {
        &self.flag
    }
}

impl Drop for SystemTray {
    fn drop(&mut self) {
        unsafe {
            if self.is_active() {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            }
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
            if !self.state_ptr.is_null() {
                drop(Box::from_raw(self.state_ptr));
                self.state_ptr = std::ptr::null_mut();
            }
        }
    }
}

fn dispatch_tray_ipc(command: &'static str, mut payload: serde_json::Value) {
    if let Some(token) = aethershell_ipc::SessionToken::load() {
        if let Some(obj) = payload.as_object_mut() {
            if !obj.contains_key("session_token") {
                obj.insert("session_token".into(), serde_json::json!(token));
            }
        }
    }

    let pipe_name = {
        let lock = TRAY_PIPE_NAME.read().ok();
        lock.and_then(|g| g.clone())
    };

    if let Some(pipe) = pipe_name {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                let _ = rt.block_on(aethershell_ipc::IpcClient::send_command(
                    &pipe, command, payload,
                ));
            }
        });
    }
}

fn handle_open_settings() {
    dispatch_tray_ipc("open_settings", serde_json::json!({}));
    if let Ok(curr) = std::env::current_exe() {
        if let Some(parent) = curr.parent() {
            let settings_exe = parent.join("aethershell-settings.exe");
            if settings_exe.exists() {
                let _ = std::process::Command::new(settings_exe).spawn();
            } else if let Some(grandparent) = parent.parent() {
                let settings_exe2 = grandparent.join("aethershell-settings.exe");
                if settings_exe2.exists() {
                    let _ = std::process::Command::new(settings_exe2).spawn();
                }
            }
        }
    }
}

fn handle_safe_mode_restart() {
    if let Ok(curr) = std::env::current_exe() {
        let _ = std::process::Command::new(curr).arg("--safe-mode").spawn();
    }
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    unsafe {
        PostQuitMessage(0);
    }
}

fn handle_export_diagnostics() {
    let pipe_name = {
        let lock = TRAY_PIPE_NAME.read().ok();
        lock.and_then(|g| g.clone())
    };

    let target_dir = std::env::temp_dir().join("aethershell").join("diagnostics");
    let _ = std::fs::create_dir_all(&target_dir);
    let now_str = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let file_path = target_dir.join(format!("diagnostics_{}.json", now_str));

    if let Some(pipe) = pipe_name {
        let out_path = file_path.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                let resp = rt.block_on(aethershell_ipc::IpcClient::send_command(
                    &pipe,
                    "get_status",
                    serde_json::json!({}),
                ));
                if let Ok(r) = resp {
                    if let Ok(json_str) = serde_json::to_string_pretty(&r.payload) {
                        let _ = std::fs::write(&out_path, json_str);
                        tracing::info!("Exported diagnostics report to: {:?}", out_path);
                    }
                } else {
                    let report = serde_json::json!({
                        "exported_at": chrono::Utc::now().to_rfc3339(),
                        "status": "ipc_query_failed",
                    });
                    if let Ok(json_str) = serde_json::to_string_pretty(&report) {
                        let _ = std::fs::write(&out_path, json_str);
                    }
                }
            }
        });
    } else {
        let report = serde_json::json!({
            "exported_at": chrono::Utc::now().to_rfc3339(),
            "status": "offline_export",
        });
        if let Ok(json_str) = serde_json::to_string_pretty(&report) {
            let _ = std::fs::write(&file_path, json_str);
            tracing::info!("Exported offline diagnostics report to: {:?}", file_path);
        }
    }
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = taskbar_created_msg();
    if taskbar_created != 0 && msg == taskbar_created {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayWindowState;
        if !ptr.is_null() {
            let state = &mut *ptr;
            let now_tick = GetTickCount64();
            let prev_tick = state.last_reconnect_tick.load(Ordering::Relaxed);
            // Debounce: prevent duplicate handling / restart loops within 300ms
            if now_tick >= prev_tick && now_tick - prev_tick < 300 {
                return LRESULT(0);
            }
            state.last_reconnect_tick.store(now_tick, Ordering::Relaxed);

            tracing::info!(
                "Tray (HWND {:?}): Processing TaskbarCreated (Explorer restart); re-registering notification tray icon",
                hwnd.0
            );

            // Clean up stale registration first to eliminate duplicate icons
            let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
            // Re-add icon to newly created taskbar notification area with bounded retries
            let mut added = Shell_NotifyIconW(NIM_ADD, &state.nid).as_bool();
            if !added {
                // Taskbar notification area might still be initializing during Explorer startup;
                // perform bounded retry (up to 3 attempts with 50ms interval)
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
                    added = Shell_NotifyIconW(NIM_ADD, &state.nid).as_bool();
                    if added {
                        break;
                    }
                }
            }
            state.is_active.store(added, Ordering::SeqCst);
            state.reconnect_count.fetch_add(1, Ordering::SeqCst);

            tracing::info!("Tray: Re-registered notification icon (added={})", added);

            if let Ok(guard) = state.on_explorer_restart.lock() {
                if let Some(ref cb) = *guard {
                    cb();
                }
            }
        }
        return LRESULT(0);
    }

    match msg {
        WM_TRAYICON => {
            if lparam.0 as u32 == WM_RBUTTONUP {
                show_tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = wparam.0;
            match cmd_id {
                ID_TRAY_EXIT => {
                    tracing::info!("Tray: Exit clicked; requesting shutdown");
                    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                    dispatch_tray_ipc("shutdown", serde_json::json!({}));
                    PostQuitMessage(0);
                }
                ID_TRAY_OPEN_SETTINGS => {
                    tracing::info!("Tray: Open Settings clicked");
                    handle_open_settings();
                }
                ID_TRAY_INTERACTIVE_MODE => {
                    tracing::info!(
                        "Tray: Interactive Mode clicked; entering temporary interactive mode"
                    );
                    dispatch_tray_ipc(
                        "enter_interactive_mode",
                        serde_json::json!({ "timeout_seconds": 8 }),
                    );
                }
                ID_TRAY_UNLOCK_LAYOUT => {
                    tracing::info!(
                        "Tray: Unlock Layout clicked; entering temporary interactive mode"
                    );
                    dispatch_tray_ipc(
                        "enter_interactive_mode",
                        serde_json::json!({ "timeout_seconds": 8 }),
                    );
                }
                ID_TRAY_PAUSE_AUDIO => {
                    tracing::info!("Tray: Pause Audio Visualization clicked");
                    AUDIO_IS_PAUSED.store(true, Ordering::SeqCst);
                    dispatch_tray_ipc("pause_audio", serde_json::json!({}));
                }
                ID_TRAY_RESUME_AUDIO => {
                    tracing::info!("Tray: Resume Audio Visualization clicked");
                    AUDIO_IS_PAUSED.store(false, Ordering::SeqCst);
                    dispatch_tray_ipc("resume_audio", serde_json::json!({}));
                }
                ID_TRAY_SAFE_MODE => {
                    tracing::info!("Tray: Safe Mode toggled; restarting in safe mode");
                    handle_safe_mode_restart();
                }
                ID_TRAY_DIAGNOSTICS => {
                    tracing::info!("Tray: Diagnostics clicked; exporting report");
                    handle_export_diagnostics();
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => {
            tracing::info!(
                "Tray window (HWND {:?}): WM_QUERYENDSESSION received; confirming safe shutdown capability (LRESULT 1)",
                hwnd.0
            );
            LRESULT(1)
        }
        WM_ENDSESSION => {
            let is_ending = wparam.0 != 0;
            tracing::info!(
                "Tray window (HWND {:?}): WM_ENDSESSION received (is_ending={})",
                hwnd.0,
                is_ending
            );
            if is_ending {
                SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                dispatch_tray_ipc("shutdown", serde_json::json!({ "reason": "WM_ENDSESSION" }));
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_CLOSE | WM_DESTROY => {
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };

    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN_SETTINGS, w!("Open Settings"));
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_TRAY_INTERACTIVE_MODE,
        w!("Interactive Mode (Win+Ctrl+A)"),
    );
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_UNLOCK_LAYOUT, w!("Unlock Layout"));

    if AUDIO_IS_PAUSED.load(Ordering::Relaxed) {
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_TRAY_RESUME_AUDIO,
            w!("Resume Audio Visualization"),
        );
    } else {
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_TRAY_PAUSE_AUDIO,
            w!("Pause Audio Visualization"),
        );
    }

    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_TRAY_SAFE_MODE,
        w!("Restart in Safe Mode"),
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_TRAY_DIAGNOSTICS,
        w!("Export Diagnostics..."),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, w!("Exit AetherShell"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenuEx(
        menu,
        (TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON).0,
        pt.x,
        pt.y,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

    static TRAY_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_tray_initialization_non_panicking() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let tray = SystemTray::new();
        if let Ok(t) = tray {
            println!("Tray icon initialized: {}", t.is_active());
        }
    }

    #[test]
    fn test_tray_thread_lifecycle_iva_p1_001() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (join_handle, tray_shutdown) =
            SystemTray::spawn_tray_thread(Arc::clone(&flag), Some(r"\\.\pipe\test-tray".into()));

        // Sleep briefly to allow thread message pump loop to run
        std::thread::sleep(std::time::Duration::from_millis(60));

        // Signal stop
        tray_shutdown.store(true, Ordering::SeqCst);
        let _ = join_handle.join();
    }

    #[test]
    fn test_tray_context_menu_command_dispatch() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        // Test audio pause/resume toggle state
        SystemTray::set_audio_paused_state(false);
        assert!(!SystemTray::is_audio_paused());

        SystemTray::set_audio_paused_state(true);
        assert!(SystemTray::is_audio_paused());

        // Test export diagnostics offline fallback execution
        handle_export_diagnostics();
    }

    #[test]
    fn test_tray_taskbar_created_reconnect_and_debounce() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let msg = taskbar_created_msg();
        assert!(
            msg >= 0xC000,
            "TaskbarCreated dynamic message must be >= 0xC000, got 0x{:04X}",
            msg
        );

        let mut tray = match SystemTray::new() {
            Ok(t) => t,
            Err(e) => {
                println!("SystemTray creation skipped or failed in test env: {}", e);
                return;
            }
        };

        assert_eq!(tray.reconnect_count(), 0);

        let restart_fired = Arc::new(AtomicBool::new(false));
        let rf_clone = Arc::clone(&restart_fired);
        tray.set_on_explorer_restart(move || {
            rf_clone.store(true, Ordering::SeqCst);
        });

        // 1. Dispatch TaskbarCreated message to tray_window_proc
        unsafe {
            let res = tray_window_proc(tray.hwnd(), msg, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(0));
        }

        assert_eq!(tray.reconnect_count(), 1);
        assert!(restart_fired.load(Ordering::SeqCst));

        // 2. Immediate duplicate TaskbarCreated message within 300ms must be debounced
        unsafe {
            let res = tray_window_proc(tray.hwnd(), msg, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(0));
        }
        // Count remains 1 due to debounce loop prevention
        assert_eq!(tray.reconnect_count(), 1);

        // Immediate explicit handle_taskbar_created within 300ms must also be debounced
        let _ = tray.handle_taskbar_created();
        assert_eq!(
            tray.reconnect_count(),
            1,
            "handle_taskbar_created within debounce interval must not double-increment reconnect count"
        );

        // 3. Explicit handle_taskbar_created invocation after debounce expiry (>300ms + VM timer quantum margin)
        std::thread::sleep(std::time::Duration::from_millis(450));
        let added = tray.handle_taskbar_created();
        println!("Explicit handle_taskbar_created added: {}", added);
        assert_eq!(tray.reconnect_count(), 2);
    }

    #[test]
    fn test_tray_event_driven_blocking_and_shutdown_handle() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (join_handle, tray_shutdown) = SystemTray::spawn_tray_thread(Arc::clone(&flag), None);

        assert_ne!(tray_shutdown.thread_id(), 0, "Thread ID must be captured");
        assert!(!tray_shutdown.load(Ordering::Relaxed));

        // Sleep briefly to allow thread message pump loop to enter GetMessageW
        std::thread::sleep(std::time::Duration::from_millis(50));

        let start = std::time::Instant::now();
        // Signal stop via TrayShutdownHandle
        tray_shutdown.signal();
        let join_res = join_handle.join();
        let elapsed = start.elapsed();

        assert!(join_res.is_ok(), "Tray thread must terminate cleanly");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "GetMessageW must unblock immediately upon TrayShutdownHandle signal; elapsed: {:?}",
            elapsed
        );
        assert!(tray_shutdown.load(Ordering::Relaxed));
    }

    #[test]
    fn test_tray_concurrent_shutdown_signals() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (join_handle, tray_shutdown) = SystemTray::spawn_tray_thread(Arc::clone(&flag), None);

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut workers = Vec::new();
        for _ in 0..8 {
            let shutdown_clone = tray_shutdown.clone();
            workers.push(std::thread::spawn(move || {
                shutdown_clone.signal();
            }));
        }

        for worker in workers {
            assert!(worker.join().is_ok());
        }

        let join_res = join_handle.join();
        assert!(
            join_res.is_ok(),
            "Tray thread must terminate cleanly under concurrent signals"
        );
        assert!(tray_shutdown.load(Ordering::Relaxed));
    }

    #[test]
    fn test_tray_wm_queryendsession_and_wm_endsession_handling() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (join_handle, tray_shutdown) = SystemTray::spawn_tray_thread(
            Arc::clone(&flag),
            Some(r"\\.\pipe\test-endsession".into()),
        );

        std::thread::sleep(std::time::Duration::from_millis(60));

        let hwnd = tray_shutdown.hwnd();
        assert!(!hwnd.0.is_null(), "Tray window HWND must be valid");

        // 1. Dispatch WM_QUERYENDSESSION: Window must return LRESULT(1) confirming safe shutdown permission
        unsafe {
            let res = SendMessageW(hwnd, WM_QUERYENDSESSION, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(1), "WM_QUERYENDSESSION must return LRESULT(1)");
        }

        // 2. Dispatch WM_ENDSESSION with wParam = 1 (session ending)
        let start = std::time::Instant::now();
        unsafe {
            let res = SendMessageW(hwnd, WM_ENDSESSION, WPARAM(1), LPARAM(0));
            assert_eq!(res, LRESULT(0), "WM_ENDSESSION must return LRESULT(0)");
        }

        let join_res = join_handle.join();
        let elapsed = start.elapsed();

        assert!(
            join_res.is_ok(),
            "Tray thread must terminate cleanly on WM_ENDSESSION"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "Tray thread must shutdown within 1.5s (PRD Gate G01 requirement); elapsed: {:?}",
            elapsed
        );
        assert!(
            flag.load(Ordering::SeqCst),
            "Engine shutdown_flag must be set on WM_ENDSESSION"
        );
    }

    #[test]
    fn test_tray_wm_close_signals_engine_shutdown() {
        let _test_lock = TRAY_TEST_MUTEX.lock().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (join_handle, tray_shutdown) = SystemTray::spawn_tray_thread(Arc::clone(&flag), None);

        std::thread::sleep(std::time::Duration::from_millis(60));

        let hwnd = tray_shutdown.hwnd();
        assert!(!hwnd.0.is_null(), "Tray window HWND must be valid");

        // Dispatch WM_CLOSE to tray window
        let start = std::time::Instant::now();
        unsafe {
            let res = SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(0), "WM_CLOSE must return LRESULT(0)");
        }

        let join_res = join_handle.join();
        let elapsed = start.elapsed();

        assert!(
            join_res.is_ok(),
            "Tray thread must terminate cleanly on WM_CLOSE"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "Tray shutdown under WM_CLOSE must complete within 1.5s; elapsed: {:?}",
            elapsed
        );
        assert!(
            flag.load(Ordering::SeqCst),
            "Engine shutdown_flag must be set on WM_CLOSE"
        );
    }
}
