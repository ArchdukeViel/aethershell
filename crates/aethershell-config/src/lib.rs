//! Configuration schema, validation, path containment, and persistence for AetherShell.

pub mod path_safety;
pub mod schema;
pub mod storage;

pub use path_safety::{safe_join_contained, validate_package_relative_path, PathSafetyError};
pub use schema::{
    AppConfig, AudioConfig, DesktopConfig, GeneralConfig, PowerConfig, TaskbarConfig,
};
pub use storage::{ConfigManager, ConfigStorageError};
