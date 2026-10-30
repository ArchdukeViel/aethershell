use aethershell_core::{MonitorTaskbarState, TaskbarEffect, TaskbarResolution};
use aethershell_win::{EffectSupportVerdict, WindowsVersionInfo};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskbarBackendError {
    #[error("Taskbar window not found")]
    TaskbarNotFound,
    #[error("Platform unsupported: {0}")]
    Unsupported(String),
    #[error("Conflict detected: competing customizer active; yielding")]
    ConflictYielded,
    #[error("Experimental Explorer bridge is not permitted in stable v1 releases per PRD Section 4.2 Backend C and Owner Decisions Q02:C / Q35:B")]
    ExperimentalNotPermittedInStable,
    #[error("Disruptive experimental shell operations are prohibited on daily-use physical host machines per PRD Section 18.3 and Decision Q37:A")]
    PhysicalHostDisruptiveBlocked,
    #[error("Experimental Explorer bridge requires explicit user consent token per PRD Section 18.2 Gate G05 and Decision Q41:A")]
    ExperimentalConsentMissing,
    #[error("Safe Mode active: experimental taskbar mutations strictly prohibited")]
    SafeModeActive,
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

/// Backend B: External Appearance (PRD Section 4.2, Section 2.1 Gate G02).
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

    pub fn with_version_and_verification(
        version_info: WindowsVersionInfo,
        is_capability_verified: bool,
    ) -> Self {
        Self {
            version_info,
            is_capability_verified,
        }
    }

    pub fn version_info(&self) -> &WindowsVersionInfo {
        &self.version_info
    }

    pub fn is_capability_verified(&self) -> bool {
        self.is_capability_verified
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
        // Honest state-based fallbacks (PRD Section 4.1 TB-04, Section 2.2 [S05], Section 4.3)
        if effect != TaskbarEffect::NativeNoop {
            match state {
                MonitorTaskbarState::BatterySaver => {
                    return Ok(TaskbarResolution {
                        monitor_id: monitor_id.to_string(),
                        state,
                        requested_effect: effect,
                        applied_effect: TaskbarEffect::NativeNoop,
                        backend: "external_appearance".into(),
                        fallback_reason: Some(
                            "Battery saver active; taskbar composition effects suppressed per PRD Section 2.2 [S05] / TB-04".into(),
                        ),
                    });
                }
                MonitorTaskbarState::Suspended => {
                    return Ok(TaskbarResolution {
                        monitor_id: monitor_id.to_string(),
                        state,
                        requested_effect: effect,
                        applied_effect: TaskbarEffect::NativeNoop,
                        backend: "external_appearance".into(),
                        fallback_reason: Some(
                            "Presentation suspended; taskbar composition suppressed per PRD Section 4.3".into(),
                        ),
                    });
                }
                MonitorTaskbarState::ExplorerUnavailable | MonitorTaskbarState::Disconnected => {
                    return Ok(TaskbarResolution {
                        monitor_id: monitor_id.to_string(),
                        state,
                        requested_effect: effect,
                        applied_effect: TaskbarEffect::NativeNoop,
                        backend: "external_appearance".into(),
                        fallback_reason: Some(
                            "Explorer shell or display unavailable; taskbar mutations safely routed to native_noop per PRD Section 4.3".into(),
                        ),
                    });
                }
                _ => {}
            }
        }

        let verdict = self
            .version_info
            .family
            .evaluate_taskbar_effect(effect, self.is_capability_verified);

        match verdict {
            EffectSupportVerdict::Supported => Ok(TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state,
                requested_effect: effect,
                applied_effect: effect,
                backend: "external_appearance".into(),
                fallback_reason: None,
            }),
            EffectSupportVerdict::UnsupportedFallback {
                reason,
                target_effect,
            } => Ok(TaskbarResolution {
                monitor_id: monitor_id.to_string(),
                state,
                requested_effect: effect,
                applied_effect: target_effect,
                backend: "external_appearance".into(),
                fallback_reason: Some(reason),
            }),
        }
    }

    fn restore_native(&mut self, _monitor_id: &str) -> Result<(), TaskbarBackendError> {
        let _ = crate::recovery::restore_system_taskbars();
        Ok(())
    }
}

/// Backend C: Explorer XAML Experimental (PRD Section 4.2 Backend C [POC], Section 18.2 Gate G05).
/// Represents the opt-in experimental Explorer XAML bridge research track.
/// By architectural invariant (Owner Decisions Q02:C, Q35:B, Q37:A, Q41:A), this backend is
/// strictly excluded from stable release distributions and fails closed unless:
/// 1. Built with experimental flags (`is_stable_build == false`).
/// 2. Executing inside an isolated disposable VM guest (`is_disposable_vm == true`).
/// 3. Verified release-specific user consent is granted (`user_consent_token.is_some()`).
/// 4. Crash-loop recovery harness is qualified and active (`crash_loop_recovery_verified == true`).
#[derive(Debug, Clone)]
pub struct ExplorerXamlExperimentalBackend {
    is_stable_build: bool,
    is_disposable_vm: bool,
    user_consent_token: Option<String>,
    crash_loop_recovery_verified: bool,
}

impl ExplorerXamlExperimentalBackend {
    /// Creates a default experimental backend configured under stable build constraints (fail-closed).
    pub fn new() -> Self {
        Self {
            is_stable_build: true,   // Defaults to stable build rules (Q02:C / Q35:B)
            is_disposable_vm: false, // Defaults to physical host rules (Q37:A)
            user_consent_token: None,
            crash_loop_recovery_verified: false,
        }
    }

    /// Builder for testing and qualification harnesses with explicit parameters.
    pub fn with_params(
        is_stable_build: bool,
        is_disposable_vm: bool,
        user_consent_token: Option<String>,
        crash_loop_recovery_verified: bool,
    ) -> Self {
        Self {
            is_stable_build,
            is_disposable_vm,
            user_consent_token,
            crash_loop_recovery_verified,
        }
    }

    pub fn is_stable_build(&self) -> bool {
        self.is_stable_build
    }

    pub fn is_disposable_vm(&self) -> bool {
        self.is_disposable_vm
    }

    pub fn has_user_consent(&self) -> bool {
        self.user_consent_token.is_some()
    }

    pub fn is_crash_recovery_verified(&self) -> bool {
        self.crash_loop_recovery_verified
    }
}

impl Default for ExplorerXamlExperimentalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskbarBackend for ExplorerXamlExperimentalBackend {
    fn name(&self) -> &'static str {
        "explorer_xaml_experimental"
    }

    fn apply_effect(
        &mut self,
        monitor_id: &str,
        state: MonitorTaskbarState,
        effect: TaskbarEffect,
    ) -> Result<TaskbarResolution, TaskbarBackendError> {
        // Guard 1: Stable core release invariant (Q02:C, Q35:B, PRD Section 4.2 Backend C)
        if self.is_stable_build {
            return Err(TaskbarBackendError::ExperimentalNotPermittedInStable);
        }

        // Guard 2: Safety boundary invariant on physical host (Q37:A, PRD Section 18.3)
        if !self.is_disposable_vm {
            return Err(TaskbarBackendError::PhysicalHostDisruptiveBlocked);
        }

        // Guard 3: User consent token requirement (Q41:A, PRD Section 4.2)
        if self.user_consent_token.is_none() {
            return Err(TaskbarBackendError::ExperimentalConsentMissing);
        }

        // Guard 4: Crash recovery harness qualification requirement (PRD POC-TB-03)
        if !self.crash_loop_recovery_verified {
            return Err(TaskbarBackendError::Unsupported(
                "Crash-loop recovery harness not qualified for experimental bridge (POC-TB-03)"
                    .into(),
            ));
        }

        // When all 4 guards are fulfilled inside an experimental disposable-VM harness:
        Ok(TaskbarResolution {
            monitor_id: monitor_id.to_string(),
            state,
            requested_effect: effect,
            applied_effect: effect,
            backend: "explorer_xaml_experimental".into(),
            fallback_reason: Some(
                "Experimental Explorer bridge active in disposable VM qualification mode".into(),
            ),
        })
    }

    fn restore_native(&mut self, _monitor_id: &str) -> Result<(), TaskbarBackendError> {
        let _ = crate::recovery::restore_system_taskbars();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethershell_win::WindowsBuildFamily;

    fn make_test_version(family: WindowsBuildFamily) -> WindowsVersionInfo {
        let (major, minor, build) = match family {
            WindowsBuildFamily::Win10_22H2 => (10, 0, 19045),
            WindowsBuildFamily::Win11_21H2 => (10, 0, 22000),
            WindowsBuildFamily::Win11_22H2 => (10, 0, 22621),
            WindowsBuildFamily::Win11_23H2 => (10, 0, 22631),
            WindowsBuildFamily::Win11_24H2 => (10, 0, 26100),
            WindowsBuildFamily::Win11_25H2 => (10, 0, 26200),
            WindowsBuildFamily::Win11_26H2 => (10, 0, 26300),
            WindowsBuildFamily::Win11_26H1 => (10, 0, 28000),
            WindowsBuildFamily::Unknown(b) => (10, 0, b),
        };

        WindowsVersionInfo {
            major,
            minor,
            build_number: build,
            ubr: Some(1000),
            family,
            is_supported_v1: family.is_v1_supported(),
        }
    }

    #[test]
    fn test_external_backend_native_noop_always_supported() {
        let mut backend = ExternalAppearanceBackend::new();
        let res = backend
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::Desktop,
                TaskbarEffect::NativeNoop,
            )
            .expect("NativeNoop must always succeed");
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert_eq!(res.backend, "external_appearance");
        assert!(res.fallback_reason.is_none());
    }

    #[test]
    fn test_external_backend_win10_honest_fallback() {
        let win10_info = make_test_version(WindowsBuildFamily::Win10_22H2);

        // 1. Unverified Clear falls back to NativeNoop
        let mut backend_unverified =
            ExternalAppearanceBackend::with_version_and_verification(win10_info.clone(), false);
        let res1 = backend_unverified
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res1.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res1.fallback_reason.unwrap().contains("safely routed"));

        // 2. Verified Clear applies successfully
        let mut backend_verified =
            ExternalAppearanceBackend::with_version_and_verification(win10_info.clone(), true);
        let res2 = backend_verified
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res2.applied_effect, TaskbarEffect::Clear);
        assert!(res2.fallback_reason.is_none());

        // 3. AcrylicLike on Win10 honestly falls back even if verified (PRD Section 2.2 [S05])
        let res3 = backend_verified
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::Desktop,
                TaskbarEffect::AcrylicLike,
            )
            .unwrap();
        assert_eq!(res3.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res3
            .fallback_reason
            .unwrap()
            .contains("Acrylic / Mica materials are not supported on Windows 10"));

        // 4. BlurLike on Win10 honestly falls back even if verified (PRD Section 2.2 / TB-03)
        let res4 = backend_verified
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::Desktop,
                TaskbarEffect::BlurLike,
            )
            .unwrap();
        assert_eq!(res4.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res4
            .fallback_reason
            .unwrap()
            .contains("Blur composition on taskbar is not supported via external APIs"));
    }

    #[test]
    fn test_external_backend_win11_xaml_taskbar_gating() {
        let win11_info = make_test_version(WindowsBuildFamily::Win11_26H2);

        // 1. Unverified on Win11 XAML taskbar safely routes to NativeNoop
        let mut backend_unverified =
            ExternalAppearanceBackend::with_version_and_verification(win11_info.clone(), false);
        let res1 = backend_unverified
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res1.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res1
            .fallback_reason
            .unwrap()
            .contains("External composition unverified on modern Windows 11 XAML taskbar"));

        // 2. Verified on Win11 applies requested effect
        let mut backend_verified =
            ExternalAppearanceBackend::with_version_and_verification(win11_info, true);
        let res2 = backend_verified
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res2.applied_effect, TaskbarEffect::Clear);
        assert!(res2.fallback_reason.is_none());
    }

    #[test]
    fn test_external_backend_special_tier_and_unknown_fallback() {
        // Special validation tier 28000
        let win11_special = make_test_version(WindowsBuildFamily::Win11_26H1);
        let mut backend_special =
            ExternalAppearanceBackend::with_version_and_verification(win11_special, true);
        let res_special = backend_special
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res_special.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res_special
            .fallback_reason
            .unwrap()
            .contains("special validation tier outside v1.0.0 production baseline"));

        // Unknown build 99999
        let unknown = make_test_version(WindowsBuildFamily::Unknown(99999));
        let mut backend_unknown =
            ExternalAppearanceBackend::with_version_and_verification(unknown, true);
        let res_unknown = backend_unknown
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res_unknown.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res_unknown
            .fallback_reason
            .unwrap()
            .contains("outside authoritative PRD Section 2.1 matrix"));
    }

    #[test]
    fn test_external_backend_win11_21h2_fallback() {
        let win11_21h2 = make_test_version(WindowsBuildFamily::Win11_21H2);
        let mut backend =
            ExternalAppearanceBackend::with_version_and_verification(win11_21h2, true);
        let res = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap();
        assert_eq!(res.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res
            .fallback_reason
            .unwrap()
            .contains("below initial Windows 11 baseline"));
    }

    #[test]
    fn test_external_backend_runtime_states_fallback() {
        let win11 = make_test_version(WindowsBuildFamily::Win11_26H2);
        let mut backend = ExternalAppearanceBackend::with_version_and_verification(win11, true);

        // 1. BatterySaver forces NativeNoop even when verified
        let res_battery = backend
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::BatterySaver,
                TaskbarEffect::Clear,
            )
            .unwrap();
        assert_eq!(res_battery.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res_battery
            .fallback_reason
            .unwrap()
            .contains("Battery saver active"));

        // 2. Suspended forces NativeNoop
        let res_suspended = backend
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::Suspended,
                TaskbarEffect::Clear,
            )
            .unwrap();
        assert_eq!(res_suspended.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res_suspended
            .fallback_reason
            .unwrap()
            .contains("Presentation suspended"));

        // 3. ExplorerUnavailable forces NativeNoop
        let res_exp = backend
            .apply_effect(
                "mon-1",
                MonitorTaskbarState::ExplorerUnavailable,
                TaskbarEffect::Clear,
            )
            .unwrap();
        assert_eq!(res_exp.applied_effect, TaskbarEffect::NativeNoop);
        assert!(res_exp
            .fallback_reason
            .unwrap()
            .contains("Explorer shell or display unavailable"));
    }

    #[test]
    fn test_experimental_backend_default_fails_in_stable() {
        let mut backend = ExplorerXamlExperimentalBackend::new();
        assert_eq!(backend.name(), "explorer_xaml_experimental");
        assert!(backend.is_stable_build());
        assert!(!backend.is_disposable_vm());
        assert!(!backend.has_user_consent());
        assert!(!backend.is_crash_recovery_verified());

        let err = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap_err();
        assert_eq!(err, TaskbarBackendError::ExperimentalNotPermittedInStable);
    }

    #[test]
    fn test_experimental_backend_fails_on_physical_host() {
        // Experimental build, but physical host (not disposable VM)
        let mut backend = ExplorerXamlExperimentalBackend::with_params(
            false, // is_stable_build = false
            false, // is_disposable_vm = false (physical host)
            Some("consent-token-12345".into()),
            true,
        );

        let err = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap_err();
        assert_eq!(err, TaskbarBackendError::PhysicalHostDisruptiveBlocked);
    }

    #[test]
    fn test_experimental_backend_fails_without_consent() {
        // Experimental build in disposable VM, but missing user consent
        let mut backend = ExplorerXamlExperimentalBackend::with_params(
            false, // is_stable_build = false
            true,  // is_disposable_vm = true
            None,  // missing consent
            true,
        );

        let err = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap_err();
        assert_eq!(err, TaskbarBackendError::ExperimentalConsentMissing);
    }

    #[test]
    fn test_experimental_backend_fails_without_crash_recovery() {
        // Experimental build in VM with consent, but unverified crash recovery
        let mut backend = ExplorerXamlExperimentalBackend::with_params(
            false,
            true,
            Some("consent-token-12345".into()),
            false, // unverified crash recovery
        );

        let err = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .unwrap_err();
        match err {
            TaskbarBackendError::Unsupported(msg) => {
                assert!(msg.contains("Crash-loop recovery harness not qualified"));
            }
            other => panic!("Expected Unsupported error, got {:?}", other),
        }
    }

    #[test]
    fn test_experimental_backend_succeeds_in_fully_qualified_vm() {
        // All 4 qualification conditions met
        let mut backend = ExplorerXamlExperimentalBackend::with_params(
            false, // experimental build
            true,  // disposable VM
            Some("consent-token-12345".into()),
            true, // crash recovery verified
        );

        let res = backend
            .apply_effect("mon-1", MonitorTaskbarState::Desktop, TaskbarEffect::Clear)
            .expect("Fully qualified experimental harness must succeed");

        assert_eq!(res.backend, "explorer_xaml_experimental");
        assert_eq!(res.applied_effect, TaskbarEffect::Clear);
        assert!(res.fallback_reason.unwrap().contains("qualification mode"));
    }
}
