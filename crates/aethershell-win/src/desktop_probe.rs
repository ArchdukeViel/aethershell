use serde::{Deserialize, Serialize};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopHierarchyProbe {
    pub progman_hwnd: Option<usize>,
    pub shelldll_defview_hwnd: Option<usize>,
    pub syslistview32_hwnd: Option<usize>,
    pub worker_w_hwnd: Option<usize>,
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
        let probe = probe_desktop_hierarchy();
        println!("Desktop probe: {:?}", probe);
        // Ensure no crash or undefined behavior occurs
    }
}
