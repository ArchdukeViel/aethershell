//! Real Win32 Desktop Host Window embedded in Windows shell hierarchy.

use crate::host::{DesktopHost, DesktopHostError};
use crate::interaction::DismissalReason;
use aethershell_win::{enumerate_monitors, DesktopHierarchyProbe};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW,
    LoadCursorW, PeekMessageW, RegisterClassExW, SetWindowLongPtrW, SetWindowPos, TranslateMessage,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, GWL_EXSTYLE, HWND_BOTTOM, HWND_NOTOPMOST,
    IDC_ARROW, MSG, PM_REMOVE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WM_ACTIVATE, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_QUIT,
    WNDCLASSEXW, WS_POPUP, WS_VISIBLE,
};

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
const DESKTOP_HOST_CLASS_NAME: windows::core::PCWSTR = w!("AetherShellDesktopHostWindow");

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

/// Internal state stored on the Win32 HWND via GWLP_USERDATA.
pub struct DesktopWindowState {
    pub is_interactive: Arc<AtomicBool>,
    pub shelldll_defview: Option<HWND>,
    pub on_dismiss: std::sync::Mutex<Option<DismissCallback>>,
    pub on_paint: std::sync::Mutex<Option<PaintCallback>>,
}

/// Represents a real Win32 desktop overlay window for a specific monitor.
pub struct DesktopHostWindow {
    pub hwnd: HWND,
    pub monitor_id: String,
    pub bounds: [i32; 4], // [x, y, width, height]
    pub dpi: u32,
    pub is_interactive: Arc<AtomicBool>,
    pub shelldll_defview_hwnd: Option<HWND>,
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

        let state = Box::new(DesktopWindowState {
            is_interactive: Arc::clone(&is_interactive),
            shelldll_defview,
            on_dismiss: std::sync::Mutex::new(None),
            on_paint: std::sync::Mutex::new(None),
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

            // Register session notifications (WM_WTSSESSION_CHANGE)
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

            // Stacking order: Place behind SHELLDLL_DefView (or HWND_BOTTOM)
            let insert_after = shelldll_defview.unwrap_or(HWND_BOTTOM);
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
                dpi: 96,
                is_interactive,
                shelldll_defview_hwnd: shelldll_defview,
                state_ptr,
            })
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
                let behind = self.shelldll_defview_hwnd.unwrap_or(HWND_BOTTOM);
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
        if let Some(rect) = suggested_rect {
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            self.bounds = [rect.left, rect.top, w, h];
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
}

impl Drop for DesktopHostWindow {
    fn drop(&mut self) {
        if !self.hwnd.0.is_null() {
            unsafe {
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
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if lparam.0 != 0 {
                let rect = *(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x1B {
                // Escape key dismisses temporary interactive mode
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    if state.is_interactive.load(Ordering::SeqCst) {
                        state.is_interactive.store(false, Ordering::SeqCst);
                        let passive_styles = DesktopHost::surface_extended_styles(false);
                        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, passive_styles as isize);
                        let behind = state.shelldll_defview.unwrap_or(HWND_BOTTOM);
                        let _ = SetWindowPos(
                            hwnd,
                            behind,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                        );
                        if let Ok(guard) = state.on_dismiss.lock() {
                            if let Some(ref cb) = *guard {
                                cb(DismissalReason::EscapeKey);
                            }
                        }
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
        WM_NCDESTROY => {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn trigger_focus_loss_dismissal(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
    if !ptr.is_null() {
        let state = &*ptr;
        if state.is_interactive.load(Ordering::SeqCst) {
            state.is_interactive.store(false, Ordering::SeqCst);
            let passive_styles = DesktopHost::surface_extended_styles(false);
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, passive_styles as isize);
            let behind = state.shelldll_defview.unwrap_or(HWND_BOTTOM);
            let _ = SetWindowPos(
                hwnd,
                behind,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
            if let Ok(guard) = state.on_dismiss.lock() {
                if let Some(ref cb) = *guard {
                    cb(DismissalReason::FocusLost);
                }
            }
        }
    }
}

unsafe fn trigger_session_lock_dismissal(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DesktopWindowState;
    if !ptr.is_null() {
        let state = &*ptr;
        if state.is_interactive.load(Ordering::SeqCst) {
            state.is_interactive.store(false, Ordering::SeqCst);
            let passive_styles = DesktopHost::surface_extended_styles(false);
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, passive_styles as isize);
            let behind = state.shelldll_defview.unwrap_or(HWND_BOTTOM);
            let _ = SetWindowPos(
                hwnd,
                behind,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
            if let Ok(guard) = state.on_dismiss.lock() {
                if let Some(ref cb) = *guard {
                    cb(DismissalReason::SessionLocked);
                }
            }
        }
    }
}

/// Coordinates multi-monitor desktop overlay windows and interaction sessions.
pub struct DesktopWindowManager {
    windows: Vec<DesktopHostWindow>,
    shelldll_defview: Option<HWND>,
    worker_w: Option<HWND>,
}

unsafe impl Send for DesktopWindowManager {}
unsafe impl Sync for DesktopWindowManager {}

impl DesktopWindowManager {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            shelldll_defview: None,
            worker_w: None,
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
                Ok(win) => {
                    self.windows.push(win);
                    created += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to create host window for {}: {}", mon_id, e);
                }
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
    }

    /// Sets an auto-dismissal callback on all managed desktop host windows.
    pub fn set_on_dismiss<F>(&mut self, cb: F)
    where
        F: Fn(DismissalReason) + Send + Sync + 'static,
    {
        let cb_arc = Arc::new(cb);
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
        let cb_arc = Arc::new(cb);
        for win in &mut self.windows {
            let cb_clone = Arc::clone(&cb_arc);
            win.set_on_paint(move |hwnd| {
                cb_clone(hwnd);
            });
        }
    }

    /// Drains and dispatches pending Win32 messages (WM_PAINT, WM_KEYDOWN, WM_ACTIVATE, etc.).
    pub fn pump_messages() {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
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
}
