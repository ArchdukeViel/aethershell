//! Taskbar customization engine, backends, and conflict coordination.

pub mod backend;
pub mod coordinator;
pub mod recovery;

pub use backend::{
    ExplorerXamlExperimentalBackend, ExternalAppearanceBackend, NativeNoopBackend, TaskbarBackend,
    TaskbarBackendError,
};
pub use coordinator::TaskbarCoordinator;
pub use recovery::{
    clean_exit_event_name, heartbeat_event_name, restore_system_taskbars, HeartbeatPulseGuard,
    RecoveryError, TaskbarRecoveryCoordinator, TaskbarRecoveryEvent, TaskbarRecoveryWatchdog,
    TaskbarRestorationReport, WatchdogCheckResult, WatchdogExitVerdict,
};
