use serde::{Deserialize, Serialize};
use std::fmt;

/// Authoritative status vocabulary defined in PRD Section 2.3 and Master Prompt Section 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// No implementation exists yet.
    NotStarted,
    /// Code exists but behavior has not been empirically demonstrated.
    ImplementedUnverified,
    /// Some acceptance requirements have passed.
    PartiallyVerified,
    /// All defined acceptance checks passed on an identified candidate commit and environment.
    Verified,
    /// A concrete missing dependency, permission, platform limitation or unresolved failure prevents progress.
    Blocked,
    /// A capability is intentionally unavailable on a given platform or build.
    NotSupported,
    /// Requires special qualification and cannot be presented as stable functionality.
    Experimental,
}

impl fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted => write!(f, "NOT_STARTED"),
            Self::ImplementedUnverified => write!(f, "IMPLEMENTED_UNVERIFIED"),
            Self::PartiallyVerified => write!(f, "PARTIALLY_VERIFIED"),
            Self::Verified => write!(f, "VERIFIED"),
            Self::Blocked => write!(f, "BLOCKED"),
            Self::NotSupported => write!(f, "NOT_SUPPORTED"),
            Self::Experimental => write!(f, "EXPERIMENTAL"),
        }
    }
}

/// Truthful verification disposition for test results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestDisposition {
    Passed,
    Failed,
    Blocked,
    Partial,
    Unavailable,
    NotRun,
}

impl fmt::Display for TestDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::Failed => write!(f, "failed"),
            Self::Blocked => write!(f, "blocked"),
            Self::Partial => write!(f, "partial"),
            Self::Unavailable => write!(f, "unavailable"),
            Self::NotRun => write!(f, "not_run"),
        }
    }
}
