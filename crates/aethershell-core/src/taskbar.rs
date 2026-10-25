use serde::{Deserialize, Serialize};

/// Taskbar visual effect types (PRD Section 4.1 TB-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskbarEffect {
    #[default]
    NativeNoop,
    Clear,
    Tint,
    BlurLike,
    AcrylicLike,
    Solid,
}

/// Taskbar state machine states per monitor (PRD Section 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorTaskbarState {
    Disconnected,
    Desktop,
    WindowMaximized,
    FlyoutVisible,
    Fullscreen,
    BatterySaver,
    Suspended,
    ExplorerUnavailable,
    SafeMode,
}

/// Resolved taskbar decision containing diagnostics and fallback reasons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskbarResolution {
    pub monitor_id: String,
    pub state: MonitorTaskbarState,
    pub requested_effect: TaskbarEffect,
    pub applied_effect: TaskbarEffect,
    pub backend: String,
    pub fallback_reason: Option<String>,
}

/// Taskbar rule engine prioritizing rules deterministically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskbarRule {
    pub id: String,
    pub priority: u32,
    pub target_state: Option<MonitorTaskbarState>,
    pub executable_filter: Option<String>,
    pub effect: TaskbarEffect,
    pub color_hex: Option<String>,
    pub opacity: f32,
}

impl TaskbarRule {
    /// Determines whether this rule matches the current context.
    pub fn matches(&self, current_state: MonitorTaskbarState, active_exe: Option<&str>) -> bool {
        if let Some(target) = self.target_state {
            if target != current_state {
                return false;
            }
        }
        if let Some(ref filter) = self.executable_filter {
            match active_exe {
                Some(exe) => {
                    if !exe.eq_ignore_ascii_case(filter) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Selects the highest-priority matching rule from a list.
pub fn resolve_taskbar_rule<'a>(
    rules: &'a [TaskbarRule],
    state: MonitorTaskbarState,
    active_exe: Option<&str>,
) -> Option<&'a TaskbarRule> {
    rules
        .iter()
        .filter(|r| r.matches(state, active_exe))
        .max_by_key(|r| r.priority)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taskbar_rule_resolution_priority() {
        let rules = vec![
            TaskbarRule {
                id: "desktop_clear".into(),
                priority: 10,
                target_state: Some(MonitorTaskbarState::Desktop),
                executable_filter: None,
                effect: TaskbarEffect::Clear,
                color_hex: None,
                opacity: 0.0,
            },
            TaskbarRule {
                id: "maximized_tint".into(),
                priority: 20,
                target_state: Some(MonitorTaskbarState::WindowMaximized),
                executable_filter: None,
                effect: TaskbarEffect::Tint,
                color_hex: Some("#141925".into()),
                opacity: 0.85,
            },
            TaskbarRule {
                id: "code_editor_override".into(),
                priority: 50,
                target_state: Some(MonitorTaskbarState::WindowMaximized),
                executable_filter: Some("Code.exe".into()),
                effect: TaskbarEffect::Tint,
                color_hex: Some("#25243A".into()),
                opacity: 0.9,
            },
        ];

        // On Desktop state
        let resolved = resolve_taskbar_rule(&rules, MonitorTaskbarState::Desktop, None);
        assert_eq!(resolved.map(|r| r.id.as_str()), Some("desktop_clear"));

        // Maximized without specific app
        let resolved = resolve_taskbar_rule(
            &rules,
            MonitorTaskbarState::WindowMaximized,
            Some("notepad.exe"),
        );
        assert_eq!(resolved.map(|r| r.id.as_str()), Some("maximized_tint"));

        // Maximized with Code.exe (priority 50 wins)
        let resolved = resolve_taskbar_rule(
            &rules,
            MonitorTaskbarState::WindowMaximized,
            Some("Code.exe"),
        );
        assert_eq!(
            resolved.map(|r| r.id.as_str()),
            Some("code_editor_override")
        );

        // Maximized with code.exe (case-insensitive)
        let resolved = resolve_taskbar_rule(
            &rules,
            MonitorTaskbarState::WindowMaximized,
            Some("code.exe"),
        );
        assert_eq!(
            resolved.map(|r| r.id.as_str()),
            Some("code_editor_override")
        );
    }
}
