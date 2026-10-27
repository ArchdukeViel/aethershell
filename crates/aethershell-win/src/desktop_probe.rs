use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, IsWindow, RegisterWindowMessageW,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopHierarchyProbe {
    pub progman_hwnd: Option<usize>,
    pub shelldll_defview_hwnd: Option<usize>,
    pub syslistview32_hwnd: Option<usize>,
    pub worker_w_hwnd: Option<usize>,
}

impl DesktopHierarchyProbe {
    /// Non-invasively probes the Windows desktop shell hierarchy.
    pub fn probe() -> Self {
        probe_desktop_hierarchy()
    }

    /// Returns whether the discovered hierarchy is viable for passive widget layering.
    pub fn is_layering_viable(&self) -> bool {
        self.shelldll_defview_hwnd.is_some()
    }

    /// Verifies whether any previously recorded shell window handle has become stale or invalid.
    pub fn is_stale(&self) -> bool {
        unsafe {
            if let Some(h) = self.progman_hwnd {
                if !IsWindow(HWND(h as *mut _)).as_bool() {
                    return true;
                }
            }
            if let Some(h) = self.shelldll_defview_hwnd {
                if !IsWindow(HWND(h as *mut _)).as_bool() {
                    return true;
                }
            }
            if let Some(h) = self.worker_w_hwnd {
                if !IsWindow(HWND(h as *mut _)).as_bool() {
                    return true;
                }
            }
            false
        }
    }
}

/// Registers or retrieves the dynamic Windows message `TaskbarCreated` broadcast when Explorer starts or restarts.
pub fn register_taskbar_created_message() -> u32 {
    static CACHED_MSG: AtomicU32 = AtomicU32::new(0);
    let cached = CACHED_MSG.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    unsafe {
        let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        CACHED_MSG.store(msg, Ordering::Relaxed);
        msg
    }
}

/// Discovers the Windows desktop shell hierarchy non-invasively.
///
/// Implements PRD Section 5.2 and Appendix B:
/// Probes `Progman` -> `SHELLDLL_DefView` -> `SysListView32`.
/// If `WorkerW` is not found, returns `None` honestly without fabricating handles.
pub fn probe_desktop_hierarchy() -> DesktopHierarchyProbe {
    let mut probe = DesktopHierarchyProbe {
        progman_hwnd: None,
        shelldll_defview_hwnd: None,
        syslistview32_hwnd: None,
        worker_w_hwnd: None,
    };

    unsafe {
        // Find Progman
        if let Ok(progman) = FindWindowW(w!("Progman"), None) {
            if !progman.0.is_null() {
                probe.progman_hwnd = Some(progman.0 as usize);

                // Child SHELLDLL_DefView
                if let Ok(defview) =
                    FindWindowExW(progman, HWND::default(), w!("SHELLDLL_DefView"), None)
                {
                    if !defview.0.is_null() {
                        probe.shelldll_defview_hwnd = Some(defview.0 as usize);

                        // Child SysListView32 (desktop icons)
                        if let Ok(listview) =
                            FindWindowExW(defview, HWND::default(), w!("SysListView32"), None)
                        {
                            if !listview.0.is_null() {
                                probe.syslistview32_hwnd = Some(listview.0 as usize);
                            }
                        }
                    }
                }
            }
        }

        // Search for WorkerW holding SHELLDLL_DefView if not found under Progman
        if probe.shelldll_defview_hwnd.is_none() {
            let mut current = HWND::default();
            while let Ok(worker) = FindWindowExW(HWND::default(), current, w!("WorkerW"), None) {
                if worker.0.is_null() {
                    break;
                }
                if let Ok(defview) =
                    FindWindowExW(worker, HWND::default(), w!("SHELLDLL_DefView"), None)
                {
                    if !defview.0.is_null() {
                        probe.worker_w_hwnd = Some(worker.0 as usize);
                        probe.shelldll_defview_hwnd = Some(defview.0 as usize);

                        if let Ok(listview) =
                            FindWindowExW(defview, HWND::default(), w!("SysListView32"), None)
                        {
                            if !listview.0.is_null() {
                                probe.syslistview32_hwnd = Some(listview.0 as usize);
                            }
                        }
                        break;
                    }
                }
                current = worker;
            }
        }
    }

    probe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_probing_safe_execution() {
        let probe = DesktopHierarchyProbe::probe();
        println!("Desktop probe: {:?}", probe);
        // Method matches standalone function
        assert_eq!(probe, probe_desktop_hierarchy());
    }

    #[test]
    fn test_desktop_probe_stale_handle_detection() {
        // A probe with non-existent fake HWNDs should be detected as stale
        let stale_probe = DesktopHierarchyProbe {
            progman_hwnd: Some(0xDEADBEEF),
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
            worker_w_hwnd: None,
        };
        assert!(stale_probe.is_stale());

        // An empty probe is not stale (no recorded handles to be invalid)
        let empty_probe = DesktopHierarchyProbe {
            progman_hwnd: None,
            shelldll_defview_hwnd: None,
            syslistview32_hwnd: None,
            worker_w_hwnd: None,
        };
        assert!(!empty_probe.is_stale());
    }

    #[test]
    fn test_taskbar_created_message_registration() {
        let msg = register_taskbar_created_message();
        // Dynamic window messages registered via RegisterWindowMessageW are in range 0xC000..=0xFFFF
        assert!(
            msg >= 0xC000,
            "TaskbarCreated message ID must be in dynamic message range >= 0xC000, got: 0x{:04X}",
            msg
        );
        // Subsequent calls return identical cached value
        assert_eq!(register_taskbar_created_message(), msg);
    }
}
