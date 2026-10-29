use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use toml::Value;

/// Supported schema versions for AetherShell configuration (Gate G12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SchemaVersion {
    V1_0_0,
    V1_1_0,
    V2_0_0,
}

impl SchemaVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
            Self::V1_1_0 => "1.1.0",
            Self::V2_0_0 => "2.0.0",
        }
    }

    pub fn latest() -> Self {
        Self::V1_0_0 // Currently production baseline
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SchemaVersion {
    type Err = MigrationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "1.0.0" => Ok(Self::V1_0_0),
            "1.1.0" => Ok(Self::V1_1_0),
            "2.0.0" => Ok(Self::V2_0_0),
            other => Err(MigrationError::UnsupportedVersion(other.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("Unsupported schema version: '{0}'")]
    UnsupportedVersion(String),
    #[error("Missing schema_version field in configuration")]
    MissingSchemaVersion,
    #[error("Invalid TOML content: {0}")]
    InvalidToml(#[from] toml::de::Error),
    #[error("TOML serialization error: {0}")]
    SerializationError(#[from] toml::ser::Error),
    #[error("Migration step failed: {0}")]
    StepFailed(String),
}

/// Migrates a raw TOML configuration string to the requested target schema version.
///
/// Supports bidirectional migrations:
/// - Forward: `1.0.0 -> 1.1.0`, `1.1.0 -> 2.0.0`
/// - Reversible / Rollback: `2.0.0 -> 1.1.0`, `1.1.0 -> 1.0.0`
///
/// Preserves custom and unknown user fields while adjusting version-specific schemas.
pub fn migrate_toml_string(
    content: &str,
    target_version: SchemaVersion,
) -> Result<String, MigrationError> {
    let mut table: toml::Table = toml::from_str(content)?;

    let current_ver_str = table
        .get("schema_version")
        .and_then(|v| v.as_str())
        .ok_or(MigrationError::MissingSchemaVersion)?;

    let mut current_version = SchemaVersion::from_str(current_ver_str)?;

    if current_version == target_version {
        return Ok(content.to_string());
    }

    // Step-by-step forward or backward migrations
    while current_version != target_version {
        if current_version < target_version {
            // Forward step
            match current_version {
                SchemaVersion::V1_0_0 => {
                    migrate_v1_0_to_v1_1(&mut table)?;
                    current_version = SchemaVersion::V1_1_0;
                }
                SchemaVersion::V1_1_0 => {
                    migrate_v1_1_to_v2_0(&mut table)?;
                    current_version = SchemaVersion::V2_0_0;
                }
                SchemaVersion::V2_0_0 => unreachable!(),
            }
        } else {
            // Reversible backward / downgrade step
            match current_version {
                SchemaVersion::V2_0_0 => {
                    migrate_v2_0_to_v1_1(&mut table)?;
                    current_version = SchemaVersion::V1_1_0;
                }
                SchemaVersion::V1_1_0 => {
                    migrate_v1_1_to_v1_0(&mut table)?;
                    current_version = SchemaVersion::V1_0_0;
                }
                SchemaVersion::V1_0_0 => unreachable!(),
            }
        }
    }

    table.insert(
        "schema_version".to_string(),
        Value::String(target_version.as_str().to_string()),
    );

    let migrated = toml::to_string_pretty(&table)?;
    Ok(migrated)
}

/// Forward migration from 1.0.0 to 1.1.0:
/// Adds fine-grained dirty presentation gating options and battery telemetry interval.
fn migrate_v1_0_to_v1_1(table: &mut toml::Table) -> Result<(), MigrationError> {
    // 1. Update schema version
    table.insert(
        "schema_version".to_string(),
        Value::String("1.1.0".to_string()),
    );

    // 2. Add telemetry.dynamic_battery_backoff_ms if telemetry section exists
    if let Some(Value::Table(telem)) = table.get_mut("telemetry") {
        if !telem.contains_key("dynamic_battery_backoff_ms") {
            telem.insert(
                "dynamic_battery_backoff_ms".to_string(),
                Value::Integer(5000),
            );
        }
    }

    // 3. Add audio.silence_threshold and decay_rate if audio section exists
    if let Some(Value::Table(audio)) = table.get_mut("audio") {
        if !audio.contains_key("silence_threshold") {
            audio.insert("silence_threshold".to_string(), Value::Float(0.001));
        }
        if !audio.contains_key("decay_rate") {
            audio.insert("decay_rate".to_string(), Value::Float(0.85));
        }
    }

    // 4. Add desktop.enable_dirty_presentation_gating if desktop section exists
    if let Some(Value::Table(desktop)) = table.get_mut("desktop") {
        if !desktop.contains_key("enable_dirty_presentation_gating") {
            desktop.insert(
                "enable_dirty_presentation_gating".to_string(),
                Value::Boolean(true),
            );
        }
    }

    Ok(())
}

/// Forward migration from 1.1.0 to 2.0.0:
/// Restores or adds dedicated runtime section while preserving any existing user customizations.
fn migrate_v1_1_to_v2_0(table: &mut toml::Table) -> Result<(), MigrationError> {
    table.insert(
        "schema_version".to_string(),
        Value::String("2.0.0".to_string()),
    );

    // If _preserved_runtime exists from a previous downgrade, restore user customizations intact
    if let Some(preserved) = table.remove("_preserved_runtime") {
        table.insert("runtime".to_string(), preserved);
    } else if !table.contains_key("runtime") {
        let mut runtime = toml::Table::new();
        runtime.insert("process_isolation".to_string(), Value::Boolean(true));
        runtime.insert(
            "job_object_memory_limit_mb".to_string(),
            Value::Integer(128),
        );
        runtime.insert("ipc_timeout_ms".to_string(), Value::Integer(1500));
        table.insert("runtime".to_string(), Value::Table(runtime));
    }

    Ok(())
}

/// Reversible rollback migration from 2.0.0 to 1.1.0:
/// Non-destructively preserves user-defined runtime configurations under _preserved_runtime.
fn migrate_v2_0_to_v1_1(table: &mut toml::Table) -> Result<(), MigrationError> {
    table.insert(
        "schema_version".to_string(),
        Value::String("1.1.0".to_string()),
    );

    if let Some(runtime_table) = table.remove("runtime") {
        table.insert("_preserved_runtime".to_string(), runtime_table);
    }
    Ok(())
}

/// Rollback migration from 1.1.0 to 1.0.0:
/// Strips 1.1-specific keys so the configuration validates against pure 1.0.0 schema.
fn migrate_v1_1_to_v1_0(table: &mut toml::Table) -> Result<(), MigrationError> {
    table.insert(
        "schema_version".to_string(),
        Value::String("1.0.0".to_string()),
    );

    if let Some(Value::Table(telem)) = table.get_mut("telemetry") {
        telem.remove("dynamic_battery_backoff_ms");
    }

    if let Some(Value::Table(audio)) = table.get_mut("audio") {
        audio.remove("silence_threshold");
        audio.remove("decay_rate");
    }

    if let Some(Value::Table(desktop)) = table.get_mut("desktop") {
        desktop.remove("enable_dirty_presentation_gating");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_BASE_TOML: &str = r#"
schema_version = "1.0.0"
profile = "signature_minimal"

[general]
start_with_windows = true
language = "system"

[desktop]
placement = "between_wallpaper_and_icons"
locked = true

[audio]
visualizer_enabled = true
render_fps = 30

[telemetry]
sample_interval_ms = 1000
"#;

    #[test]
    fn test_parse_schema_versions() {
        assert_eq!(
            SchemaVersion::from_str("1.0.0").unwrap(),
            SchemaVersion::V1_0_0
        );
        assert_eq!(
            SchemaVersion::from_str("1.1.0").unwrap(),
            SchemaVersion::V1_1_0
        );
        assert_eq!(
            SchemaVersion::from_str("2.0.0").unwrap(),
            SchemaVersion::V2_0_0
        );
        assert!(SchemaVersion::from_str("0.9.0").is_err());
        assert!(SchemaVersion::from_str("unknown").is_err());
    }

    #[test]
    fn test_idempotent_migration() {
        let res = migrate_toml_string(V1_BASE_TOML, SchemaVersion::V1_0_0).unwrap();
        assert_eq!(res, V1_BASE_TOML);
    }

    #[test]
    fn test_forward_migration_v1_0_to_v1_1() {
        let v1_1 = migrate_toml_string(V1_BASE_TOML, SchemaVersion::V1_1_0).unwrap();
        assert!(v1_1.contains(r#"schema_version = "1.1.0""#));
        assert!(v1_1.contains("dynamic_battery_backoff_ms = 5000"));
        assert!(v1_1.contains("silence_threshold = 0.001"));
        assert!(v1_1.contains("decay_rate = 0.85"));
        assert!(v1_1.contains("enable_dirty_presentation_gating = true"));
        // Preserves original fields
        assert!(v1_1.contains(r#"profile = "signature_minimal""#));
    }

    #[test]
    fn test_forward_migration_v1_0_to_v2_0() {
        let v2_0 = migrate_toml_string(V1_BASE_TOML, SchemaVersion::V2_0_0).unwrap();
        assert!(v2_0.contains(r#"schema_version = "2.0.0""#));
        assert!(v2_0.contains("[runtime]"));
        assert!(v2_0.contains("process_isolation = true"));
        assert!(v2_0.contains("job_object_memory_limit_mb = 128"));
    }

    #[test]
    fn test_bidirectional_roundtrip_reversible_migration() {
        // 1.0.0 -> 2.0.0 -> 1.0.0
        let v2_0 = migrate_toml_string(V1_BASE_TOML, SchemaVersion::V2_0_0).unwrap();
        let rolled_back_v1 = migrate_toml_string(&v2_0, SchemaVersion::V1_0_0).unwrap();

        assert!(rolled_back_v1.contains(r#"schema_version = "1.0.0""#));
        assert!(!rolled_back_v1.contains("[runtime]"));
        assert!(!rolled_back_v1.contains("dynamic_battery_backoff_ms"));
        assert!(rolled_back_v1.contains(r#"profile = "signature_minimal""#));
    }

    #[test]
    fn test_migration_rejects_missing_schema_version() {
        let bad_toml = r#"profile = "test""#;
        let res = migrate_toml_string(bad_toml, SchemaVersion::V1_1_0);
        assert!(matches!(res, Err(MigrationError::MissingSchemaVersion)));
    }

    #[test]
    fn test_migration_rejects_malformed_toml() {
        let bad_toml = r#"schema_version = "1.0.0" this is not toml [["#;
        let res = migrate_toml_string(bad_toml, SchemaVersion::V1_1_0);
        assert!(matches!(res, Err(MigrationError::InvalidToml(_))));
    }
}
