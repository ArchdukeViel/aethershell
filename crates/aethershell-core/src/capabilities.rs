use crate::status::VerificationStatus;
use serde::{Deserialize, Serialize};

/// Risk tier for Windows-facing integrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Safe, standard user-mode Win32 API.
    SafeUserMode,
    /// Out-of-process component with failure recovery.
    SupervisedChildProcess,
    /// Desktop window layering or non-invasive hook.
    DesktopLayering,
    /// Experimental shell component (VM-only qualification required).
    ExperimentalShell,
}

/// Feature capability declaration per PRD Section 2.3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureCapability {
    pub feature_id: String,
    pub implementation_backend: String,
    pub minimum_os_build: u32,
    pub tested_builds: Vec<u32>,
    pub requires_explorer_module: bool,
    pub required_permissions: String,
    pub compatible_package_channels: Vec<String>,
    pub probe_method: String,
    pub known_conflicts: Vec<String>,
    pub fallback: String,
    pub rollback: String,
    pub status: VerificationStatus,
    pub risk_tier: RiskTier,
}

impl FeatureCapability {
    /// Evaluates if the feature is supported on the target Windows build number.
    pub fn is_supported_on_build(&self, build_number: u32) -> bool {
        if self.requires_explorer_module {
            return false; // Stable builds never support explorer injection
        }
        if build_number < self.minimum_os_build {
            return false;
        }
        matches!(
            self.status,
            VerificationStatus::Verified | VerificationStatus::PartiallyVerified
        )
    }
}
