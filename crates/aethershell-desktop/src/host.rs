use aethershell_core::DesktopLayer;
use aethershell_win::{probe_desktop_hierarchy, DesktopHierarchyProbe};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum DesktopHostError {
    #[error("Desktop icon view (SHELLDLL_DefView) not found")]
    ShellDllDefViewNotFound,
    #[error("Unsupported desktop shell hierarchy")]
    UnsupportedHierarchy,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_host_discovery() {
        let host = DesktopHost::discover();
        assert_eq!(host.target_layer, DesktopLayer::AetherShellWidgets);
        println!(
            "Desktop host layering available: {}",
            host.is_layering_available
        );
    }
}
