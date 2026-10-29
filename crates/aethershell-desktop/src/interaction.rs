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

/// A bounded interactive region on the desktop host surface (PRD Section 5.3 & Q31:A).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveRegion {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub action: Option<String>,
    #[serde(default)]
    pub accessible_name: Option<String>,
    #[serde(default)]
    pub accessible_role: Option<String>,
    #[serde(default)]
    pub accessible_value: Option<String>,
}

impl InteractiveRegion {
    pub fn new(
        id: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        action: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            width,
            height,
            action,
            accessible_name: None,
            accessible_role: None,
            accessible_value: None,
        }
    }

    /// Attaches accessibility metadata for Screen Readers / Narrator (Gate G14).
    pub fn with_accessibility(
        mut self,
        name: impl Into<String>,
        role: impl Into<String>,
        value: Option<String>,
    ) -> Self {
        self.accessible_name = Some(name.into());
        self.accessible_role = Some(role.into());
        self.accessible_value = value;
        self
    }

    /// Generates a human- and screen-reader-friendly accessible description of this widget region.
    pub fn accessible_description(&self) -> String {
        let name = self.accessible_name.as_deref().unwrap_or(self.id.as_str());
        let role = self
            .accessible_role
            .as_deref()
            .unwrap_or(if self.action.is_some() {
                "button"
            } else {
                "widget"
            });

        match (&self.accessible_value, &self.action) {
            (Some(val), Some(act)) => {
                format!("{}: {}, role: {}, action: {}", name, val, role, act)
            }
            (Some(val), None) => format!("{}: {}, role: {}", name, val, role),
            (None, Some(act)) => format!("{} {}, action: {}", name, role, act),
            (None, None) => format!("{} {}", name, role),
        }
    }

    /// Determines whether a 2D pointer coordinate falls inside this region's bounding box.
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= (self.x + self.width) && py >= self.y && py <= (self.y + self.height)
    }
}

/// The result of a keyboard navigation action across desktop interactive widgets (Gate G14).
#[derive(Debug, Clone, PartialEq)]
pub enum KeyboardNavigationResult {
    /// Escape dismissed interactive mode back to passive desktop icons.
    Dismissed(DismissalReason),
    /// Focus moved to a widget region (Tab / Shift+Tab / Arrows).
    FocusChanged { index: usize, description: String },
    /// Focused widget was activated (Enter / Space).
    Activated { action: String },
    /// Key ignored or not applicable.
    Ignored,
}

/// The result of a spatial pointer hit-test dispatch in interactive mode.
#[derive(Debug, Clone, PartialEq)]
pub enum PointerHitResult {
    /// Pointer clicked on an active interactive widget region.
    Hit { region: InteractiveRegion },
    /// Pointer clicked outside all interactive widget regions; session dismissed.
    MissClickOutside,
    /// Host overlay is in locked/passive mode; pointer events fall through to desktop icons.
    IgnoredPassive,
}

/// Manages interactive state transitions, timers, keyboard navigation, and automatic dismissals.
pub struct InteractionController {
    current_mode: InteractionMode,
    session_start: Option<DateTime<Utc>>,
    session_deadline: Option<DateTime<Utc>>,
    last_dismissal: Option<DismissalReason>,
    focused_region_index: Option<usize>,
    pending_announcements: Vec<String>,
}

impl InteractionController {
    pub fn new() -> Self {
        Self {
            current_mode: InteractionMode::LockedPassive,
            session_start: None,
            session_deadline: None,
            last_dismissal: None,
            focused_region_index: None,
            pending_announcements: Vec::new(),
        }
    }

    pub fn current_mode(&self) -> InteractionMode {
        self.current_mode
    }

    pub fn last_dismissal(&self) -> Option<DismissalReason> {
        self.last_dismissal
    }

    pub fn focused_region_index(&self) -> Option<usize> {
        self.focused_region_index
    }

    pub fn focused_region<'a>(
        &self,
        regions: &'a [InteractiveRegion],
    ) -> Option<&'a InteractiveRegion> {
        self.focused_region_index.and_then(|idx| regions.get(idx))
    }

    pub fn set_focus(&mut self, index: Option<usize>, regions: &[InteractiveRegion]) {
        self.focused_region_index = index.filter(|&i| i < regions.len());
        if let Some(r) = self.focused_region(regions) {
            self.pending_announcements.push(r.accessible_description());
        }
    }

    /// Moves focus to next widget region (Tab / Right / Down). Wraps around.
    pub fn focus_next(&mut self, regions: &[InteractiveRegion]) -> Option<usize> {
        if regions.is_empty() {
            self.focused_region_index = None;
            return None;
        }
        let next_idx = match self.focused_region_index {
            Some(curr) => (curr + 1) % regions.len(),
            None => 0,
        };
        self.focused_region_index = Some(next_idx);
        let desc = format!(
            "Focused: {} ({}/{})",
            regions[next_idx].accessible_description(),
            next_idx + 1,
            regions.len()
        );
        self.pending_announcements.push(desc);
        self.reset_activity();
        Some(next_idx)
    }

    /// Moves focus to previous widget region (Shift+Tab / Left / Up). Wraps around.
    pub fn focus_prev(&mut self, regions: &[InteractiveRegion]) -> Option<usize> {
        if regions.is_empty() {
            self.focused_region_index = None;
            return None;
        }
        let prev_idx = match self.focused_region_index {
            Some(0) | None => regions.len() - 1,
            Some(curr) => curr - 1,
        };
        self.focused_region_index = Some(prev_idx);
        let desc = format!(
            "Focused: {} ({}/{})",
            regions[prev_idx].accessible_description(),
            prev_idx + 1,
            regions.len()
        );
        self.pending_announcements.push(desc);
        self.reset_activity();
        Some(prev_idx)
    }

    /// Activates the currently focused widget region.
    pub fn activate_focused(&mut self, regions: &[InteractiveRegion]) -> Option<String> {
        let action = self.focused_region(regions).and_then(|r| r.action.clone());
        if action.is_some() {
            self.reset_activity();
        }
        action
    }

    /// Retrieves and drains the next pending accessible announcement for Narrator.
    pub fn pop_accessible_announcement(&mut self) -> Option<String> {
        if self.pending_announcements.is_empty() {
            None
        } else {
            Some(self.pending_announcements.remove(0))
        }
    }

    /// Handles keyboard navigation across interactive widget regions (Gate G14).
    pub fn handle_keyboard_navigation(
        &mut self,
        vk_code: u32,
        shift_pressed: bool,
        regions: &[InteractiveRegion],
    ) -> KeyboardNavigationResult {
        if self.current_mode == InteractionMode::LockedPassive {
            return KeyboardNavigationResult::Ignored;
        }

        match vk_code {
            0x1B => {
                // VK_ESCAPE
                self.dismiss(DismissalReason::EscapeKey);
                KeyboardNavigationResult::Dismissed(DismissalReason::EscapeKey)
            }
            0x09 => {
                // VK_TAB
                let new_idx = if shift_pressed {
                    self.focus_prev(regions)
                } else {
                    self.focus_next(regions)
                };
                if let Some(idx) = new_idx {
                    let desc = regions[idx].accessible_description();
                    KeyboardNavigationResult::FocusChanged {
                        index: idx,
                        description: desc,
                    }
                } else {
                    KeyboardNavigationResult::Ignored
                }
            }
            0x27 | 0x28 => {
                // VK_RIGHT (0x27) or VK_DOWN (0x28)
                if let Some(idx) = self.focus_next(regions) {
                    let desc = regions[idx].accessible_description();
                    KeyboardNavigationResult::FocusChanged {
                        index: idx,
                        description: desc,
                    }
                } else {
                    KeyboardNavigationResult::Ignored
                }
            }
            0x25 | 0x26 => {
                // VK_LEFT (0x25) or VK_UP (0x26)
                if let Some(idx) = self.focus_prev(regions) {
                    let desc = regions[idx].accessible_description();
                    KeyboardNavigationResult::FocusChanged {
                        index: idx,
                        description: desc,
                    }
                } else {
                    KeyboardNavigationResult::Ignored
                }
            }
            0x0D | 0x20 => {
                // VK_RETURN (0x0D) or VK_SPACE (0x20)
                if let Some(action) = self.activate_focused(regions) {
                    KeyboardNavigationResult::Activated { action }
                } else {
                    KeyboardNavigationResult::Ignored
                }
            }
            _ => KeyboardNavigationResult::Ignored,
        }
    }

    /// User enters temporary interactive mode with configured timeout in seconds.
    pub fn enter_temporary_mode(&mut self, timeout_seconds: u32) {
        let now = Utc::now();
        self.current_mode = InteractionMode::TemporaryInteractive { timeout_seconds };
        self.session_start = Some(now);
        self.session_deadline = Some(now + Duration::seconds(timeout_seconds as i64));
        self.last_dismissal = None;
        self.focused_region_index = None;
        self.pending_announcements.clear();
    }

    /// User enters visual layout editing mode.
    pub fn enter_edit_mode(&mut self) {
        self.current_mode = InteractionMode::EditLayout;
        self.session_start = Some(Utc::now());
        self.session_deadline = None;
        self.last_dismissal = None;
        self.focused_region_index = None;
        self.pending_announcements.clear();
    }

    /// Returns to locked/passive default with an explicit reason.
    pub fn dismiss(&mut self, reason: DismissalReason) {
        self.current_mode = InteractionMode::LockedPassive;
        self.session_start = None;
        self.session_deadline = None;
        self.last_dismissal = Some(reason);
        self.focused_region_index = None;
        self.pending_announcements.clear();
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

    /// Resets the inactivity deadline if currently in temporary interactive mode.
    pub fn reset_activity(&mut self) {
        if let InteractionMode::TemporaryInteractive { timeout_seconds } = self.current_mode {
            let now = Utc::now();
            self.session_deadline = Some(now + Duration::seconds(timeout_seconds as i64));
        }
    }

    /// Spatially tests if a 2D coordinate intersects any active interactive region.
    /// Evaluates in reverse order (topmost visual element first).
    pub fn hit_test<'a>(
        &self,
        px: f32,
        py: f32,
        regions: &'a [InteractiveRegion],
    ) -> Option<&'a InteractiveRegion> {
        regions.iter().rev().find(|r| r.contains_point(px, py))
    }

    /// Handles pointer down (mouse click) in interactive mode.
    /// If an interactive region is hit, refreshes the inactivity deadline and returns `Hit`.
    /// If no region is hit (click outside), dismisses the session and returns `MissClickOutside`.
    /// If in locked passive mode, returns `IgnoredPassive`.
    pub fn handle_pointer_down(
        &mut self,
        px: f32,
        py: f32,
        regions: &[InteractiveRegion],
    ) -> PointerHitResult {
        match self.current_mode {
            InteractionMode::TemporaryInteractive { timeout_seconds } => {
                if let Some(hit) = self.hit_test(px, py, regions) {
                    let region = hit.clone();
                    let now = Utc::now();
                    self.session_deadline = Some(now + Duration::seconds(timeout_seconds as i64));
                    PointerHitResult::Hit { region }
                } else {
                    self.dismiss(DismissalReason::ClickOutside);
                    PointerHitResult::MissClickOutside
                }
            }
            InteractionMode::EditLayout => {
                if let Some(hit) = self.hit_test(px, py, regions) {
                    PointerHitResult::Hit {
                        region: hit.clone(),
                    }
                } else {
                    self.dismiss(DismissalReason::ClickOutside);
                    PointerHitResult::MissClickOutside
                }
            }
            InteractionMode::LockedPassive => PointerHitResult::IgnoredPassive,
        }
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

    #[test]
    fn test_interactive_region_contains_point() {
        let region = InteractiveRegion::new(
            "btn_play",
            100.0,
            50.0,
            60.0,
            30.0,
            Some("play_pause".into()),
        );
        assert!(region.contains_point(100.0, 50.0));
        assert!(region.contains_point(130.0, 65.0));
        assert!(region.contains_point(160.0, 80.0));
        assert!(!region.contains_point(99.0, 50.0));
        assert!(!region.contains_point(161.0, 65.0));
        assert!(!region.contains_point(130.0, 49.0));
        assert!(!region.contains_point(130.0, 81.0));
    }

    #[test]
    fn test_interaction_controller_pointer_hit_and_click_outside() {
        let mut controller = InteractionController::new();
        let regions = vec![
            InteractiveRegion::new("btn_prev", 10.0, 10.0, 40.0, 30.0, Some("prev".into())),
            InteractiveRegion::new("btn_play", 60.0, 10.0, 50.0, 30.0, Some("play".into())),
        ];

        // In locked passive mode, clicks are ignored
        let result = controller.handle_pointer_down(65.0, 15.0, &regions);
        assert_eq!(result, PointerHitResult::IgnoredPassive);
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);

        // Enter temporary interactive mode
        controller.enter_temporary_mode(8);

        // Hit on play button (65, 15)
        let result = controller.handle_pointer_down(65.0, 15.0, &regions);
        match result {
            PointerHitResult::Hit { region } => {
                assert_eq!(region.id, "btn_play");
                assert_eq!(region.action.as_deref(), Some("play"));
            }
            _ => panic!("Expected Hit result"),
        }
        assert!(matches!(
            controller.current_mode(),
            InteractionMode::TemporaryInteractive { .. }
        ));

        // Click outside any region (e.g. 500, 500)
        let result = controller.handle_pointer_down(500.0, 500.0, &regions);
        assert_eq!(result, PointerHitResult::MissClickOutside);
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(
            controller.last_dismissal(),
            Some(DismissalReason::ClickOutside)
        );
    }

    #[test]
    fn test_interactive_region_accessibility_metadata_and_description() {
        // 1. Button with action and accessible name
        let play_btn = InteractiveRegion::new(
            "btn_play",
            10.0,
            10.0,
            50.0,
            30.0,
            Some("media.play".into()),
        )
        .with_accessibility("Play", "button", None);
        assert_eq!(
            play_btn.accessible_description(),
            "Play button, action: media.play"
        );

        // 2. Status widget with value
        let cpu_widget = InteractiveRegion::new("cpu_gauge", 70.0, 10.0, 100.0, 40.0, None)
            .with_accessibility("CPU Load", "progressbar", Some("24%".into()));
        assert_eq!(
            cpu_widget.accessible_description(),
            "CPU Load: 24%, role: progressbar"
        );

        // 3. Fallback when accessibility unset
        let raw_widget = InteractiveRegion::new("clock_text", 180.0, 10.0, 80.0, 30.0, None);
        assert_eq!(raw_widget.accessible_description(), "clock_text widget");
    }

    #[test]
    fn test_keyboard_navigation_tab_and_shift_tab_focus_cycle() {
        let mut controller = InteractionController::new();
        let regions = vec![
            InteractiveRegion::new("prev", 0.0, 0.0, 40.0, 30.0, Some("media.prev".into()))
                .with_accessibility("Previous Track", "button", None),
            InteractiveRegion::new("play", 50.0, 0.0, 40.0, 30.0, Some("media.play".into()))
                .with_accessibility("Play", "button", None),
            InteractiveRegion::new("next", 100.0, 0.0, 40.0, 30.0, Some("media.next".into()))
                .with_accessibility("Next Track", "button", None),
        ];

        controller.enter_temporary_mode(10);
        assert_eq!(controller.focused_region_index(), None);

        // 1. Tab -> Focus index 0 (Previous)
        let res = controller.handle_keyboard_navigation(0x09, false, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 0, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(0));

        // 2. Tab -> Focus index 1 (Play)
        let res = controller.handle_keyboard_navigation(0x09, false, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 1, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(1));

        // 3. Tab -> Focus index 2 (Next)
        let res = controller.handle_keyboard_navigation(0x09, false, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 2, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(2));

        // 4. Tab wraps around -> Focus index 0 (Previous)
        let res = controller.handle_keyboard_navigation(0x09, false, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 0, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(0));

        // 5. Shift+Tab wraps backwards -> Focus index 2 (Next)
        let res = controller.handle_keyboard_navigation(0x09, true, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 2, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(2));

        // 6. Shift+Tab -> Focus index 1 (Play)
        let res = controller.handle_keyboard_navigation(0x09, true, &regions);
        assert!(matches!(
            res,
            KeyboardNavigationResult::FocusChanged { index: 1, .. }
        ));
        assert_eq!(controller.focused_region_index(), Some(1));
    }

    #[test]
    fn test_keyboard_navigation_activation_with_enter_and_space() {
        let mut controller = InteractionController::new();
        let regions =
            vec![
                InteractiveRegion::new("mute", 0.0, 0.0, 40.0, 30.0, Some("audio.mute".into()))
                    .with_accessibility("Mute Audio", "button", None),
            ];

        controller.enter_temporary_mode(10);
        controller.focus_next(&regions);
        assert_eq!(controller.focused_region_index(), Some(0));

        // Enter key activates
        let res = controller.handle_keyboard_navigation(0x0D, false, &regions);
        assert_eq!(
            res,
            KeyboardNavigationResult::Activated {
                action: "audio.mute".into()
            }
        );

        // Space key activates
        let res = controller.handle_keyboard_navigation(0x20, false, &regions);
        assert_eq!(
            res,
            KeyboardNavigationResult::Activated {
                action: "audio.mute".into()
            }
        );

        // Escape dismisses
        let res = controller.handle_keyboard_navigation(0x1B, false, &regions);
        assert_eq!(
            res,
            KeyboardNavigationResult::Dismissed(DismissalReason::EscapeKey)
        );
        assert_eq!(controller.current_mode(), InteractionMode::LockedPassive);
        assert_eq!(controller.focused_region_index(), None);
    }

    #[test]
    fn test_accessible_announcements_drain() {
        let mut controller = InteractionController::new();
        let regions = vec![
            InteractiveRegion::new("btn", 0.0, 0.0, 10.0, 10.0, Some("act".into()))
                .with_accessibility("Test Button", "button", None),
        ];

        controller.enter_temporary_mode(10);
        controller.focus_next(&regions);

        let ann = controller.pop_accessible_announcement();
        assert!(ann.is_some());
        assert!(ann.unwrap().contains("Test Button"));

        assert_eq!(controller.pop_accessible_announcement(), None);
    }
}
