use aethershell_core::{MonitorTaskbarState, TaskbarEffect, TaskbarResolution};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskbarBackendError {
    #[error("Taskbar window not found")]
    TaskbarNotFound,
    #[error("Platform unsupported: {0}")]
    Unsupported(String),
    #[error("Conflict detected: competing customizer active; yielding")]
    ConflictYielded,
}

/// Abstract backend trait for taskbar appearance mutations.
pub trait TaskbarBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply_effect(
        &mut self,
        monitor_id: &str,
        state: MonitorTaskbarState,
        effect: TaskbarEffect,
    ) -> Result<TaskbarResolution, TaskbarBackendError>;
    fn restore_native(&mut self, monitor_id: &str) -> Result<(), TaskbarBackendError>;
}

/// Backend A: Native No-Op (PRD Section 4.2).
/// Does not mutate the system taskbar. Universal safe fallback and default.
pub struct NativeNoopBackend;

impl TaskbarBackend for NativeNoopBackend {
    fn name(&self) -> &'static str {
        "native_noop"
    }

    fn apply_effect(
        &mut self,
        monitor_id: &str,
        state: MonitorTaskbarState,
        _effect: TaskbarEffect,
    ) -> Result<TaskbarResolution, TaskbarBackendError> {
        Ok(TaskbarResolution {
            monitor_id: monitor_id.to_string(),
            state,
            requested_effect: _effect,
            applied_effect: TaskbarEffect::NativeNoop,
            backend: "native_noop".into(),
            fallback_reason: Some("Native no-op fallback active".into()),
        })
    }

    fn restore_native(&mut self, _monitor_id: &str) -> Result<(), TaskbarBackendError> {
        Ok(())
    }
}
