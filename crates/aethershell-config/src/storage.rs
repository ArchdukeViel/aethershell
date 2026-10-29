use crate::migration::{migrate_toml_string, MigrationError, SchemaVersion};
use crate::schema::{is_supported_schema_version, AppConfig, SUPPORTED_SCHEMA_VERSIONS};
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, MoveFileExW, ReplaceFileW, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, REPLACE_FILE_FLAGS,
};

const GENERIC_READ_WRITE: u32 = 0x8000_0000 | 0x4000_0000;

#[derive(Debug, Error)]
pub enum ConfigStorageError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("TOML serialization/deserialization error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("Unsupported schema version: '{0}', expected '{1}'")]
    UnsupportedSchemaVersion(String, String),
    #[error("Configuration validation error: {0}")]
    Validation(#[from] crate::schema::ConfigValidationError),
    #[error("Atomic rename failed: {0}")]
    AtomicRename(String),
    #[error("Concurrent write conflict: {0}")]
    ConcurrentConflict(String),
    #[error("Schema migration error: {0}")]
    Migration(#[from] MigrationError),
}

/// RAII guard representing an exclusive Win32 file lock on `config.lock`.
///
/// Prevents concurrent cross-process write collisions between AetherShell instances (Gate G12).
pub struct ConfigLockGuard {
    handle: HANDLE,
    lock_path: PathBuf,
}

impl Drop for ConfigLockGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Manages loading, saving, and atomic rollbacks of AetherShell configurations.
pub struct ConfigManager {
    config_path: PathBuf,
    backup_path: PathBuf,
}

impl ConfigManager {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            config_path: config_dir.join("config.toml"),
            backup_path: config_dir.join("config.toml.bak"),
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }

    pub fn lock_path(&self) -> PathBuf {
        self.config_path.with_extension("lock")
    }

    /// Attempts to acquire an exclusive, non-blocking Win32 file lock on `config.lock`.
    ///
    /// Resolves Gate G12: Prevents concurrent instance write collisions across multiple running processes.
    /// If another process is holding the lock, returns `ConfigStorageError::ConcurrentConflict`.
    pub fn acquire_exclusive_lock(&self) -> Result<ConfigLockGuard, ConfigStorageError> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let lock_path = self.lock_path();
        let lock_u16: Vec<u16> = lock_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                PCWSTR(lock_u16.as_ptr()),
                GENERIC_READ_WRITE,
                FILE_SHARE_MODE(0), // Exclusive: 0 sharing allowed
                None,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };

        match handle {
            Ok(h) if !h.is_invalid() => Ok(ConfigLockGuard {
                handle: h,
                lock_path,
            }),
            _ => {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(32) {
                    // ERROR_SHARING_VIOLATION (os error 32)
                    Err(ConfigStorageError::ConcurrentConflict(
                        "Another AetherShell process currently holds the configuration lock (ERROR_SHARING_VIOLATION)".into(),
                    ))
                } else {
                    Err(ConfigStorageError::Io(err))
                }
            }
        }
    }

    /// Saves the configuration atomically under an exclusive write lock.
    pub fn save_atomic_exclusive(&self, config: &AppConfig) -> Result<(), ConfigStorageError> {
        let _lock = self.acquire_exclusive_lock()?;
        self.save_atomic(config)
    }

    /// Reversibly migrates the configuration on disk to the target schema version.
    ///
    /// Resolves Gate G12: Supports forward and rollback migrations while preserving pre-migration backup.
    pub fn migrate_to(&self, target_version: SchemaVersion) -> Result<String, ConfigStorageError> {
        let _lock = self.acquire_exclusive_lock()?;

        if !self.config_path.exists() {
            let default_cfg = AppConfig::default();
            self.save_atomic(&default_cfg)?;
            return Ok(toml::to_string_pretty(&default_cfg)?);
        }

        let content = fs::read_to_string(&self.config_path)?;
        let migrated_str = migrate_toml_string(&content, target_version)?;

        // Validate that migrated content is syntactically valid TOML
        let _: toml::Table = toml::from_str(&migrated_str)?;

        // Save pre-migration backup before writing
        let pre_backup = self.config_path.with_extension("pre_migrate.bak");
        fs::copy(&self.config_path, &pre_backup).map_err(ConfigStorageError::Io)?;

        self.heal_from_backup(&migrated_str)?;
        Ok(migrated_str)
    }

    /// Loads the configuration if the file exists on disk, healing from backup if corrupted.
    /// If the file does not exist, returns `Ok(None)` without writing anything to disk.
    pub fn load_if_exists(&self) -> Result<Option<AppConfig>, ConfigStorageError> {
        if !self.config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.config_path)?;
        match toml::from_str::<AppConfig>(&content) {
            Ok(cfg) => {
                if !is_supported_schema_version(&cfg.schema_version) {
                    return Err(ConfigStorageError::UnsupportedSchemaVersion(
                        cfg.schema_version,
                        format!("{:?}", SUPPORTED_SCHEMA_VERSIONS),
                    ));
                }
                cfg.validate()?;
                Ok(Some(cfg))
            }
            Err(e) => {
                if self.backup_path.exists() {
                    let backup_content = fs::read_to_string(&self.backup_path)?;
                    if let Ok(backup_cfg) = toml::from_str::<AppConfig>(&backup_content) {
                        if !is_supported_schema_version(&backup_cfg.schema_version) {
                            return Err(ConfigStorageError::UnsupportedSchemaVersion(
                                backup_cfg.schema_version,
                                format!("{:?}", SUPPORTED_SCHEMA_VERSIONS),
                            ));
                        }
                        backup_cfg.validate()?;
                        tracing::warn!("Recovered configuration from backup after detecting corruption in primary config");
                        self.heal_from_backup(&backup_content)?;
                        return Ok(Some(backup_cfg));
                    }
                }
                Err(ConfigStorageError::Toml(e))
            }
        }
    }

    /// Loads the configuration, falling back to backup if corrupted, or default if missing.
    ///
    /// Resolves Finding IVA-002: When recovering from backup, heals `config.toml` safely and atomically
    /// so the corrupt file is not retained on disk and backup is not damaged.
    pub fn load_or_default(&self) -> Result<AppConfig, ConfigStorageError> {
        if !self.config_path.exists() {
            let default_cfg = AppConfig::default();
            self.save_atomic(&default_cfg)?;
            return Ok(default_cfg);
        }

        let content = fs::read_to_string(&self.config_path)?;
        match toml::from_str::<AppConfig>(&content) {
            Ok(cfg) => {
                if !is_supported_schema_version(&cfg.schema_version) {
                    return Err(ConfigStorageError::UnsupportedSchemaVersion(
                        cfg.schema_version,
                        format!("{:?}", SUPPORTED_SCHEMA_VERSIONS),
                    ));
                }
                cfg.validate()?;
                Ok(cfg)
            }
            Err(e) => {
                // If corrupted, attempt backup recovery
                if self.backup_path.exists() {
                    let backup_content = fs::read_to_string(&self.backup_path)?;
                    if let Ok(backup_cfg) = toml::from_str::<AppConfig>(&backup_content) {
                        if !is_supported_schema_version(&backup_cfg.schema_version) {
                            return Err(ConfigStorageError::UnsupportedSchemaVersion(
                                backup_cfg.schema_version,
                                format!("{:?}", SUPPORTED_SCHEMA_VERSIONS),
                            ));
                        }
                        backup_cfg.validate()?;
                        tracing::warn!("Recovered configuration from backup after detecting corruption in primary config");
                        // Safely and atomically heal primary config file with known-good backup
                        self.heal_from_backup(&backup_content)?;
                        return Ok(backup_cfg);
                    }
                }
                Err(ConfigStorageError::Toml(e))
            }
        }
    }

    /// Safely and atomically heals the primary config file from backup content without
    /// modifying or overwriting the backup file itself.
    pub fn heal_from_backup(&self, backup_content: &str) -> Result<(), ConfigStorageError> {
        let temp_path = self
            .config_path
            .with_extension(format!("tmp.heal.{}", uuid::Uuid::new_v4()));

        fs::write(&temp_path, backup_content.as_bytes())?;

        let temp_u16: Vec<u16> = temp_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dest_u16: Vec<u16> = self
            .config_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut replace_ok = false;
        let mut last_err = io::Error::other("Atomic healing ReplaceFileW failed");

        for attempt in 0..4 {
            // Per Microsoft documentation, REPLACEFILE_WRITE_THROUGH (0x1) is unsupported.
            // Using REPLACEFILE_FLAGS(0) with NULL lpBackupFileName to ensure backup is preserved.
            let res = unsafe {
                ReplaceFileW(
                    PCWSTR(dest_u16.as_ptr()),
                    PCWSTR(temp_u16.as_ptr()),
                    PCWSTR::null(),
                    REPLACE_FILE_FLAGS(0),
                    None,
                    None,
                )
            };

            if res.is_ok() {
                replace_ok = true;
                break;
            }

            last_err = io::Error::last_os_error();
            if attempt < 3 {
                std::thread::sleep(std::time::Duration::from_millis(30 * (attempt + 1) as u64));
            }
        }

        if !replace_ok {
            // Fallback to MoveFileExW if destination file did not exist
            let move_res = unsafe {
                MoveFileExW(
                    PCWSTR(temp_u16.as_ptr()),
                    PCWSTR(dest_u16.as_ptr()),
                    MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING,
                )
            };
            let _ = fs::remove_file(&temp_path);
            if move_res.is_err() {
                let err = io::Error::last_os_error();
                return Err(ConfigStorageError::AtomicRename(format!(
                    "Failed to atomically heal config file from backup: {}. Last ReplaceFileW error: {}",
                    err, last_err
                )));
            }
        } else {
            let _ = fs::remove_file(&temp_path);
        }

        Ok(())
    }

    /// Saves the configuration atomically using Win32 `ReplaceFileW` and preserves the prior version as backup.
    ///
    /// Resolves Finding IVA-002 and Owner Directive: Only promotes `config_path` to `backup_path` if the existing
    /// file parses cleanly, preventing backup poisoning. Employs genuine Windows atomic file replacement (`ReplaceFileW`)
    /// without direct-copy fallback to guarantee zero corruption or partial writes.
    pub fn save_atomic(&self, config: &AppConfig) -> Result<(), ConfigStorageError> {
        config.validate()?;

        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = toml::to_string_pretty(config)?;
        // Sanity pre-validation: ensure serialized configuration parses back cleanly
        let _parsed: AppConfig = toml::from_str(&serialized)?;

        let temp_path = self
            .config_path
            .with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

        fs::write(&temp_path, &serialized)?;

        let temp_u16: Vec<u16> = temp_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dest_u16: Vec<u16> = self
            .config_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut save_ok = false;
        let mut last_err = std::io::Error::other("Atomic replacement failed");

        for attempt in 0..5 {
            if self.config_path.exists() {
                // Determine whether destination should be backed up
                let dest_is_valid =
                    if let Ok(existing_content) = fs::read_to_string(&self.config_path) {
                        toml::from_str::<AppConfig>(&existing_content).is_ok()
                    } else {
                        false
                    };

                let backup_u16: Vec<u16> = if dest_is_valid {
                    self.backup_path
                        .as_os_str()
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect()
                } else {
                    tracing::warn!(
                        "Existing config on disk is invalid; preserving existing backup from being overwritten"
                    );
                    Vec::new()
                };

                let lp_backup = if dest_is_valid {
                    PCWSTR(backup_u16.as_ptr())
                } else {
                    PCWSTR::null()
                };

                // Per Microsoft documentation: REPLACEFILE_WRITE_THROUGH (0x00000001) is explicitly not supported.
                // We use REPLACE_FILE_FLAGS(0).
                let res = unsafe {
                    ReplaceFileW(
                        PCWSTR(dest_u16.as_ptr()),
                        PCWSTR(temp_u16.as_ptr()),
                        lp_backup,
                        REPLACE_FILE_FLAGS(0),
                        None,
                        None,
                    )
                };

                if res.is_ok() {
                    save_ok = true;
                    break;
                }
                last_err = std::io::Error::last_os_error();
            } else {
                let res = unsafe {
                    MoveFileExW(
                        PCWSTR(temp_u16.as_ptr()),
                        PCWSTR(dest_u16.as_ptr()),
                        MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING,
                    )
                };

                if res.is_ok() {
                    save_ok = true;
                    break;
                }
                last_err = std::io::Error::last_os_error();
            }

            if attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(30 * (attempt + 1) as u64));
            }
        }

        // Clean up temporary file in all paths (ReplaceFileW/MoveFileExW delete temp on success, but guarantee cleanup)
        let _ = fs::remove_file(&temp_path);

        if !save_ok {
            return Err(ConfigStorageError::AtomicRename(format!(
                "Atomic file replacement failed: {}. Live configuration preserved untouched.",
                last_err
            )));
        }

        Ok(())
    }
}

/// Resolves the application data directory following packaging identity rules (PRD Section 16 & Gate G20 / Q38:B).
///
/// Per PRD Q38:B: Portable application binaries use canonical AppData settings, not portable settings beside executable.
/// Consistently resolves configuration to `%LOCALAPPDATA%\AetherShell` (or fallback `./data` if LOCALAPPDATA is unset).
pub fn resolve_app_data_dir() -> PathBuf {
    if let Ok(appdata_override) = std::env::var("AETHERSHELL_APP_DATA_DIR") {
        let path = PathBuf::from(appdata_override);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    if let Ok(override_path) = std::env::var("AETHERSHELL_CONFIG_DIR") {
        let path = PathBuf::from(override_path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    resolve_app_data_dir_from_base(
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from),
    )
}

/// Internal helper for deterministic testing of portable vs installed AppData semantics.
///
/// Per PRD Q38:B and Gate G20, canonical AppData resolution (`%LOCALAPPDATA%\AetherShell`)
/// is consistently enforced across all distribution channels, including portable packages.
/// Executable directory redirects (e.g. `<exe_dir>\data`) are strictly eliminated to prevent
/// fragmented duplicate configuration stores.
pub fn resolve_app_data_dir_from_base(
    _exe_dir: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
) -> PathBuf {
    if let Some(appdata) = local_appdata {
        appdata.join("AetherShell")
    } else {
        PathBuf::from("data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_save_and_load() {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_test_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        let initial = manager.load_or_default().expect("load or default");
        assert_eq!(initial.schema_version, "1.0.0");
        assert_eq!(initial.profile, "signature_minimal");

        let mut modified = initial.clone();
        modified.general.language = "ja-JP".into();
        manager.save_atomic(&modified).expect("save atomic");

        let loaded = manager.load_or_default().expect("reload");
        assert_eq!(loaded.general.language, "ja-JP");
        assert!(manager.backup_path.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_atomic_preserves_original_on_invalid_data() {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_atomic_neg_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        let initial = manager.load_or_default().expect("load default");
        assert_eq!(initial.profile, "signature_minimal");

        // Second save atomically replaces config and populates backup
        manager
            .save_atomic(&initial)
            .expect("second save creates backup");
        assert!(manager.backup_path.exists());

        // Corrupt config.toml directly with garbage text
        fs::write(&manager.config_path, "GARBAGE TOML [[[ :::").unwrap();
        // But backup exists from previous valid save!
        let recovered = manager.load_or_default().expect("recovered from backup");
        assert_eq!(recovered.schema_version, "1.0.0");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_backup_poisoning_prevention_iva_002() {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_poison_test_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        // 1. Save valid initial config twice to ensure backup is populated
        let initial = AppConfig::default();
        manager.save_atomic(&initial).expect("save initial");
        manager
            .save_atomic(&initial)
            .expect("save second to populate backup");
        assert!(
            manager.backup_path.exists(),
            "Backup must exist before corruption test"
        );

        // 2. Corrupt config.toml directly
        fs::write(&manager.config_path, "INVALID TOML [[[[").expect("corrupt config");

        // 3. load_or_default should recover from backup and heal config.toml
        let recovered = manager.load_or_default().expect("recovery from backup");
        assert_eq!(recovered.profile, "signature_minimal");

        // 4. Verify config.toml is healed
        let healed_content = fs::read_to_string(&manager.config_path).expect("read healed config");
        assert!(toml::from_str::<AppConfig>(&healed_content).is_ok());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_portable_mode_app_data_resolution_g20() {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_portable_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let fake_local_appdata = PathBuf::from(r"C:\Users\Test\AppData\Local");

        // 1. Without marker -> resolves to %LOCALAPPDATA%\AetherShell
        let normal = resolve_app_data_dir_from_base(
            Some(temp_dir.clone()),
            Some(fake_local_appdata.clone()),
        );
        assert_eq!(normal, fake_local_appdata.join("AetherShell"));

        // 2. Per PRD Q38:B / Gate G20, portable mode consistently resolves to canonical
        // %LOCALAPPDATA%\AetherShell instead of creating competing settings beside the executable.
        let marker = temp_dir.join("aethershell.portable");
        fs::write(&marker, "").unwrap();

        let portable = resolve_app_data_dir_from_base(
            Some(temp_dir.clone()),
            Some(fake_local_appdata.clone()),
        );
        assert_eq!(portable, fake_local_appdata.join("AetherShell"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_concurrent_atomic_writers() {
        use std::sync::Arc;
        use std::thread;

        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_concurrent_{}", uuid::Uuid::new_v4()));
        let manager = Arc::new(ConfigManager::new(&temp_dir));

        let initial = manager.load_or_default().expect("load default");
        assert_eq!(initial.schema_version, "1.0.0");

        let mut handles = Vec::new();
        for i in 0..6 {
            let mgr = Arc::clone(&manager);
            let mut cfg = initial.clone();
            cfg.general.language = format!("en-US-{}", i);
            handles.push(thread::spawn(move || mgr.save_atomic(&cfg)));
        }

        let mut successes = 0;
        let mut rename_collisions = 0;
        for h in handles {
            match h.join().expect("thread join") {
                Ok(()) => successes += 1,
                Err(ConfigStorageError::AtomicRename(_)) => rename_collisions += 1,
                Err(other) => panic!("Unexpected concurrent writer error: {:?}", other),
            }
        }

        assert!(successes >= 1, "At least one atomic writer must succeed");
        assert_eq!(
            successes + rename_collisions,
            6,
            "Every concurrent writer must have a verified outcome (success or AtomicRename collision)"
        );

        // Must load cleanly and parse validly without corruption
        let final_cfg = manager.load_or_default().expect("load final config");
        assert!(final_cfg.general.language.starts_with("en-US"));
        assert!(manager.config_path.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_recovery_after_restart_and_interrupted_write() {
        let temp_dir = std::env::temp_dir().join(format!(
            "aethershell_restart_recovery_{}",
            uuid::Uuid::new_v4()
        ));
        let manager = ConfigManager::new(&temp_dir);

        // 1. Initial valid saves to establish state and backup
        let mut initial = AppConfig::default();
        initial.general.language = "de-DE".into();
        manager.save_atomic(&initial).expect("save initial");
        manager
            .save_atomic(&initial)
            .expect("save second to populate backup");
        assert!(manager.config_path.exists());
        assert!(manager.backup_path.exists());

        // 2. Simulate interrupted write: leave an orphaned .tmp file
        let orphan_tmp = temp_dir.join("config.toml.tmp.orphan");
        fs::write(&orphan_tmp, "INCOMPLETE TOML PAYLOAD").unwrap();

        // 3. Simulate sudden restart / power loss corrupting live config.toml
        fs::write(&manager.config_path, "CORRUPT [TOML").unwrap();

        // 4. Recovery after restart: load_or_default heals live config from valid backup
        let new_manager_instance = ConfigManager::new(&temp_dir);
        let recovered = new_manager_instance
            .load_or_default()
            .expect("load_or_default must recover from backup after restart");
        assert_eq!(recovered.general.language, "de-DE");

        // 5. Verify live config.toml is healed and parseable
        let healed_content = fs::read_to_string(&manager.config_path).expect("read healed config");
        let parsed: Result<AppConfig, _> = toml::from_str(&healed_content);
        assert!(parsed.is_ok(), "Healed config must be valid TOML");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_replacement_failure_preserves_live_config_untouched() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_replace_fail_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        let initial = AppConfig::default();
        manager.save_atomic(&initial).expect("initial save");
        let original_content = fs::read_to_string(&manager.config_path).unwrap();

        // Lock destination with exclusive access (share_mode 0 = deny read, write, and delete sharing)
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&manager.config_path);

        if let Ok(_locked) = lock_file {
            let mut updated = initial.clone();
            updated.general.language = "fr-FR".into();

            // Attempt save: ReplaceFileW will fail due to sharing violation
            let res = manager.save_atomic(&updated);

            // Must fail safely with AtomicRename error
            assert!(res.is_err(), "Save to locked destination must fail");
            match res.err().unwrap() {
                ConfigStorageError::AtomicRename(msg) => {
                    assert!(msg.contains("Live configuration preserved untouched"));
                }
                other => panic!("Expected AtomicRename error, got {:?}", other),
            }

            // Drop lock
            drop(_locked);

            // Verify live file was preserved untouched
            let after_content = fs::read_to_string(&manager.config_path).unwrap();
            assert_eq!(original_content, after_content);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_atomic_recovery_heals_corrupt_config_and_preserves_backup() {
        let temp_dir = std::env::temp_dir().join(format!(
            "aethershell_heal_backup_preservation_{}",
            uuid::Uuid::new_v4()
        ));
        let manager = ConfigManager::new(&temp_dir);

        // 1. Establish valid config and backup
        let mut initial = AppConfig::default();
        initial.general.language = "en-GB".into();
        manager.save_atomic(&initial).expect("initial save");
        manager
            .save_atomic(&initial)
            .expect("second save populating backup");

        let backup_content_before = fs::read_to_string(&manager.backup_path).expect("read backup");

        // 2. Corrupt live config
        fs::write(&manager.config_path, "COMPLETELY_INVALID_TOML_DATA[[[").expect("corrupt live");

        // 3. Trigger load_or_default: heals config.toml atomically
        let recovered = manager.load_or_default().expect("load_or_default recovery");
        assert_eq!(recovered.general.language, "en-GB");

        // 4. Verify backup was preserved completely intact
        let backup_content_after =
            fs::read_to_string(&manager.backup_path).expect("read backup after");
        assert_eq!(
            backup_content_before, backup_content_after,
            "Backup must remain unaltered during recovery"
        );

        // 5. Verify live config is valid TOML
        let healed_live = fs::read_to_string(&manager.config_path).expect("read healed live");
        assert!(toml::from_str::<AppConfig>(&healed_live).is_ok());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_atomic_recovery_failure_on_locked_destination() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_heal_locked_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        let initial = AppConfig::default();
        manager.save_atomic(&initial).expect("initial save");
        manager.save_atomic(&initial).expect("backup save");

        let backup_content_before = fs::read_to_string(&manager.backup_path).expect("read backup");

        // Corrupt live config
        fs::write(&manager.config_path, "CORRUPT [CONFIG").expect("corrupt config");

        // Lock live config denying write/delete sharing (share_mode 1 = FILE_SHARE_READ: allows reading to discover corruption)
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(1)
            .open(&manager.config_path);

        if let Ok(_locked) = lock_file {
            // Attempt recovery while live config is exclusively locked: ReplaceFileW will fail
            let res = manager.load_or_default();
            assert!(
                res.is_err(),
                "Recovery must fail when primary file is exclusively locked"
            );
            match res.err().unwrap() {
                ConfigStorageError::AtomicRename(msg) => {
                    assert!(msg.contains("Failed to atomically heal config file from backup"));
                }
                other => panic!("Expected AtomicRename error, got {:?}", other),
            }

            // Drop lock
            drop(_locked);

            // Backup must still be preserved intact
            let backup_content_after =
                fs::read_to_string(&manager.backup_path).expect("read backup after");
            assert_eq!(backup_content_before, backup_content_after);

            // Now recovery succeeds cleanly
            let second_recovery = manager
                .load_or_default()
                .expect("recovery after lock released");
            assert_eq!(second_recovery.profile, "signature_minimal");
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_or_default_rejects_backup_with_unsupported_schema_version() {
        let temp_dir =
            std::env::temp_dir().join(format!("aethershell_schema_check_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        // Corrupt live config
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(&manager.config_path, "INVALID TOML").unwrap();

        // Write backup with incompatible schema version 999.0
        let incompatible = AppConfig {
            schema_version: "999.0".into(),
            ..Default::default()
        };
        let incompatible_toml = toml::to_string_pretty(&incompatible).unwrap();
        fs::write(&manager.backup_path, incompatible_toml).unwrap();

        let res = manager.load_or_default();
        assert!(
            res.is_err(),
            "Recovery must fail when backup has unsupported schema version"
        );
        match res.err().unwrap() {
            ConfigStorageError::UnsupportedSchemaVersion(actual, expected) => {
                assert_eq!(actual, "999.0");
                assert_eq!(expected, format!("{:?}", SUPPORTED_SCHEMA_VERSIONS));
            }
            other => panic!("Expected UnsupportedSchemaVersion, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_if_exists_behavior() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_load_if_exists_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        // When no file exists, returns Ok(None) without writing to disk
        assert_eq!(manager.load_if_exists().unwrap(), None);
        assert!(!manager.config_path().exists());

        // Save a valid config
        let cfg = AppConfig::default();
        manager.save_atomic(&cfg).unwrap();
        assert!(manager.config_path().exists());

        // Now load_if_exists returns Ok(Some(cfg))
        assert_eq!(manager.load_if_exists().unwrap(), Some(cfg));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_atomic_rejects_invalid_configuration() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_save_invalid_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        let invalid_cfg = AppConfig {
            audio: crate::schema::AudioConfig {
                render_fps: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let res = manager.save_atomic(&invalid_cfg);
        assert!(res.is_err());
        assert!(!manager.config_path().exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_acquire_exclusive_lock_and_concurrent_conflict() {
        let temp_dir = std::env::temp_dir().join(format!("test_lock_{}", uuid::Uuid::new_v4()));
        let manager1 = ConfigManager::new(&temp_dir);
        let manager2 = ConfigManager::new(&temp_dir);

        // 1. First manager acquires exclusive lock
        let lock1 = manager1.acquire_exclusive_lock().expect("lock1 acquire");
        assert!(manager1.lock_path().exists());

        // 2. Second manager attempts to acquire lock on same directory -> must fail with ConcurrentConflict
        let lock2_res = manager2.acquire_exclusive_lock();
        assert!(
            lock2_res.is_err(),
            "Concurrent lock acquisition must fail while first lock is held"
        );
        match lock2_res.err().unwrap() {
            ConfigStorageError::ConcurrentConflict(msg) => {
                assert!(msg.contains("ERROR_SHARING_VIOLATION"));
            }
            other => panic!("Expected ConcurrentConflict, got {:?}", other),
        }

        // 3. Drop first lock -> releases handle and removes lock file
        drop(lock1);

        // 4. Now second manager can acquire cleanly
        let lock2 = manager2.acquire_exclusive_lock().expect("lock2 acquire");
        drop(lock2);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_migrate_to_updates_disk_and_creates_backup() {
        let temp_dir = std::env::temp_dir().join(format!("test_migrate_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        // 1. Save initial 1.0.0 config
        let initial = AppConfig::default();
        manager.save_atomic(&initial).expect("initial save");

        // 2. Perform forward migration to 1.1.0
        let migrated_str = manager
            .migrate_to(SchemaVersion::V1_1_0)
            .expect("migration to 1.1.0");
        assert!(migrated_str.contains(r#"schema_version = "1.1.0""#));

        // 3. Verify on-disk file was updated
        let on_disk_str = fs::read_to_string(&manager.config_path).expect("read on-disk config");
        assert!(on_disk_str.contains(r#"schema_version = "1.1.0""#));
        assert!(on_disk_str.contains("dynamic_battery_backoff_ms = 5000"));

        // 4. Verify pre-migration backup was created
        let pre_backup = manager.config_path.with_extension("pre_migrate.bak");
        assert!(pre_backup.exists(), "pre_migrate.bak must exist");
        let backup_str = fs::read_to_string(&pre_backup).expect("read pre_migrate.bak");
        assert!(backup_str.contains(r#"schema_version = "1.0.0""#));

        // 5. Reversible rollback migration back to 1.0.0
        let rolled_back_str = manager
            .migrate_to(SchemaVersion::V1_0_0)
            .expect("rollback to 1.0.0");
        assert!(rolled_back_str.contains(r#"schema_version = "1.0.0""#));

        let on_disk_v1 = fs::read_to_string(&manager.config_path).expect("read on-disk v1");
        assert!(on_disk_v1.contains(r#"schema_version = "1.0.0""#));
        assert!(!on_disk_v1.contains("dynamic_battery_backoff_ms"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_migration_followed_by_normal_application_load() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_migrate_load_{}", uuid::Uuid::new_v4()));
        let manager = ConfigManager::new(&temp_dir);

        // 1. Initial 1.0.0 config load_or_default creates default config
        let initial = manager
            .load_or_default()
            .expect("load initial 1.0.0 default");
        assert_eq!(initial.schema_version, "1.0.0");

        // 2. Migrate to 1.1.0 and load via load_or_default and load_if_exists
        manager
            .migrate_to(SchemaVersion::V1_1_0)
            .expect("migrate to 1.1.0");
        let v1_1 = manager.load_or_default().expect("load migrated 1.1.0");
        assert_eq!(v1_1.schema_version, "1.1.0");
        assert_eq!(v1_1.telemetry.dynamic_battery_backoff_ms, 5000);
        assert_eq!(
            manager.load_if_exists().unwrap().unwrap().schema_version,
            "1.1.0"
        );

        // 3. Migrate to 2.0.0 and customize runtime
        manager
            .migrate_to(SchemaVersion::V2_0_0)
            .expect("migrate to 2.0.0");
        let mut v2 = manager.load_or_default().expect("load migrated 2.0.0");
        assert_eq!(v2.schema_version, "2.0.0");
        assert_eq!(v2.runtime.ipc_timeout_ms, 1500);

        // Customize runtime settings
        v2.runtime.ipc_timeout_ms = 4500;
        v2.runtime.job_object_memory_limit_mb = 256;
        manager.save_atomic(&v2).expect("save customized 2.0.0");

        let loaded_v2 = manager.load_or_default().expect("load customized 2.0.0");
        assert_eq!(loaded_v2.runtime.ipc_timeout_ms, 4500);

        // 4. Reversible rollback to 1.1.0 and then 1.0.0 - verify load_or_default continues to work cleanly
        manager
            .migrate_to(SchemaVersion::V1_1_0)
            .expect("rollback to 1.1.0");
        let loaded_v1_1 = manager.load_or_default().expect("load rolled back 1.1.0");
        assert_eq!(loaded_v1_1.schema_version, "1.1.0");

        manager
            .migrate_to(SchemaVersion::V1_0_0)
            .expect("rollback to 1.0.0");
        let loaded_v1_0 = manager.load_or_default().expect("load rolled back 1.0.0");
        assert_eq!(loaded_v1_0.schema_version, "1.0.0");

        // 5. Migrate back forward to 2.0.0 - customized settings preserved non-destructively
        manager
            .migrate_to(SchemaVersion::V2_0_0)
            .expect("re-upgrade to 2.0.0");
        let restored_v2 = manager.load_or_default().expect("load re-upgraded 2.0.0");
        assert_eq!(restored_v2.schema_version, "2.0.0");
        assert_eq!(restored_v2.runtime.ipc_timeout_ms, 4500);
        assert_eq!(restored_v2.runtime.job_object_memory_limit_mb, 256);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
