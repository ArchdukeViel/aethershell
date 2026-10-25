use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, ReleaseMutex};

/// Manages a session-scoped singleton mutex (PRD Section 3.2).
///
/// Prevents multiple instances within the same Windows logon session,
/// while allowing concurrent distinct user sessions (Fast User Switching / Remote Desktop).
pub struct SessionSingletonGuard {
    handle: HANDLE,
    is_primary: bool,
    session_id: u32,
}

impl SessionSingletonGuard {
    /// Attempts to acquire the session-scoped singleton mutex.
    pub fn acquire() -> Result<Self, String> {
        let mut session_id = 0u32;
        unsafe {
            let pid = GetCurrentProcessId();
            if ProcessIdToSessionId(pid, &mut session_id).is_err() {
                // Fallback session 1 if resolution fails
                session_id = 1;
            }

            let mutex_name = format!("Local\\AetherShell_Singleton_{}", session_id);
            let hstring_name = HSTRING::from(&mutex_name);

            let handle = CreateMutexW(None, true, windows::core::PCWSTR(hstring_name.as_ptr()))
                .map_err(|e| format!("Failed to create singleton mutex: {}", e))?;

            let already_exists = GetLastError() == ERROR_ALREADY_EXISTS;

            Ok(Self {
                handle,
                is_primary: !already_exists,
                session_id,
            })
        }
    }

    pub fn is_primary(&self) -> bool {
        self.is_primary
    }

    pub fn session_id(&self) -> u32 {
        self.session_id
    }
}

impl Drop for SessionSingletonGuard {
    fn drop(&mut self) {
        unsafe {
            if self.is_primary && !self.handle.0.is_null() {
                let _ = ReleaseMutex(self.handle);
            }
            if !self.handle.0.is_null() {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_singleton_acquisition() {
        let guard1 = SessionSingletonGuard::acquire().expect("acquire primary");
        assert!(guard1.is_primary());

        // A second acquisition in the same process should detect ALREADY_EXISTS
        let guard2 = SessionSingletonGuard::acquire().expect("acquire secondary");
        assert!(!guard2.is_primary());
        assert_eq!(guard1.session_id(), guard2.session_id());
    }
}
