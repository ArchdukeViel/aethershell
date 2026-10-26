use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, LoadIconW, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, TrackPopupMenuEx, IDI_APPLICATION, MF_SEPARATOR,
    MF_STRING, MSG, PM_REMOVE, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

pub const WM_TRAYICON: u32 = WM_APP + 1;

pub const ID_TRAY_OPEN_SETTINGS: usize = 1001;
pub const ID_TRAY_UNLOCK_LAYOUT: usize = 1002;
pub const ID_TRAY_PAUSE_AUDIO: usize = 1003;
pub const ID_TRAY_SAFE_MODE: usize = 1004;
pub const ID_TRAY_DIAGNOSTICS: usize = 1005;
pub const ID_TRAY_EXIT: usize = 1006;
pub const ID_TRAY_RESUME_AUDIO: usize = 1007;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_IS_PAUSED: AtomicBool = AtomicBool::new(false);
static TRAY_PIPE_NAME: RwLock<Option<String>> = RwLock::new(None);

/// System Tray controller for managing Windows shell notification icons.
pub struct SystemTray {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    is_active: bool,
}

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

            Ok(Self {
                hwnd,
                nid,
                is_active,
            })
        }
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
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

    /// Spawns the system tray on a dedicated OS thread with an active Win32 message pump.
    ///
    /// Resolves Finding IVA-P1-001: Ensures Win32 messages are pumped and context menu actions are dispatched.
    pub fn spawn_tray_thread(
        shutdown_flag: Arc<AtomicBool>,
        pipe_name: Option<String>,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        if let Some(pipe) = pipe_name {
            if let Ok(mut lock) = TRAY_PIPE_NAME.write() {
                *lock = Some(pipe);
            }
        }

        let tray_shutdown = Arc::new(AtomicBool::new(false));
        let tray_shutdown_clone = Arc::clone(&tray_shutdown);

        let handle = std::thread::spawn(move || {
            let tray = match SystemTray::new() {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Could not create system tray: {}", e);
                    return;
                }
            };

            tracing::info!(
                "System tray thread active with message pump (HWND: {:?})",
                tray.hwnd().0
            );

            unsafe {
                let mut msg = MSG::default();
                while !shutdown_flag.load(Ordering::Relaxed)
                    && !tray_shutdown_clone.load(Ordering::Relaxed)
                {
                    let has_msg = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE);
                    if has_msg.as_bool() {
                        if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                            break;
                        }
                        DispatchMessageW(&msg);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }

                    if Self::is_exit_requested() {
                        shutdown_flag.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }

            drop(tray);
            tracing::info!("System tray thread cleanly terminated.");
        });

        (handle, tray_shutdown)
    }
}

impl Drop for SystemTray {
    fn drop(&mut self) {
        unsafe {
            if self.is_active {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            }
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

fn dispatch_tray_ipc(command: &'static str, payload: serde_json::Value) {
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
        WM_CLOSE | WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
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

    #[test]
    fn test_tray_initialization_non_panicking() {
        let tray = SystemTray::new();
        if let Ok(t) = tray {
            println!("Tray icon initialized: {}", t.is_active());
        }
    }

    #[test]
    fn test_tray_thread_lifecycle_iva_p1_001() {
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
        // Test audio pause/resume toggle state
        SystemTray::set_audio_paused_state(false);
        assert!(!SystemTray::is_audio_paused());

        SystemTray::set_audio_paused_state(true);
        assert!(SystemTray::is_audio_paused());

        // Test export diagnostics offline fallback execution
        handle_export_diagnostics();
    }
}
