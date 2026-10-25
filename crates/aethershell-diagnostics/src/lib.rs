//! Diagnostics, health tracking, and privacy redaction for AetherShell.

pub mod benchmark;
pub mod health;
pub mod redaction;

pub use benchmark::{query_process_memory_mb, run_benchmark, BenchmarkReport, PerformanceBudgets};
pub use health::{SubsystemHealth, SystemDiagnosticReport};
pub use redaction::redact_sensitive_info;
