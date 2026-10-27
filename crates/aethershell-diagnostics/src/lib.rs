//! Diagnostics, health tracking, and privacy redaction for AetherShell.

pub mod benchmark;
pub mod health;
pub mod redaction;
pub mod runtime_benchmark;

pub use benchmark::{
    query_process_memory_mb, query_process_working_set_mb, run_benchmark, BenchmarkReport,
    PerformanceBudgets,
};
pub use health::{SubsystemHealth, SystemDiagnosticReport};
pub use redaction::redact_sensitive_info;
pub use runtime_benchmark::{
    calculate_percentile, find_running_core_process, validate_process_identity, BenchmarkVerdict,
    BudgetClassification, DisplayTopologyInfo, EnvironmentMetadata, GpuMetricStatus,
    PresentationMetricStatus, ProcessAggregateMetrics, ProcessAnomalyEvent, ProcessIdentityInfo,
    ProcessMetricsSample, ProcessRole, ProcessSampler, ProcessValidationError, RepetitionRecord,
    ResourceBudget, RuntimeBenchmarkLedger, RuntimeBenchmarkSession, WakeupMetricStatus,
    WorkloadProfile,
};
