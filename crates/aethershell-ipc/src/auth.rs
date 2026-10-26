use std::io;
use std::path::PathBuf;

/// Helper for managing AetherShell session capability tokens.
pub struct SessionToken;

impl SessionToken {
    /// Canonical path to session capability token (%LOCALAPPDATA%\AetherShell\session.token)
    pub fn token_path() -> PathBuf {
        if let Ok(override_path) = std::env::var("AETHERSHELL_SESSION_TOKEN_PATH") {
            return PathBuf::from(override_path);
        }
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(appdata)
                .join("AetherShell")
                .join("session.token")
        } else {
            std::env::temp_dir().join("aethershell_session.token")
        }
    }

    /// Loads the active session token from disk, if present.
    pub fn load() -> Option<String> {
        let path = Self::token_path();
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Removes the session token file from disk.
    pub fn cleanup() {
        let path = Self::token_path();
        let _ = std::fs::remove_file(path);
    }
}

/// RAII Guard that creates and persists a session capability token on initialization
/// and deletes the token file on drop.
pub struct SessionTokenGuard {
    token: String,
    path: PathBuf,
}

impl SessionTokenGuard {
    pub fn create() -> io::Result<Self> {
        let token = uuid::Uuid::new_v4().to_string();
        Self::with_token(token)
    }

    pub fn with_token(token: impl Into<String>) -> io::Result<Self> {
        let token = token.into();
        let path = SessionToken::token_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, token.as_bytes())?;
        Ok(Self { token, path })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for SessionTokenGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_guard_lifecycle() {
        let custom_dir =
            std::env::temp_dir().join(format!("test_session_token_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&custom_dir);
        let custom_token_file = custom_dir.join("session.token");

        std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &custom_token_file);

        {
            let guard = SessionTokenGuard::create().expect("SessionTokenGuard::create");
            assert!(!guard.token().is_empty());
            assert!(custom_token_file.exists());

            let loaded = SessionToken::load();
            assert_eq!(loaded, Some(guard.token().to_string()));
        }

        // On drop, token file must be deleted
        assert!(!custom_token_file.exists());
        assert_eq!(SessionToken::load(), None);

        std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
        let _ = std::fs::remove_dir_all(custom_dir);
    }
}
