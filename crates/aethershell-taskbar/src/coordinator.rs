use crate::backend::{NativeNoopBackend, TaskbarBackend, TaskbarBackendError};
use aethershell_core::{
    resolve_taskbar_rule, MonitorTaskbarState, TaskbarEffect, TaskbarResolution, TaskbarRule,
};
use aethershell_win::{detect_taskbar_conflicts, ConflictInfo};
use std::collections::HashMap;

/// Taskbar coordinator orchestrating per-monitor taskbar states, precedence rules, and safe fallbacks.
pub struct TaskbarCoordinator {
    backend: Box<dyn TaskbarBackend>,
    per_monitor_states: HashMap<String, MonitorTaskbarState>,
    active_resolutions: HashMap<String, TaskbarResolution>,
    conflict_info: Option<ConflictInfo>,
    is_safe_mode: bool,
    rules: Vec<TaskbarRule>,
}

impl TaskbarCoordinator {
    pub fn new() -> Self {
        Self {
            backend: Box::new(NativeNoopBackend),
            per_monitor_states: HashMap::new(),
            active_resolutions: HashMap::new(),
            conflict_info: None,
            is_safe_mode: false,
            rules: TaskbarRule::default_rules(),
        }
    }

    pub fn with_backend(backend: Box<dyn TaskbarBackend>) -> Self {
        Self {
            backend,
            per_monitor_states: HashMap::new(),
            active_resolutions: HashMap::new(),
            conflict_info: None,
            is_safe_mode: false,
            rules: TaskbarRule::default_rules(),
        }
    }

    pub fn set_safe_mode(&mut self, enabled: bool) {
        self.is_safe_mode = enabled;
    }

    pub fn is_safe_mode(&self) -> bool {
        self.is_safe_mode
    }

    pub fn set_rules(&mut self, rules: Vec<TaskbarRule>) {
        self.rules = rules;
    }

    pub fn rules(&self) -> &[TaskbarRule] {
        &self.rules
    }

    /// Checks if any monitored display has an active full-screen window (Gaming Shield trigger).
    pub fn is_any_monitor_fullscreen(&self) -> bool {
        self.per_monitor_states
            .values()
            .any(|state| matches!(state, MonitorTaskbarState::Fullscreen))
    }

    pub fn set_conflict_info(&mut self, conflict: Option<ConflictInfo>) {
        self.conflict_info = conflict;
    }

    pub fn conflict_info(&self) -> Option<&ConflictInfo> {
        self.conflict_info.as_ref()
    }

    /// Checks for active competing taskbar customizers and updates conflict status.
    pub fn probe_and_update_conflicts(&mut self) -> Option<ConflictInfo> {
        let detected = detect_taskbar_conflicts();
        self.conflict_info = detected.clone();
        detected
    }

    /// Evaluates rules for the given monitor state and applies effects through the backend.
    pub fn evaluate_and_apply(
        &mut self,
        monitor_id: &str,
        new_state: MonitorTaskbarState,
    ) -> TaskbarResolution {
        self.per_monitor_states
            .insert(monitor_id.to_string(), new_state);

        // Precedence 1: Safe Mode (PRD Section 3.4)
        if self.is_safe_mode {
            let res = TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect: TaskbarEffect::NativeNoop,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "safe_mode".into(),
                fallback_reason: Some("Safe mode active; all taskbar mutations suppressed".into()),
            };
            self.active_resolutions
                .insert(monitor_id.to_string(), res.clone());
            return res;
        }

        // Precedence 2: Competing customizer conflict yield (PRD Q21:A)
        if let Some(ref conflict) = self.conflict_info {
            let res = TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect: TaskbarEffect::NativeNoop,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "native_noop".into(),
                fallback_reason: Some(format!(
                    "Competing customizer ({}) active; yielded per Q21:A",
                    conflict.name
                )),
            };
            self.active_resolutions
                .insert(monitor_id.to_string(), res.clone());
            return res;
        }

        // Precedence 3: Rule evaluation from precedence table
        let requested_effect = resolve_taskbar_rule(&self.rules, new_state, None)
            .map(|r| r.effect)
            .unwrap_or(TaskbarEffect::NativeNoop);

        let resolution = match self
            .backend
            .apply_effect(monitor_id, new_state, requested_effect)
        {
            Ok(res) => res,
            Err(e) => TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "native_noop".into(),
                fallback_reason: Some(format!("Backend error: {}", e)),
            },
        };

        self.active_resolutions
            .insert(monitor_id.to_string(), resolution.clone());
        resolution
    }

    /// Legacy convenience wrapper
    pub fn update_state(
        &mut self,
        monitor_id: &str,
        new_state: MonitorTaskbarState,
        requested_effect: TaskbarEffect,
    ) -> TaskbarResolution {
        self.per_monitor_states
            .insert(monitor_id.to_string(), new_state);

        if let Some(ref conflict) = self.conflict_info {
            return TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "native_noop".into(),
                fallback_reason: Some(format!(
                    "Competing customizer ({}) detected; yielded per Q21:A",
                    conflict.name
                )),
            };
        }

        match self
            .backend
            .apply_effect(monitor_id, new_state, requested_effect)
        {
            Ok(resolution) => resolution,
            Err(e) => TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "native_noop".into(),
                fallback_reason: Some(format!("Backend error: {}", e)),
            },
        }
    }

    pub fn get_resolution(&self, monitor_id: &str) -> Option<&TaskbarResolution> {
        self.active_resolutions.get(monitor_id)
    }

    /// Restores native taskbars across all active monitors.
    pub fn restore_all_native(&mut self) -> Result<(), TaskbarBackendError> {
        let keys: Vec<String> = self.per_monitor_states.keys().cloned().collect();
        for key in &keys {
            self.backend.restore_native(key)?;
        }
        if keys.is_empty() {
            self.backend.restore_native("primary")?;
        }
        Ok(())
    }
}

impl Default for TaskbarCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ExternalAppearanceBackend;

    #[test]
    fn test_taskbar_coordinator_conflict_yielding() {
        let mut coordinator = TaskbarCoordinator::new();
        coordinator.set_conflict_info(Some(ConflictInfo {
            name: "TranslucentTB".into(),
            description: "Test description".into(),
            recommendation: "Yield per Q21:A".into(),
        }));

        let res =
            coordinator.update_state("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res
            .fallback_reason
            .unwrap()
            .contains("Competing customizer"));
    }

    #[test]
    fn test_taskbar_coordinator_default_native_noop() {
        let mut coordinator = TaskbarCoordinator::new();
        let res =
            coordinator.update_state("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert_eq!(res.backend, "native_noop");
    }

    #[test]
    fn test_taskbar_coordinator_safe_mode_precedence() {
        let mut coordinator = TaskbarCoordinator::new();
        coordinator.set_safe_mode(true);

        let res = coordinator.evaluate_and_apply("mon-1", MonitorTaskbarState::Desktop);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert_eq!(res.backend, "safe_mode");
        assert!(res.fallback_reason.unwrap().contains("Safe mode active"));
    }

    #[test]
    fn test_taskbar_coordinator_external_appearance_gated_fallback() {
        // Without VM verification, ExternalAppearanceBackend gracefully routes to native_noop
        let mut coordinator =
            TaskbarCoordinator::with_backend(Box::new(ExternalAppearanceBackend::new()));

        let res = coordinator.evaluate_and_apply("mon-1", MonitorTaskbarState::Desktop);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert_eq!(res.backend, "external_appearance");
        assert!(res.fallback_reason.unwrap().contains("safely routed"));
    }
}
