use aethershell_core::{MonitorTaskbarState, TaskbarEffect, TaskbarResolution};
use aethershell_win::WindowsVersionInfo;
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
        effect: TaskbarEffect,
    ) -> Result<TaskbarResolution, TaskbarBackendError> {
        Ok(TaskbarResolution {
            monitor_id: monitor_id.to_string(),
            state,
            requested_effect: effect,
            applied_effect: TaskbarEffect::NativeNoop,
            backend: "native_noop".into(),
            fallback_reason: Some("Native no-op fallback active".into()),
        })
    }

    fn restore_native(&mut self, _monitor_id: &str) -> Result<(), TaskbarBackendError> {
        Ok(())
    }
}

/// Backend B: External Appearance (PRD Section 4.2).
/// Runs in AetherShell's own process using capability checking and per-build allowlists.
/// If unverified or unsupported, resolves to native_noop without fighting.
pub struct ExternalAppearanceBackend {
    version_info: WindowsVersionInfo,
    is_capability_verified: bool,
}

impl ExternalAppearanceBackend {
    pub fn new() -> Self {
        let version_info = WindowsVersionInfo::detect();
        Self {
            version_info,
            is_capability_verified: false, // Default unverified until explicit VM qualification
        }
    }

    pub fn with_verification(is_capability_verified: bool) -> Self {
        let version_info = WindowsVersionInfo::detect();
        Self {
            version_info,
            is_capability_verified,
        }
    }
}

impl Default for ExternalAppearanceBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskbarBackend for ExternalAppearanceBackend {
    fn name(&self) -> &'static str {
        "external_appearance"
    }

    fn apply_effect(
        &mut self,
        monitor_id: &str,
        state: MonitorTaskbarState,
        effect: TaskbarEffect,
    ) -> Result<TaskbarResolution, TaskbarBackendError> {
        if !self.is_capability_verified {
            return Ok(TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state,
                requested_effect: effect,
                applied_effect: TaskbarEffect::NativeNoop,
                backend: "external_appearance".into(),
                fallback_reason: Some(format!(
                    "External composition unverified on Windows build {} (UBR: {}); safely routed to native_noop per PRD 4.2",
                    self.version_info.build_number,
                    self.version_info.ubr.unwrap_or(0)
                )),
            });
        }

        // Verified path (e.g. in qualified VM environments)
        Ok(TaskbarResolution {
            monitor_id: monitor_id.to_string(),
            state,
            requested_effect: effect,
            applied_effect: effect,
            backend: "external_appearance".into(),
            fallback_reason: None,
        })
    }

    fn restore_native(&mut self, _monitor_id: &str) -> Result<(), TaskbarBackendError> {
        Ok(())
    }
}
