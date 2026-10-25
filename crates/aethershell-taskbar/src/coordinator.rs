use crate::backend::{NativeNoopBackend, TaskbarBackend};
use aethershell_core::{MonitorTaskbarState, TaskbarEffect, TaskbarResolution};
use std::collections::HashMap;

/// Taskbar coordinator orchestrating per-monitor taskbar states and safe fallbacks.
pub struct TaskbarCoordinator {
    backend: Box<dyn TaskbarBackend>,
    per_monitor_states: HashMap<String, MonitorTaskbarState>,
    conflict_detected: bool,
}

impl TaskbarCoordinator {
    pub fn new() -> Self {
        Self {
            backend: Box::new(NativeNoopBackend),
            per_monitor_states: HashMap::new(),
            conflict_detected: false,
        }
    }

    pub fn set_conflict_detected(&mut self, conflict: bool) {
        self.conflict_detected = conflict;
    }

    pub fn update_state(
        &mut self,
        monitor_id: &str,
        new_state: MonitorTaskbarState,
        requested_effect: TaskbarEffect,
    ) -> TaskbarResolution {
        self.per_monitor_states
            .insert(monitor_id.to_string(), new_state);

        if self.conflict_detected {
            return TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state: new_state,
                requested_effect,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "native_noop".into(),
                fallback_reason: Some("Competing customizer detected; yielded per Q21:A".into()),
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
}

impl Default for TaskbarCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taskbar_coordinator_conflict_yielding() {
        let mut coordinator = TaskbarCoordinator::new();
        coordinator.set_conflict_detected(true);

        let res =
            coordinator.update_state("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res
            .fallback_reason
            .unwrap()
            .contains("Competing customizer detected"));
    }

    #[test]
    fn test_taskbar_coordinator_default_native_noop() {
        let mut coordinator = TaskbarCoordinator::new();
        let res =
            coordinator.update_state("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear);
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert_eq!(res.backend, "native_noop");
    }
}
