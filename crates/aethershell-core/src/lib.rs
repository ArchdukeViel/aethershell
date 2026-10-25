//! Core data structures, state models, and contracts for AetherShell.
//!
//! This crate contains pure domain logic and is free of platform-specific Win32 or GUI bindings.

pub mod capabilities;
pub mod desktop;
pub mod lifecycle;
pub mod status;
pub mod taskbar;

pub use capabilities::{FeatureCapability, RiskTier};
pub use desktop::{DesktopLayer, DipRect, InteractionMode, LayoutAnchor};
pub use lifecycle::{LifecycleState, RecoveryPrecedence};
pub use status::{TestDisposition, VerificationStatus};
pub use taskbar::{
    resolve_taskbar_rule, MonitorTaskbarState, TaskbarEffect, TaskbarResolution, TaskbarRule,
};
