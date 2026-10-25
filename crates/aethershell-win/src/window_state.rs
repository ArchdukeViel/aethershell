use aethershell_core::MonitorTaskbarState;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsWindow, IsWindowVisible, IsZoomed,
    GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

use crate::monitor::MonitorInfo;

/// Evaluates the taskbar state for a monitor given the foreground/active window.
pub struct WindowStateEvaluator;

impl WindowStateEvaluator {
    /// Determines the taskbar state for the specified monitor given the foreground window.
    pub fn evaluate_state(
        foreground_hwnd: HWND,
        monitor: &MonitorInfo,
        desktop_workerw: Option<HWND>,
        desktop_progman: Option<HWND>,
    ) -> MonitorTaskbarState {
        // If no window, or window is destroyed/invisible, default to Desktop
        if foreground_hwnd.0.is_null() || !unsafe { IsWindow(foreground_hwnd).as_bool() } {
            return MonitorTaskbarState::Desktop;
        }

        // Check if window is the desktop itself
        if Some(foreground_hwnd) == desktop_workerw || Some(foreground_hwnd) == desktop_progman {
            return MonitorTaskbarState::Desktop;
        }

        // Get class name
        let class_name = get_window_class_name(foreground_hwnd);

        // Check desktop classes
        if class_name == "Progman" || class_name == "WorkerW" {
            return MonitorTaskbarState::Desktop;
        }

        // Check Flyout / Shell popups
        if is_flyout_class(&class_name) {
            return MonitorTaskbarState::FlyoutVisible;
        }

        // Check visibility and cloaking
        if !unsafe { IsWindowVisible(foreground_hwnd).as_bool() } || is_cloaked(foreground_hwnd) {
            return MonitorTaskbarState::Desktop;
        }

        // Filter out tool windows
        let ex_style = unsafe { GetWindowLongPtrW(foreground_hwnd, GWL_EXSTYLE) as u32 };
        if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
            return MonitorTaskbarState::Desktop;
        }

        // Check window rect against monitor bounds
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(foreground_hwnd, &mut rect) }.is_err() {
            return MonitorTaskbarState::Desktop;
        }

        // Evaluate fullscreen
        if is_fullscreen_rect(&rect, monitor) {
            return MonitorTaskbarState::Fullscreen;
        }

        // Evaluate maximized
        if unsafe { IsZoomed(foreground_hwnd).as_bool() } || is_maximized_rect(&rect, monitor) {
            return MonitorTaskbarState::WindowMaximized;
        }

        // Normal window on desktop
        MonitorTaskbarState::Desktop
    }

    /// Evaluates rect bounds against monitor dimensions (pure calculation for testing).
    pub fn evaluate_rect_state(
        rect: &RECT,
        monitor: &MonitorInfo,
        is_zoomed: bool,
    ) -> MonitorTaskbarState {
        if is_fullscreen_rect(rect, monitor) {
            MonitorTaskbarState::Fullscreen
        } else if is_zoomed || is_maximized_rect(rect, monitor) {
            MonitorTaskbarState::WindowMaximized
        } else {
            MonitorTaskbarState::Desktop
        }
    }
}

/// Retrieves the Win32 class name for a window safely.
pub fn get_window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

/// Checks if a window class corresponds to a known flyout or shell menu.
pub fn is_flyout_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Windows.UI.Core.CoreWindow"
            | "Xaml_WindowedPopupClass"
            | "TopLevelWindowForOverflowXamlIsland"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
    )
}

/// Checks whether a window is DWM-cloaked (hidden on other virtual desktops, suspended UWP, etc.).
pub fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let res = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if res.is_ok() {
        cloaked != 0
    } else {
        false
    }
}

/// Checks whether a window rectangle spans the entire physical monitor.
pub fn is_fullscreen_rect(rect: &RECT, monitor: &MonitorInfo) -> bool {
    // Window bounds must cover or exceed monitor bounds (with +/- 2px tolerance for borders)
    rect.left <= (monitor.bounds[0] + 2)
        && rect.top <= (monitor.bounds[1] + 2)
        && rect.right >= (monitor.bounds[2] - 2)
        && rect.bottom >= (monitor.bounds[3] - 2)
}

/// Checks whether a window rectangle spans the monitor work area (maximized).
pub fn is_maximized_rect(rect: &RECT, monitor: &MonitorInfo) -> bool {
    // Window bounds match the work area bounds
    let left_diff = (rect.left - monitor.work_area[0]).abs();
    let top_diff = (rect.top - monitor.work_area[1]).abs();
    let right_diff = (rect.right - monitor.work_area[2]).abs();
    let bottom_diff = (rect.bottom - monitor.work_area[3]).abs();

    left_diff <= 10 && top_diff <= 10 && right_diff <= 10 && bottom_diff <= 10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_monitor() -> MonitorInfo {
        MonitorInfo {
            hmonitor: 1,
            is_primary: true,
            bounds: [0, 0, 1920, 1080],
            work_area: [0, 0, 1920, 1040], // 40px taskbar
        }
    }

    #[test]
    fn test_evaluate_desktop_state_for_normal_window() {
        let monitor = make_test_monitor();
        let normal_rect = RECT {
            left: 100,
            top: 100,
            right: 800,
            bottom: 600,
        };

        let state = WindowStateEvaluator::evaluate_rect_state(&normal_rect, &monitor, false);
        assert_eq!(state, MonitorTaskbarState::Desktop);
    }

    #[test]
    fn test_evaluate_maximized_state() {
        let monitor = make_test_monitor();
        let normal_rect = RECT {
            left: 100,
            top: 100,
            right: 800,
            bottom: 600,
        };

        // If IsZoomed is true
        let state = WindowStateEvaluator::evaluate_rect_state(&normal_rect, &monitor, true);
        assert_eq!(state, MonitorTaskbarState::WindowMaximized);

        // If rect matches work area
        let max_rect = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let state2 = WindowStateEvaluator::evaluate_rect_state(&max_rect, &monitor, false);
        assert_eq!(state2, MonitorTaskbarState::WindowMaximized);
    }

    #[test]
    fn test_evaluate_fullscreen_state() {
        let monitor = make_test_monitor();
        let fs_rect = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };

        let state = WindowStateEvaluator::evaluate_rect_state(&fs_rect, &monitor, false);
        assert_eq!(state, MonitorTaskbarState::Fullscreen);
    }

    #[test]
    fn test_flyout_class_detection() {
        assert!(is_flyout_class("Windows.UI.Core.CoreWindow"));
        assert!(is_flyout_class("Xaml_WindowedPopupClass"));
        assert!(is_flyout_class("TopLevelWindowForOverflowXamlIsland"));
        assert!(is_flyout_class("Shell_TrayWnd"));
        assert!(!is_flyout_class("Notepad"));
        assert!(!is_flyout_class("Chrome_WidgetWin_1"));
    }
}
