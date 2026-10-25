use serde::{Deserialize, Serialize};

/// High-level lifecycle state for AetherShell core and services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Initializing,
    Running,
    Suspended,
    SafeMode,
    ShuttingDown,
    Terminated,
}

/// Recovery precedence order matching PRD Section 3.4:
/// safe_mode > system_accessibility > fullscreen_gaming_shield > manual_user_disable > power_saving > per-app_rule > window_state > default_theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RecoveryPrecedence {
    DefaultTheme = 0,
    WindowState = 1,
    PerAppRule = 2,
    PowerSaving = 3,
    ManualUserDisable = 4,
    FullscreenGamingShield = 5,
    SystemAccessibility = 6,
    SafeMode = 7,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precedence_ordering() {
        assert!(RecoveryPrecedence::SafeMode > RecoveryPrecedence::SystemAccessibility);
        assert!(
            RecoveryPrecedence::SystemAccessibility > RecoveryPrecedence::FullscreenGamingShield
        );
        assert!(RecoveryPrecedence::FullscreenGamingShield > RecoveryPrecedence::ManualUserDisable);
        assert!(RecoveryPrecedence::ManualUserDisable > RecoveryPrecedence::PowerSaving);
        assert!(RecoveryPrecedence::PowerSaving > RecoveryPrecedence::PerAppRule);
        assert!(RecoveryPrecedence::PerAppRule > RecoveryPrecedence::WindowState);
        assert!(RecoveryPrecedence::WindowState > RecoveryPrecedence::DefaultTheme);
    }
}
