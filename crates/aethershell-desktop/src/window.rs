use crate::accessibility::{
    create_root_provider, DesktopAccessibilityState, DesktopWidgetFragmentProvider,
    EVENT_OBJECT_FOCUS, OBJID_CLIENT,
};
use crate::host::{DesktopHost, DesktopHostError};
use crate::interaction::{DismissalReason, InteractiveRegion};
use aethershell_win::{enumerate_monitors, DesktopHierarchyProbe};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Accessibility::{
    IRawElementProviderSimple, NotifyWinEvent, UIA_AutomationFocusChangedEventId,
    UIA_Invoke_InvokedEventId, UiaRaiseAutomationEvent, UiaReturnRawElementProvider,
    UiaRootObjectId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow, DispatchMessageW,
    GetWindowLongPtrW, IsWindow, LoadCursorW, PeekMessageW, RegisterClassExW,
    RegisterShellHookWindow, RegisterWindowMessageW, SetCursor, SetWindowLongPtrW, SetWindowPos,
    TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, GWL_EXSTYLE,
    HWND_BOTTOM, HWND_NOTOPMOST, IDC_ARROW, IDC_HAND, MSG, PM_REMOVE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE,
    WM_ACTIVATE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENDSESSION, WM_ERASEBKGND, WM_GETOBJECT,
    WM_HOTKEY, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
    WM_NCDESTROY, WM_PAINT, WM_QUERYENDSESSION, WM_QUIT, WNDCLASSEXW, WS_POPUP, WS_VISIBLE,
};

/// Hotkey ID for toggling Temporary Interactive Mode (PRD Section 5.3 & Q31:A).
pub const HOTKEY_ID_INTERACTIVE_MODE: i32 = 0xAE01;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
const DESKTOP_HOST_CLASS_NAME: windows::core::PCWSTR = w!("AetherShellDesktopHostWindow");

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

/// Registers or retrieves the dynamic `SHELLHOOK` message broadcast on non-invasive shell events (Gate G06).
pub fn shellhook_msg() -> u32 {
    static CACHED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = CACHED.load(Ordering::Relaxed);
    if id != 0 {
        return id;
    }
    unsafe {
        let msg = RegisterWindowMessageW(w!("SHELLHOOK"));
        CACHED.store(msg, Ordering::Relaxed);
        msg
    }
}

// Shell hook event codes (Win32 HSHELL_*)
pub const HSHELL_WINDOWCREATED: usize = 1;
pub const HSHELL_WINDOWDESTROYED: usize = 2;
pub const HSHELL_ACTIVATESHELLWINDOW: usize = 3;
pub const HSHELL_WINDOWACTIVATED: usize = 4;
pub const HSHELL_GETMINRECT: usize = 5;
pub const HSHELL_REDRAW: usize = 6;
pub const HSHELL_TASKMAN: usize = 7;
pub const HSHELL_LANGUAGE: usize = 8;
pub const HSHELL_SYSMENU: usize = 9;
pub const HSHELL_ENDTASK: usize = 10;
pub const HSHELL_ACCESSIBILITYSTATE: usize = 11;
pub const HSHELL_APPCOMMAND: usize = 12;
pub const HSHELL_WINDOWREPLACED: usize = 13;
pub const HSHELL_WINDOWREPLACING: usize = 14;
pub const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
pub const HSHELL_FLASH: usize = 0x8006;

fn register_desktop_host_class() -> Result<(), DesktopHostError> {
    if CLASS_REGISTERED.load(Ordering::SeqCst) {
        return Ok(());
    }

    unsafe {
        let hinstance = GetModuleHandleW(None).map_err(|e| {
            DesktopHostError::WindowCreationFailed(format!("GetModuleHandleW: {}", e))
        })?;

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(desktop_host_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: DESKTOP_HOST_CLASS_NAME,
            ..Default::default()
        };

        let atom = RegisterClassExW(&wnd_class);
        if atom == 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(1410) {
                // 1410 = ERROR_CLASS_ALREADY_EXISTS
                return Err(DesktopHostError::WindowCreationFailed(format!(
                    "RegisterClassExW failed: {}",
                    err
                )));
            }
        }

        CLASS_REGISTERED.store(true, Ordering::SeqCst);
        Ok(())
    }
}

pub type DismissCallback = Arc<dyn Fn(DismissalReason) + Send + Sync>;
pub type PaintCallback = Arc<dyn Fn(HWND) + Send + Sync>;
pub type ExplorerRestartCallback = Arc<dyn Fn() + Send + Sync>;
pub type ActionCallback = Arc<dyn Fn(&InteractiveRegion, f32, f32) + Send + Sync>;
pub type HotkeyCallback = Arc<dyn Fn(bool) + Send + Sync>;
pub type DpiChangedCallback = Arc<dyn Fn(HWND, u32, &RECT) + Send + Sync>;
pub type DisplayChangedCallback = Arc<dyn Fn() + Send + Sync>;
pub type ShellEventCallback = Arc<dyn Fn(usize, isize) + Send + Sync>;
pub type SessionEndCallback = Arc<dyn Fn() + Send + Sync>;

/// Returns a verified safe Z-order target HWND.
/// If `target` is Some and is still a valid OS window, returns it;
/// otherwise safely falls back to `HWND_BOTTOM`.
pub fn safe_stacking_target(target: Option<HWND>) -> HWND {
    match target {
        Some(h) if !h.0.is_null() && unsafe { IsWindow(h).as_bool() } => h,
        _ => HWND_BOTTOM,
    }
}

/// Internal state stored on the Win32 HWND via GWLP_USERDATA.
pub struct DesktopWindowState {
    pub is_interactive: Arc<AtomicBool>,
    pub shelldll_defview: Option<HWND>,
    pub worker_w: Option<HWND>,
    pub on_dismiss: std::sync::Mutex<Option<DismissCallback>>,
    pub on_paint: std::sync::Mutex<Option<PaintCallback>>,
    pub on_explorer_restart: std::sync::Mutex<Option<ExplorerRestartCallback>>,
    pub on_action: Arc<std::sync::Mutex<Option<ActionCallback>>>,
    pub on_hotkey: std::sync::Mutex<Option<HotkeyCallback>>,
    pub on_dpi_changed: std::sync::Mutex<Option<DpiChangedCallback>>,
    pub on_display_change: std::sync::Mutex<Option<DisplayChangedCallback>>,
    pub on_shell_event: std::sync::Mutex<Option<ShellEventCallback>>,
    pub on_session_end: std::sync::Mutex<Option<SessionEndCallback>>,
    pub interactive_regions: Arc<std::sync::Mutex<Vec<InteractiveRegion>>>,
    pub focused_region_index: Arc<std::sync::Mutex<Option<usize>>>,
    pub current_dpi: Arc<AtomicU32>,
    pub current_bounds: Arc<std::sync::Mutex<[i32; 4]>>,
    pub explorer_restart_count: Arc<AtomicUsize>,
    pub win_d_restack_count: Arc<AtomicUsize>,
    pub last_restart_tick: Arc<AtomicU64>,
    pub last_shell_tick: Arc<AtomicU64>,
    pub last_activity_tick: Arc<AtomicU64>,
    pub is_hotkey_registered: Arc<AtomicBool>,
}

impl DesktopWindowState {
    /// Generates thread-safe accessibility state for Windows UI Automation providers (Gate G14).
    pub fn accessibility_state(&self, hwnd: HWND) -> DesktopAccessibilityState {
        DesktopAccessibilityState::new(
            hwnd,
            Arc::clone(&self.is_interactive),
            Arc::clone(&self.interactive_regions),
            Arc::clone(&self.focused_region_index),
            Arc::clone(&self.current_bounds),
            Arc::clone(&self.on_action),
            Arc::clone(&self.last_activity_tick),
        )
    }
}

/// Represents a real Win32 desktop overlay window for a specific monitor.
pub struct DesktopHostWindow {
    pub hwnd: HWND,
    pub monitor_id: String,
    pub bounds: [i32; 4], // [x, y, width, height]
    pub dpi: u32,
    pub is_interactive: Arc<AtomicBool>,
    pub shelldll_defview_hwnd: Option<HWND>,
    pub worker_w_hwnd: Option<HWND>,
    pub explorer_restart_count: Arc<AtomicUsize>,
    pub win_d_restack_count: Arc<AtomicUsize>,
    state_ptr: *mut DesktopWindowState,
}

unsafe impl Send for DesktopHostWindow {}
unsafe impl Sync for DesktopHostWindow {}

impl DesktopHostWindow {
    /// Creates a new desktop overlay window embedded behind `SHELLDLL_DefView`.
    pub fn create(
        monitor_id: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        shelldll_defview: Option<HWND>,
        worker_w: Option<HWND>,
    ) -> Result<Self, DesktopHostError> {
        register_desktop_host_class()?;

        let ex_styles = WINDOW_EX_STYLE(DesktopHost::surface_extended_styles(false));
        let parent = worker_w.unwrap_or_default();
        let is_interactive = Arc::new(AtomicBool::new(false));
        let explorer_restart_count = Arc::new(AtomicUsize::new(0));
        let win_d_restack_count = Arc::new(AtomicUsize::new(0));
        let last_restart_tick = Arc::new(AtomicU64::new(0));
        let last_shell_tick = Arc::new(AtomicU64::new(0));
        let last_activity_tick = Arc::new(AtomicU64::new(0));
        let is_hotkey_registered = Arc::new(AtomicBool::new(false));

        let initial_dpi = 96u32;
        let current_dpi = Arc::new(AtomicU32::new(initial_dpi));
        let current_bounds = Arc::new(std::sync::Mutex::new([x, y, width, height]));
        let on_action = Arc::new(std::sync::Mutex::new(None));
        let interactive_regions = Arc::new(std::sync::Mutex::new(Vec::new()));
        let focused_region_index = Arc::new(std::sync::Mutex::new(None));

        let state = Box::new(DesktopWindowState {
            is_interactive: Arc::clone(&is_interactive),
            shelldll_defview,
            worker_w,
            on_dismiss: std::sync::Mutex::new(None),
            on_paint: std::sync::Mutex::new(None),
            on_explorer_restart: std::sync::Mutex::new(None),
            on_action,
            on_hotkey: std::sync::Mutex::new(None),
            on_dpi_changed: std::sync::Mutex::new(None),
            on_display_change: std::sync::Mutex::new(None),
            on_shell_event: std::sync::Mutex::new(None),
            on_session_end: std::sync::Mutex::new(None),
            interactive_regions,
            focused_region_index,
            current_dpi: Arc::clone(&current_dpi),
            current_bounds,
            explorer_restart_count: Arc::clone(&explorer_restart_count),
            win_d_restack_count: Arc::clone(&win_d_restack_count),
            last_restart_tick,
            last_shell_tick,
            last_activity_tick,
            is_hotkey_registered,
        });
        let state_ptr = Box::into_raw(state);

        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| {
                DesktopHostError::WindowCreationFailed(format!("GetModuleHandleW: {}", e))
            })?;

            let hwnd = CreateWindowExW(
                ex_styles,
                DESKTOP_HOST_CLASS_NAME,
                w!("AetherShell Desktop Overlay"),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                width,
                height,
                parent,
                None,
                hinstance,
                Some(state_ptr as *const c_void),
            )
            .map_err(|e| {
                DesktopHostError::WindowCreationFailed(format!("CreateWindowExW failed: {}", e))
            })?;

            if hwnd.0.is_null() {
                drop(Box::from_raw(state_ptr));
                return Err(DesktopHostError::WindowCreationFailed(
                    "CreateWindowExW returned NULL HWND".into(),
                ));
            }

            // Ensure GWLP_USERDATA is set in case WM_NCCREATE didn't run synchronously
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            // Query actual initial window DPI
            let real_dpi = aethershell_win::get_window_dpi(hwnd);
            (*state_ptr).current_dpi.store(real_dpi, Ordering::Relaxed);

            // Register session notifications (WM_WTSSESSION_CHANGE)
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

            // Register as shell hook window to receive non-invasive shell events (Win+D, window activation)
            let _ = RegisterShellHookWindow(hwnd);

            // Stacking order: Place behind SHELLDLL_DefView (or HWND_BOTTOM)
            let insert_after = safe_stacking_target(shelldll_defview);
            let _ = SetWindowPos(
                hwnd,
                insert_after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );

            Ok(Self {
                hwnd,
                monitor_id: monitor_id.to_string(),
                bounds: [x, y, width, height],
                dpi: real_dpi,
                is_interactive,
                shelldll_defview_hwnd: shelldll_defview,
                worker_w_hwnd: worker_w,
                explorer_restart_count,
                win_d_restack_count,
                state_ptr,
            })
        }
    }

    #[doc(hidden)]
    pub fn create_dummy_for_test(monitor_id: &str, bounds: [i32; 4]) -> Self {
        Self {
            hwnd: HWND(std::ptr::null_mut()),
            monitor_id: monitor_id.to_string(),
            bounds,
            dpi: 96,
            is_interactive: Arc::new(AtomicBool::new(false)),
            shelldll_defview_hwnd: None,
            worker_w_hwnd: None,
            explorer_restart_count: Arc::new(AtomicUsize::new(0)),
            win_d_restack_count: Arc::new(AtomicUsize::new(0)),
            state_ptr: std::ptr::null_mut(),
        }
    }

    /// Sets an auto-dismissal callback invoked when user presses Escape, focus is lost, or session is locked.
    pub fn set_on_dismiss<F>(&mut self, cb: F)
    where
        F: Fn(DismissalReason) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_dismiss.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets a paint callback invoked on WM_PAINT messages.
    pub fn set_on_paint<F>(&mut self, cb: F)
    where
        F: Fn(HWND) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_paint.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets an action callback invoked when user clicks an interactive region in interactive mode.
    pub fn set_on_action<F>(&mut self, cb: F)
    where
        F: Fn(&InteractiveRegion, f32, f32) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_action.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets a hotkey toggle callback invoked when the global hotkey triggers mode change.
    pub fn set_on_hotkey<F>(&mut self, cb: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_hotkey.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets a DPI change callback invoked when PerMonitorV2 DPI changes (`WM_DPICHANGED`).
    pub fn set_on_dpi_changed<F>(&mut self, cb: F)
    where
        F: Fn(HWND, u32, &RECT) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_dpi_changed.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets a display change callback invoked when monitor resolution or topology changes (`WM_DISPLAYCHANGE`).
    pub fn set_on_display_change<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_display_change.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Sets a session end callback invoked when WM_ENDSESSION is received.
    pub fn set_on_session_end<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_session_end.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }

    /// Returns the currently keyboard-focused interactive region index (Gate G14).
    pub fn focused_region_index(&self) -> Option<usize> {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(guard) = (*self.state_ptr).focused_region_index.lock() {
                    return *guard;
                }
            }
        }
        None
    }

    /// Sets or clears the currently keyboard-focused interactive region index (Gate G14).
    pub fn set_focused_region_index(&self, index: Option<usize>) {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).focused_region_index.lock() {
                    *guard = index;
                }
            }
        }
    }

    /// Returns the current DPI of this window.
    pub fn dpi(&self) -> u32 {
        if !self.state_ptr.is_null() {
            unsafe {
                return (*self.state_ptr).current_dpi.load(Ordering::Relaxed);
            }
        }
        self.dpi
    }

    /// Returns the DPI scale factor (1.0 = 96 DPI / 100%, 1.25 = 120 DPI / 125%, 1.5 = 144 DPI / 150%, etc.).
    pub fn scale_factor(&self) -> f32 {
        aethershell_win::dpi_to_scale_factor(self.dpi())
    }

    /// Returns the current bounds of this window [x, y, width, height].
    pub fn bounds(&self) -> [i32; 4] {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(guard) = (*self.state_ptr).current_bounds.lock() {
                    return *guard;
                }
            }
        }
        self.bounds
    }

    /// Sets the list of active interactive regions on this window surface.
    pub fn set_interactive_regions(&mut self, regions: Vec<InteractiveRegion>) {
        if !self.state_ptr.is_null() {
            unsafe {
                let regions_len = regions.len();
                if let Ok(mut guard) = (*self.state_ptr).interactive_regions.lock() {
                    *guard = regions;
                }
                if let Ok(mut focus_guard) = (*self.state_ptr).focused_region_index.lock() {
                    if let Some(idx) = *focus_guard {
                        if idx >= regions_len {
                            *focus_guard = None;
                        }
                    }
                }
            }
        }
    }

    /// Retrieves a clone of the current interactive regions.
    pub fn get_interactive_regions(&self) -> Vec<InteractiveRegion> {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(guard) = (*self.state_ptr).interactive_regions.lock() {
                    return guard.clone();
                }
            }
        }
        Vec::new()
    }

    /// Registers a system-wide global hotkey on this desktop host window (PRD Section 5.3 & Q31:A).
    pub fn register_global_hotkey(
        &mut self,
        modifiers: HOT_KEY_MODIFIERS,
        vk: u32,
    ) -> Result<(), DesktopHostError> {
        if self.hwnd.0.is_null() {
            return Err(DesktopHostError::WindowCreationFailed(
                "Cannot register hotkey on NULL HWND".into(),
            ));
        }
        unsafe {
            RegisterHotKey(
                self.hwnd,
                HOTKEY_ID_INTERACTIVE_MODE,
                modifiers | MOD_NOREPEAT,
                vk,
            )
            .map_err(|e| {
                DesktopHostError::WindowCreationFailed(format!("RegisterHotKey failed: {}", e))
            })?;
            if !self.state_ptr.is_null() {
                (*self.state_ptr)
                    .is_hotkey_registered
                    .store(true, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    /// Unregisters the system-wide global hotkey from this window.
    pub fn unregister_global_hotkey(&mut self) -> Result<(), DesktopHostError> {
        if self.hwnd.0.is_null() {
            return Ok(());
        }
        unsafe {
            let res = UnregisterHotKey(self.hwnd, HOTKEY_ID_INTERACTIVE_MODE);
            if !self.state_ptr.is_null() {
                (*self.state_ptr)
                    .is_hotkey_registered
                    .store(false, Ordering::SeqCst);
            }
            res.map_err(|e| {
                DesktopHostError::WindowCreationFailed(format!("UnregisterHotKey failed: {}", e))
            })
        }
    }

    /// Checks if the global hotkey is currently registered on this window.
    pub fn is_hotkey_registered(&self) -> bool {
        if !self.state_ptr.is_null() {
            unsafe {
                return (*self.state_ptr)
                    .is_hotkey_registered
                    .load(Ordering::SeqCst);
            }
        }
        false
    }

    /// Returns whether the window is currently in interactive mode.
    pub fn is_interactive(&self) -> bool {
        self.is_interactive.load(Ordering::SeqCst)
    }

    /// Sets interactive mode, switching extended window styles and Z-order.
    ///
    /// Per PRD Section 5.3 & Q31:A:
    /// In interactive mode: stays beneath normal application windows (`HWND_NOTOPMOST`).
    /// In passive mode: stays behind desktop icons (`HWND_BOTTOM` / `shelldll_defview`).
    pub fn set_interactive(&mut self, interactive: bool) {
        if self.is_interactive.load(Ordering::SeqCst) == interactive {
            return;
        }
        self.is_interactive.store(interactive, Ordering::SeqCst);

        let ex_styles = DesktopHost::surface_extended_styles(interactive);

        unsafe {
            let _ = SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex_styles as isize);

            let (z_target, flags) = if interactive {
                (
                    HWND_NOTOPMOST,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                )
            } else {
                let behind = safe_stacking_target(self.shelldll_defview_hwnd);
                (
                    behind,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                )
            };

            let _ = SetWindowPos(self.hwnd, z_target, 0, 0, 0, 0, flags);
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Handles display resolution or monitor layout change (`WM_DISPLAYCHANGE`).
    pub fn handle_display_change(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.bounds = [x, y, width, height];
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).current_bounds.lock() {
                    *guard = [x, y, width, height];
                }
            }
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND::default(),
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Handles per-monitor DPI change (`WM_DPICHANGED`).
    pub fn handle_dpi_change(&mut self, new_dpi: u32, suggested_rect: Option<RECT>) {
        self.dpi = new_dpi;
        if !self.state_ptr.is_null() {
            unsafe {
                (*self.state_ptr)
                    .current_dpi
                    .store(new_dpi, Ordering::Relaxed);
            }
        }
        if let Some(rect) = suggested_rect {
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            self.bounds = [rect.left, rect.top, w, h];
            if !self.state_ptr.is_null() {
                unsafe {
                    if let Ok(mut guard) = (*self.state_ptr).current_bounds.lock() {
                        *guard = [rect.left, rect.top, w, h];
                    }
                }
            }
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND::default(),
                    rect.left,
                    rect.top,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Re-probes the desktop shell hierarchy, invalidates stale shell handles,
    /// and restores correct passive stacking order.
    pub fn handle_explorer_restart(&mut self) -> DesktopHierarchyProbe {
        let probe = DesktopHierarchyProbe::probe();
        self.handle_explorer_restart_with_probe(&probe);
        probe
    }

    /// Re-applies the probed desktop hierarchy, updates shell handles,
    /// and restores correct passive stacking order using the provided probe.
    pub fn handle_explorer_restart_with_probe(&mut self, probe: &DesktopHierarchyProbe) {
        let fresh_defview = probe.shelldll_defview_hwnd.map(|h| HWND(h as *mut _));
        let fresh_worker_w = probe.worker_w_hwnd.map(|h| HWND(h as *mut _));

        self.shelldll_defview_hwnd = fresh_defview;
        self.worker_w_hwnd = fresh_worker_w;

        if !self.state_ptr.is_null() {
            unsafe {
                (*self.state_ptr).shelldll_defview = fresh_defview;
                (*self.state_ptr).worker_w = fresh_worker_w;
                let now_tick = GetTickCount64();
                let prev_tick = (*self.state_ptr).last_restart_tick.load(Ordering::Relaxed);
                if now_tick == 0 || now_tick < prev_tick || now_tick - prev_tick >= 300 {
                    (*self.state_ptr)
                        .explorer_restart_count
                        .fetch_add(1, Ordering::SeqCst);
                    (*self.state_ptr)
                        .last_restart_tick
                        .store(now_tick, Ordering::Relaxed);
                }
            }
        }

        // Restore passive stacking order if not in interactive mode
        if !self.is_interactive() {
            let behind = safe_stacking_target(fresh_defview);
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    behind,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                );
                let _ = InvalidateRect(self.hwnd, None, false);
            }
        }
    }

    /// Sets an auto-dismissal / recovery callback invoked when Explorer restarts.
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

    /// Returns the number of times this window processed an Explorer restart recovery.
    pub fn explorer_restart_count(&self) -> usize {
        self.explorer_restart_count.load(Ordering::SeqCst)
    }

    /// Checks if the underlying Win32 HWND is still a valid OS window.
    pub fn is_window_valid(&self) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        unsafe { IsWindow(self.hwnd).as_bool() }
    }

    /// Returns the number of times desktop stacking was re-asserted (e.g. after Win+D / shell events).
    pub fn win_d_restack_count(&self) -> usize {
        self.win_d_restack_count.load(Ordering::SeqCst)
    }

    /// Explicitly re-asserts desktop overlay stacking behind desktop icons if in passive mode (Gate G06).
    pub fn reassert_desktop_stacking(&self) {
        if !self.is_interactive() {
            let behind = safe_stacking_target(self.shelldll_defview_hwnd);
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    behind,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.win_d_restack_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Sets a callback invoked when a non-invasive shell hook event is received (Gate G06).
    pub fn set_on_shell_event<F>(&mut self, cb: F)
    where
        F: Fn(usize, isize) + Send + Sync + 'static,
    {
        if !self.state_ptr.is_null() {
            unsafe {
                if let Ok(mut guard) = (*self.state_ptr).on_shell_event.lock() {
                    *guard = Some(Arc::new(cb));
                }
            }
        }
    }
}

impl Drop for DesktopHostWindow {
    fn drop(&mut self) {
        if !self.state_ptr.is_null() && !self.hwnd.0.is_null() {
            unsafe {
                if (*self.state_ptr)
                    .is_hotkey_registered
                    .load(Ordering::SeqCst)
                {
                    let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_INTERACTIVE_MODE);
                    (*self.state_ptr)
                        .is_hotkey_registered
                        .store(false, Ordering::SeqCst);
                }
            }
        }
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DeregisterShellHookWindow(self.hwnd);
                let _ = WTSUnRegisterSessionNotification(self.hwnd);
                let _ = DestroyWindow(self.hwnd);
            }
        }
        if !self.state_ptr.is_null() {
            unsafe {
                drop(Box::from_raw(self.state_ptr));
                self.state_ptr = std::ptr::null_mut();
            }
        }
    }
}

/// Window procedure for desktop host overlay windows.
unsafe extern "system" fn desktop_host_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = taskbar_created_msg();
    if taskbar_created != 0 && msg == taskbar_created {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
        if !ptr.is_null() {
            let state = &mut *ptr;
            let now_tick = GetTickCount64();
            let prev_tick = state.last_restart_tick.load(Ordering::Relaxed);
            // Debounce: prevent duplicate rapid handling / restart loops within 300ms
            if now_tick >= prev_tick && now_tick - prev_tick < 300 {
                return LRESULT(0);
            }
            state.last_restart_tick.store(now_tick, Ordering::Relaxed);

            tracing::info!(
                "DesktopHostWindow (HWND {:?}): Processing TaskbarCreated (Explorer restart)",
                hwnd.0
            );

            // Re-probe desktop hierarchy non-invasively
            let fresh_probe = DesktopHierarchyProbe::probe();
            let fresh_defview = fresh_probe.shelldll_defview_hwnd.map(|h| HWND(h as *mut _));
            let fresh_worker_w = fresh_probe.worker_w_hwnd.map(|h| HWND(h as *mut _));

            // Invalidate stale handles safely
            state.shelldll_defview = fresh_defview;
            state.worker_w = fresh_worker_w;
            state.explorer_restart_count.fetch_add(1, Ordering::SeqCst);

            // Restore correct passive desktop stacking order if not in interactive mode
            if !state.is_interactive.load(Ordering::SeqCst) {
                let behind = safe_stacking_target(fresh_defview);
                let _ = SetWindowPos(
                    hwnd,
                    behind,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                );
                let _ = InvalidateRect(hwnd, None, false);
            }

            if let Ok(guard) = state.on_explorer_restart.lock() {
                if let Some(ref cb) = *guard {
                    cb();
                }
            }
        }
        return LRESULT(0);
    }

    let shell_hook = shellhook_msg();
    if shell_hook != 0 && msg == shell_hook {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
        if !ptr.is_null() {
            let state = &mut *ptr;
            let event_code = wparam.0;
            let target_hwnd = lparam.0;

            if let Ok(guard) = state.on_shell_event.lock() {
                if let Some(ref cb) = *guard {
                    cb(event_code, target_hwnd);
                }
            }

            match event_code {
                HSHELL_WINDOWACTIVATED
                | HSHELL_RUDEAPPACTIVATED
                | HSHELL_ACTIVATESHELLWINDOW
                | HSHELL_REDRAW => {
                    let now_tick = GetTickCount64();
                    let prev_tick = state.last_shell_tick.load(Ordering::Relaxed);
                    // Debounce rapid shell hook bursts within 50ms
                    if now_tick >= prev_tick && now_tick - prev_tick < 50 {
                        return LRESULT(0);
                    }
                    state.last_shell_tick.store(now_tick, Ordering::Relaxed);

                    // Re-assert passive desktop stacking behind icons on Win+D / shell activation
                    if !state.is_interactive.load(Ordering::SeqCst) {
                        let behind = safe_stacking_target(state.shelldll_defview);
                        let _ = SetWindowPos(
                            hwnd,
                            behind,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                        state.win_d_restack_count.fetch_add(1, Ordering::SeqCst);
                    }
                }
                _ => {}
            }
        }
        return LRESULT(0);
    }

    match msg {
        WM_NCCREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            if !cs.is_null() {
                let ptr = (*cs).lpCreateParams as isize;
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => LRESULT(1), // Suppress default background erase (flicker prevention)
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _hdc = BeginPaint(hwnd, &mut ps);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if let Ok(guard) = state.on_paint.lock() {
                    if let Some(ref cb) = *guard {
                        cb(hwnd);
                    }
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if let Ok(guard) = state.on_display_change.lock() {
                    if let Some(ref cb) = *guard {
                        cb();
                    }
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if lparam.0 != 0 {
                let rect = *(lparam.0 as *const RECT);
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;
                if !ptr.is_null() {
                    let state = &*ptr;
                    state.current_dpi.store(new_dpi, Ordering::Relaxed);
                    if let Ok(mut bounds) = state.current_bounds.lock() {
                        *bounds = [rect.left, rect.top, w, h];
                    }
                    if let Ok(guard) = state.on_dpi_changed.lock() {
                        if let Some(ref cb) = *guard {
                            cb(hwnd, new_dpi, &rect);
                        }
                    }
                }
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    rect.left,
                    rect.top,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            } else if !ptr.is_null() {
                let state = &*ptr;
                state.current_dpi.store(new_dpi, Ordering::Relaxed);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID_INTERACTIVE_MODE {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    let current = state.is_interactive.load(Ordering::SeqCst);
                    let next = !current;

                    if next {
                        state.is_interactive.store(true, Ordering::SeqCst);
                        let ex_styles = DesktopHost::surface_extended_styles(true);
                        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_styles as isize);
                        state.last_activity_tick.store(GetTickCount64(), Ordering::Relaxed);
                        let _ = SetWindowPos(
                            hwnd,
                            HWND_NOTOPMOST,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                        );
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        dismiss_interactive_mode(hwnd, state, DismissalReason::ExplicitExit);
                    }

                    if let Ok(guard) = state.on_hotkey.lock() {
                        if let Some(ref cb) = *guard {
                            cb(next);
                        }
                    }
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if state.is_interactive.load(Ordering::SeqCst) {
                    let px = (lparam.0 & 0xFFFF) as i16 as f32;
                    let py = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

                    let regions = if let Ok(guard) = state.interactive_regions.lock() {
                        guard.clone()
                    } else {
                        Vec::new()
                    };

                    let hit = regions.iter().rev().find(|r| r.contains_point(px, py)).cloned();

                    if let Some(region) = hit {
                        tracing::info!(
                            "DesktopHostWindow (HWND {:?}): Click on interactive region '{}' at ({}, {})",
                            hwnd.0,
                            region.id,
                            px,
                            py
                        );
                        state.last_activity_tick.store(GetTickCount64(), Ordering::Relaxed);
                        if let Ok(guard) = state.on_action.lock() {
                            if let Some(ref cb) = *guard {
                                cb(&region, px, py);
                            }
                        }
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        tracing::info!(
                            "DesktopHostWindow (HWND {:?}): Click outside interactive regions at ({}, {}). Restoring passive mode.",
                            hwnd.0,
                            px,
                            py
                        );
                        dismiss_interactive_mode(hwnd, state, DismissalReason::ClickOutside);
                    }
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if state.is_interactive.load(Ordering::SeqCst) {
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if state.is_interactive.load(Ordering::SeqCst) {
                    let px = (lparam.0 & 0xFFFF) as i16 as f32;
                    let py = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let regions = if let Ok(guard) = state.interactive_regions.lock() {
                        guard.clone()
                    } else {
                        Vec::new()
                    };
                    let is_hovering = regions.iter().any(|r| r.contains_point(px, py));
                    if is_hovering {
                        if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                            let _ = SetCursor(cursor);
                        }
                    } else {
                        if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
                            let _ = SetCursor(cursor);
                        }
                    }
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                if state.is_interactive.load(Ordering::Relaxed) {
                    match vk {
                        0x1B => {
                            // VK_ESCAPE: dismiss temporary interactive mode
                            dismiss_interactive_mode(hwnd, state, DismissalReason::EscapeKey);
                        }
                        0x09 => {
                            // VK_TAB: cycle focus forward (or backward if Shift is held)
                            let shift = (GetKeyState(0x10 /* VK_SHIFT */) as u16 & 0x8000) != 0;
                            let new_focus_idx = {
                                let regions_guard = state.interactive_regions.lock();
                                let focus_guard = state.focused_region_index.lock();
                                if let (Ok(regions), Ok(mut focus)) = (regions_guard, focus_guard) {
                                    if !regions.is_empty() {
                                        let next = match *focus {
                                            Some(curr) => {
                                                if shift {
                                                    if curr == 0 {
                                                        regions.len() - 1
                                                    } else {
                                                        curr - 1
                                                    }
                                                } else {
                                                    (curr + 1) % regions.len()
                                                }
                                            }
                                            None => {
                                                if shift {
                                                    regions.len() - 1
                                                } else {
                                                    0
                                                }
                                            }
                                        };
                                        *focus = Some(next);
                                        Some(next)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            };

                            if let Some(next_idx) = new_focus_idx {
                                state
                                    .last_activity_tick
                                    .store(GetTickCount64(), Ordering::Relaxed);
                                let _ = InvalidateRect(hwnd, None, false);
                                NotifyWinEvent(
                                    EVENT_OBJECT_FOCUS,
                                    hwnd,
                                    OBJID_CLIENT,
                                    (next_idx + 1) as i32,
                                );
                                let acc_state = state.accessibility_state(hwnd);
                                let child = DesktopWidgetFragmentProvider::new(
                                    acc_state,
                                    next_idx,
                                );
                                let simple: IRawElementProviderSimple = child.into();
                                let _ = UiaRaiseAutomationEvent(
                                    &simple,
                                    UIA_AutomationFocusChangedEventId,
                                );
                            }
                        }
                        0x27 | 0x28 => {
                            // VK_RIGHT or VK_DOWN: move focus forward
                            let new_focus_idx = {
                                let regions_guard = state.interactive_regions.lock();
                                let focus_guard = state.focused_region_index.lock();
                                if let (Ok(regions), Ok(mut focus)) = (regions_guard, focus_guard) {
                                    if !regions.is_empty() {
                                        let next = match *focus {
                                            Some(curr) => (curr + 1) % regions.len(),
                                            None => 0,
                                        };
                                        *focus = Some(next);
                                        Some(next)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            };

                            if let Some(next_idx) = new_focus_idx {
                                state
                                    .last_activity_tick
                                    .store(GetTickCount64(), Ordering::Relaxed);
                                let _ = InvalidateRect(hwnd, None, false);
                                NotifyWinEvent(
                                    EVENT_OBJECT_FOCUS,
                                    hwnd,
                                    OBJID_CLIENT,
                                    (next_idx + 1) as i32,
                                );
                                let acc_state = state.accessibility_state(hwnd);
                                let child = DesktopWidgetFragmentProvider::new(
                                    acc_state,
                                    next_idx,
                                );
                                let simple: IRawElementProviderSimple = child.into();
                                let _ = UiaRaiseAutomationEvent(
                                    &simple,
                                    UIA_AutomationFocusChangedEventId,
                                );
                            }
                        }
                        0x25 | 0x26 => {
                            // VK_LEFT or VK_UP: move focus backward
                            let new_focus_idx = {
                                let regions_guard = state.interactive_regions.lock();
                                let focus_guard = state.focused_region_index.lock();
                                if let (Ok(regions), Ok(mut focus)) = (regions_guard, focus_guard) {
                                    if !regions.is_empty() {
                                        let prev = match *focus {
                                            Some(curr) => {
                                                if curr == 0 {
                                                    regions.len() - 1
                                                } else {
                                                    curr - 1
                                                }
                                            }
                                            None => regions.len() - 1,
                                        };
                                        *focus = Some(prev);
                                        Some(prev)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            };

                            if let Some(prev_idx) = new_focus_idx {
                                state
                                    .last_activity_tick
                                    .store(GetTickCount64(), Ordering::Relaxed);
                                let _ = InvalidateRect(hwnd, None, false);
                                NotifyWinEvent(
                                    EVENT_OBJECT_FOCUS,
                                    hwnd,
                                    OBJID_CLIENT,
                                    (prev_idx + 1) as i32,
                                );
                                let acc_state = state.accessibility_state(hwnd);
                                let child = DesktopWidgetFragmentProvider::new(
                                    acc_state,
                                    prev_idx,
                                );
                                let simple: IRawElementProviderSimple = child.into();
                                let _ = UiaRaiseAutomationEvent(
                                    &simple,
                                    UIA_AutomationFocusChangedEventId,
                                );
                            }
                        }
                        0x0D | 0x20 => {
                            // VK_RETURN or VK_SPACE: activate focused widget
                            let target_region_and_idx = {
                                let focus_guard =
                                    state.focused_region_index.lock().ok().and_then(|g| *g);
                                let regions_guard = state.interactive_regions.lock();
                                if let (Some(idx), Ok(regions)) = (focus_guard, regions_guard) {
                                    regions.get(idx).cloned().map(|reg| (reg, idx))
                                } else {
                                    None
                                }
                            };

                            if let Some((region, idx)) = target_region_and_idx {
                                state
                                    .last_activity_tick
                                    .store(GetTickCount64(), Ordering::Relaxed);
                                if let Ok(guard) = state.on_action.lock() {
                                    if let Some(ref cb) = *guard {
                                        cb(&region, region.x, region.y);
                                    }
                                }
                                NotifyWinEvent(
                                    0x8008, /* EVENT_SYSTEM_ALERT */
                                    hwnd,
                                    OBJID_CLIENT,
                                    (idx + 1) as i32,
                                );
                                let acc_state = state.accessibility_state(hwnd);
                                let child = DesktopWidgetFragmentProvider::new(
                                    acc_state,
                                    idx,
                                );
                                let simple: IRawElementProviderSimple = child.into();
                                let _ = UiaRaiseAutomationEvent(
                                    &simple,
                                    UIA_Invoke_InvokedEventId,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            let active = (wparam.0 & 0xFFFF) as u32;
            if active == 0 {
                // WA_INACTIVE: window lost focus
                trigger_focus_loss_dismissal(hwnd);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_KILLFOCUS => {
            trigger_focus_loss_dismissal(hwnd);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        0x02B1 /* WM_WTSSESSION_CHANGE */ => {
            if wparam.0 == 0x7 /* WTS_SESSION_LOCK */ || wparam.0 == 0x5 /* WTS_SESSION_LOGOFF */ {
                trigger_session_lock_dismissal(hwnd);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_GETOBJECT => {
            let obj_id = lparam.0 as i32;
            if obj_id == OBJID_CLIENT || obj_id == UiaRootObjectId {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    let acc_state = state.accessibility_state(hwnd);
                    let root_provider = create_root_provider(acc_state);
                    return UiaReturnRawElementProvider(hwnd, wparam, lparam, &root_provider);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_QUERYENDSESSION => {
            tracing::info!(
                "DesktopHostWindow (HWND {:?}): WM_QUERYENDSESSION received; confirming safe shutdown capability (LRESULT 1)",
                hwnd.0
            );
            LRESULT(1)
        }
        WM_ENDSESSION => {
            let is_ending = wparam.0 != 0;
            tracing::info!(
                "DesktopHostWindow (HWND {:?}): WM_ENDSESSION received (is_ending={})",
                hwnd.0,
                is_ending
            );
            if is_ending {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    dismiss_interactive_mode(hwnd, state, DismissalReason::SessionLocked);
                    if let Ok(guard) = state.on_session_end.lock() {
                        if let Some(ref cb) = *guard {
                            cb();
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Helper function to dismiss interactive mode cleanly and safely restore passive icon stacking.
unsafe fn dismiss_interactive_mode(
    hwnd: HWND,
    state: &DesktopWindowState,
    reason: DismissalReason,
) {
    if state.is_interactive.load(Ordering::SeqCst) {
        state.is_interactive.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = state.focused_region_index.lock() {
            *guard = None;
        }
        let passive_styles = DesktopHost::surface_extended_styles(false);
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, passive_styles as isize);
        let behind = safe_stacking_target(state.shelldll_defview);
        let _ = SetWindowPos(
            hwnd,
            behind,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        let _ = InvalidateRect(hwnd, None, false);
        if let Ok(guard) = state.on_dismiss.lock() {
            if let Some(ref cb) = *guard {
                cb(reason);
            }
        }
    }
}

unsafe fn trigger_focus_loss_dismissal(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
    if !ptr.is_null() {
        let state = &*ptr;
        dismiss_interactive_mode(hwnd, state, DismissalReason::FocusLost);
    }
}

unsafe fn trigger_session_lock_dismissal(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
    if !ptr.is_null() {
        let state = &*ptr;
        dismiss_interactive_mode(hwnd, state, DismissalReason::SessionLocked);
    }
}

/// Coordinates multi-monitor desktop overlay windows and interaction sessions.
pub struct DesktopWindowManager {
    windows: Vec<DesktopHostWindow>,
    shelldll_defview: Option<HWND>,
    worker_w: Option<HWND>,
    on_dismiss_cb: Option<DismissCallback>,
    on_paint_cb: Option<PaintCallback>,
    on_explorer_restart_cb: Option<ExplorerRestartCallback>,
    on_action_cb: Option<ActionCallback>,
    on_hotkey_cb: Option<HotkeyCallback>,
    on_dpi_changed_cb: Option<DpiChangedCallback>,
    on_display_changed_cb: Option<DisplayChangedCallback>,
    on_session_end_cb: Option<SessionEndCallback>,
    interactive_regions: Vec<InteractiveRegion>,
    is_hotkey_registered: bool,
}

unsafe impl Send for DesktopWindowManager {}
unsafe impl Sync for DesktopWindowManager {}

impl DesktopWindowManager {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            shelldll_defview: None,
            worker_w: None,
            on_dismiss_cb: None,
            on_paint_cb: None,
            on_explorer_restart_cb: None,
            on_action_cb: None,
            on_hotkey_cb: None,
            on_dpi_changed_cb: None,
            on_display_changed_cb: None,
            on_session_end_cb: None,
            interactive_regions: Vec::new(),
            is_hotkey_registered: false,
        }
    }

    /// Initializes desktop host windows for all connected monitors.
    pub fn initialize(&mut self, probe: &DesktopHierarchyProbe) -> Result<usize, DesktopHostError> {
        let defview_hwnd = probe.shelldll_defview_hwnd.map(|h| HWND(h as *mut _));
        let worker_hwnd = probe.worker_w_hwnd.map(|h| HWND(h as *mut _));

        self.shelldll_defview = defview_hwnd;
        self.worker_w = worker_hwnd;

        let monitors = enumerate_monitors();
        let mut created = 0;

        for (i, mon) in monitors.iter().enumerate() {
            let x = mon.bounds[0];
            let y = mon.bounds[1];
            let width = (mon.bounds[2] - mon.bounds[0]).max(100);
            let height = (mon.bounds[3] - mon.bounds[1]).max(100);
            let mon_id = format!("monitor-{}", i);

            match DesktopHostWindow::create(&mon_id, x, y, width, height, defview_hwnd, worker_hwnd)
            {
                Ok(mut win) => {
                    // Re-apply preserved callbacks to newly created window
                    if let Some(ref cb) = self.on_dismiss_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_dismiss(move |reason| cb_clone(reason));
                    }
                    if let Some(ref cb) = self.on_paint_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_paint(move |hwnd| cb_clone(hwnd));
                    }
                    if let Some(ref cb) = self.on_explorer_restart_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_explorer_restart(move || cb_clone());
                    }
                    if let Some(ref cb) = self.on_action_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_action(move |reg, px, py| cb_clone(reg, px, py));
                    }
                    if let Some(ref cb) = self.on_hotkey_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_hotkey(move |active| cb_clone(active));
                    }
                    if let Some(ref cb) = self.on_dpi_changed_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_dpi_changed(move |h, dpi, rect| cb_clone(h, dpi, rect));
                    }
                    if let Some(ref cb) = self.on_display_changed_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_display_change(move || cb_clone());
                    }
                    if let Some(ref cb) = self.on_session_end_cb {
                        let cb_clone = Arc::clone(cb);
                        win.set_on_session_end(move || cb_clone());
                    }
                    win.set_interactive_regions(self.interactive_regions.clone());

                    self.windows.push(win);
                    created += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to create host window for {}: {}", mon_id, e);
                }
            }
        }

        if self.is_hotkey_registered {
            if let Some(primary) = self.windows.first_mut() {
                let _ = primary.register_global_hotkey(
                    windows::Win32::UI::Input::KeyboardAndMouse::MOD_WIN
                        | windows::Win32::UI::Input::KeyboardAndMouse::MOD_CONTROL,
                    0x41, // Win+Ctrl+A
                );
            }
        }

        Ok(created)
    }

    pub fn windows(&self) -> &[DesktopHostWindow] {
        &self.windows
    }

    pub fn windows_mut(&mut self) -> &mut [DesktopHostWindow] {
        &mut self.windows
    }

    pub fn primary_window(&self) -> Option<&DesktopHostWindow> {
        self.windows.first()
    }

    pub fn primary_window_mut(&mut self) -> Option<&mut DesktopHostWindow> {
        self.windows.first_mut()
    }

    #[doc(hidden)]
    pub fn push_window_for_test(&mut self, window: DesktopHostWindow) {
        self.windows.push(window);
    }

    /// Enters temporary interactive mode across all desktop windows.
    pub fn enter_interactive_mode(&mut self) {
        for win in &mut self.windows {
            win.set_interactive(true);
        }
    }

    /// Exits interactive mode and returns to passive desktop layering.
    pub fn exit_interactive_mode(&mut self) {
        for win in &mut self.windows {
            win.set_interactive(false);
        }
    }

    /// Returns the primary desktop window DPI, defaulting to 96 if uninitialized.
    pub fn primary_dpi(&self) -> u32 {
        self.primary_window().map(|w| w.dpi()).unwrap_or(96)
    }

    /// Returns the primary desktop window DPI scale factor, defaulting to 1.0 if uninitialized.
    pub fn primary_scale_factor(&self) -> f32 {
        aethershell_win::dpi_to_scale_factor(self.primary_dpi())
    }

    /// Refreshes monitor geometry when displays are added, removed, or resized.
    pub fn refresh_monitors(&mut self) {
        let monitors = enumerate_monitors();
        for (i, win) in self.windows.iter_mut().enumerate() {
            if let Some(mon) = monitors.get(i) {
                let x = mon.bounds[0];
                let y = mon.bounds[1];
                let width = (mon.bounds[2] - mon.bounds[0]).max(100);
                let height = (mon.bounds[3] - mon.bounds[1]).max(100);
                win.handle_display_change(x, y, width, height);
            }
        }
        if let Some(ref cb) = self.on_display_changed_cb {
            cb();
        }
    }

    /// Sets a DPI change callback across all managed desktop host windows.
    pub fn set_on_dpi_changed<F>(&mut self, cb: F)
    where
        F: Fn(HWND, u32, &RECT) + Send + Sync + 'static,
    {
        let cb_arc: DpiChangedCallback = Arc::new(cb);
        self.on_dpi_changed_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_dpi_changed(move |h, dpi, rect| {
                cb_clone(h, dpi, rect);
            });
        }
    }

    /// Sets a display change callback across all managed desktop host windows.
    pub fn set_on_display_change<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb_arc: DisplayChangedCallback = Arc::new(cb);
        self.on_display_changed_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_display_change(move || {
                cb_clone();
            });
        }
    }

    /// Sets an auto-dismissal callback on all managed desktop host windows.
    pub fn set_on_dismiss<F>(&mut self, cb: F)
    where
        F: Fn(DismissalReason) + Send + Sync + 'static,
    {
        let cb_arc: DismissCallback = Arc::new(cb);
        self.on_dismiss_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_dismiss(move |reason| {
                cb_clone(reason);
            });
        }
    }

    /// Sets a paint callback on all managed desktop host windows.
    pub fn set_on_paint<F>(&mut self, cb: F)
    where
        F: Fn(HWND) + Send + Sync + 'static,
    {
        let cb_arc: PaintCallback = Arc::new(cb);
        self.on_paint_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_paint(move |hwnd| {
                cb_clone(hwnd);
            });
        }
    }

    /// Sets an action callback on all managed desktop host windows.
    pub fn set_on_action<F>(&mut self, cb: F)
    where
        F: Fn(&InteractiveRegion, f32, f32) + Send + Sync + 'static,
    {
        let cb_arc: ActionCallback = Arc::new(cb);
        self.on_action_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_action(move |reg, px, py| {
                cb_clone(reg, px, py);
            });
        }
    }

    /// Sets a hotkey toggle callback across all managed desktop host windows.
    pub fn set_on_hotkey<F>(&mut self, cb: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        let cb_arc: HotkeyCallback = Arc::new(cb);
        self.on_hotkey_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_hotkey(move |active| {
                cb_clone(active);
            });
        }
    }

    /// Sets the list of interactive regions across all managed windows.
    pub fn set_interactive_regions(&mut self, regions: Vec<InteractiveRegion>) {
        self.interactive_regions = regions.clone();
        for win in &mut self.windows {
            win.set_interactive_regions(regions.clone());
        }
    }

    /// Retrieves the current interactive regions.
    pub fn interactive_regions(&self) -> &[InteractiveRegion] {
        &self.interactive_regions
    }

    /// Registers a system-wide global hotkey on the primary desktop host window.
    pub fn register_global_hotkey(
        &mut self,
        modifiers: HOT_KEY_MODIFIERS,
        vk: u32,
    ) -> Result<(), DesktopHostError> {
        if let Some(primary) = self.primary_window_mut() {
            primary.register_global_hotkey(modifiers, vk)?;
            self.is_hotkey_registered = true;
            Ok(())
        } else {
            Err(DesktopHostError::WindowCreationFailed(
                "No primary desktop window available for hotkey registration".into(),
            ))
        }
    }

    /// Unregisters the system-wide global hotkey.
    pub fn unregister_global_hotkey(&mut self) -> Result<(), DesktopHostError> {
        if let Some(primary) = self.primary_window_mut() {
            primary.unregister_global_hotkey()?;
        }
        self.is_hotkey_registered = false;
        Ok(())
    }

    /// Checks if the global hotkey is currently registered.
    pub fn is_hotkey_registered(&self) -> bool {
        self.is_hotkey_registered
    }

    /// Handles Explorer restart across all managed desktop host overlay windows.
    ///
    /// Re-probes the desktop shell hierarchy, invalidates stale shell handles,
    /// recreates destroyed/orphaned overlay windows safely, and restores passive stacking.
    pub fn handle_explorer_restart(&mut self) -> DesktopHierarchyProbe {
        let probe = DesktopHierarchyProbe::probe();
        let fresh_defview = probe.shelldll_defview_hwnd.map(|h| HWND(h as *mut _));
        let fresh_worker_w = probe.worker_w_hwnd.map(|h| HWND(h as *mut _));

        self.shelldll_defview = fresh_defview;
        self.worker_w = fresh_worker_w;

        let any_invalid = self.windows.iter().any(|w| !w.is_window_valid());
        if any_invalid || self.windows.is_empty() {
            tracing::warn!(
                "Desktop host window invalidated during Explorer restart; safely recreating windows"
            );
            self.windows.clear();
            let _ = self.initialize(&probe);
        } else {
            for win in &mut self.windows {
                win.handle_explorer_restart_with_probe(&probe);
            }
        }

        probe
    }

    /// Sets an Explorer restart callback across all managed desktop host windows.
    pub fn set_on_explorer_restart<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb_arc: ExplorerRestartCallback = Arc::new(cb);
        self.on_explorer_restart_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_explorer_restart(move || {
                cb_clone();
            });
        }
    }

    /// Sets a session end callback across all managed desktop host windows.
    pub fn set_on_session_end<F>(&mut self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb_arc: SessionEndCallback = Arc::new(cb);
        self.on_session_end_cb = Some(Arc::clone(&cb_arc));
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_session_end(move || {
                cb_clone();
            });
        }
    }

    /// Destroys and cleans up all managed desktop host windows on the current thread.
    pub fn clear_windows(&mut self) {
        self.windows.clear();
    }

    /// Returns whether any managed desktop host window has become an invalid or stale OS window.
    pub fn has_stale_windows(&self) -> bool {
        self.windows.iter().any(|w| !w.is_window_valid())
    }

    /// Drains and dispatches pending Win32 messages (WM_PAINT, WM_KEYDOWN, WM_ACTIVATE, etc.).
    /// Returns true if WM_QUIT was encountered on the thread message queue.
    pub fn pump_messages() -> bool {
        unsafe {
            let mut msg = MSG::default();
            let mut quit_received = false;
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    quit_received = true;
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            quit_received
        }
    }
}

impl Default for DesktopWindowManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_host_window_lifecycle_and_stacking() {
        let probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        };

        let mut manager = DesktopWindowManager::new();
        let res = manager.initialize(&probe);
        assert!(res.is_ok());

        let count = res.unwrap();
        println!("Created {} desktop host overlay windows", count);

        if let Some(win) = manager.windows_mut().first_mut() {
            assert!(!win.is_interactive());
            assert!(!win.hwnd.0.is_null());

            // Enter interactive mode: must not have WS_EX_TOPMOST
            win.set_interactive(true);
            assert!(win.is_interactive());

            // Exit interactive mode: passive styles restored
            win.set_interactive(false);
            assert!(!win.is_interactive());

            // Handle display change
            win.handle_display_change(0, 0, 1920, 1080);
            assert_eq!(win.bounds, [0, 0, 1920, 1080]);

            // Handle DPI change
            win.handle_dpi_change(144, None);
            assert_eq!(win.dpi, 144);
        }
    }

    #[test]
    fn test_desktop_host_window_dismissal_wiring() {
        use std::sync::atomic::AtomicU32;

        let _probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        };

        let mut win = DesktopHostWindow::create("test-mon", 0, 0, 300, 200, None, None)
            .expect("Failed to create test window");

        let dismissed_count = Arc::new(AtomicU32::new(0));
        let dismissed_clone = Arc::clone(&dismissed_count);

        win.set_on_dismiss(move |reason| {
            if reason == DismissalReason::EscapeKey || reason == DismissalReason::FocusLost {
                dismissed_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        win.set_interactive(true);
        assert!(win.is_interactive());

        // Simulate Escape key press message directly to wnd_proc
        unsafe {
            desktop_host_wnd_proc(win.hwnd, WM_KEYDOWN, WPARAM(0x1B), LPARAM(0));
        }

        assert!(!win.is_interactive());
        assert_eq!(dismissed_count.load(Ordering::SeqCst), 1);

        // Re-enter and simulate session lock (WM_WTSSESSION_CHANGE = 0x02B1 with WTS_SESSION_LOCK = 7)
        let session_locked = Arc::new(AtomicBool::new(false));
        let sl_clone = Arc::clone(&session_locked);
        win.set_on_dismiss(move |reason| {
            if reason == DismissalReason::SessionLocked {
                sl_clone.store(true, Ordering::SeqCst);
            }
        });

        win.set_interactive(true);
        assert!(win.is_interactive());

        unsafe {
            desktop_host_wnd_proc(win.hwnd, 0x02B1, WPARAM(7), LPARAM(0));
        }

        assert!(!win.is_interactive());
        assert!(session_locked.load(Ordering::SeqCst));

        // Test paint callback invocation
        let painted = Arc::new(AtomicBool::new(false));
        let paint_clone = Arc::clone(&painted);
        win.set_on_paint(move |_hwnd| {
            paint_clone.store(true, Ordering::SeqCst);
        });

        unsafe {
            desktop_host_wnd_proc(win.hwnd, WM_PAINT, WPARAM(0), LPARAM(0));
        }
        assert!(painted.load(Ordering::SeqCst));

        // Test pump_messages does not panic
        DesktopWindowManager::pump_messages();
    }

    #[test]
    fn test_safe_stacking_target_fallback() {
        // None falls back to HWND_BOTTOM
        assert_eq!(safe_stacking_target(None), HWND_BOTTOM);

        // Dead / invalid HWND falls back to HWND_BOTTOM
        let dead_hwnd = HWND(0xDEADBEEF as *mut _);
        assert_eq!(safe_stacking_target(Some(dead_hwnd)), HWND_BOTTOM);

        // Valid window handle is preserved
        let win = DesktopHostWindow::create("test-valid-stack", 0, 0, 100, 100, None, None)
            .expect("create test window");
        assert_eq!(safe_stacking_target(Some(win.hwnd)), win.hwnd);
    }

    #[test]
    fn test_explorer_restart_taskbar_created_handling() {
        let taskbar_created = taskbar_created_msg();
        assert!(taskbar_created >= 0xC000);

        let mut win = DesktopHostWindow::create("test-mon-restart", 0, 0, 300, 200, None, None)
            .expect("Failed to create test window");

        assert_eq!(win.explorer_restart_count(), 0);
        assert!(win.is_window_valid());

        let restart_fired = Arc::new(AtomicBool::new(false));
        let restart_clone = Arc::clone(&restart_fired);
        win.set_on_explorer_restart(move || {
            restart_clone.store(true, Ordering::SeqCst);
        });

        // 1. Simulate TaskbarCreated broadcast to wnd_proc
        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, taskbar_created, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(0));
        }

        assert_eq!(win.explorer_restart_count(), 1);
        assert!(restart_fired.load(Ordering::SeqCst));

        // 2. Test restart loop / debounce protection:
        // Immediate second message within 300ms must be debounced and ignored
        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, taskbar_created, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(0));
        }
        // Count should still be 1 due to debounce
        assert_eq!(win.explorer_restart_count(), 1);

        // Immediate invocation of handle_explorer_restart within 300ms must also be debounced
        // to prevent double-counting when engine callback invokes manager recovery
        let _ = win.handle_explorer_restart();
        assert_eq!(
            win.explorer_restart_count(),
            1,
            "Immediate handle_explorer_restart within debounce interval must not double-increment count"
        );

        // 3. Test handle_explorer_restart after debounce interval elapses (>300ms + VM timer quantum margin)
        std::thread::sleep(std::time::Duration::from_millis(450));
        let probe = win.handle_explorer_restart();
        assert_eq!(win.explorer_restart_count(), 2);
        println!(
            "Re-probed desktop hierarchy after debounce expiry: {:?}",
            probe
        );
    }

    #[test]
    fn test_desktop_window_manager_explorer_restart_recovery() {
        let probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        };

        let mut manager = DesktopWindowManager::new();
        let _ = manager.initialize(&probe);
        assert!(!manager.has_stale_windows());

        let restart_count = Arc::new(AtomicUsize::new(0));
        let rc_clone = Arc::clone(&restart_count);
        manager.set_on_explorer_restart(move || {
            rc_clone.fetch_add(1, Ordering::SeqCst);
        });

        let fresh_probe = manager.handle_explorer_restart();
        assert_eq!(fresh_probe, DesktopHierarchyProbe::probe());
        assert!(!manager.has_stale_windows());
    }

    #[test]
    fn test_desktop_window_manager_callback_preservation_across_recreation() {
        let probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        };

        let mut manager = DesktopWindowManager::new();
        let _ = manager.initialize(&probe);

        let dismiss_fired = Arc::new(AtomicBool::new(false));
        let df_clone = Arc::clone(&dismiss_fired);
        manager.set_on_dismiss(move |_| {
            df_clone.store(true, Ordering::SeqCst);
        });

        let paint_fired = Arc::new(AtomicBool::new(false));
        let pf_clone = Arc::clone(&paint_fired);
        manager.set_on_paint(move |_| {
            pf_clone.store(true, Ordering::SeqCst);
        });

        let restart_fired = Arc::new(AtomicBool::new(false));
        let rf_clone = Arc::clone(&restart_fired);
        manager.set_on_explorer_restart(move || {
            rf_clone.store(true, Ordering::SeqCst);
        });

        // Simulate window invalidation (e.g. Explorer crash destroying HWND)
        if let Some(win) = manager.primary_window_mut() {
            unsafe {
                let _ = DestroyWindow(win.hwnd);
            }
        }
        assert!(manager.has_stale_windows());

        // Recovery recreates windows
        let _ = manager.handle_explorer_restart();
        assert!(!manager.has_stale_windows());

        // Verify callbacks were preserved and re-registered on recreated windows
        if let Some(win) = manager.primary_window() {
            // Trigger dismiss
            unsafe {
                trigger_focus_loss_dismissal(win.hwnd);
            }
            // Trigger paint
            unsafe {
                let _ = InvalidateRect(win.hwnd, None, false);
            }
            // Trigger explorer restart
            let taskbar_created = taskbar_created_msg();
            unsafe {
                let _ = desktop_host_wnd_proc(win.hwnd, taskbar_created, WPARAM(0), LPARAM(0));
            }
        }

        assert!(restart_fired.load(Ordering::SeqCst));
    }

    #[test]
    fn test_desktop_host_window_hotkey_toggle_and_dismissal() {
        let mut win = DesktopHostWindow::create("test-mon-hotkey", 0, 0, 300, 200, None, None)
            .expect("Failed to create test window");

        assert!(!win.is_interactive());

        let hotkey_toggled = Arc::new(AtomicBool::new(false));
        let ht_clone = Arc::clone(&hotkey_toggled);
        win.set_on_hotkey(move |active| {
            ht_clone.store(active, Ordering::SeqCst);
        });

        let dismiss_reason = Arc::new(std::sync::Mutex::new(None));
        let dr_clone = Arc::clone(&dismiss_reason);
        win.set_on_dismiss(move |reason| {
            if let Ok(mut guard) = dr_clone.lock() {
                *guard = Some(reason);
            }
        });

        // 1. Simulate WM_HOTKEY to activate interactive mode
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                WM_HOTKEY,
                WPARAM(HOTKEY_ID_INTERACTIVE_MODE as usize),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }

        assert!(win.is_interactive());
        assert!(hotkey_toggled.load(Ordering::SeqCst));

        // 2. Simulate second WM_HOTKEY to deactivate back to passive
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                WM_HOTKEY,
                WPARAM(HOTKEY_ID_INTERACTIVE_MODE as usize),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }

        assert!(!win.is_interactive());
        assert!(!hotkey_toggled.load(Ordering::SeqCst));
        assert_eq!(
            *dismiss_reason.lock().unwrap(),
            Some(DismissalReason::ExplicitExit)
        );
    }

    #[test]
    fn test_desktop_host_window_pointer_hit_test_and_action_dispatch() {
        let mut win = DesktopHostWindow::create("test-mon-hit", 0, 0, 400, 300, None, None)
            .expect("Failed to create test window");

        let regions = vec![
            InteractiveRegion::new(
                "btn_play",
                50.0,
                50.0,
                60.0,
                30.0,
                Some("play_pause".into()),
            ),
            InteractiveRegion::new("btn_next", 120.0, 50.0, 40.0, 30.0, Some("next".into())),
        ];
        win.set_interactive_regions(regions);

        let action_invoked = Arc::new(std::sync::Mutex::new(None));
        let ai_clone = Arc::clone(&action_invoked);
        win.set_on_action(move |reg, x, y| {
            if let Ok(mut guard) = ai_clone.lock() {
                *guard = Some((reg.id.clone(), reg.action.clone(), x, y));
            }
        });

        // Enter interactive mode
        win.set_interactive(true);
        assert!(win.is_interactive());

        // 1. Click inside "btn_play" at (70, 65)
        // LPARAM encodes x in low word, y in high word: (65 << 16) | 70
        let lparam_inside = LPARAM(((65i16 as isize) << 16) | (70i16 as isize));
        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_inside);
            assert_eq!(res, LRESULT(0));
        }

        // Action callback should have fired with "btn_play" and "play_pause"
        let invoked = action_invoked.lock().unwrap().clone();
        assert!(invoked.is_some());
        let (id, action, x, y) = invoked.unwrap();
        assert_eq!(id, "btn_play");
        assert_eq!(action.as_deref(), Some("play_pause"));
        assert_eq!(x, 70.0);
        assert_eq!(y, 65.0);

        // Window should still be interactive after successful hit
        assert!(win.is_interactive());

        // 2. Click outside any region at (300, 250)
        let dismiss_fired = Arc::new(std::sync::Mutex::new(None));
        let df_clone = Arc::clone(&dismiss_fired);
        win.set_on_dismiss(move |reason| {
            if let Ok(mut guard) = df_clone.lock() {
                *guard = Some(reason);
            }
        });

        let lparam_outside = LPARAM(((250i16 as isize) << 16) | (300i16 as isize));
        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_outside);
            assert_eq!(res, LRESULT(0));
        }

        // Window must immediately dismiss to passive mode on click outside
        assert!(!win.is_interactive());
        assert_eq!(
            *dismiss_fired.lock().unwrap(),
            Some(DismissalReason::ClickOutside)
        );
    }

    #[test]
    fn test_desktop_window_manager_interactive_regions_and_action_preservation() {
        let probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            worker_w_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
        };

        let mut manager = DesktopWindowManager::new();
        let _ = manager.initialize(&probe);

        let regions = vec![InteractiveRegion::new(
            "btn_vol",
            10.0,
            10.0,
            50.0,
            25.0,
            Some("volume_up".into()),
        )];
        manager.set_interactive_regions(regions.clone());
        assert_eq!(manager.interactive_regions(), &regions);

        let action_recorded = Arc::new(AtomicBool::new(false));
        let ar_clone = Arc::clone(&action_recorded);
        manager.set_on_action(move |_, _, _| {
            ar_clone.store(true, Ordering::SeqCst);
        });

        // Simulate Explorer restart recreating windows
        if let Some(win) = manager.primary_window_mut() {
            unsafe {
                let _ = DestroyWindow(win.hwnd);
            }
        }
        let _ = manager.handle_explorer_restart();
        assert!(!manager.has_stale_windows());

        // Verify that recreated windows retained interactive regions and action callback
        if let Some(win) = manager.primary_window_mut() {
            assert_eq!(win.get_interactive_regions(), regions);
            win.set_interactive(true);

            let lparam_hit = LPARAM(((20i16 as isize) << 16) | (20i16 as isize));
            unsafe {
                let _ = desktop_host_wnd_proc(win.hwnd, WM_LBUTTONDOWN, WPARAM(0), lparam_hit);
            }
        }

        assert!(action_recorded.load(Ordering::SeqCst));
    }

    #[test]
    fn test_desktop_host_window_wm_getobject_and_uia_provider() {
        let mut win = DesktopHostWindow::create("test-mon-uia", 0, 0, 600, 400, None, None)
            .expect("create test window");

        let regions = vec![InteractiveRegion::new(
            "btn_mute",
            20.0,
            30.0,
            80.0,
            40.0,
            Some("toggle_mute".into()),
        )
        .with_accessibility("Mute Audio", "button", Some("Unmuted".into()))];
        win.set_interactive_regions(regions);

        // 1. Dispatch WM_GETOBJECT with OBJID_CLIENT (-4)
        unsafe {
            let res_client = desktop_host_wnd_proc(
                win.hwnd,
                WM_GETOBJECT,
                WPARAM(0),
                LPARAM(OBJID_CLIENT as isize),
            );
            // UiaReturnRawElementProvider produces a valid non-zero LRESULT handle for the UIA bridge
            println!("WM_GETOBJECT OBJID_CLIENT returned: {:?}", res_client);
        }

        // 2. Dispatch WM_GETOBJECT with UiaRootObjectId (-25)
        unsafe {
            let res_uia = desktop_host_wnd_proc(
                win.hwnd,
                WM_GETOBJECT,
                WPARAM(0),
                LPARAM(UiaRootObjectId as isize),
            );
            println!("WM_GETOBJECT UiaRootObjectId returned: {:?}", res_uia);
        }

        // 3. Dispatch WM_GETOBJECT with arbitrary non-accessibility lParam (0) -> handled by DefWindowProcW
        unsafe {
            let res_def = desktop_host_wnd_proc(win.hwnd, WM_GETOBJECT, WPARAM(0), LPARAM(0));
            assert_eq!(res_def, LRESULT(0));
        }
    }

    #[test]
    fn test_desktop_host_window_accessibility_keyboard_focus_notification() {
        use windows::core::Interface;
        use windows::Win32::UI::Accessibility::{
            IRawElementProviderFragment, IRawElementProviderFragmentRoot,
            IRawElementProviderSimple, UIA_HasKeyboardFocusPropertyId, UIA_NamePropertyId,
        };

        let mut win = DesktopHostWindow::create("test-mon-acc-focus", 0, 0, 800, 600, None, None)
            .expect("create test window");

        let regions = vec![
            InteractiveRegion::new("btn_prev", 10.0, 10.0, 50.0, 30.0, Some("prev".into()))
                .with_accessibility("Previous Track", "button", None),
            InteractiveRegion::new("btn_play", 70.0, 10.0, 60.0, 30.0, Some("play".into()))
                .with_accessibility("Play Track", "button", None),
            InteractiveRegion::new("btn_next", 140.0, 10.0, 50.0, 30.0, Some("next".into()))
                .with_accessibility("Next Track", "button", None),
        ];
        win.set_interactive_regions(regions);

        let action_invoked = Arc::new(std::sync::Mutex::new(None));
        let ai_clone = Arc::clone(&action_invoked);
        win.set_on_action(move |reg, _x, _y| {
            if let Ok(mut guard) = ai_clone.lock() {
                *guard = Some(reg.id.clone());
            }
        });

        win.set_interactive(true);
        assert!(win.is_interactive());
        assert_eq!(win.focused_region_index(), None);

        // 1. Press TAB (0x09) -> moves focus to index 0 (btn_prev)
        unsafe {
            let _ = desktop_host_wnd_proc(win.hwnd, WM_KEYDOWN, WPARAM(0x09), LPARAM(0));
        }
        assert_eq!(win.focused_region_index(), Some(0));

        // Query UIA Provider GetFocus
        let state = unsafe { (*win.state_ptr).accessibility_state(win.hwnd) };
        let root_simple = create_root_provider(state);
        let root_root: IRawElementProviderFragmentRoot = root_simple.cast().expect("cast root");
        unsafe {
            let focused: IRawElementProviderFragment = root_root.GetFocus().expect("GetFocus");
            let focused_simple: IRawElementProviderSimple = focused.cast().expect("focused simple");
            let name = focused_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Name");
            assert_eq!(name.to_string(), "Previous Track");

            let has_focus = focused_simple
                .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                .expect("HasKeyboardFocus");
            assert_eq!(has_focus.to_string(), "-1"); // VARIANT_TRUE
        }

        // 2. Press RIGHT ARROW (0x27) -> moves focus to index 1 (btn_play)
        unsafe {
            let _ = desktop_host_wnd_proc(win.hwnd, WM_KEYDOWN, WPARAM(0x27), LPARAM(0));
        }
        assert_eq!(win.focused_region_index(), Some(1));

        unsafe {
            let focused: IRawElementProviderFragment = root_root.GetFocus().expect("GetFocus");
            let focused_simple: IRawElementProviderSimple = focused.cast().expect("focused simple");
            let name = focused_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Name");
            assert_eq!(name.to_string(), "Play Track");
        }

        // 3. Press ENTER (0x0D) -> activates focused widget (btn_play)
        unsafe {
            let _ = desktop_host_wnd_proc(win.hwnd, WM_KEYDOWN, WPARAM(0x0D), LPARAM(0));
        }
        assert_eq!(*action_invoked.lock().unwrap(), Some("btn_play".into()));
    }

    #[test]
    fn test_desktop_host_window_regions_clamping_on_resize() {
        let mut win = DesktopHostWindow::create("test-mon-clamp", 0, 0, 800, 600, None, None)
            .expect("create test window");

        let regions = vec![
            InteractiveRegion::new("btn_0", 0.0, 0.0, 10.0, 10.0, None),
            InteractiveRegion::new("btn_1", 10.0, 0.0, 10.0, 10.0, None),
            InteractiveRegion::new("btn_2", 20.0, 0.0, 10.0, 10.0, None),
        ];
        win.set_interactive_regions(regions);
        win.set_focused_region_index(Some(2));
        assert_eq!(win.focused_region_index(), Some(2));

        // Shrink regions to 2 items (indices 0 and 1) -> index 2 is now out of bounds and must be clamped to None
        let smaller = vec![
            InteractiveRegion::new("btn_0", 0.0, 0.0, 10.0, 10.0, None),
            InteractiveRegion::new("btn_1", 10.0, 0.0, 10.0, 10.0, None),
        ];
        win.set_interactive_regions(smaller);
        assert_eq!(win.focused_region_index(), None);
    }

    #[test]
    fn test_desktop_host_window_shell_hook_and_win_d_stacking_reassertion() {
        let shell_hook = shellhook_msg();
        assert!(
            shell_hook >= 0xC000,
            "SHELLHOOK message must be a registered dynamic message (>= 0xC000)"
        );

        let mut win = DesktopHostWindow::create("test-mon-shellhook", 0, 0, 400, 300, None, None)
            .expect("create test window");

        assert_eq!(win.win_d_restack_count(), 0);
        assert!(win.is_window_valid());

        let shell_event_received = Arc::new(std::sync::Mutex::new(None));
        let ser_clone = Arc::clone(&shell_event_received);
        win.set_on_shell_event(move |code, hwnd_val| {
            if let Ok(mut guard) = ser_clone.lock() {
                *guard = Some((code, hwnd_val));
            }
        });

        // 1. Simulate HSHELL_WINDOWACTIVATED (Win+D or window focus switch)
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                shell_hook,
                WPARAM(HSHELL_WINDOWACTIVATED),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }

        assert_eq!(
            win.win_d_restack_count(),
            1,
            "Window must re-assert stacking behind desktop icons on shell activation"
        );
        assert_eq!(
            *shell_event_received.lock().unwrap(),
            Some((HSHELL_WINDOWACTIVATED, 0))
        );

        // 2. Debounce test: immediate rapid shell event within 50ms must be debounced
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                shell_hook,
                WPARAM(HSHELL_WINDOWACTIVATED),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }
        assert_eq!(
            win.win_d_restack_count(),
            1,
            "Burst shell events within 50ms must be debounced"
        );

        // 3. After debounce expiry, HSHELL_RUDEAPPACTIVATED must re-assert stacking
        std::thread::sleep(std::time::Duration::from_millis(60));
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                shell_hook,
                WPARAM(HSHELL_RUDEAPPACTIVATED),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }
        assert_eq!(win.win_d_restack_count(), 2);

        // 4. In interactive mode, shell events must NOT alter interactive state or disrupt overlay
        win.set_interactive(true);
        assert!(win.is_interactive());
        std::thread::sleep(std::time::Duration::from_millis(60));
        unsafe {
            let res = desktop_host_wnd_proc(
                win.hwnd,
                shell_hook,
                WPARAM(HSHELL_WINDOWACTIVATED),
                LPARAM(0),
            );
            assert_eq!(res, LRESULT(0));
        }
        assert_eq!(
            win.win_d_restack_count(),
            2,
            "Shell hook must not force passive restacking while in interactive mode"
        );
        assert!(win.is_interactive(), "Interactive mode must remain active");

        // Restore passive mode
        win.set_interactive(false);

        // 5. Explicit reassert_desktop_stacking() works in passive mode
        win.reassert_desktop_stacking();
        assert_eq!(win.win_d_restack_count(), 3);
    }

    #[test]
    fn test_desktop_host_window_wm_endsession_handling() {
        let mut win =
            match DesktopHostWindow::create("MONITOR_ENDSESSION", 0, 0, 800, 600, None, None) {
                Ok(w) => w,
                Err(e) => {
                    println!("DesktopHostWindow creation skipped in test env: {}", e);
                    return;
                }
            };

        // 1. WM_QUERYENDSESSION returns LRESULT(1)
        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, WM_QUERYENDSESSION, WPARAM(0), LPARAM(0));
            assert_eq!(res, LRESULT(1), "WM_QUERYENDSESSION must return LRESULT(1)");
        }

        // 2. WM_ENDSESSION with wParam=1 (session ending) restores passive mode, triggers on_session_end, and returns LRESULT(0)
        let session_ended = Arc::new(AtomicBool::new(false));
        let session_ended_clone = Arc::clone(&session_ended);
        win.set_on_session_end(move || {
            session_ended_clone.store(true, Ordering::SeqCst);
        });

        win.set_interactive(true);
        assert!(win.is_interactive());

        unsafe {
            let res = desktop_host_wnd_proc(win.hwnd, WM_ENDSESSION, WPARAM(1), LPARAM(0));
            assert_eq!(res, LRESULT(0), "WM_ENDSESSION must return LRESULT(0)");
        }

        assert!(
            !win.is_interactive(),
            "WM_ENDSESSION must dismiss interactive mode"
        );
        assert!(
            session_ended.load(Ordering::SeqCst),
            "WM_ENDSESSION must trigger on_session_end callback"
        );
    }
}
