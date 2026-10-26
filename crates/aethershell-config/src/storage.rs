use crate::schema::{AppConfig, CURRENT_SCHEMA_VERSION};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

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
    #[error("Atomic rename failed: {0}")]
    AtomicRename(String),
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

    /// Loads the configuration, falling back to backup if corrupted, or default if missing.
    ///
    /// Resolves Finding IVA-002: When recovering from backup, heals `config.toml` immediately
    /// so the corrupt file is not retained on disk.
    pub fn load_or_default(&self) -> Result<AppConfig, ConfigStorageError> {
        if !self.config_path.exists() {
            let default_cfg = AppConfig::default();
            self.save_atomic(&default_cfg)?;
            return Ok(default_cfg);
        }

        let content = fs::read_to_string(&self.config_path)?;
        match toml::from_str::<AppConfig>(&content) {
            Ok(cfg) => {
                if cfg.schema_version != CURRENT_SCHEMA_VERSION {
                    return Err(ConfigStorageError::UnsupportedSchemaVersion(
                        cfg.schema_version,
                        CURRENT_SCHEMA_VERSION.into(),
                    ));
                }
                Ok(cfg)
            }
            Err(e) => {
                // If corrupted, attempt backup recovery
                if self.backup_path.exists() {
                    let backup_content = fs::read_to_string(&self.backup_path)?;
                    if let Ok(backup_cfg) = toml::from_str::<AppConfig>(&backup_content) {
                        tracing::warn!("Recovered configuration from backup after detecting corruption in primary config");
                        // Heal primary config file immediately with known-good backup
                        let _ = fs::write(&self.config_path, &backup_content);
                        return Ok(backup_cfg);
                    }
                }
                Err(ConfigStorageError::Toml(e))
            }
        }
    }

    /// Saves the configuration atomically using a temporary file and preserves the prior version as backup.
    ///
    /// Resolves Finding IVA-002: Only promotes `config_path` to `backup_path` if the existing file parses cleanly,
    /// preventing backup poisoning from corrupt or half-written files.
    pub fn save_atomic(&self, config: &AppConfig) -> Result<(), ConfigStorageError> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = toml::to_string_pretty(config)?;
        let temp_path = self
            .config_path
            .with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

        fs::write(&temp_path, &serialized)?;

        // Only promote the existing config to backup if it currently parses validly
        if self.config_path.exists() {
            if let Ok(existing_content) = fs::read_to_string(&self.config_path) {
                if toml::from_str::<AppConfig>(&existing_content).is_ok() {
                    if let Err(copy_err) = fs::copy(&self.config_path, &self.backup_path) {
                        tracing::warn!("Failed to create configuration backup: {}", copy_err);
                    }
                } else {
                    tracing::warn!("Existing config on disk is invalid; preserving existing backup from being overwritten");
                }
            }
        }

        // Atomic replacement on Windows via standard rename
        fs::rename(&temp_path, &self.config_path).map_err(|e| {
            let _ = fs::remove_file(&temp_path);
            ConfigStorageError::Io(e)
        })?;

        Ok(())
    }
}

/// Resolves the application data directory following packaging identity rules (PRD Section 16 & Gate G20 / Q38:B).
///
/// Per PRD Q38:B: Portable application binaries use canonical AppData settings, not portable settings beside executable.
/// Consistently resolves configuration to `%LOCALAPPDATA%\AetherShell` (or fallback `./data` if LOCALAPPDATA is unset).
pub fn resolve_app_data_dir() -> PathBuf {
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
}
