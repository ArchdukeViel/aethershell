use aethershell_core::DesktopLayer;
use aethershell_win::{probe_desktop_hierarchy, DesktopHierarchyProbe};
use thiserror::Error;
use windows::Win32::UI::WindowsAndMessaging::{
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

#[derive(Debug, Error, PartialEq)]
pub enum DesktopHostError {
    #[error("Desktop icon view (SHELLDLL_DefView) not found")]
    ShellDllDefViewNotFound,
    #[error("Unsupported desktop shell hierarchy")]
    UnsupportedHierarchy,
    #[error("Layering disabled on monitor {0}: {1}")]
    LayeringDisabled(String, String),
    #[error("Window creation failed: {0}")]
    WindowCreationFailed(String),
}

/// Discovered desktop host context with validated stacking targets.
#[derive(Debug, Clone)]
pub struct DesktopHost {
    pub probe: DesktopHierarchyProbe,
    pub target_layer: DesktopLayer,
    pub is_layering_available: bool,
}

impl DesktopHost {
    /// Discovers desktop host windows and validates stacking feasibility.
    pub fn discover() -> Self {
        let probe = probe_desktop_hierarchy();

        // Layering is viable if SHELLDLL_DefView is located
        let is_layering_available = probe.shelldll_defview_hwnd.is_some();

        Self {
            probe,
            target_layer: DesktopLayer::AetherShellWidgets,
            is_layering_available,
        }
    }

    /// Validates stacking feasibility for a specific monitor.
    ///
    /// Per PRD Section 5.1: If the stack cannot be constructed safely, widgets
    /// are disabled on that monitor with an explanation rather than moved above normal apps.
    pub fn validate_monitor_stacking(&self, monitor_id: &str) -> Result<(), DesktopHostError> {
        if !self.is_layering_available {
            return Err(DesktopHostError::LayeringDisabled(
                monitor_id.to_string(),
                "SHELLDLL_DefView hierarchy not located; widget layer disabled to prevent obscuring icons per PRD 5.1".into(),
            ));
        }
        Ok(())
    }

    /// Computes Win32 extended window styles for passive vs interactive surfaces.
    ///
    /// Passive surfaces use `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` so pointer clicks fall
    /// directly through to desktop icons (`SHELLDLL_DefView` / `SysListView32`).
    ///
    /// Interactive surfaces stay beneath normal application windows per PRD Section 5.3 & Q31:A,
    /// so `WS_EX_TOPMOST` is strictly prohibited.
    pub fn surface_extended_styles(is_interactive: bool) -> u32 {
        if is_interactive {
            WS_EX_TOOLWINDOW.0
        } else {
            (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT).0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST;

    #[test]
    fn test_desktop_host_discovery() {
        let host = DesktopHost::discover();
        assert_eq!(host.target_layer, DesktopLayer::AetherShellWidgets);
        println!(
            "Desktop host layering available: {}",
            host.is_layering_available
        );
    }

    #[test]
    fn test_surface_extended_styles() {
        let passive_styles = DesktopHost::surface_extended_styles(false);
        assert!((passive_styles & WS_EX_TRANSPARENT.0) != 0);
        assert!((passive_styles & WS_EX_NOACTIVATE.0) != 0);
        assert!((passive_styles & WS_EX_TOOLWINDOW.0) != 0);

        let interactive_styles = DesktopHost::surface_extended_styles(true);
        assert!((interactive_styles & WS_EX_TRANSPARENT.0) == 0);
        assert!((interactive_styles & WS_EX_NOACTIVATE.0) == 0);
        assert!((interactive_styles & WS_EX_TOOLWINDOW.0) != 0);
        // Eliminate WS_EX_TOPMOST defect per PRD Section 5.3 & Q31:A
        assert_eq!(
            interactive_styles & WS_EX_TOPMOST.0,
            0,
            "Interactive surface must NOT have WS_EX_TOPMOST"
        );
    }

    #[test]
    fn test_stacking_validation_failure_handling() {
        let host = DesktopHost {
            probe: DesktopHierarchyProbe {
                progman_hwnd: None,
                worker_w_hwnd: None,
                shelldll_defview_hwnd: None,
                syslistview32_hwnd: None,
            },
            target_layer: DesktopLayer::AetherShellWidgets,
            is_layering_available: false,
        };

        let res = host.validate_monitor_stacking("mon-1");
        assert!(res.is_err());
        match res.unwrap_err() {
            DesktopHostError::LayeringDisabled(mon, reason) => {
                assert_eq!(mon, "mon-1");
                assert!(reason.contains("SHELLDLL_DefView hierarchy not located"));
            }
            _ => panic!("Expected LayeringDisabled"),
        }
    }
}
