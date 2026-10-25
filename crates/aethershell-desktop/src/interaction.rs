use aethershell_core::InteractionMode;
use chrono::{DateTime, Duration, Utc};

/// Manages interactive state transitions, timers, and automatic dismissals.
pub struct InteractionController {
    current_mode: InteractionMode,
    session_start: Option<DateTime<Utc>>,
    session_deadline: Option<DateTime<Utc>>,
}

impl InteractionController {
    pub fn new() -> Self {
        Self {
            current_mode: InteractionMode::LockedPassive,
            session_start: None,
            session_deadline: None,
        }
    }

    pub fn current_mode(&self) -> InteractionMode {
        self.current_mode
    }

    /// User enters temporary interactive mode with configured timeout in seconds.
    pub fn enter_temporary_mode(&mut self, timeout_seconds: u32) {
        let now = Utc::now();
        self.current_mode = InteractionMode::TemporaryInteractive { timeout_seconds };
        self.session_start = Some(now);
        self.session_deadline = Some(now + Duration::seconds(timeout_seconds as i64));
    }

    /// User enters visual layout editing mode.
    pub fn enter_edit_mode(&mut self) {
        self.current_mode = InteractionMode::EditLayout;
        self.session_start = Some(Utc::now());
        self.session_deadline = None;
    }

    /// Returns to locked/passive default.
    pub fn dismiss_to_passive(&mut self) {
        self.current_mode = InteractionMode::LockedPassive;
        self.session_start = None;
        self.session_deadline = None;
    }

    /// Periodic tick to check for automatic timeout expiration.
    pub fn tick(&mut self, now: DateTime<Utc>) -> bool {
        if let InteractionMode::TemporaryInteractive { .. } = self.current_mode {
            if let Some(deadline) = self.session_deadline {
                if now >= deadline {
                    self.dismiss_to_passive();
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
    }
}
