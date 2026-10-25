//! Taskbar customization engine, backends, and conflict coordination.

pub mod backend;
pub mod coordinator;

pub use backend::{
    ExternalAppearanceBackend, NativeNoopBackend, TaskbarBackend, TaskbarBackendError,
};
pub use coordinator::TaskbarCoordinator;
