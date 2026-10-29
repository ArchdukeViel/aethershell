use aethershell_win::{probe_taskbars, register_taskbar_created_message, TaskbarWindowInfo};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, STILL_ACTIVE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, HRGN, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, GetExitCodeProcess, OpenProcess, ResetEvent, SetEvent,
    WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SendMessageTimeoutW, ShowWindow, SMTO_ABORTIFHUNG, SW_SHOW, WM_THEMECHANGED,
};

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("Failed to open process handle for PID {0}: {1}")]
    OpenProcessFailed(u32, String),
    #[error("Failed to create or open event '{0}': {1}")]
    EventError(String, String),
    #[error("Process {0} not found or already terminated")]
    ProcessNotFound(u32),
    #[error("Win32 error: {0}")]
    Win32(String),
}

/// Thread-safe RAII wrapper around a Win32 HANDLE.
#[derive(Debug)]
pub struct SafeHandle(pub HANDLE);

impl SafeHandle {
    pub fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }

    pub fn is_valid(&self) -> bool {
        !self.0 .0.is_null() && self.0 .0 != -1isize as *mut c_void
    }
}

unsafe impl Send for SafeHandle {}
unsafe impl Sync for SafeHandle {}

impl Drop for SafeHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Formats the named heartbeat event for a given PID.
pub fn heartbeat_event_name(pid: u32) -> String {
    format!(r"Local\AetherShell_Heartbeat_{}", pid)
}

/// Formats the named clean exit event for a given PID.
pub fn clean_exit_event_name(pid: u32) -> String {
    format!(r"Local\AetherShell_CleanExit_{}", pid)
}

/// Outcome of handling a `TaskbarCreated` dynamic window message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskbarRecoveryEvent {
    Debounced {
        elapsed_ms: u64,
    },
    Recovered {
        primary_found: bool,
        secondary_count: usize,
        taskbars: Vec<TaskbarWindowInfo>,
    },
}

/// Result of a watchdog check on a supervised target process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WatchdogCheckResult {
    TargetRunningHealthy,
    TargetExitedCleanly { exit_code: u32 },
    TargetAbruptTermination { exit_code: u32 },
    TargetHeartbeatTimeout,
}

/// Detailed report from a system taskbar restoration operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskbarRestorationReport {
    pub primary_restored: bool,
    pub secondary_count: usize,
    pub total_discovered: usize,
    pub restored_hwnds: Vec<usize>,
}

/// Final verdict returned by the watchdog monitor when target terminates or hangs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WatchdogExitVerdict {
    CleanExit {
        exit_code: u32,
    },
    RecoveredAbruptTermination {
        exit_code: u32,
        report: TaskbarRestorationReport,
    },
    RecoveredHeartbeatHang {
        report: TaskbarRestorationReport,
    },
    TargetNotFound {
        target_pid: u32,
    },
}

/// Guard managing a background heartbeat pulsation thread.
pub struct HeartbeatPulseGuard {
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for HeartbeatPulseGuard {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Primary taskbar recovery coordinator running inside the AetherShell engine.
///
/// Responsibilities:
/// - Publishes and pulses the process heartbeat event (`Local\AetherShell_Heartbeat_<PID>`).
/// - Signals clean exit (`Local\AetherShell_CleanExit_<PID>`) on planned shutdown.
/// - Listens for dynamic `TaskbarCreated` window broadcasts (Explorer restart).
/// - Debounces rapid duplicate `TaskbarCreated` messages (300ms window) to prevent fighting loops.
pub struct TaskbarRecoveryCoordinator {
    pid: u32,
    heartbeat_handle: SafeHandle,
    clean_exit_handle: SafeHandle,
    taskbar_created_msg: u32,
    last_taskbar_created: Option<Instant>,
    debounce_duration: Duration,
    clean_exit_marked: bool,
}

impl TaskbarRecoveryCoordinator {
    /// Creates a recovery coordinator for the current process.
    pub fn new() -> Result<Self, RecoveryError> {
        let pid = unsafe { GetCurrentProcessId() };
        Self::for_pid(pid)
    }

    /// Creates a recovery coordinator for an explicit process ID.
    pub fn for_pid(pid: u32) -> Result<Self, RecoveryError> {
        let hb_name = heartbeat_event_name(pid);
        let hb_hstring = HSTRING::from(&hb_name);

        // Auto-reset event, initially non-signaled
        let hb_raw = unsafe {
            CreateEventW(
                None,
                false,
                false,
                windows::core::PCWSTR(hb_hstring.as_ptr()),
            )
        }
        .map_err(|e| RecoveryError::EventError(hb_name, e.to_string()))?;

        let ce_name = clean_exit_event_name(pid);
        let ce_hstring = HSTRING::from(&ce_name);

        // Manual-reset event, initially non-signaled
        let ce_raw = unsafe {
            CreateEventW(
                None,
                true,
                false,
                windows::core::PCWSTR(ce_hstring.as_ptr()),
            )
        }
        .map_err(|e| RecoveryError::EventError(ce_name, e.to_string()))?;

        unsafe {
            let _ = ResetEvent(hb_raw);
            let _ = ResetEvent(ce_raw);
        }

        let taskbar_created_msg = register_taskbar_created_message();

        Ok(Self {
            pid,
            heartbeat_handle: SafeHandle::new(hb_raw),
            clean_exit_handle: SafeHandle::new(ce_raw),
            taskbar_created_msg,
            last_taskbar_created: None,
            debounce_duration: Duration::from_millis(300),
            clean_exit_marked: false,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn taskbar_created_msg(&self) -> u32 {
        self.taskbar_created_msg
    }

    pub fn is_taskbar_created_message(&self, msg: u32) -> bool {
        msg == self.taskbar_created_msg && msg >= 0xC000
    }

    pub fn set_debounce_duration(&mut self, duration: Duration) {
        self.debounce_duration = duration;
    }

    pub fn debounce_duration(&self) -> Duration {
        self.debounce_duration
    }

    /// Pulses the heartbeat event once, signaling the watchdog that this process is active.
    pub fn pulse_heartbeat(&self) -> Result<(), RecoveryError> {
        unsafe {
            SetEvent(self.heartbeat_handle.raw())
                .map_err(|e| RecoveryError::Win32(format!("Failed to pulse heartbeat: {}", e)))
        }
    }

    /// Starts a background thread that pulses the heartbeat event at the given interval.
    pub fn start_heartbeat_thread(&self, interval: Duration) -> HeartbeatPulseGuard {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&stop_flag);
        let pid = self.pid;

        let thread_handle = std::thread::spawn(move || {
            let hb_name = heartbeat_event_name(pid);
            let hb_hstring = HSTRING::from(&hb_name);
            let hb_raw = unsafe {
                CreateEventW(
                    None,
                    false,
                    false,
                    windows::core::PCWSTR(hb_hstring.as_ptr()),
                )
            };

            if let Ok(handle) = hb_raw {
                let safe = SafeHandle::new(handle);
                while !flag_clone.load(Ordering::Relaxed) {
                    unsafe {
                        let _ = SetEvent(safe.raw());
                    }
                    std::thread::sleep(interval);
                }
            }
        });

        HeartbeatPulseGuard {
            stop_flag,
            thread_handle: Some(thread_handle),
        }
    }

    /// Marks that this process is shutting down cleanly and signals the clean exit event.
    pub fn mark_clean_exit(&mut self) -> Result<(), RecoveryError> {
        self.clean_exit_marked = true;
        unsafe {
            SetEvent(self.clean_exit_handle.raw())
                .map_err(|e| RecoveryError::Win32(format!("Failed to signal clean exit: {}", e)))
        }
    }

    pub fn is_clean_exit_marked(&self) -> bool {
        self.clean_exit_marked
    }

    /// Processes an incoming `TaskbarCreated` broadcast with debouncing.
    pub fn handle_taskbar_created(&mut self) -> TaskbarRecoveryEvent {
        let now = Instant::now();
        if let Some(last) = self.last_taskbar_created {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < self.debounce_duration {
                return TaskbarRecoveryEvent::Debounced {
                    elapsed_ms: elapsed.as_millis() as u64,
                };
            }
        }

        self.last_taskbar_created = Some(now);
        let taskbars = probe_taskbars();
        let primary_found = taskbars.iter().any(|t| t.is_primary);
        let secondary_count = taskbars.iter().filter(|t| !t.is_primary).count();

        TaskbarRecoveryEvent::Recovered {
            primary_found,
            secondary_count,
            taskbars,
        }
    }
}

/// Standalone non-invasive restoration of all primary and secondary taskbars.
///
/// Uses standard Win32 redraw, visibility re-assertion, and theme change notification.
/// Does not terminate or inject into Explorer, guaranteeing adherence to PRD Q37:A safety.
pub fn restore_system_taskbars() -> TaskbarRestorationReport {
    let taskbars = probe_taskbars();
    let mut restored_hwnds = Vec::new();
    let mut primary_restored = false;
    let mut secondary_count = 0;

    for tb in &taskbars {
        let hwnd = HWND(tb.hwnd as *mut c_void);
        unsafe {
            if IsWindow(hwnd).as_bool() {
                // Ensure window visibility
                let _ = ShowWindow(hwnd, SW_SHOW);

                // Force full non-invasive redraw across window frame and children
                let _ = RedrawWindow(
                    hwnd,
                    None,
                    HRGN::default(),
                    RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW,
                );

                // Notify Explorer window of theme / visual refresh with timeout protection
                let _ = SendMessageTimeoutW(
                    hwnd,
                    WM_THEMECHANGED,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    250,
                    None,
                );

                restored_hwnds.push(tb.hwnd);
                if tb.is_primary {
                    primary_restored = true;
                } else {
                    secondary_count += 1;
                }
            }
        }
    }

    TaskbarRestorationReport {
        primary_restored,
        secondary_count,
        total_discovered: taskbars.len(),
        restored_hwnds,
    }
}

/// Independent watchdog process monitor supervising an active AetherShell instance.
///
/// Holds a Win32 process handle with `PROCESS_SYNCHRONIZE` access to detect sudden termination,
/// monitors the heartbeat event, and executes immediate taskbar recovery if the process dies abruptly.
pub struct TaskbarRecoveryWatchdog {
    target_pid: u32,
    process_handle: SafeHandle,
    heartbeat_handle: SafeHandle,
    clean_exit_handle: SafeHandle,
}

impl TaskbarRecoveryWatchdog {
    /// Creates and connects a watchdog monitor for the specified target process ID.
    pub fn new(target_pid: u32) -> Result<Self, RecoveryError> {
        let process_raw = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                target_pid,
            )
        };

        let process_handle = match process_raw {
            Ok(handle) => SafeHandle::new(handle),
            Err(e) => return Err(RecoveryError::OpenProcessFailed(target_pid, e.to_string())),
        };

        let hb_name = heartbeat_event_name(target_pid);
        let hb_hstring = HSTRING::from(&hb_name);
        let hb_raw = unsafe {
            CreateEventW(
                None,
                false,
                false,
                windows::core::PCWSTR(hb_hstring.as_ptr()),
            )
        }
        .map_err(|e| RecoveryError::EventError(hb_name, e.to_string()))?;

        let ce_name = clean_exit_event_name(target_pid);
        let ce_hstring = HSTRING::from(&ce_name);
        let ce_raw = unsafe {
            CreateEventW(
                None,
                true,
                false,
                windows::core::PCWSTR(ce_hstring.as_ptr()),
            )
        }
        .map_err(|e| RecoveryError::EventError(ce_name, e.to_string()))?;

        Ok(Self {
            target_pid,
            process_handle,
            heartbeat_handle: SafeHandle::new(hb_raw),
            clean_exit_handle: SafeHandle::new(ce_raw),
        })
    }

    pub fn target_pid(&self) -> u32 {
        self.target_pid
    }

    /// Checks if the target process is still running.
    pub fn is_target_alive(&self) -> bool {
        unsafe {
            let wait_res = WaitForSingleObject(self.process_handle.raw(), 0);
            wait_res == WAIT_TIMEOUT
        }
    }

    /// Checks whether the clean exit event has been signaled.
    pub fn is_clean_exit_signaled(&self) -> bool {
        unsafe {
            let wait_res = WaitForSingleObject(self.clean_exit_handle.raw(), 0);
            wait_res == WAIT_OBJECT_0
        }
    }

    /// Retrieves the exit code of the target process if it has exited.
    pub fn get_target_exit_code(&self) -> Option<u32> {
        unsafe {
            let mut exit_code = 0u32;
            if GetExitCodeProcess(self.process_handle.raw(), &mut exit_code).is_ok() {
                if exit_code != STILL_ACTIVE.0 as u32 {
                    return Some(exit_code);
                }
            }
            None
        }
    }

    /// Checks the health and termination status of the target process.
    pub fn check_target_status(&self, heartbeat_timeout: Duration) -> WatchdogCheckResult {
        if !self.is_target_alive() {
            let exit_code = self.get_target_exit_code().unwrap_or(0);
            if self.is_clean_exit_signaled() {
                return WatchdogCheckResult::TargetExitedCleanly { exit_code };
            } else {
                return WatchdogCheckResult::TargetAbruptTermination { exit_code };
            }
        }

        let ms = heartbeat_timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait_res = unsafe { WaitForSingleObject(self.heartbeat_handle.raw(), ms) };
        if wait_res == WAIT_OBJECT_0 {
            WatchdogCheckResult::TargetRunningHealthy
        } else {
            // Re-check target liveness in case process terminated during wait
            if !self.is_target_alive() {
                let exit_code = self.get_target_exit_code().unwrap_or(0);
                if self.is_clean_exit_signaled() {
                    return WatchdogCheckResult::TargetExitedCleanly { exit_code };
                } else {
                    return WatchdogCheckResult::TargetAbruptTermination { exit_code };
                }
            }
            WatchdogCheckResult::TargetHeartbeatTimeout
        }
    }

    /// Restores the primary and secondary taskbars without terminating Explorer.
    pub fn restore_taskbars(&self) -> TaskbarRestorationReport {
        restore_system_taskbars()
    }

    /// Monitors the target process continuously until termination or heartbeat hang.
    pub fn monitor_target(
        &self,
        heartbeat_timeout: Duration,
        poll_interval: Duration,
    ) -> WatchdogExitVerdict {
        loop {
            match self.check_target_status(heartbeat_timeout) {
                WatchdogCheckResult::TargetRunningHealthy => {
                    std::thread::sleep(poll_interval);
                }
                WatchdogCheckResult::TargetExitedCleanly { exit_code } => {
                    tracing::info!(
                        target_pid = self.target_pid,
                        exit_code,
                        "Supervised process exited cleanly; no spurious taskbar restoration performed"
                    );
                    return WatchdogExitVerdict::CleanExit { exit_code };
                }
                WatchdogCheckResult::TargetAbruptTermination { exit_code } => {
                    tracing::warn!(
                        target_pid = self.target_pid,
                        exit_code,
                        "Sudden termination detected for supervised process! Initiating taskbar recovery..."
                    );
                    let report = self.restore_taskbars();
                    return WatchdogExitVerdict::RecoveredAbruptTermination { exit_code, report };
                }
                WatchdogCheckResult::TargetHeartbeatTimeout => {
                    tracing::warn!(
                        target_pid = self.target_pid,
                        "Heartbeat timeout detected (supervised process hung)! Initiating taskbar recovery..."
                    );
                    let report = self.restore_taskbars();
                    return WatchdogExitVerdict::RecoveredHeartbeatHang { report };
                }
            }
        }
    }

    /// Convenience execution wrapper using 5-second heartbeat timeout and 100ms poll interval.
    pub fn run(&self) -> WatchdogExitVerdict {
        self.monitor_target(Duration::from_secs(5), Duration::from_millis(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_event_names_format() {
        assert_eq!(
            heartbeat_event_name(1234),
            r"Local\AetherShell_Heartbeat_1234"
        );
        assert_eq!(
            clean_exit_event_name(1234),
            r"Local\AetherShell_CleanExit_1234"
        );
    }

    #[test]
    fn test_coordinator_initialization_and_heartbeat_pulse() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let coordinator =
            TaskbarRecoveryCoordinator::new().expect("Coordinator must initialize cleanly");
        assert!(coordinator.pid() > 0);
        assert!(!coordinator.is_clean_exit_marked());

        coordinator
            .pulse_heartbeat()
            .expect("Heartbeat pulse must succeed");
    }

    #[test]
    fn test_coordinator_clean_exit_marking() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut coordinator =
            TaskbarRecoveryCoordinator::new().expect("Coordinator must initialize");
        unsafe {
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
        assert!(!coordinator.is_clean_exit_marked());

        coordinator
            .mark_clean_exit()
            .expect("Mark clean exit must succeed");
        assert!(coordinator.is_clean_exit_marked());
        unsafe {
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
    }

    #[test]
    fn test_taskbar_created_registration_and_debouncing() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut coordinator =
            TaskbarRecoveryCoordinator::new().expect("Coordinator must initialize");
        let msg = coordinator.taskbar_created_msg();
        assert!(
            msg >= 0xC000,
            "TaskbarCreated message must be in dynamic range >= 0xC000"
        );
        assert!(coordinator.is_taskbar_created_message(msg));
        assert!(!coordinator.is_taskbar_created_message(0x0010)); // WM_CLOSE is not TaskbarCreated

        // 1. Initial invocation recovers cleanly
        let event1 = coordinator.handle_taskbar_created();
        assert!(matches!(event1, TaskbarRecoveryEvent::Recovered { .. }));

        // 2. Immediate duplicate invocation within 300ms must be debounced
        let event2 = coordinator.handle_taskbar_created();
        assert!(matches!(event2, TaskbarRecoveryEvent::Debounced { .. }));

        // 3. Invocation after debounce duration succeeds again
        coordinator.set_debounce_duration(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(20));
        let event3 = coordinator.handle_taskbar_created();
        assert!(matches!(event3, TaskbarRecoveryEvent::Recovered { .. }));
    }

    #[test]
    fn test_watchdog_monitoring_self_alive() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let pid = unsafe { GetCurrentProcessId() };
        let coordinator = TaskbarRecoveryCoordinator::for_pid(pid).expect("Coordinator init");
        unsafe {
            let _ = ResetEvent(coordinator.heartbeat_handle.raw());
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
        let watchdog = TaskbarRecoveryWatchdog::new(pid).expect("Watchdog init");

        assert_eq!(watchdog.target_pid(), pid);
        assert!(watchdog.is_target_alive());

        coordinator.pulse_heartbeat().expect("Pulse");
        let status = watchdog.check_target_status(Duration::from_millis(500));
        assert_eq!(status, WatchdogCheckResult::TargetRunningHealthy);
    }

    #[test]
    fn test_restore_system_taskbars_safe_execution() {
        let report = restore_system_taskbars();
        // Safe execution without crash or panic
        println!("Taskbars restoration report: {:?}", report);
    }

    #[test]
    fn test_heartbeat_background_thread_pulsing() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let coordinator = TaskbarRecoveryCoordinator::new().expect("Coordinator init");
        unsafe {
            let _ = ResetEvent(coordinator.heartbeat_handle.raw());
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
        let watchdog = TaskbarRecoveryWatchdog::new(coordinator.pid()).expect("Watchdog init");

        let _guard = coordinator.start_heartbeat_thread(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(150));

        let status = watchdog.check_target_status(Duration::from_millis(200));
        assert_eq!(status, WatchdogCheckResult::TargetRunningHealthy);
    }

    #[test]
    fn test_watchdog_invalid_pid_fails_gracefully() {
        // PID 0 or an absurdly high PID should fail OpenProcess
        let res = TaskbarRecoveryWatchdog::new(0);
        assert!(res.is_err(), "Watchdog creation with PID 0 must fail");
        if let Err(RecoveryError::OpenProcessFailed(pid, _)) = res {
            assert_eq!(pid, 0);
        } else {
            panic!("Expected RecoveryError::OpenProcessFailed");
        }
    }

    #[test]
    fn test_heartbeat_timeout_detection() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let pid = unsafe { GetCurrentProcessId() };
        let coordinator = TaskbarRecoveryCoordinator::for_pid(pid).expect("Coordinator init");
        unsafe {
            let _ = ResetEvent(coordinator.heartbeat_handle.raw());
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
        let watchdog = TaskbarRecoveryWatchdog::new(pid).expect("Watchdog init");
        unsafe {
            let _ = ResetEvent(watchdog.heartbeat_handle.raw());
            let _ = ResetEvent(watchdog.clean_exit_handle.raw());
        }

        // Do not pulse heartbeat: check status with very short timeout
        let status = watchdog.check_target_status(Duration::from_millis(20));
        assert_eq!(status, WatchdogCheckResult::TargetHeartbeatTimeout);
    }

    #[test]
    fn test_clean_exit_signaled_observation() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let pid = unsafe { GetCurrentProcessId() };
        let mut coordinator = TaskbarRecoveryCoordinator::for_pid(pid).expect("Coordinator init");
        unsafe {
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
        let watchdog = TaskbarRecoveryWatchdog::new(pid).expect("Watchdog init");

        assert!(!watchdog.is_clean_exit_signaled());
        coordinator.mark_clean_exit().expect("Mark clean exit");
        assert!(watchdog.is_clean_exit_signaled());
        unsafe {
            let _ = ResetEvent(coordinator.clean_exit_handle.raw());
        }
    }
}
