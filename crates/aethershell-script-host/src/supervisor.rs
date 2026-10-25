use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Status and health of the supervised script host child process (PRD Section 13.2 / Gate G10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorState {
    Dormant,
    Spawning,
    Running { pid: u32 },
    Quarantined { reason: String },
    Terminated,
}

/// Script supervisor managing on-demand child worker process and crash loop recovery.
#[derive(Debug)]
pub struct ScriptSupervisor {
    state: SupervisorState,
    crash_count: u32,
    max_crashes: u32,
    last_heartbeat: Option<Instant>,
    heartbeat_timeout_secs: u32,
}

impl ScriptSupervisor {
    pub fn new(max_crashes: u32) -> Self {
        Self {
            state: SupervisorState::Dormant,
            crash_count: 0,
            max_crashes,
            last_heartbeat: None,
            heartbeat_timeout_secs: 5,
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

    pub fn record_start(&mut self, pid: u32) {
        self.state = SupervisorState::Running { pid };
        self.last_heartbeat = Some(Instant::now());
    }

    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
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

    /// Checks if worker has missed heartbeat responses beyond grace period.
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
                    return true; // Timed out
                }
            }
        }
        false
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
}
