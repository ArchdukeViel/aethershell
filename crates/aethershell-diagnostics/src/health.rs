use aethershell_core::{LifecycleState, VerificationStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemHealth {
    pub name: String,
    pub status: VerificationStatus,
    pub healthy: bool,
    pub last_heartbeat: DateTime<Utc>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDiagnosticReport {
    pub timestamp: DateTime<Utc>,
    pub overall_state: LifecycleState,
    pub os_build: u32,
    pub subsystems: Vec<SubsystemHealth>,
}

impl SystemDiagnosticReport {
    pub fn new(overall_state: LifecycleState, os_build: u32) -> Self {
        Self {
            timestamp: Utc::now(),
            overall_state,
            os_build,
            subsystems: Vec::new(),
        }
    }

    pub fn record_subsystem(&mut self, health: SubsystemHealth) {
        self.subsystems.push(health);
    }
}
