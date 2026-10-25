use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum PathSafetyError {
    #[error("Path traversal detected: {0}")]
    TraversalDetected(String),
    #[error("Absolute paths are not permitted in relative package references: {0}")]
    AbsolutePathNotAllowed(String),
    #[error("Path escapes target root boundary: {0}")]
    EscapedRoot(String),
    #[error("Invalid path characters or reserved DOS device name: {0}")]
    InvalidCharacters(String),
}

const RESERVED_DOS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validates that a relative path within a skin or theme package does not escape its root.
///
/// Resolves Finding IVA-006: Rejects NTFS alternate data streams and reserved DOS device names.
pub fn validate_package_relative_path(rel_path: &str) -> Result<PathBuf, PathSafetyError> {
    let path = Path::new(rel_path);

    if path.is_absolute() {
        return Err(PathSafetyError::AbsolutePathNotAllowed(rel_path.into()));
    }

    // Reject NTFS alternate data streams (e.g., file.txt:hidden)
    if rel_path.contains(':') {
        return Err(PathSafetyError::InvalidCharacters(
            "Alternate data streams not permitted".into(),
        ));
    }

    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(PathSafetyError::TraversalDetected(rel_path.into()));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(PathSafetyError::AbsolutePathNotAllowed(rel_path.into()));
            }
            Component::CurDir => {}
            Component::Normal(os_str) => {
                let name = os_str.to_string_lossy();
                let stem = Path::new(&*name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&name);
                for reserved in RESERVED_DOS_NAMES {
                    if stem.eq_ignore_ascii_case(reserved) {
                        return Err(PathSafetyError::InvalidCharacters(format!(
                            "Reserved device name '{}' is prohibited",
                            reserved
                        )));
                    }
                }
            }
        }
    }

    Ok(path.to_path_buf())
}

/// Safely joins a trusted root with an untrusted relative subpath, ensuring containment.
pub fn safe_join_contained(root: &Path, rel_subpath: &str) -> Result<PathBuf, PathSafetyError> {
    let clean_rel = validate_package_relative_path(rel_subpath)?;
    let joined = root.join(clean_rel);

    // Verify it starts with the canonical root if root exists
    if root.exists() {
        if let Ok(canon_root) = root.canonicalize() {
            if let Ok(canon_joined) = joined.canonicalize() {
                if !canon_joined.starts_with(&canon_root) {
                    return Err(PathSafetyError::EscapedRoot(rel_subpath.into()));
                }
            }
        }
    }

    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_relative_paths() {
        assert!(validate_package_relative_path("assets/preview.png").is_ok());
        assert!(validate_package_relative_path("layout.toml").is_ok());
        assert!(validate_package_relative_path("icons/sub/icon.svg").is_ok());
    }

    #[test]
    fn test_rejection_of_traversal() {
        assert_eq!(
            validate_package_relative_path("../secret.txt"),
            Err(PathSafetyError::TraversalDetected("../secret.txt".into()))
        );
        assert_eq!(
            validate_package_relative_path("assets/../../secret.txt"),
            Err(PathSafetyError::TraversalDetected(
                "assets/../../secret.txt".into()
            ))
        );
    }

    #[test]
    fn test_rejection_of_absolute_paths() {
        assert_eq!(
            validate_package_relative_path("/etc/passwd"),
            Err(PathSafetyError::AbsolutePathNotAllowed(
                "/etc/passwd".into()
            ))
        );
    }

    #[test]
    fn test_rejection_of_reserved_dos_names_iva_006() {
        assert!(matches!(
            validate_package_relative_path("aux.json"),
            Err(PathSafetyError::InvalidCharacters(_))
        ));
        assert!(matches!(
            validate_package_relative_path("sub/nul.txt"),
            Err(PathSafetyError::InvalidCharacters(_))
        ));
        assert!(matches!(
            validate_package_relative_path("stream.txt:hidden"),
            Err(PathSafetyError::InvalidCharacters(_))
        ));
    }
}
