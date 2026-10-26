//! Session Capability Token Management & Threat Boundary Analysis
//!
//! ## Threat Boundary & Security Properties
//! 1. **Cross-Session Isolation**:
//!    Multiple interactive or remote desktop Windows sessions (RDP, Fast User Switching) running under
//!    the same Windows user account share the `%LOCALAPPDATA%` directory. To prevent token collision
//!    between distinct concurrent instances in separate sessions, capability tokens are strictly scoped
//!    per-session: `%LOCALAPPDATA%\AetherShell\session_{session_id}.token`.
//!
//! 2. **Cross-Role Capability Separation**:
//!    The session capability token acts as a privilege gating token separating unprivileged or supervised
//!    callers (such as Lua script worker processes or untrusted local processes) from privileged management
//!    endpoints (shutdown, layout mutation, desktop interaction mode toggles).
//!
//! 3. **Same-User Threat Model Boundary (Explicit Non-Claim)**:
//!    Under Windows security architecture, any code executing with the same user token in the same session
//!    shares filesystem permissions to `%LOCALAPPDATA%`. We do NOT claim that LocalAppData file storage
//!    provides a cryptographic boundary against hostile malware already executing within the same user session.
//!    Instead, kernel Named Pipe client PID and session verification (`GetNamedPipeClientProcessId`,
//!    `GetNamedPipeClientSessionId`), combined with fail-closed DACL security descriptors and out-of-process
//!    Job Object sandboxing, enforce worker isolation.
//!
//! 4. **Crash Recovery & Rotation**:
//!    Stale token files from ungraceful engine crashes are safely overwritten upon new instance creation,
//!    and token files are automatically removed upon RAII drop during clean shutdown.

use std::io;
use std::path::PathBuf;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Resolves the Windows session ID for the current process.
pub fn current_session_id() -> u32 {
    let mut session_id = 0u32;
    unsafe {
        let pid = GetCurrentProcessId();
        if ProcessIdToSessionId(pid, &mut session_id).is_ok() {
            session_id
        } else {
            1
        }
    }
}

/// Helper for managing AetherShell session capability tokens.
pub struct SessionToken;

impl SessionToken {
    /// Canonical path to session capability token for a specific session ID:
    /// `%LOCALAPPDATA%\AetherShell\session_{session_id}.token`
    pub fn token_path_for_session(session_id: u32) -> PathBuf {
        if let Ok(override_path) = std::env::var("AETHERSHELL_SESSION_TOKEN_PATH") {
            return PathBuf::from(override_path);
        }
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(appdata)
                .join("AetherShell")
                .join(format!("session_{}.token", session_id))
        } else {
            std::env::temp_dir().join(format!("aethershell_session_{}.token", session_id))
        }
    }

    /// Canonical path to session capability token for the current session.
    pub fn token_path() -> PathBuf {
        Self::token_path_for_session(current_session_id())
    }

    /// Loads the active session token for a given session ID from disk, if present.
    pub fn load_for_session(session_id: u32) -> Option<String> {
        let path = Self::token_path_for_session(session_id);
        if let Ok(s) = std::fs::read_to_string(&path) {
            let trimmed = s.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        // Fallback check for legacy session.token path
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            let legacy_path = PathBuf::from(appdata)
                .join("AetherShell")
                .join("session.token");
            if let Ok(s) = std::fs::read_to_string(&legacy_path) {
                let trimmed = s.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        None
    }

    /// Loads the active session token from disk for the current session, if present.
    pub fn load() -> Option<String> {
        Self::load_for_session(current_session_id())
    }

    /// Removes the session token file from disk for a given session ID.
    pub fn cleanup_for_session(session_id: u32) {
        let path = Self::token_path_for_session(session_id);
        let _ = std::fs::remove_file(path);
    }

    /// Removes the session token file from disk for the current session.
    pub fn cleanup() {
        Self::cleanup_for_session(current_session_id());
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
        Self::create_for_session(current_session_id())
    }

    pub fn create_for_session(session_id: u32) -> io::Result<Self> {
        let token = uuid::Uuid::new_v4().to_string();
        Self::with_token_and_session(token, session_id)
    }

    pub fn with_token(token: impl Into<String>) -> io::Result<Self> {
        Self::with_token_and_session(token, current_session_id())
    }

    pub fn with_token_and_session(token: impl Into<String>, session_id: u32) -> io::Result<Self> {
        let token = token.into();
        let path = SessionToken::token_path_for_session(session_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Write fresh token, replacing any stale token left behind by an ungraceful crash
        std::fs::write(&path, token.as_bytes())?;
        Ok(Self { token, path })
    }

    /// Rotates the session token, generating a fresh UUID and updating the token file.
    pub fn rotate(&mut self) -> io::Result<String> {
        let new_token = uuid::Uuid::new_v4().to_string();
        std::fs::write(&self.path, new_token.as_bytes())?;
        self.token = new_token.clone();
        Ok(new_token)
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_session_token_guard_lifecycle() {
        let _lock = ENV_LOCK.lock().unwrap();
        let custom_dir =
            std::env::temp_dir().join(format!("test_session_token_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&custom_dir);
        let custom_token_file = custom_dir.join("session_1.token");

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

    #[test]
    fn test_distinct_sessions_no_collision() {
        let _lock = ENV_LOCK.lock().unwrap();
        // Unset any override
        std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");

        let custom_appdata =
            std::env::temp_dir().join(format!("test_appdata_multi_{}", uuid::Uuid::new_v4()));
        let orig_appdata = std::env::var("LOCALAPPDATA").ok();
        std::env::set_var("LOCALAPPDATA", &custom_appdata);

        let guard_session_1 =
            SessionTokenGuard::with_token_and_session("token_sess_1", 1).expect("guard session 1");
        let guard_session_2 =
            SessionTokenGuard::with_token_and_session("token_sess_2", 2).expect("guard session 2");

        // Paths must differ
        assert_ne!(guard_session_1.path(), guard_session_2.path());
        assert!(guard_session_1
            .path()
            .to_string_lossy()
            .contains("session_1.token"));
        assert!(guard_session_2
            .path()
            .to_string_lossy()
            .contains("session_2.token"));

        // Both must load independently without collision
        assert_eq!(
            SessionToken::load_for_session(1),
            Some("token_sess_1".into())
        );
        assert_eq!(
            SessionToken::load_for_session(2),
            Some("token_sess_2".into())
        );

        drop(guard_session_1);
        // Session 1 deleted, Session 2 preserved
        assert_eq!(SessionToken::load_for_session(1), None);
        assert_eq!(
            SessionToken::load_for_session(2),
            Some("token_sess_2".into())
        );

        drop(guard_session_2);
        assert_eq!(SessionToken::load_for_session(2), None);

        if let Some(orig) = orig_appdata {
            std::env::set_var("LOCALAPPDATA", orig);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }
        let _ = std::fs::remove_dir_all(custom_appdata);
    }

    #[test]
    fn test_token_rotation() {
        let _lock = ENV_LOCK.lock().unwrap();
        let custom_dir = std::env::temp_dir().join(format!("test_rotate_{}", uuid::Uuid::new_v4()));
        let custom_file = custom_dir.join("session_10.token");
        std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &custom_file);

        let mut guard = SessionTokenGuard::create_for_session(10).expect("create guard");
        let first_token = guard.token().to_string();
        assert_eq!(
            SessionToken::load_for_session(10),
            Some(first_token.clone())
        );

        let rotated_token = guard.rotate().expect("rotate token");
        assert_ne!(first_token, rotated_token);
        assert_eq!(guard.token(), rotated_token.as_str());
        assert_eq!(
            SessionToken::load_for_session(10),
            Some(rotated_token.clone())
        );

        drop(guard);
        assert!(!custom_file.exists());
        std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
        let _ = std::fs::remove_dir_all(custom_dir);
    }

    #[test]
    fn test_crash_recovery_stale_token_overwritten() {
        let _lock = ENV_LOCK.lock().unwrap();
        let custom_dir = std::env::temp_dir().join(format!("test_crash_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&custom_dir);
        let stale_token_file = custom_dir.join("session_5.token");
        // Simulate stale token left on disk by a crashed process
        std::fs::write(&stale_token_file, "STALE_DEAD_TOKEN").unwrap();

        std::env::set_var("AETHERSHELL_SESSION_TOKEN_PATH", &stale_token_file);

        let guard = SessionTokenGuard::create_for_session(5).expect("recovery create");
        assert_ne!(guard.token(), "STALE_DEAD_TOKEN");
        assert_eq!(
            SessionToken::load_for_session(5),
            Some(guard.token().to_string())
        );

        drop(guard);
        assert!(!stale_token_file.exists());
        std::env::remove_var("AETHERSHELL_SESSION_TOKEN_PATH");
        let _ = std::fs::remove_dir_all(custom_dir);
    }
}
