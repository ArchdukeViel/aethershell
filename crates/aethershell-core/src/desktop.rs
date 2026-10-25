use serde::{Deserialize, Serialize};

/// Stacking layer specification matching PRD Section 5.1:
/// Top: Normal apps -> Desktop Icons -> AetherShell Widgets -> Wallpaper
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DesktopLayer {
    Wallpaper = 0,
    AetherShellWidgets = 1,
    DesktopIcons = 2,
    ApplicationWindows = 3,
}

/// Interaction mode state matching PRD Section 5.3 (Q31:A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    /// Locked/passive default. Widget visual surface is below icons and never captures input.
    #[default]
    LockedPassive,
    /// Deliberately opened temporary topmost-to-desktop interaction surface with timeout/escape.
    TemporaryInteractive { timeout_seconds: u32 },
    /// Visual editing mode showing widget handles, snapping guides, and layout nudges.
    EditLayout,
}

/// Anchor point for device-independent pixel (DIP) layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    Center,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Device-Independent Pixel geometry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DipRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DipRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= (self.x + self.width) && py >= self.y && py <= (self.y + self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_stacking_order() {
        assert!(DesktopLayer::Wallpaper < DesktopLayer::AetherShellWidgets);
        assert!(DesktopLayer::AetherShellWidgets < DesktopLayer::DesktopIcons);
        assert!(DesktopLayer::DesktopIcons < DesktopLayer::ApplicationWindows);
    }

    #[test]
    fn test_dip_rect_bounds() {
        let rect = DipRect::new(10.0, 20.0, 100.0, 50.0);
        assert!(rect.contains(15.0, 25.0));
        assert!(rect.contains(10.0, 20.0));
        assert!(rect.contains(110.0, 70.0));
        assert!(!rect.contains(5.0, 25.0));
        assert!(!rect.contains(15.0, 75.0));
    }
}
