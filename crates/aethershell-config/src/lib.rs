//! Configuration schema, validation, path containment, and persistence for AetherShell.

pub mod migration;
pub mod path_safety;
pub mod schema;
pub mod storage;

pub use migration::{migrate_toml_string, MigrationError, SchemaVersion};

pub use path_safety::{safe_join_contained, validate_package_relative_path, PathSafetyError};
pub use schema::{
    AppConfig, AudioConfig, ConfigValidationError, DesktopConfig, GeneralConfig, PowerConfig,
    TaskbarConfig, CURRENT_SCHEMA_VERSION,
};
pub use storage::{
    resolve_app_data_dir, resolve_app_data_dir_from_base, ConfigLockGuard, ConfigManager,
    ConfigStorageError,
};
