use aethershell_core::DesktopLayer;
use aethershell_win::{probe_desktop_hierarchy, DesktopHierarchyProbe};
use thiserror::Error;
use windows::Win32::UI::WindowsAndMessaging::{
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
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

/// The authoritative 4-tier desktop stacking order (PRD Section 5.1).
///
/// Order from bottom to top:
/// 1. Wallpaper: Static wallpaper or live/animated wallpaper host (`WorkerW` / `Progman`).
/// 2. AetherShellWidgets: Passive desktop widget surface (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`).
/// 3. DesktopIcons: Windows desktop icons (`SHELLDLL_DefView` / `SysListView32`).
/// 4. NormalAppWindows: Ordinary application windows, taskbar, and full-screen games (`HWND_NOTOPMOST`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DesktopStackingTier {
    /// Bottom tier: Static wallpaper or live/animated wallpaper host (`WorkerW` / `Progman`).
    Wallpaper = 0,
    /// Second tier: AetherShell desktop widget surface (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`).
    AetherShellWidgets = 1,
    /// Third tier: Windows desktop icons (`SHELLDLL_DefView` / `SysListView32`).
    DesktopIcons = 2,
    /// Top tier: Normal application windows, taskbar, and full-screen games (`HWND_NOTOPMOST`).
    NormalAppWindows = 3,
}

impl DesktopStackingTier {
    /// Returns a human-readable display name for this stacking tier.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Wallpaper => "Wallpaper",
            Self::AetherShellWidgets => "AetherShell Widgets",
            Self::DesktopIcons => "Desktop Icons (SHELLDLL_DefView)",
            Self::NormalAppWindows => "Normal Application Windows",
        }
    }
}

/// Detailed diagnostic verdict of desktop layering feasibility on a target display (PRD Section 5.1, Section 5.2, Gate G06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopLayeringVerdict {
    /// Safe stacking order established: wallpaper < widgets < desktop icons.
    StackingVerified {
        shelldll_defview_found: bool,
        worker_w_found: bool,
        progman_found: bool,
        stacking_tier: DesktopStackingTier,
    },
    /// Hierarchy cannot be established safely: widgets disabled on this monitor with honest reason per PRD 5.1 / F44.
    Disabled { monitor_id: String, reason: String },
}

impl DesktopLayeringVerdict {
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::StackingVerified { .. })
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled { .. })
    }
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

    /// Evaluates detailed stacking verdict for a specific monitor (PRD Section 5.1, Section 5.2, Gate G06).
    pub fn evaluate_stacking_verdict(&self, monitor_id: &str) -> DesktopLayeringVerdict {
        if !self.is_layering_available {
            DesktopLayeringVerdict::Disabled {
                monitor_id: monitor_id.to_string(),
                reason: format!(
                    "SHELLDLL_DefView hierarchy not located on {}; widget layer disabled to prevent obscuring icons per PRD Section 5.1 / F44",
                    monitor_id
                ),
            }
        } else {
            DesktopLayeringVerdict::StackingVerified {
                shelldll_defview_found: self.probe.shelldll_defview_hwnd.is_some(),
                worker_w_found: self.probe.worker_w_hwnd.is_some(),
                progman_found: self.probe.progman_hwnd.is_some(),
                stacking_tier: DesktopStackingTier::AetherShellWidgets,
            }
        }
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
            (WS_EX_TOOLWINDOW | WS_EX_LAYERED).0
        } else {
            (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED).0
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
        assert!((passive_styles & WS_EX_LAYERED.0) != 0);

        let interactive_styles = DesktopHost::surface_extended_styles(true);
        assert!((interactive_styles & WS_EX_TRANSPARENT.0) == 0);
        assert!((interactive_styles & WS_EX_NOACTIVATE.0) == 0);
        assert!((interactive_styles & WS_EX_TOOLWINDOW.0) != 0);
        assert!((interactive_styles & WS_EX_LAYERED.0) != 0);
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

    #[test]
    fn test_desktop_stacking_tier_ordering_and_names() {
        assert!(DesktopStackingTier::Wallpaper < DesktopStackingTier::AetherShellWidgets);
        assert!(DesktopStackingTier::AetherShellWidgets < DesktopStackingTier::DesktopIcons);
        assert!(DesktopStackingTier::DesktopIcons < DesktopStackingTier::NormalAppWindows);

        assert_eq!(DesktopStackingTier::Wallpaper.name(), "Wallpaper");
        assert_eq!(
            DesktopStackingTier::AetherShellWidgets.name(),
            "AetherShell Widgets"
        );
        assert_eq!(
            DesktopStackingTier::DesktopIcons.name(),
            "Desktop Icons (SHELLDLL_DefView)"
        );
        assert_eq!(
            DesktopStackingTier::NormalAppWindows.name(),
            "Normal Application Windows"
        );
    }

    #[test]
    fn test_evaluate_stacking_verdict_verified_and_disabled() {
        // 1. Available hierarchy
        let verified_host = DesktopHost {
            probe: DesktopHierarchyProbe {
                progman_hwnd: Some(100),
                worker_w_hwnd: Some(200),
                shelldll_defview_hwnd: Some(300),
                syslistview32_hwnd: Some(400),
            },
            target_layer: DesktopLayer::AetherShellWidgets,
            is_layering_available: true,
        };
        let verdict = verified_host.evaluate_stacking_verdict("monitor-0");
        assert!(verdict.is_verified());
        assert!(!verdict.is_disabled());
        match verdict {
            DesktopLayeringVerdict::StackingVerified {
                shelldll_defview_found,
                worker_w_found,
                progman_found,
                stacking_tier,
            } => {
                assert!(shelldll_defview_found);
                assert!(worker_w_found);
                assert!(progman_found);
                assert_eq!(stacking_tier, DesktopStackingTier::AetherShellWidgets);
            }
            _ => panic!("Expected StackingVerified"),
        }

        // 2. Disabled hierarchy (F44)
        let disabled_host = DesktopHost {
            probe: DesktopHierarchyProbe {
                progman_hwnd: None,
                worker_w_hwnd: None,
                shelldll_defview_hwnd: None,
                syslistview32_hwnd: None,
            },
            target_layer: DesktopLayer::AetherShellWidgets,
            is_layering_available: false,
        };
        let disabled_verdict = disabled_host.evaluate_stacking_verdict("monitor-1");
        assert!(disabled_verdict.is_disabled());
        assert!(!disabled_verdict.is_verified());
        match disabled_verdict {
            DesktopLayeringVerdict::Disabled { monitor_id, reason } => {
                assert_eq!(monitor_id, "monitor-1");
                assert!(reason.contains("SHELLDLL_DefView hierarchy not located"));
            }
            _ => panic!("Expected Disabled"),
        }
    }
}
