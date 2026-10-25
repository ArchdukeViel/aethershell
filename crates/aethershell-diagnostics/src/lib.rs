//! Diagnostics, health tracking, and privacy redaction for AetherShell.

pub mod health;
pub mod redaction;

pub use health::{SubsystemHealth, SystemDiagnosticReport};
pub use redaction::redact_sensitive_info;
