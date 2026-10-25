use aethershell_core::InteractionMode;
use chrono::{DateTime, Duration, Utc};

/// Reason why an interaction session was dismissed (PRD Section 5.3 / Q31:A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DismissalReason {
    EscapeKey,
    Timeout,
    FocusLost,
    ClickOutside,
    SessionLocked,
    FullScreenShield,
    ExplicitExit,
    SafeMode,
}

/// Manages interactive state transitions, timers, and automatic dismissals.
pub struct InteractionController {
    current_mode: InteractionMode,
    session_start: Option<DateTime<Utc>>,
    session_deadline: Option<DateTime<Utc>>,
    last_dismissal: Option<DismissalReason>,
}

impl InteractionController {
    pub fn new() -> Self {
        Self {
            current_mode: InteractionMode::LockedPassive,
            session_start: None,
            session_deadline: None,
            last_dismissal: None,
        }
    }

    pub fn current_mode(&self) -> InteractionMode {
        self.current_mode
    }

    pub fn last_dismissal(&self) -> Option<DismissalReason> {
        self.last_dismissal
    }

    /// User enters temporary interactive mode with configured timeout in seconds.
    pub fn enter_temporary_mode(&mut self, timeout_seconds: u32) {
        let now = Utc::now();
        self.current_mode = InteractionMode::TemporaryInteractive { timeout_seconds };
        self.session_start = Some(now);
        self.session_deadline = Some(now + Duration::seconds(timeout_seconds as i64));
        self.last_dismissal = None;
    }

    /// User enters visual layout editing mode.
    pub fn enter_edit_mode(&mut self) {
        self.current_mode = InteractionMode::EditLayout;
        self.session_start = Some(Utc::now());
        self.session_deadline = None;
        self.last_dismissal = None;
    }

    /// Returns to locked/passive default with an explicit reason.
    pub fn dismiss(&mut self, reason: DismissalReason) {
        self.current_mode = InteractionMode::LockedPassive;
        self.session_start = None;
        self.session_deadline = None;
        self.last_dismissal = Some(reason);
    }

    /// Returns to locked/passive default.
    pub fn dismiss_to_passive(&mut self) {
        self.dismiss(DismissalReason::ExplicitExit);
    }

    /// Handles keyboard input; dismisses immediately on Escape (VK_ESCAPE = 0x1B).
    pub fn handle_key_down(&mut self, vk_code: u32) -> bool {
        if vk_code == 0x1B {
            // Escape key
            if self.current_mode != InteractionMode::LockedPassive {
                self.dismiss(DismissalReason::EscapeKey);
                return true;
            }
        }
        false
    }

    /// Handles focus loss when user clicks away or switches applications.
    pub fn handle_focus_loss(&mut self) -> bool {
        if self.current_mode != InteractionMode::LockedPassive {
            self.dismiss(DismissalReason::FocusLost);
            return true;
        }
        false
    }

    /// Handles clicks outside active interactive surface boundaries.
    pub fn handle_click_outside(&mut self) -> bool {
        if let InteractionMode::TemporaryInteractive { .. } = self.current_mode {
            self.dismiss(DismissalReason::ClickOutside);
            return true;
        }
        false
    }

    /// Handles session lock signal (Win+L / screensaver).
    pub fn handle_session_lock(&mut self) -> bool {
        if self.current_mode != InteractionMode::LockedPassive {
            self.dismiss(DismissalReason::SessionLocked);
            return true;
        }
        false
    }

    /// Handles full-screen gaming shield activation.
    pub fn handle_fullscreen_shield(&mut self) -> bool {
        if self.current_mode != InteractionMode::LockedPassive {
            self.dismiss(DismissalReason::FullScreenShield);
            return true;
        }
        false
    }

    /// Periodic tick to check for automatic timeout expiration.
    pub fn tick(&mut self, now: DateTime<Utc>) -> bool {
        if let InteractionMode::TemporaryInteractive { .. } = self.current_mode {
            if let Some(deadline) = self.session_deadline {
                if now >= deadline {
                    self.dismiss(DismissalReason::Timeout);
                    return true; // Dismissed
                }
            }
        }
        false
    }
}

impl Default for InteractionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interaction_controller_timeout_lifecycle() {
        let mut controller = InteractionController::new();
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);

        controller.enter_temporary_mode(5);
        assert!(matches!(
            controller.current_mode(),
            InteractionMode::TemporaryInteractive { .. }
        ));

        let start = controller.session_start.unwrap();
        // Tick at +2 seconds (not expired)
        let dismissed = controller.tick(start + Duration::seconds(2));
        assert!(!dismissed);
        assert!(matches!(
            controller.current_mode(),
            InteractionMode::TemporaryInteractive { .. }
        ));

        // Tick at +6 seconds (expired)
        let dismissed = controller.tick(start + Duration::seconds(6));
        assert!(dismissed);
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(controller.last_dismissal(), Some(DismissalReason::Timeout));
    }

    #[test]
    fn test_interaction_controller_escape_key_dismissal() {
        let mut controller = InteractionController::new();
        controller.enter_temporary_mode(10);

        // Press unrelated key (e.g. 0x20 Space)
        assert!(!controller.handle_key_down(0x20));
        assert!(matches!(
            controller.current_mode(),
            InteractionMode::TemporaryInteractive { .. }
        ));

        // Press Escape key (0x1B)
        assert!(controller.handle_key_down(0x1B));
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(
            controller.last_dismissal(),
            Some(DismissalReason::EscapeKey)
        );
    }

    #[test]
    fn test_interaction_controller_focus_loss_dismissal() {
        let mut controller = InteractionController::new();
        controller.enter_temporary_mode(10);

        assert!(controller.handle_focus_loss());
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(
            controller.last_dismissal(),
            Some(DismissalReason::FocusLost)
        );
    }

    #[test]
    fn test_interaction_controller_session_lock_dismissal() {
        let mut controller = InteractionController::new();
        controller.enter_edit_mode();

        assert!(controller.handle_session_lock());
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(
            controller.last_dismissal(),
            Some(DismissalReason::SessionLocked)
        );
    }
}
