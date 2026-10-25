use serde::{Deserialize, Serialize};
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowRect, IsWindowVisible,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskbarWindowInfo {
    pub is_primary: bool,
    pub hwnd: usize,
    pub visible: bool,
    pub rect: [i32; 4], // [left, top, right, bottom]
}

/// Probes the system for primary and secondary taskbars without modifying any state.
pub fn probe_taskbars() -> Vec<TaskbarWindowInfo> {
    let mut results = Vec::new();

    unsafe {
        // Primary Taskbar: "Shell_TrayWnd"
        let primary_hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
        if let Ok(hwnd) = primary_hwnd {
            if !hwnd.0.is_null() {
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let visible = IsWindowVisible(hwnd).as_bool();

                results.push(TaskbarWindowInfo {
                    is_primary: true,
                    hwnd: hwnd.0 as usize,
                    visible,
                    rect: [rect.left, rect.top, rect.right, rect.bottom],
                });
            }
        }

        // Secondary Taskbars: "Shell_SecondaryTrayWnd"
        let mut prev = HWND::default();
        while let Ok(secondary_hwnd) =
            FindWindowExW(HWND::default(), prev, w!("Shell_SecondaryTrayWnd"), None)
        {
            if secondary_hwnd.0.is_null() {
                break;
            }
            let mut rect = RECT::default();
            let _ = GetWindowRect(secondary_hwnd, &mut rect);
            let visible = IsWindowVisible(secondary_hwnd).as_bool();

            results.push(TaskbarWindowInfo {
                is_primary: false,
                hwnd: secondary_hwnd.0 as usize,
                visible,
                rect: [rect.left, rect.top, rect.right, rect.bottom],
            });
            prev = secondary_hwnd;
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taskbar_probing_safe_execution() {
        let taskbars = probe_taskbars();
        // Should execute cleanly without panics or crashes on Windows
        // In an interactive desktop session, primary taskbar is normally found
        println!("Discovered {} taskbars", taskbars.len());
    }
}
