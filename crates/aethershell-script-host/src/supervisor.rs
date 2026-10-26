use serde::{Deserialize, Serialize};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Instant;
use tracing::{info, warn};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
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

/// Windows Job Object guard ensuring all child worker processes terminate when the job or parent closes.
#[derive(Debug)]
pub struct JobObjectGuard(HANDLE);

impl JobObjectGuard {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let job = CreateJobObjectW(None, None)
                .map_err(|e| format!("CreateJobObjectW failed: {}", e))?;

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

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

    pub fn assign_process(&self, child: &Child) -> Result<(), String> {
        unsafe {
            let child_handle = HANDLE(child.as_raw_handle());
            AssignProcessToJobObject(self.0, child_handle)
                .map_err(|e| format!("AssignProcessToJobObject failed: {}", e))
        }
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
    child: Option<Child>,
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

        // 3. Ensure Job Object is available
        if self.job.is_none() {
            self.job = JobObjectGuard::new().ok();
        }

        // 4. Spawn child process
        let mut cmd = Command::new(exe_path);
        cmd.arg("--script")
            .arg(script_path)
            .arg("--digest")
            .arg(approved_digest);
        if !pipe_name.is_empty() {
            cmd.arg("--pipe").arg(pipe_name);
        }
        cmd.stdin(Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn child worker process: {}", e))?;
        let pid = child.id();

        // 5. Assign to Windows Job Object
        if let Some(ref job) = self.job {
            if let Err(e) = job.assign_process(&child) {
                warn!("Job Object assignment failed for PID {}: {}", pid, e);
            } else {
                info!(
                    "Assigned worker PID {} to Windows Job Object (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)",
                    pid
                );
            }
        }

        self.record_start(pid);
        self.child = Some(child);

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
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    self.child = None;
                    self.record_exit(exit_code);
                    Some(exit_code)
                }
                Ok(None) => {
                    self.check_heartbeat_timeout();
                    None
                }
                Err(e) => {
                    warn!("Error waiting on worker child: {}", e);
                    self.child = None;
                    self.record_exit(-1);
                    Some(-1)
                }
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
        if let SupervisorState::Running { pid } = self.state {
            if let Some(last) = self.last_heartbeat {
                if last.elapsed().as_secs() >= self.heartbeat_timeout_secs as u64 {
                    self.state = SupervisorState::Quarantined {
                        reason: format!(
                            "Worker PID {} hung or missed heartbeats for >{}s. Terminated.",
                            pid, self.heartbeat_timeout_secs
                        ),
                    };
                    if let Some(mut c) = self.child.take() {
                        let _ = c.kill();
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
            let _ = child.kill();
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

        // Test assigning a real child process (e.g. powershell or cmd)
        let child = Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Start-Sleep -Milliseconds 200")
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
