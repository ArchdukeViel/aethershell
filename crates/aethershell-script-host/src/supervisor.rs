use serde::{Deserialize, Serialize};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::Child;
use std::time::Instant;
use tracing::info;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, WaitForSingleObject,
    CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
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
            if !self.process_handle.0.is_null() {
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
            if !self.process_handle.0.is_null() {
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
        unsafe {
            let job = CreateJobObjectW(None, None)
                .map_err(|e| format!("CreateJobObjectW failed: {}", e))?;

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = 128 * 1024 * 1024; // 128 MiB per-worker memory limit (PRD Table 17-1)

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
}

impl Drop for JobObjectGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0 .0.is_null() {
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

    pub fn active_child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
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
        }
        self.state = SupervisorState::Terminated;
        self.last_heartbeat = None;
    }

    pub fn record_start(&mut self, pid: u32) {
        self.state = SupervisorState::Running { pid };
        self.last_heartbeat = Some(Instant::now());
    }

    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
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
}
