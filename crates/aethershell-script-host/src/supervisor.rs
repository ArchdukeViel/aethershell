use serde::{Deserialize, Serialize};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Instant;
use tracing::info;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, GetExitCodeProcess, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
};

/// Status and health of the supervised script host child process (PRD Section 13.2 / Gate G10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorState {
    Dormant,
    Spawning,
    Running { pid: u32 },
    Quarantined { reason: String },
    Terminated,
}

/// Integrity verification error for running child workers (Hypothesis A / Continuous Enforcement).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkerIntegrityError {
    #[error("Active worker child exists but has no registered script path or identity metadata.")]
    NoActiveScript,
    #[error("Script file '{0}' does not exist or was deleted from disk.")]
    ScriptNotFound(PathBuf),
    #[error("Failed to read script file '{0}': {1}")]
    IoError(PathBuf, String),
    #[error("Script content on disk modified! Running digest '{running}', current disk digest '{actual}'.")]
    ContentModified {
        script: PathBuf,
        running: String,
        actual: String,
    },
    #[error("Script trust revoked or unapproved in active trust ledger: {0}")]
    TrustRevoked(String),
}

/// RAII wrapper managing a Win32 child process handle.
#[derive(Debug)]
pub struct SupervisedChild {
    process_handle: HANDLE,
    pid: u32,
}

impl SupervisedChild {
    pub fn new(process_handle: HANDLE, pid: u32) -> Self {
        Self {
            process_handle,
            pid,
        }
    }

    /// Creates a `SupervisedChild` from an existing `Child` process by duplicating its process handle.
    /// This prevents double-close handle corruption when both `SupervisedChild` and `Child` are dropped.
    pub fn from_child(child: &Child) -> Result<Self, String> {
        let raw = child.as_raw_handle();
        let cur_proc = unsafe { GetCurrentProcess() };
        let mut dup_handle = HANDLE::default();
        let res = unsafe {
            DuplicateHandle(
                cur_proc,
                HANDLE(raw),
                cur_proc,
                &mut dup_handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if res.is_ok() {
            Ok(Self::new(dup_handle, child.id()))
        } else {
            Err(format!(
                "Failed to duplicate child process handle: {:?}",
                res.err()
            ))
        }
    }

    pub fn id(&self) -> u32 {
        self.pid
    }

    pub fn handle(&self) -> HANDLE {
        self.process_handle
    }

    pub fn is_alive(&self) -> bool {
        unsafe {
            let mut exit_code = 0u32;
            if GetExitCodeProcess(self.process_handle, &mut exit_code).is_ok() {
                exit_code == 259 // STILL_ACTIVE (0x103)
            } else {
                false
            }
        }
    }

    pub fn kill(&mut self) {
        unsafe {
            if !self.process_handle.0.is_null() && self.process_handle != INVALID_HANDLE_VALUE {
                let _ = TerminateProcess(self.process_handle, 1);
            }
        }
    }

    pub fn wait(&mut self) -> Result<u32, String> {
        unsafe {
            let _ = WaitForSingleObject(self.process_handle, 5000);
            let mut exit_code = 0u32;
            if GetExitCodeProcess(self.process_handle, &mut exit_code).is_ok() {
                Ok(exit_code)
            } else {
                Err("GetExitCodeProcess failed".into())
            }
        }
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        unsafe {
            if !self.process_handle.0.is_null() && self.process_handle != INVALID_HANDLE_VALUE {
                let _ = CloseHandle(self.process_handle);
            }
        }
    }
}

unsafe impl Send for SupervisedChild {}
unsafe impl Sync for SupervisedChild {}

/// Windows Job Object guard ensuring all child worker processes terminate when the job or parent closes.
#[derive(Debug)]
pub struct JobObjectGuard(HANDLE);

impl JobObjectGuard {
    pub fn new() -> Result<Self, String> {
        // 128 MiB per-worker memory limit (PRD Table 17-1)
        Self::with_memory_limit(128 * 1024 * 1024)
    }

    pub fn with_memory_limit(memory_limit_bytes: usize) -> Result<Self, String> {
        unsafe {
            let job = CreateJobObjectW(None, None)
                .map_err(|e| format!("CreateJobObjectW failed: {}", e))?;

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = memory_limit_bytes;

            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| {
                let _ = CloseHandle(job);
                format!("SetInformationJobObject failed: {}", e)
            })?;

            Ok(Self(job))
        }
    }

    pub fn assign_handle(&self, child_handle: HANDLE) -> Result<(), String> {
        unsafe {
            AssignProcessToJobObject(self.0, child_handle)
                .map_err(|e| format!("AssignProcessToJobObject failed: {}", e))
        }
    }

    pub fn assign_process(&self, child: &Child) -> Result<(), String> {
        self.assign_handle(HANDLE(child.as_raw_handle()))
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    /// Queries the configured per-process memory limit from the Windows Job Object.
    pub fn query_memory_limit(&self) -> Result<usize, String> {
        unsafe {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            QueryInformationJobObject(
                self.0,
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
            .map_err(|e| format!("QueryInformationJobObject failed: {}", e))?;
            Ok(info.ProcessMemoryLimit)
        }
    }

    #[cfg(test)]
    pub fn from_raw_handle_for_test(handle: HANDLE) -> Self {
        Self(handle)
    }
}

impl Drop for JobObjectGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0 .0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

unsafe impl Send for JobObjectGuard {}
unsafe impl Sync for JobObjectGuard {}

/// Script supervisor managing on-demand child worker process and crash loop recovery.
#[derive(Debug)]
pub struct ScriptSupervisor {
    state: SupervisorState,
    crash_count: u32,
    max_crashes: u32,
    last_heartbeat: Option<Instant>,
    heartbeat_timeout_secs: u32,
    job: Option<JobObjectGuard>,
    child: Option<SupervisedChild>,
    current_script: Option<PathBuf>,
    current_script_str: Option<String>,
    current_digest: Option<String>,
}

impl ScriptSupervisor {
    pub fn new(max_crashes: u32) -> Self {
        Self {
            state: SupervisorState::Dormant,
            crash_count: 0,
            max_crashes,
            last_heartbeat: None,
            heartbeat_timeout_secs: 5,
            job: JobObjectGuard::new().ok(),
            child: None,
            current_script: None,
            current_script_str: None,
            current_digest: None,
        }
    }

    pub fn state(&self) -> &SupervisorState {
        &self.state
    }

    pub fn crash_count(&self) -> u32 {
        self.crash_count
    }

    pub fn is_quarantined(&self) -> bool {
        matches!(self.state, SupervisorState::Quarantined { .. })
    }

    pub fn has_active_child(&self) -> bool {
        self.child.is_some()
    }

    pub fn is_child_alive(&self) -> bool {
        self.child.as_ref().map(|c| c.is_alive()).unwrap_or(false)
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            SupervisorState::Running { .. } | SupervisorState::Spawning
        ) && self.is_child_alive()
    }

    pub fn active_child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    pub fn current_script(&self) -> Option<&Path> {
        self.current_script.as_deref()
    }

    pub fn current_digest(&self) -> Option<&str> {
        self.current_digest.as_deref()
    }

    pub fn assign_supervised_child(&mut self, child: SupervisedChild) {
        let pid = child.id();
        self.child = Some(child);
        self.state = SupervisorState::Running { pid };
        self.last_heartbeat = Some(Instant::now());
    }

    pub fn invalidate_worker(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill();
            let _ = child.wait();
        }
        self.state = SupervisorState::Terminated;
        self.last_heartbeat = None;
        self.current_script = None;
        self.current_script_str = None;
        self.current_digest = None;
    }

    #[doc(hidden)]
    pub fn set_current_script_for_test(&mut self, script: PathBuf, digest: String) {
        self.current_script = Some(script.clone());
        self.current_script_str = Some(script.to_string_lossy().to_string());
        self.current_digest = Some(digest);
    }

    #[doc(hidden)]
    pub fn clear_current_script_for_test(&mut self) {
        self.current_script = None;
    }

    #[doc(hidden)]
    pub fn clear_current_digest_for_test(&mut self) {
        self.current_digest = None;
    }

    pub fn record_start(&mut self, pid: u32) {
        self.state = SupervisorState::Running { pid };
        self.last_heartbeat = Some(Instant::now());
    }

    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
    }

    #[cfg(test)]
    pub fn set_job_for_test(&mut self, job: Option<JobObjectGuard>) {
        self.job = job;
    }

    /// Spawns a supervised child process with pure-Rust SHA-256 pre-verification and Windows Job Object containment.
    pub fn spawn_worker(
        &mut self,
        exe_path: &Path,
        script_path: &Path,
        approved_digest: &str,
        pipe_name: &str,
    ) -> Result<u32, String> {
        if self.is_quarantined() {
            return Err(format!(
                "Worker cannot spawn: currently quarantined. State: {:?}",
                self.state
            ));
        }

        // 1. Pre-spawn verification of script digest
        crate::verify_script_file_digest(script_path, approved_digest)
            .map_err(|e| format!("Pre-spawn digest verification failed: {}", e))?;

        // 2. Terminate existing worker if any
        self.terminate_worker();

        // 3. Ensure Job Object is available with strict fail-closed policy (PRD G10)
        if self.job.is_none() {
            self.job = Some(JobObjectGuard::new().map_err(|e| {
                format!(
                    "CRITICAL: Job Object initialization failed: {}. Worker spawn aborted (fail-closed).",
                    e
                )
            })?);
        }
        let job = self.job.as_ref().ok_or("Job Object unavailable")?;

        // 4. Create child process in SUSPENDED state to guarantee zero uncontained execution
        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let cmd_line = format!(
            "\"{}\" --script \"{}\" --digest \"{}\"{}",
            exe_path.display(),
            script_path.display(),
            approved_digest,
            if pipe_name.is_empty() {
                String::new()
            } else {
                format!(" --pipe \"{}\"", pipe_name)
            }
        );
        let mut cmd_line_u16: Vec<u16> =
            cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        let create_res = unsafe {
            CreateProcessW(
                None,
                windows::core::PWSTR(cmd_line_u16.as_mut_ptr()),
                None,
                None,
                false,
                CREATE_SUSPENDED,
                None,
                None,
                &si,
                &mut pi,
            )
        };

        if create_res.is_err() {
            return Err(format!("CreateProcessW failed: {:?}", create_res.err()));
        }

        // 5. Assign to Job Object while primary thread is suspended (guarantee containment)
        if let Err(e) = job.assign_handle(pi.hProcess) {
            unsafe {
                let _ = TerminateProcess(pi.hProcess, 1);
                let _ = CloseHandle(pi.hThread);
                let _ = CloseHandle(pi.hProcess);
            }
            return Err(format!(
                "CRITICAL: Job Object assignment failed: {}. Child worker terminated before execution (fail-closed).",
                e
            ));
        }

        // 6. Assignment succeeded under containment: resume primary thread and close thread handle
        let resume_res = unsafe { ResumeThread(pi.hThread) };
        unsafe {
            let _ = CloseHandle(pi.hThread);
        }

        if resume_res == u32::MAX {
            unsafe {
                let _ = TerminateProcess(pi.hProcess, 1);
                let _ = CloseHandle(pi.hProcess);
            }
            return Err("Failed to resume suspended child worker thread".into());
        }

        let pid = pi.dwProcessId;
        info!(
            "Spawned and contained worker PID {} in Windows Job Object (128 MiB memory quota, kill-on-close)",
            pid
        );

        self.child = Some(SupervisedChild::new(pi.hProcess, pid));
        self.state = SupervisorState::Spawning;
        self.last_heartbeat = Some(Instant::now());
        self.current_script = Some(
            crate::normalize_script_path(script_path).unwrap_or_else(|_| script_path.to_path_buf()),
        );
        self.current_script_str = Some(script_path.to_string_lossy().to_string());
        self.current_digest = Some(approved_digest.to_string());

        Ok(pid)
    }

    pub fn child_pid(&self) -> Option<u32> {
        match self.state {
            SupervisorState::Running { pid } => Some(pid),
            _ => None,
        }
    }

    pub fn set_heartbeat_timeout_secs(&mut self, secs: u32) {
        self.heartbeat_timeout_secs = secs;
    }

    /// Polls child process exit status and enforces crash recovery / quarantine circuit breaker (PRD 10.2).
    pub fn poll_worker_status(&mut self) -> Option<i32> {
        if let Some(ref mut child) = self.child {
            if child.is_alive() {
                self.check_heartbeat_timeout();
                None
            } else {
                let exit_code = child.wait().unwrap_or(1) as i32;
                self.child = None;
                self.record_exit(exit_code);
                Some(exit_code)
            }
        } else {
            self.check_heartbeat_timeout();
            None
        }
    }

    /// Records an unexpected worker exit/crash and applies quarantine if limit exceeded.
    pub fn record_exit(&mut self, exit_code: i32) {
        self.current_script = None;
        self.current_script_str = None;
        self.current_digest = None;
        if exit_code != 0 {
            self.crash_count += 1;
            if self.crash_count >= self.max_crashes {
                self.state = SupervisorState::Quarantined {
                    reason: format!(
                        "Script host crashed {} times (exceeded limit of {}). Quarantined per PRD Section 10.2.",
                        self.crash_count, self.max_crashes
                    ),
                };
                return;
            }
        }
        self.state = SupervisorState::Terminated;
    }

    /// Checks if worker has missed heartbeat responses beyond grace period and terminates it.
    pub fn check_heartbeat_timeout(&mut self) -> bool {
        let active_pid = match self.state {
            SupervisorState::Running { pid } => Some(pid),
            SupervisorState::Spawning => self.child.as_ref().map(|c| c.id()),
            _ => None,
        };

        if let Some(pid) = active_pid {
            if let Some(last) = self.last_heartbeat {
                if last.elapsed().as_secs() >= self.heartbeat_timeout_secs as u64 {
                    self.current_script = None;
                    self.current_script_str = None;
                    self.current_digest = None;
                    self.state = SupervisorState::Quarantined {
                        reason: format!(
                            "Worker PID {} hung or missed heartbeats for >{}s. Terminated.",
                            pid, self.heartbeat_timeout_secs
                        ),
                    };
                    if let Some(mut c) = self.child.take() {
                        c.kill();
                        let _ = c.wait();
                    }
                    return true; // Timed out
                }
            }
        }
        false
    }

    /// Terminates worker process if running and resets state.
    pub fn terminate_worker(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill();
            let _ = child.wait();
        }
        self.stop();
    }

    /// Explicitly stops worker and resets state to Dormant.
    pub fn stop(&mut self) {
        self.state = SupervisorState::Dormant;
        self.last_heartbeat = None;
        self.current_script = None;
        self.current_script_str = None;
        self.current_digest = None;
    }

    /// Continuously validates the active script worker's file on disk and cryptographic trust ledger approval.
    ///
    /// Checks:
    /// a) If there is an active child worker (`self.has_active_child() && self.current_script.is_some()`). If not, returns `Ok(())`.
    /// b) Checks whether `current_script` still exists on disk as a file.
    /// c) Computes its actual SHA-256 digest from current disk content.
    /// d) Verifies that actual digest matches `self.current_digest`.
    /// e) If `trusted_digests` is provided, verifies that `current_script`'s canonical path and actual digest
    ///    are still authorized in `trusted_digests`.
    /// f) If any check fails (file missing, read error, digest mismatch, or unapproved in `trusted_digests`),
    ///    invalidates/terminates the worker (`self.invalidate_worker()`), clears state, and returns an `Err(WorkerIntegrityError)`.
    pub fn verify_active_worker_integrity(
        &mut self,
        trusted_digests: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<(), WorkerIntegrityError> {
        // a) Return Ok if no active child worker is present
        if !self.has_active_child() {
            return Ok(());
        }

        // b) Active child worker with missing script path or missing running digest is an inconsistent state (Fail-closed / A1)
        let script_path = match self.current_script.clone() {
            Some(path) => path,
            None => {
                self.invalidate_worker();
                return Err(WorkerIntegrityError::NoActiveScript);
            }
        };

        let running_digest = match self.current_digest.as_deref() {
            Some(d) => d.to_string(),
            None => {
                self.invalidate_worker();
                return Err(WorkerIntegrityError::NoActiveScript);
            }
        };

        // c) Checks whether current_script still exists on disk as a file
        if !script_path.is_file() {
            self.invalidate_worker();
            return Err(WorkerIntegrityError::ScriptNotFound(script_path));
        }

        // d) Computes its actual SHA-256 digest in a single disk read
        let script_bytes = match std::fs::read(&script_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.invalidate_worker();
                return Err(WorkerIntegrityError::IoError(script_path, e.to_string()));
            }
        };
        let actual_digest = crate::trust::sha256_hex(&script_bytes);

        // e) Verifies that actual digest matches self.current_digest
        if !actual_digest.eq_ignore_ascii_case(&running_digest) {
            self.invalidate_worker();
            return Err(WorkerIntegrityError::ContentModified {
                script: script_path,
                running: running_digest,
                actual: actual_digest,
            });
        }

        // f) Mandatory authorization ledger validation (Fail-closed / A2)
        // If trusted_digests is None (e.g. config lock poisoned or inaccessible),
        // mandatory authorization CANNOT be verified. Active worker must NOT be treated as authorized.
        let digests = match trusted_digests {
            Some(d) => d,
            None => {
                self.invalidate_worker();
                return Err(WorkerIntegrityError::TrustRevoked(
                    "Authoritative trust ledger is missing or inaccessible".into(),
                ));
            }
        };

        let configured_str = self
            .current_script_str
            .as_deref()
            .unwrap_or_else(|| script_path.to_str().unwrap_or(""));

        match crate::trust::authorize_script_digest_trust(
            &script_path,
            configured_str,
            &actual_digest,
            digests,
        ) {
            Ok(approved_digest) => {
                if !approved_digest.eq_ignore_ascii_case(&actual_digest) {
                    self.invalidate_worker();
                    return Err(WorkerIntegrityError::ContentModified {
                        script: script_path,
                        running: running_digest,
                        actual: actual_digest,
                    });
                }
            }
            Err(e) => {
                self.invalidate_worker();
                return Err(WorkerIntegrityError::TrustRevoked(e.to_string()));
            }
        }

        Ok(())
    }
}

impl Default for ScriptSupervisor {
    fn default() -> Self {
        Self::new(2) // Default 2 restarts per PRD 10.2
    }
}

impl Drop for ScriptSupervisor {
    fn drop(&mut self) {
        self.terminate_worker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::process::Command;

    #[test]
    fn test_supervisor_crash_limit_and_quarantine() {
        let mut supervisor = ScriptSupervisor::new(2);
        assert_eq!(*supervisor.state(), SupervisorState::Dormant);

        // Crash 1: increment crash count, return to Terminated
        supervisor.record_start(1001);
        supervisor.record_exit(1);
        assert_eq!(supervisor.crash_count(), 1);
        assert_eq!(*supervisor.state(), SupervisorState::Terminated);
        assert!(!supervisor.is_quarantined());

        // Crash 2: hit limit -> Quarantined
        supervisor.record_start(1002);
        supervisor.record_exit(1);
        assert_eq!(supervisor.crash_count(), 2);
        assert!(supervisor.is_quarantined());
    }

    #[test]
    fn test_supervisor_clean_exit_does_not_increment_crash_count() {
        let mut supervisor = ScriptSupervisor::new(2);
        supervisor.record_start(2001);
        supervisor.record_exit(0); // Clean exit
        assert_eq!(supervisor.crash_count(), 0);
        assert_eq!(*supervisor.state(), SupervisorState::Terminated);
    }

    #[test]
    fn test_job_object_guard_creation_and_assignment() {
        let job = JobObjectGuard::new();
        assert!(job.is_ok(), "JobObject creation must succeed on Windows");
        let job = job.unwrap();
        assert!(!job.handle().0.is_null());

        // Test assigning a real child process (e.g. cmd.exe)
        let child = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "1", "127.0.0.1"])
            .spawn();

        if let Ok(mut c) = child {
            let res = job.assign_process(&c);
            assert!(
                res.is_ok(),
                "Assigning child process to Job Object must succeed: {:?}",
                res
            );
            let _ = c.wait();
        }
    }

    #[test]
    fn test_job_object_guard_invalid_handle_failure() {
        let job = JobObjectGuard::new().expect("create job");
        let invalid_res = job.assign_handle(HANDLE::default());
        assert!(
            invalid_res.is_err(),
            "Assigning null handle to Job Object must return Err"
        );
    }

    #[test]
    fn test_job_object_within_memory_limit_succeeds() {
        let job = JobObjectGuard::with_memory_limit(128 * 1024 * 1024)
            .expect("create job with 128 MiB limit");

        let mut child = Command::new("cmd.exe")
            .args(["/c", "exit", "0"])
            .spawn()
            .expect("spawn cmd");

        let assign_res = job.assign_process(&child);
        assert!(assign_res.is_ok(), "Process assignment must succeed");

        let status = child.wait().expect("wait child");
        assert!(
            status.success(),
            "Process within memory limit must succeed: status={:?}",
            status
        );
    }

    fn resolve_script_host_binary() -> std::path::PathBuf {
        if let Ok(cur) = std::env::current_exe() {
            if let Some(target_dir) = cur.parent().and_then(|p| p.parent()) {
                let candidate = target_dir.join("aethershell-script-host.exe");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
        let candidates = [
            "target\\x86_64-pc-windows-msvc\\debug\\aethershell-script-host.exe",
            "target\\debug\\aethershell-script-host.exe",
            "target\\x86_64-pc-windows-msvc\\release\\aethershell-script-host.exe",
            "target\\release\\aethershell-script-host.exe",
            "..\\..\\target\\x86_64-pc-windows-msvc\\debug\\aethershell-script-host.exe",
            "..\\..\\target\\debug\\aethershell-script-host.exe",
        ];
        for c in candidates {
            let p = std::path::PathBuf::from(c);
            if p.exists() {
                return p;
            }
        }

        // If not found, build it using cargo (e.g. during clean CI runs where lib tests run before bin build)
        let mut cmd = Command::new("cargo");
        cmd.args([
            "build",
            "-p",
            "aethershell-script-host",
            "--bin",
            "aethershell-script-host",
        ]);
        if let Ok(status) = cmd.status() {
            if status.success() {
                if let Ok(cur) = std::env::current_exe() {
                    if let Some(target_dir) = cur.parent().and_then(|p| p.parent()) {
                        let candidate = target_dir.join("aethershell-script-host.exe");
                        if candidate.exists() {
                            return candidate;
                        }
                    }
                }
                for c in candidates {
                    let p = std::path::PathBuf::from(c);
                    if p.exists() {
                        return p;
                    }
                }
            }
        }

        std::path::PathBuf::from("aethershell-script-host.exe")
    }

    #[test]
    fn test_job_object_memory_limit_enforced_on_real_worker() {
        // 1. Verify Job Object exists with expected configured limit
        let limit_20mib = 20 * 1024 * 1024;
        let job =
            JobObjectGuard::with_memory_limit(limit_20mib).expect("create job with 20 MiB limit");
        let queried_limit = job.query_memory_limit().expect("query memory limit");
        assert_eq!(
            queried_limit, limit_20mib,
            "Job Object limit must match configured value"
        );

        // Also verify production default limit
        let prod_job = JobObjectGuard::new().expect("create prod job");
        assert_eq!(
            prod_job.query_memory_limit().unwrap(),
            128 * 1024 * 1024,
            "Production Job Object memory limit must be 128 MiB per PRD Table 17-1"
        );

        let worker_bin = resolve_script_host_binary();
        assert!(
            worker_bin.exists(),
            "Dedicated test worker binary must exist: {:?}",
            worker_bin
        );

        // 2. Below-limit test (5 MiB under 20 MiB limit):
        // Worker actually starts, is assigned before execution, and succeeds.
        let child_below = Command::new(&worker_bin)
            .args(["--test-alloc-mib", "5"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn below-limit worker");

        job.assign_process(&child_below)
            .expect("assign below-limit worker to job");

        let output_below = child_below.wait_with_output().expect("wait below-limit");
        assert!(
            output_below.status.success(),
            "Below-limit worker must succeed with exit code 0: {:?}",
            output_below.status
        );
        let stdout_below = String::from_utf8_lossy(&output_below.stdout);
        assert!(
            stdout_below.contains("WORKER_STARTED:"),
            "Worker must start: {}",
            stdout_below
        );
        assert!(
            stdout_below.contains("ALLOCATED_5_MIB_SUCCESS"),
            "Below-limit allocation must succeed: {}",
            stdout_below
        );

        // 3. Above-limit test (50 MiB under 20 MiB limit):
        // Worker starts, attempts allocation, and produces a quota-related OS failure.
        let child_above = Command::new(&worker_bin)
            .args(["--test-alloc-mib", "50"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn above-limit worker");

        job.assign_process(&child_above)
            .expect("assign above-limit worker to job");

        let output_above = child_above.wait_with_output().expect("wait above-limit");
        assert!(
            !output_above.status.success(),
            "Above-limit worker must fail or be terminated by Job Object: {:?}",
            output_above.status
        );
        let stdout_above = String::from_utf8_lossy(&output_above.stdout);
        assert!(
            !stdout_above.contains("ALLOCATED_50_MIB_SUCCESS"),
            "Above-limit allocation must NOT succeed: {}",
            stdout_above
        );
    }

    #[test]
    fn test_spawn_worker_nonexistent_binary_fails_before_execution() {
        let mut supervisor = ScriptSupervisor::new(2);
        let nonexistent = Path::new("C:\\NonExistent_Worker_Path_12345.exe");
        let script = std::env::temp_dir().join(format!(
            "test_s_{}_{}.lua",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&script, b"-- dummy lua").unwrap();

        let sha = crate::sha256_hex(b"-- dummy lua");
        let res = supervisor.spawn_worker(nonexistent, &script, &sha, "");
        assert!(
            res.is_err(),
            "Spawning nonexistent binary must fail cleanly"
        );
        assert!(res.unwrap_err().contains("CreateProcessW failed"));

        let _ = std::fs::remove_file(script);
    }

    #[test]
    fn test_spawn_worker_digest_mismatch_fails_before_process_creation() {
        let mut supervisor = ScriptSupervisor::new(2);
        let script = std::env::temp_dir().join(format!(
            "test_mismatch_{}_{}.lua",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&script, b"-- actual content").unwrap();

        let bogus_sha = "0000000000000000000000000000000000000000000000000000000000000000";
        let res = supervisor.spawn_worker(Path::new("cmd.exe"), &script, bogus_sha, "");
        assert!(
            res.is_err(),
            "Digest mismatch must reject spawn before any process creation"
        );
        assert!(res
            .unwrap_err()
            .contains("Pre-spawn digest verification failed"));

        let _ = std::fs::remove_file(script);
    }

    #[test]
    fn test_job_object_creation_with_zero_limit_fails_closed() {
        // Zero memory limit with JOB_OBJECT_LIMIT_PROCESS_MEMORY is invalid in Win32
        let res = JobObjectGuard::with_memory_limit(0);
        assert!(res.is_err(), "Zero memory limit must fail closed");
        assert!(res.unwrap_err().contains("SetInformationJobObject failed"));
    }

    #[test]
    fn test_spawn_worker_assignment_failure_terminates_suspended_child_fail_closed() {
        let mut supervisor = ScriptSupervisor::new(2);
        // Inject invalid Job Object handle (INVALID_HANDLE_VALUE)
        let invalid_job = JobObjectGuard::from_raw_handle_for_test(INVALID_HANDLE_VALUE);
        supervisor.set_job_for_test(Some(invalid_job));

        let script = std::env::temp_dir().join(format!(
            "test_assign_fail_{}_{}.lua",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&script, b"-- dummy lua content").unwrap();
        let sha = crate::sha256_hex(b"-- dummy lua content");

        // spawn_worker will create the process in CREATE_SUSPENDED state,
        // then job.assign_handle will fail, triggering immediate fail-closed termination.
        let res = supervisor.spawn_worker(Path::new("cmd.exe"), &script, &sha, "");
        assert!(
            res.is_err(),
            "Spawn must fail when Job Object assignment fails"
        );
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("CRITICAL: Job Object assignment failed"),
            "Error must report Job Object assignment failure: {}",
            err_msg
        );
        assert!(
            !supervisor.has_active_child(),
            "Supervisor must have no active child after assignment failure"
        );

        let _ = std::fs::remove_file(script);
    }

    #[test]
    fn test_thread_resumption_failure_returns_max() {
        // Verify Win32 ResumeThread on invalid handle returns u32::MAX
        unsafe {
            let res = ResumeThread(HANDLE(99999999 as *mut _));
            assert_eq!(
                res,
                u32::MAX,
                "ResumeThread on invalid handle must return u32::MAX"
            );
        }
    }

    #[test]
    fn test_verify_active_worker_integrity_no_child_is_noop() {
        let mut supervisor = ScriptSupervisor::new(2);
        let res = supervisor.verify_active_worker_integrity(None);
        assert!(res.is_ok(), "No child worker present should return Ok(())");
    }

    #[test]
    fn test_verify_active_worker_integrity_detects_deletion_and_modification_and_revocation() {
        let mut supervisor = ScriptSupervisor::new(2);

        // Spawn a real benign child process (cmd.exe /c timeout)
        let mut child_proc = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child");

        let temp_script = std::env::temp_dir().join(format!(
            "test_integrity_{}_{}.lua",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let content_v1 = b"-- original script content";
        std::fs::write(&temp_script, content_v1).unwrap();
        let digest_v1 = crate::sha256_hex(content_v1);

        let supervised_child =
            SupervisedChild::from_child(&child_proc).expect("duplicate child handle");
        supervisor.assign_supervised_child(supervised_child);
        supervisor.set_current_script_for_test(temp_script.clone(), digest_v1.clone());

        assert!(supervisor.has_active_child());
        assert!(supervisor.is_running());

        // 1. Valid state with matching digest and approval
        let mut trust_ledger = HashMap::new();
        trust_ledger.insert(temp_script.to_string_lossy().to_string(), digest_v1.clone());
        let res = supervisor.verify_active_worker_integrity(Some(&trust_ledger));
        assert!(
            res.is_ok(),
            "Untampered script with approval must verify successfully"
        );
        assert!(
            supervisor.has_active_child(),
            "Child worker must remain active"
        );

        // 2. Modified content detection
        std::fs::write(&temp_script, b"-- tampered script content").unwrap();
        let res_tampered = supervisor.verify_active_worker_integrity(Some(&trust_ledger));
        assert!(
            res_tampered.is_err(),
            "Modified content on disk must fail verification"
        );
        assert!(
            matches!(
                res_tampered.unwrap_err(),
                WorkerIntegrityError::ContentModified { .. }
            ),
            "Expected ContentModified error"
        );
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated upon content modification"
        );
        assert_eq!(*supervisor.state(), SupervisorState::Terminated);

        // Restore script and assign a new child for next check
        std::fs::write(&temp_script, content_v1).unwrap();
        let mut child_proc_2 = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child 2");
        let supervised_child_2 =
            SupervisedChild::from_child(&child_proc_2).expect("duplicate child 2 handle");
        supervisor.assign_supervised_child(supervised_child_2);
        supervisor.set_current_script_for_test(temp_script.clone(), digest_v1.clone());

        // 3. Trust revocation detection
        trust_ledger.clear(); // Revoke trust
        let res_revoked = supervisor.verify_active_worker_integrity(Some(&trust_ledger));
        assert!(
            res_revoked.is_err(),
            "Revoked trust ledger must fail verification"
        );
        assert!(
            matches!(
                res_revoked.unwrap_err(),
                WorkerIntegrityError::TrustRevoked(..)
            ),
            "Expected TrustRevoked error"
        );
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated upon trust revocation"
        );

        // Restore script and assign a new child for deletion check
        let mut child_proc_3 = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child 3");
        let supervised_child_3 =
            SupervisedChild::from_child(&child_proc_3).expect("duplicate child 3 handle");
        supervisor.assign_supervised_child(supervised_child_3);
        supervisor.set_current_script_for_test(temp_script.clone(), digest_v1.clone());

        // 4. Script deletion detection
        let _ = std::fs::remove_file(&temp_script);
        let res_deleted = supervisor.verify_active_worker_integrity(None);
        assert!(
            res_deleted.is_err(),
            "Deleted script file must fail verification"
        );
        assert!(
            matches!(
                res_deleted.unwrap_err(),
                WorkerIntegrityError::ScriptNotFound(..)
            ),
            "Expected ScriptNotFound error"
        );
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated upon script deletion"
        );

        let _ = child_proc.kill();
        let _ = child_proc.wait();
        let _ = child_proc_2.kill();
        let _ = child_proc_2.wait();
        let _ = child_proc_3.kill();
        let _ = child_proc_3.wait();
    }

    #[test]
    fn test_verify_active_worker_integrity_active_child_missing_metadata_fails_closed() {
        let mut supervisor = ScriptSupervisor::new(2);
        let mut child_proc = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child");

        let supervised_child =
            SupervisedChild::from_child(&child_proc).expect("duplicate child handle");
        supervisor.assign_supervised_child(supervised_child);
        // Explicitly leave current_script as None (active child missing metadata)
        assert!(supervisor.has_active_child());
        assert!(supervisor.current_script().is_none());

        let trust_ledger = HashMap::new();
        let res = supervisor.verify_active_worker_integrity(Some(&trust_ledger));

        assert!(
            res.is_err(),
            "Active child with missing script metadata must fail closed"
        );
        assert_eq!(
            res.unwrap_err(),
            WorkerIntegrityError::NoActiveScript,
            "Must return WorkerIntegrityError::NoActiveScript"
        );
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated fail-closed"
        );

        let _ = child_proc.kill();
        let _ = child_proc.wait();
    }

    #[test]
    fn test_verify_active_worker_integrity_active_child_missing_digest_fails_closed() {
        let mut supervisor = ScriptSupervisor::new(2);
        let mut child_proc = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child");

        let supervised_child =
            SupervisedChild::from_child(&child_proc).expect("duplicate child handle");
        supervisor.assign_supervised_child(supervised_child);
        supervisor.set_current_script_for_test(
            PathBuf::from("dummy.lua"),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        );
        // Explicitly clear current_digest while current_script is present
        supervisor.clear_current_digest_for_test();
        assert!(supervisor.has_active_child());
        assert!(supervisor.current_script().is_some());

        let trust_ledger = HashMap::new();
        let res = supervisor.verify_active_worker_integrity(Some(&trust_ledger));

        assert!(
            res.is_err(),
            "Active child with missing running digest metadata must fail closed"
        );
        assert_eq!(
            res.unwrap_err(),
            WorkerIntegrityError::NoActiveScript,
            "Must return WorkerIntegrityError::NoActiveScript when digest is missing"
        );
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated fail-closed"
        );

        let _ = child_proc.kill();
        let _ = child_proc.wait();
    }

    #[test]
    fn test_verify_active_worker_integrity_missing_trust_ledger_fails_closed() {
        let mut supervisor = ScriptSupervisor::new(2);
        let mut child_proc = Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "10", "127.0.0.1"])
            .spawn()
            .expect("spawn dummy child");

        let temp_script = std::env::temp_dir().join(format!(
            "test_missing_ledger_{}_{}.lua",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let content = b"-- test missing ledger script";
        std::fs::write(&temp_script, content).unwrap();
        let digest = crate::sha256_hex(content);

        let supervised_child =
            SupervisedChild::from_child(&child_proc).expect("duplicate child handle");
        supervisor.assign_supervised_child(supervised_child);
        supervisor.set_current_script_for_test(temp_script.clone(), digest);

        assert!(supervisor.has_active_child());

        // When trusted_digests is None (e.g. config lock poisoned or inaccessible)
        let res = supervisor.verify_active_worker_integrity(None);

        assert!(
            res.is_err(),
            "Missing authoritative trust ledger must fail closed"
        );
        match res.unwrap_err() {
            WorkerIntegrityError::TrustRevoked(msg) => {
                assert!(
                    msg.contains("missing or inaccessible"),
                    "Expected missing or inaccessible ledger message, got: {}",
                    msg
                );
            }
            other => panic!("Expected TrustRevoked error variant, got: {:?}", other),
        }
        assert!(
            !supervisor.has_active_child(),
            "Worker must be terminated fail-closed"
        );

        let _ = std::fs::remove_file(&temp_script);
        let _ = child_proc.kill();
        let _ = child_proc.wait();
    }

    #[test]
    fn test_supervised_child_from_child_duplicates_handle_safely() {
        // Verify that SupervisedChild::from_child creates an independent duplicate handle.
        // Dropping SupervisedChild must NOT invalidate child_proc's handle, preventing double-close.
        let mut child = Command::new("cmd.exe")
            .args(["/c", "exit", "0"])
            .spawn()
            .expect("spawn test child");

        {
            let mut supervised = SupervisedChild::from_child(&child).expect("duplicate handle");
            assert_ne!(
                supervised.handle().0,
                child.as_raw_handle(),
                "Duplicate handle must differ from original raw handle"
            );
            assert!(
                !supervised.handle().0.is_null() && supervised.handle() != INVALID_HANDLE_VALUE,
                "Duplicate handle must be a valid non-null handle"
            );
            let _ = supervised.wait();
            // supervised is dropped here, closing the duplicate handle ONLY
        }

        // The original Child must still have a valid handle that can be waited and dropped without OS error
        let status = child.wait().expect("wait on child with original handle");
        assert!(status.success(), "Child process must report success");
    }

    #[test]
    fn test_concurrent_script_trust_authorization_and_worker_lifecycle() {
        // Concurrently run script trust authorization (which opens and canonicalizes files)
        // and worker lifecycle invalidations (which spawn and drop processes) to prove
        // zero handle recycling interference and zero ERROR_INVALID_HANDLE failures.
        let temp_dir = std::env::temp_dir().join(format!(
            "test_concurrent_trust_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let script = temp_dir.join("concurrent.lua");
        let content = "function init() return 42 end";
        std::fs::write(&script, content).unwrap();
        let digest = crate::trust::sha256_hex(content.as_bytes());

        let mut registry = HashMap::new();
        registry.insert(script.to_string_lossy().to_string(), digest.clone());

        let script_clone = script.clone();
        let digest_clone = digest.clone();
        let registry_clone = registry.clone();

        let h1 = std::thread::spawn(move || {
            for _ in 0..30 {
                let res = crate::trust::authorize_script_trust(
                    &script_clone,
                    &script_clone.to_string_lossy(),
                    &registry_clone,
                );
                assert_eq!(
                    res,
                    Ok(digest_clone.clone()),
                    "Authorization must succeed without handle invalidation"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        let h2 = std::thread::spawn(move || {
            for _ in 0..30 {
                let mut sup = ScriptSupervisor::new(2);
                let mut child = Command::new("cmd.exe")
                    .args(["/c", "exit", "0"])
                    .spawn()
                    .expect("spawn child");
                let sc = SupervisedChild::from_child(&child).expect("from_child");
                sup.assign_supervised_child(sc);
                sup.invalidate_worker();
                let _ = child.wait();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        h1.join().unwrap();
        h2.join().unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
