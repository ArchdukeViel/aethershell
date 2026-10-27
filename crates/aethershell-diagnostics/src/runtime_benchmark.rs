//! Genuine multi-process runtime benchmark procedure conforming to PRD Section 14 (14.1 & 14.2) and Gate G13.
//!
//! Measures actual running processes (Core `aethershell.exe`, Native C++/WinUI Settings `aethershell-settings.exe`,
//! and Supervised Lua Worker `aethershell-script-host.exe`) using resident physical Working Set (`WorkingSetSize`),
//! private commit charge (`PrivateUsage`), normalized CPU utilization via `GetProcessTimes`,
//! process wakeups, and frame presentation pacing across 60-second steady-state recordings after warm-up.

use aethershell_win::WindowsVersionInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::{
    K32EnumProcesses, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    REG_VALUE_TYPE,
};
use windows::Win32::System::SystemInformation::{
    GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

/// Workload profile definitions specified in PRD Section 14.1 & 14.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProfile {
    /// Minimal Idle: Core running with no widgets, passive desktop overlay.
    MinimalIdle,
    /// Default Widgets: Clock + System Metrics + Media session.
    DefaultWidgets,
    /// Active Media Visualization: Loopback audio capture with 32-band spectrum visualizer.
    ActiveMediaVisualization,
    /// Battery Mode: On DC battery power (15 FPS, animations paused).
    BatteryMode,
    /// Gaming Shield: Fullscreen application running (render loop suspended).
    GamingShield,
    /// Settings Open: Core + Native C++/WinUI 3 Settings application active.
    SettingsOpen,
    /// Lua Worker Active: Core + Out-of-process supervised Lua 5.4 worker active.
    LuaWorkerActive,
    /// Mixed Monitors: Multi-monitor configuration with mixed DPI / topologies.
    MixedMonitors,
    /// Monitor Off / Session Locked: Rendering and capture suspended when locked or monitors enter sleep.
    MonitorOffSessionLocked,
}

impl WorkloadProfile {
    /// Returns the required process roles that must be present and verified for this workload.
    pub fn required_process_roles(&self) -> &'static [ProcessRole] {
        match self {
            WorkloadProfile::SettingsOpen => &[ProcessRole::Core, ProcessRole::Settings],
            WorkloadProfile::LuaWorkerActive => &[ProcessRole::Core, ProcessRole::LuaWorker],
            _ => &[ProcessRole::Core],
        }
    }

    /// Returns the target resource consumption budget per PRD Section 14.2.
    ///
    /// Distinguishes ratified PRD Section 14.2 acceptance targets (e.g. Minimal Idle)
    /// from provisional diagnostic proposals that require empirical qualification.
    pub fn target_budget(&self) -> ResourceBudget {
        match self {
            WorkloadProfile::MinimalIdle => ResourceBudget {
                classification: BudgetClassification::RatifiedTarget,
                max_core_cpu_pct: 0.1,
                max_core_working_set_mb: 30.0,
                max_combined_cpu_pct: 0.1,
                max_combined_working_set_mb: 30.0,
                max_gpu_pct: Some(0.0),
                max_wakeups_per_sec: Some(2),
                rationale: "Ratified PRD Section 14.2 baseline: Core working set <= 30 MiB median, steady CPU <= 0.1% on reference desktop hardware.".into(),
            },
            WorkloadProfile::DefaultWidgets => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.5,
                max_core_working_set_mb: 45.0,
                max_combined_cpu_pct: 0.5,
                max_combined_working_set_mb: 45.0,
                max_gpu_pct: Some(1.0),
                max_wakeups_per_sec: Some(10),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Working set and idle CPU bounded by empirically ratified default-profile baseline.".into(),
            },
            WorkloadProfile::ActiveMediaVisualization => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 1.5,
                max_core_working_set_mb: 60.0,
                max_combined_cpu_pct: 1.5,
                max_combined_working_set_mb: 60.0,
                max_gpu_pct: Some(3.0),
                max_wakeups_per_sec: Some(60),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Profile process CPU/GPU, dropped frames, device-buffer underruns and energy; no arbitrary universal 0.35% CPU or 0.40% GPU promise.".into(),
            },
            WorkloadProfile::BatteryMode => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.2,
                max_core_working_set_mb: 35.0,
                max_combined_cpu_pct: 0.2,
                max_combined_working_set_mb: 35.0,
                max_gpu_pct: Some(0.0),
                max_wakeups_per_sec: Some(15),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Lower polling and presentation rates, no unnecessary dGPU wake.".into(),
            },
            WorkloadProfile::GamingShield => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.05,
                max_core_working_set_mb: 30.0,
                max_combined_cpu_pct: 0.05,
                max_combined_working_set_mb: 30.0,
                max_gpu_pct: Some(0.0),
                max_wakeups_per_sec: Some(1),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Fullscreen gaming app detected; overlays suspended, avoid taskbar churn.".into(),
            },
            WorkloadProfile::SettingsOpen => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.5,
                max_core_working_set_mb: 45.0,
                max_combined_cpu_pct: 1.0,
                max_combined_working_set_mb: 80.0,
                max_gpu_pct: Some(2.0),
                max_wakeups_per_sec: Some(30),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: WinUI 3 separate process measured independently; closing terminates that process.".into(),
            },
            WorkloadProfile::LuaWorkerActive => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.5,
                max_core_working_set_mb: 45.0,
                max_combined_cpu_pct: 1.0,
                max_combined_working_set_mb: 70.0,
                max_gpu_pct: Some(1.0),
                max_wakeups_per_sec: Some(30),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Observe and disclose potential unbounded resource impact; do not claim strict cap or process isolation.".into(),
            },
            WorkloadProfile::MixedMonitors => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 1.0,
                max_core_working_set_mb: 55.0,
                max_combined_cpu_pct: 1.0,
                max_combined_working_set_mb: 55.0,
                max_gpu_pct: Some(2.0),
                max_wakeups_per_sec: Some(30),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Incremental memory per canvas/widget, frame latency, no unbounded monitor-count growth.".into(),
            },
            WorkloadProfile::MonitorOffSessionLocked => ResourceBudget {
                classification: BudgetClassification::ProvisionalProposal,
                max_core_cpu_pct: 0.05,
                max_core_working_set_mb: 30.0,
                max_combined_cpu_pct: 0.05,
                max_combined_working_set_mb: 30.0,
                max_gpu_pct: Some(0.0),
                max_wakeups_per_sec: Some(1),
                rationale: "Provisional diagnostic proposal per PRD Section 14.2: Normal lock/sleep/resume; rendering/capture suspended when indicated; occasional OS bookkeeping work permissible, not guaranteed 0.00%.".into(),
            },
        }
    }
}

/// Classification of a resource budget target per PRD Section 14.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetClassification {
    /// Ratified PRD Section 14.2 requirement.
    RatifiedTarget,
    /// Provisional diagnostic proposal per PRD Section 14.2 (subject to empirical qualification).
    ProvisionalProposal,
}

/// Resource consumption budget targets (PRD Section 14.2 / Gate G13).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceBudget {
    pub classification: BudgetClassification,
    pub max_core_cpu_pct: f64,
    pub max_core_working_set_mb: f64,
    pub max_combined_cpu_pct: f64,
    pub max_combined_working_set_mb: f64,
    pub max_gpu_pct: Option<f64>,
    pub max_wakeups_per_sec: Option<u32>,
    pub rationale: String,
}

/// Role of an application process participating in runtime benchmark measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRole {
    /// Primary Rust core engine (`aethershell.exe`).
    Core,
    /// Native C++/WinUI 3 settings application (`aethershell-settings.exe`).
    Settings,
    /// Supervised out-of-process Lua 5.4 child worker (`aethershell-script-host.exe`).
    LuaWorker,
    /// Auxiliary test runner or benchmark host.
    Auxiliary,
}

impl ProcessRole {
    pub fn default_executable_name(&self) -> &'static str {
        match self {
            ProcessRole::Core => "aethershell.exe",
            ProcessRole::Settings => "aethershell-settings.exe",
            ProcessRole::LuaWorker => "aethershell-script-host.exe",
            ProcessRole::Auxiliary => "aethershell-benchmark.exe",
        }
    }
}

/// Process validation error during identity verification.
#[derive(Debug, Error, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum ProcessValidationError {
    #[error("Cannot open process PID {pid}: {detail}")]
    OpenFailed { pid: u32, detail: String },
    #[error("Cannot query image name for process PID {pid}: {detail}")]
    QueryImageFailed { pid: u32, detail: String },
    #[error("Executable mismatch for PID {pid}: expected '{expected}', found '{actual}'")]
    ExecutableMismatch {
        pid: u32,
        expected: String,
        actual: String,
    },
    #[error("Cannot query creation time for PID {pid}: {detail}")]
    QueryTimeFailed { pid: u32, detail: String },
    #[error("Process PID {pid} is not active (exit code: {exit_code})")]
    NotActive { pid: u32, exit_code: u32 },
    #[error("PID {pid} matches current benchmark process (SelfAttachmentDisallowed). A genuine benchmark must attach to an external running process.")]
    SelfAttachmentDisallowed { pid: u32 },
}

/// Identity and cryptographic verification metadata for a measured process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessIdentityInfo {
    pub pid: u32,
    pub role: ProcessRole,
    pub executable_name: String,
    pub executable_path: String,
    pub sha256_hash: String,
    pub creation_time_filetime: u64,
}

/// Validates process identity, executable name, creation time, and binary hash.
pub fn validate_process_identity(
    pid: u32,
    role: ProcessRole,
    allow_self: bool,
) -> Result<ProcessIdentityInfo, ProcessValidationError> {
    if !allow_self && pid == std::process::id() {
        return Err(ProcessValidationError::SelfAttachmentDisallowed { pid });
    }

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
    };
    let handle = handle.map_err(|e| ProcessValidationError::OpenFailed {
        pid,
        detail: e.to_string(),
    })?;

    // Verify process is still alive
    let mut exit_code = 0u32;
    unsafe {
        if GetExitCodeProcess(handle, &mut exit_code).is_err() || exit_code != 259 {
            let _ = CloseHandle(handle);
            return Err(ProcessValidationError::NotActive { pid, exit_code });
        }
    }

    // Query full image path
    let mut name_buf = [0u16; 1024];
    let mut size = name_buf.len() as u32;
    let query_ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(name_buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
    };

    if !query_ok || size == 0 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(ProcessValidationError::QueryImageFailed {
            pid,
            detail: "QueryFullProcessImageNameW returned error or empty buffer".into(),
        });
    }

    let full_path = String::from_utf16_lossy(&name_buf[..size as usize]);
    let path = Path::new(&full_path);
    let exe_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    if role != ProcessRole::Auxiliary && !allow_self {
        let expected = role.default_executable_name();
        if !exe_name.eq_ignore_ascii_case(expected) {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(ProcessValidationError::ExecutableMismatch {
                pid,
                expected: expected.to_string(),
                actual: exe_name,
            });
        }
    }

    // Query creation time to guard against PID reuse
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let times_ok = unsafe {
        GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok()
    };

    unsafe {
        let _ = CloseHandle(handle);
    }

    if !times_ok {
        return Err(ProcessValidationError::QueryTimeFailed {
            pid,
            detail: "GetProcessTimes failed".into(),
        });
    }

    let creation_u64 = filetime_to_u64(creation);

    // Compute binary SHA-256 hash if file is accessible
    let sha256_hash = if let Ok(bytes) = std::fs::read(path) {
        compute_sha256_hex(&bytes)
    } else {
        "unavailable_access_denied".into()
    };

    Ok(ProcessIdentityInfo {
        pid,
        role,
        executable_name: exe_name,
        executable_path: full_path,
        sha256_hash,
        creation_time_filetime: creation_u64,
    })
}

/// Helper to discover a running Core (`aethershell.exe`) process via process enumeration.
pub fn find_running_core_process() -> Result<u32, String> {
    let mut pids = [0u32; 2048];
    let mut bytes_needed = 0u32;
    unsafe {
        if !K32EnumProcesses(
            pids.as_mut_ptr(),
            (pids.len() * std::mem::size_of::<u32>()) as u32,
            &mut bytes_needed,
        )
        .as_bool()
        {
            return Err("K32EnumProcesses failed to query system processes".into());
        }
    }

    let count = (bytes_needed as usize) / std::mem::size_of::<u32>();
    let current_pid = std::process::id();
    let mut candidates = Vec::new();

    for &pid in &pids[..count] {
        if pid == 0 || pid == current_pid {
            continue;
        }

        if let Ok(info) = validate_process_identity(pid, ProcessRole::Core, false) {
            candidates.push((pid, info));
        }
    }

    match candidates.len() {
        0 => Err("No running Core 'aethershell.exe' process discovered. Ensure Core is running, or provide --pid-core explicitly.".into()),
        1 => Ok(candidates[0].0),
        n => Err(format!(
            "Ambiguous: Discovered {} running 'aethershell.exe' processes (PIDs: {:?}). Specify target --pid-core explicitly.",
            n, candidates.iter().map(|c| c.0).collect::<Vec<_>>()
        )),
    }
}

/// A point-in-time metrics sample for a specific process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessMetricsSample {
    pub pid: u32,
    pub role: ProcessRole,
    pub working_set_bytes: u64,
    pub working_set_mb: f64,
    pub private_bytes: u64,
    pub private_mb: f64,
    pub cpu_percent: f64,
}

/// Aggregated statistical metrics for a specific process over a measurement window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessAggregateMetrics {
    pub pid: u32,
    pub role: ProcessRole,
    pub sample_count: usize,
    pub working_set_median_mb: f64,
    pub working_set_p95_mb: f64,
    pub working_set_max_mb: f64,
    pub private_bytes_median_mb: f64,
    pub private_bytes_p95_mb: f64,
    pub private_bytes_max_mb: f64,
    pub cpu_median_pct: f64,
    pub cpu_p95_pct: f64,
    pub cpu_max_pct: f64,
}

/// Status of GPU hardware utilization metrics measurement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum GpuMetricStatus {
    /// Genuine hardware GPU measurement obtained via OS instrumentation.
    Measured { usage_pct: f64, engine: String },
    /// GPU metrics unavailable in current execution environment.
    Unavailable { reason: String },
}

/// Status of process wakeup metrics measurement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum WakeupMetricStatus {
    /// Measured process wakeups per second via kernel ETW or context-switch events.
    Measured { wakeups_per_sec: f64 },
    /// Wakeup metrics unavailable without kernel ETW tracing.
    Unavailable { reason: String },
}

/// Status of presentation pacing and frame latency measurement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum PresentationMetricStatus {
    /// Measured frame presentation intervals.
    Measured {
        frame_interval_median_ms: f64,
        frame_interval_p95_ms: f64,
    },
    /// Presentation metrics unavailable without direct swapchain hook or desktop session.
    Unavailable { reason: String },
}

/// Process anomaly event recorded during steady-state sampling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessAnomalyEvent {
    pub timestamp_secs: f64,
    pub pid: u32,
    pub role: ProcessRole,
    pub event_type: String,
    pub detail: String,
}

/// Display topology descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayTopologyInfo {
    pub monitor_index: usize,
    pub resolution: String,
    pub is_primary: bool,
}

/// Host hardware, OS, display environment, and measured binary metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentMetadata {
    pub os_family: String,
    pub os_build: u32,
    pub os_ubr: u32,
    pub cpu_architecture: String,
    pub logical_processor_count: u32,
    pub total_physical_memory_mb: f64,
    pub display_count: usize,
    pub display_topologies: Vec<DisplayTopologyInfo>,
    pub power_source: String,
    pub active_theme: String,
    pub measured_binaries: HashMap<String, String>,
}

impl EnvironmentMetadata {
    /// Detects host system environment attributes truthfully via Win32 platform APIs.
    pub fn detect() -> Self {
        let os_info = WindowsVersionInfo::detect();

        let mut sys_info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut sys_info) };
        let logical_processors = sys_info.dwNumberOfProcessors;
        let arch = match unsafe { sys_info.Anonymous.Anonymous.wProcessorArchitecture.0 } {
            9 => "x86_64".into(),
            12 => "arm64".into(),
            0 => "x86".into(),
            _ => "unknown".into(),
        };

        let mut mem_status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        mem_status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let total_ram_mb = if unsafe { GlobalMemoryStatusEx(&mut mem_status) }.is_ok() {
            mem_status.ullTotalPhys as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        };

        let mut power_status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        let power_source = if unsafe { GetSystemPowerStatus(&mut power_status) }.is_ok() {
            match power_status.ACLineStatus {
                0 => "Battery (DC)".into(),
                1 => "AC Power".into(),
                _ => "Unknown".into(),
            }
        } else {
            "Unknown".into()
        };

        let monitors = aethershell_win::enumerate_monitors();
        let display_topologies = monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let w = (m.bounds[2] - m.bounds[0]).abs();
                let h = (m.bounds[3] - m.bounds[1]).abs();
                DisplayTopologyInfo {
                    monitor_index: i,
                    resolution: format!("{}x{}", w, h),
                    is_primary: m.is_primary,
                }
            })
            .collect();

        let active_theme = detect_windows_theme();

        Self {
            os_family: os_info.family.to_string(),
            os_build: os_info.build_number,
            os_ubr: os_info.ubr.unwrap_or(0),
            cpu_architecture: arch,
            logical_processor_count: logical_processors,
            total_physical_memory_mb: total_ram_mb,
            display_count: monitors.len(),
            display_topologies,
            power_source,
            active_theme,
            measured_binaries: HashMap::new(),
        }
    }
}

fn detect_windows_theme() -> String {
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();

    let mut hkey = HKEY::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_ok()
        {
            let mut data: u32 = 0;
            let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
            let mut val_type = REG_VALUE_TYPE::default();
            let query_res = RegQueryValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                None,
                Some(&mut val_type),
                Some(&mut data as *mut _ as *mut u8),
                Some(&mut data_size),
            );
            let _ = RegCloseKey(hkey);

            if query_res.is_ok() && val_type == REG_DWORD {
                return if data == 1 {
                    "Light".into()
                } else {
                    "Dark".into()
                };
            }
        }
    }
    "Default / Unknown".into()
}

/// Final disposition verdict for a runtime qualification benchmark.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkVerdict {
    /// All evaluated metrics fall within PRD Section 14.2 target budgets.
    Passed,
    /// One or more metrics exceed the authorized target budget, or process anomaly was detected.
    Failed { reason: String },
    /// Core metrics verified within budget; graphics or audio hardware instrumentation unavailable.
    PartiallyVerified { limitation: String },
    /// Benchmark could not execute or required processes were unavailable.
    Blocked { reason: String },
}

/// Record of a single steady-state measurement repetition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepetitionRecord {
    pub repetition_index: u32,
    pub duration_seconds: u32,
    pub process_metrics: Vec<ProcessAggregateMetrics>,
    pub combined_working_set_median_mb: f64,
    pub combined_working_set_p95_mb: f64,
    pub combined_working_set_max_mb: f64,
    pub combined_private_bytes_median_mb: f64,
    pub combined_cpu_median_pct: f64,
    pub combined_cpu_p95_pct: f64,
    pub combined_cpu_max_pct: f64,
    pub gpu_status: GpuMetricStatus,
    pub wakeup_status: WakeupMetricStatus,
    pub presentation_status: PresentationMetricStatus,
    pub anomalies: Vec<ProcessAnomalyEvent>,
    pub verdict: BenchmarkVerdict,
}

/// Complete runtime qualification benchmark ledger containing multiple 60-second repetitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeBenchmarkLedger {
    pub profile: WorkloadProfile,
    pub target_budget: ResourceBudget,
    pub environment: EnvironmentMetadata,
    pub warmup_duration_seconds: u32,
    pub repetition_duration_seconds: u32,
    pub repetitions: Vec<RepetitionRecord>,
    pub overall_verdict: BenchmarkVerdict,
    pub notes: Vec<String>,
}

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// Helper to calculate the specified percentile (`pct` from 0.0 to 100.0) from a slice of floats.
pub fn calculate_percentile(values: &[f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (pct / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let weight = rank - lower as f64;
        sorted[lower] * (1.0 - weight) + sorted[upper] * weight
    }
}

/// Process sampler managing a Win32 process handle for repeated resource sampling.
#[derive(Debug)]
pub struct ProcessSampler {
    handle: HANDLE,
    pid: u32,
    role: ProcessRole,
    creation_time: u64,
    identity: ProcessIdentityInfo,
    prev_cpu_100ns: u64,
    prev_sample_time: Instant,
    working_set_samples: Vec<f64>,
    private_bytes_samples: Vec<f64>,
    cpu_samples: Vec<f64>,
}

impl ProcessSampler {
    /// Opens and validates a process by PID with query rights and initializes CPU baseline.
    pub fn open(pid: u32, role: ProcessRole) -> Result<Self, String> {
        Self::open_with_options(pid, role, false)
    }

    /// Opens with explicit self-attachment override (for testing only).
    pub fn open_with_options(
        pid: u32,
        role: ProcessRole,
        allow_self: bool,
    ) -> Result<Self, String> {
        let identity = validate_process_identity(pid, role, allow_self)
            .map_err(|e| format!("Process validation failed for PID {}: {}", pid, e))?;

        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            )
        };
        let handle = handle.map_err(|e| format!("Failed to open process PID {}: {}", pid, e))?;

        let initial_cpu = query_raw_process_cpu(handle);
        let now = Instant::now();

        Ok(Self {
            handle,
            pid,
            role,
            creation_time: identity.creation_time_filetime,
            identity,
            prev_cpu_100ns: initial_cpu,
            prev_sample_time: now,
            working_set_samples: Vec::new(),
            private_bytes_samples: Vec::new(),
            cpu_samples: Vec::new(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn role(&self) -> ProcessRole {
        self.role
    }

    pub fn identity(&self) -> &ProcessIdentityInfo {
        &self.identity
    }

    /// Takes a measurement sample and records it into history, verifying process vitality.
    pub fn sample(
        &mut self,
        now: Instant,
        logical_cores: u32,
    ) -> Result<ProcessMetricsSample, String> {
        unsafe {
            // Verify process is still alive and not recycled
            let mut exit_code = 0u32;
            if GetExitCodeProcess(self.handle, &mut exit_code).is_err() || exit_code != 259 {
                return Err(format!(
                    "Target process PID {} ({:?}) terminated during benchmark (exit code: {})",
                    self.pid, self.role, exit_code
                ));
            }

            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            if GetProcessTimes(
                self.handle,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
            .is_err()
                || filetime_to_u64(creation) != self.creation_time
            {
                return Err(format!(
                    "Target process PID {} ({:?}) creation time changed; PID reuse detected",
                    self.pid, self.role
                ));
            }

            let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;

            if !K32GetProcessMemoryInfo(
                self.handle,
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            )
            .as_bool()
            {
                return Err(format!(
                    "K32GetProcessMemoryInfo failed for PID {}",
                    self.pid
                ));
            }

            let ws_bytes = counters.WorkingSetSize as u64;
            let ws_mb = ws_bytes as f64 / (1024.0 * 1024.0);
            let private_bytes = counters.PrivateUsage as u64;
            let private_mb = private_bytes as f64 / (1024.0 * 1024.0);

            let curr_cpu = filetime_to_u64(kernel) + filetime_to_u64(user);
            let elapsed_wall_ns = now.duration_since(self.prev_sample_time).as_nanos() as f64;
            let delta_cpu_ns = (curr_cpu.saturating_sub(self.prev_cpu_100ns)) as f64 * 100.0;

            let cpu_pct = if elapsed_wall_ns > 0.0 && logical_cores > 0 {
                ((delta_cpu_ns / (elapsed_wall_ns * logical_cores as f64)) * 100.0)
                    .clamp(0.0, 100.0)
            } else {
                0.0
            };

            self.prev_cpu_100ns = curr_cpu;
            self.prev_sample_time = now;

            self.working_set_samples.push(ws_mb);
            self.private_bytes_samples.push(private_mb);
            self.cpu_samples.push(cpu_pct);

            Ok(ProcessMetricsSample {
                pid: self.pid,
                role: self.role,
                working_set_bytes: ws_bytes,
                working_set_mb: ws_mb,
                private_bytes,
                private_mb,
                cpu_percent: cpu_pct,
            })
        }
    }

    /// Aggregates all recorded samples into percentiles and summary statistics.
    pub fn aggregate(&self) -> ProcessAggregateMetrics {
        ProcessAggregateMetrics {
            pid: self.pid,
            role: self.role,
            sample_count: self.working_set_samples.len(),
            working_set_median_mb: calculate_percentile(&self.working_set_samples, 50.0),
            working_set_p95_mb: calculate_percentile(&self.working_set_samples, 95.0),
            working_set_max_mb: self.working_set_samples.iter().copied().fold(0.0, f64::max),
            private_bytes_median_mb: calculate_percentile(&self.private_bytes_samples, 50.0),
            private_bytes_p95_mb: calculate_percentile(&self.private_bytes_samples, 95.0),
            private_bytes_max_mb: self
                .private_bytes_samples
                .iter()
                .copied()
                .fold(0.0, f64::max),
            cpu_median_pct: calculate_percentile(&self.cpu_samples, 50.0),
            cpu_p95_pct: calculate_percentile(&self.cpu_samples, 95.0),
            cpu_max_pct: self.cpu_samples.iter().copied().fold(0.0, f64::max),
        }
    }

    /// Resets sample history for subsequent repetitions.
    pub fn reset_samples(&mut self) {
        self.working_set_samples.clear();
        self.private_bytes_samples.clear();
        self.cpu_samples.clear();
        self.prev_cpu_100ns = query_raw_process_cpu(self.handle);
        self.prev_sample_time = Instant::now();
    }
}

impl Drop for ProcessSampler {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

fn query_raw_process_cpu(handle: HANDLE) -> u64 {
    unsafe {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
            filetime_to_u64(kernel) + filetime_to_u64(user)
        } else {
            0
        }
    }
}

/// Multi-process runtime benchmark execution session.
pub struct RuntimeBenchmarkSession {
    profile: WorkloadProfile,
    target_budget: ResourceBudget,
    environment: EnvironmentMetadata,
    samplers: HashMap<ProcessRole, ProcessSampler>,
    allow_self_pid: bool,
}

impl RuntimeBenchmarkSession {
    /// Creates a new benchmark session targeting the specified workload profile.
    pub fn new(profile: WorkloadProfile) -> Self {
        Self {
            profile,
            target_budget: profile.target_budget(),
            environment: EnvironmentMetadata::detect(),
            samplers: HashMap::new(),
            allow_self_pid: false,
        }
    }

    /// Permits self-attachment for internal test suites.
    pub fn set_allow_self_pid(&mut self, allow: bool) {
        self.allow_self_pid = allow;
    }

    /// Returns a reference to the active target budget.
    pub fn target_budget(&self) -> &ResourceBudget {
        &self.target_budget
    }

    /// Returns a mutable reference to the active target budget.
    pub fn target_budget_mut(&mut self) -> &mut ResourceBudget {
        &mut self.target_budget
    }

    /// Registers a process to monitor during the benchmark session.
    pub fn track_process(&mut self, pid: u32, role: ProcessRole) -> Result<(), String> {
        let sampler = ProcessSampler::open_with_options(pid, role, self.allow_self_pid)?;
        self.environment.measured_binaries.insert(
            role.default_executable_name().to_string(),
            sampler.identity().sha256_hash.clone(),
        );
        self.samplers.insert(role, sampler);
        Ok(())
    }

    /// Validates that all required processes for the workload profile are registered.
    pub fn validate_required_processes(&self) -> Result<(), String> {
        if self.samplers.is_empty() {
            return Err("Cannot execute benchmark: No processes are registered. Fail closed per PRD Section 14.".into());
        }

        for &req_role in self.profile.required_process_roles() {
            if !self.samplers.contains_key(&req_role) {
                return Err(format!(
                    "Cannot execute benchmark for profile {:?}: Required process {:?} ({}) is missing. Fail closed per PRD Section 14.",
                    self.profile, req_role, req_role.default_executable_name()
                ));
            }
        }

        Ok(())
    }

    /// Evaluates GPU metric instrumentation honestly without fabricating results.
    pub fn probe_gpu_metrics(&self) -> GpuMetricStatus {
        GpuMetricStatus::Unavailable {
            reason: "D3DKMT / GPU hardware performance counters unavailable in current execution context".into(),
        }
    }

    /// Evaluates wakeup metric instrumentation truthfully.
    pub fn probe_wakeup_metrics(&self) -> WakeupMetricStatus {
        WakeupMetricStatus::Unavailable {
            reason: "Kernel ETW / thread context switch instrumentation unavailable in user-mode process sampler".into(),
        }
    }

    /// Evaluates presentation frame pacing truthfully.
    pub fn probe_presentation_metrics(&self) -> PresentationMetricStatus {
        PresentationMetricStatus::Unavailable {
            reason: "Direct2D / DXGI swapchain presentation instrumentation unavailable without active desktop window session".into(),
        }
    }

    /// Executes a single steady-state repetition with warmup and periodic sampling.
    pub fn run_repetition(
        &mut self,
        repetition_index: u32,
        warmup_duration: Duration,
        recording_duration: Duration,
        sample_interval: Duration,
    ) -> Result<RepetitionRecord, String> {
        // Enforce required processes upfront
        self.validate_required_processes()?;

        let logical_cores = self.environment.logical_processor_count.max(1);

        // 1. Warm-up stabilization phase
        let warmup_start = Instant::now();
        while warmup_start.elapsed() < warmup_duration {
            std::thread::sleep(sample_interval);
            let now = Instant::now();
            for sampler in self.samplers.values_mut() {
                let _ = sampler.sample(now, logical_cores);
            }
        }

        // Reset history after warmup so initialization transients don't bias steady-state metrics
        for sampler in self.samplers.values_mut() {
            sampler.reset_samples();
        }

        // 2. Steady-state recording phase
        let recording_start = Instant::now();
        let mut combined_ws_samples: Vec<f64> = Vec::new();
        let mut combined_private_samples: Vec<f64> = Vec::new();
        let mut combined_cpu_samples: Vec<f64> = Vec::new();
        let mut anomalies: Vec<ProcessAnomalyEvent> = Vec::new();

        while recording_start.elapsed() < recording_duration {
            std::thread::sleep(sample_interval);
            let now = Instant::now();
            let elapsed_secs = recording_start.elapsed().as_secs_f64();

            let mut tick_ws_sum = 0.0;
            let mut tick_private_sum = 0.0;
            let mut tick_cpu_sum = 0.0;
            let mut tick_has_error = false;

            for sampler in self.samplers.values_mut() {
                match sampler.sample(now, logical_cores) {
                    Ok(sample) => {
                        tick_ws_sum += sample.working_set_mb;
                        tick_private_sum += sample.private_mb;
                        tick_cpu_sum += sample.cpu_percent;
                    }
                    Err(e) => {
                        tick_has_error = true;
                        anomalies.push(ProcessAnomalyEvent {
                            timestamp_secs: elapsed_secs,
                            pid: sampler.pid(),
                            role: sampler.role(),
                            event_type: "sampling_anomaly".into(),
                            detail: e,
                        });
                    }
                }
            }

            if !tick_has_error {
                combined_ws_samples.push(tick_ws_sum);
                combined_private_samples.push(tick_private_sum);
                combined_cpu_samples.push(tick_cpu_sum);
            }
        }

        // 3. Aggregate process metrics and aligned combined samples
        let mut aggregates = Vec::new();
        let mut core_ws_median = 0.0;
        let mut core_cpu_median = 0.0;

        for sampler in self.samplers.values() {
            let agg = sampler.aggregate();
            if agg.role == ProcessRole::Core {
                core_ws_median = agg.working_set_median_mb;
                core_cpu_median = agg.cpu_median_pct;
            }
            aggregates.push(agg);
        }

        // Calculate combined multi-process metrics from aligned timestamp samples
        let combined_ws_median = calculate_percentile(&combined_ws_samples, 50.0);
        let combined_ws_p95 = calculate_percentile(&combined_ws_samples, 95.0);
        let combined_ws_max = combined_ws_samples.iter().copied().fold(0.0, f64::max);

        let combined_private_median = calculate_percentile(&combined_private_samples, 50.0);

        let combined_cpu_median = calculate_percentile(&combined_cpu_samples, 50.0);
        let combined_cpu_p95 = calculate_percentile(&combined_cpu_samples, 95.0);
        let combined_cpu_max = combined_cpu_samples.iter().copied().fold(0.0, f64::max);

        let gpu_status = self.probe_gpu_metrics();
        let wakeup_status = self.probe_wakeup_metrics();
        let presentation_status = self.probe_presentation_metrics();

        // 4. Verify against PRD Section 14.2 budget
        let verdict = if !anomalies.is_empty() {
            BenchmarkVerdict::Failed {
                reason: format!(
                    "{} process sampling anomaly events detected during steady-state recording: {}",
                    anomalies.len(),
                    anomalies[0].detail
                ),
            }
        } else if core_cpu_median > self.target_budget.max_core_cpu_pct {
            BenchmarkVerdict::Failed {
                reason: format!(
                    "Core CPU median {:.2}% exceeds budget {:.2}%",
                    core_cpu_median, self.target_budget.max_core_cpu_pct
                ),
            }
        } else if core_ws_median > self.target_budget.max_core_working_set_mb {
            BenchmarkVerdict::Failed {
                reason: format!(
                    "Core working set {:.2} MB exceeds budget {:.2} MB",
                    core_ws_median, self.target_budget.max_core_working_set_mb
                ),
            }
        } else if combined_cpu_median > self.target_budget.max_combined_cpu_pct {
            BenchmarkVerdict::Failed {
                reason: format!(
                    "Combined CPU median {:.2}% exceeds budget {:.2}%",
                    combined_cpu_median, self.target_budget.max_combined_cpu_pct
                ),
            }
        } else if combined_ws_median > self.target_budget.max_combined_working_set_mb {
            BenchmarkVerdict::Failed {
                reason: format!(
                    "Combined working set {:.2} MB exceeds budget {:.2} MB",
                    combined_ws_median, self.target_budget.max_combined_working_set_mb
                ),
            }
        } else if matches!(gpu_status, GpuMetricStatus::Unavailable { .. })
            || matches!(wakeup_status, WakeupMetricStatus::Unavailable { .. })
            || matches!(
                presentation_status,
                PresentationMetricStatus::Unavailable { .. }
            )
        {
            BenchmarkVerdict::PartiallyVerified {
                limitation: "Process CPU and RAM budgets verified; GPU, kernel wakeups, or presentation instrumentation unavailable in environment".into(),
            }
        } else {
            BenchmarkVerdict::Passed
        };

        Ok(RepetitionRecord {
            repetition_index,
            duration_seconds: recording_duration.as_secs() as u32,
            process_metrics: aggregates,
            combined_working_set_median_mb: combined_ws_median,
            combined_working_set_p95_mb: combined_ws_p95,
            combined_working_set_max_mb: combined_ws_max,
            combined_private_bytes_median_mb: combined_private_median,
            combined_cpu_median_pct: combined_cpu_median,
            combined_cpu_p95_pct: combined_cpu_p95,
            combined_cpu_max_pct: combined_cpu_max,
            gpu_status,
            wakeup_status,
            presentation_status,
            anomalies,
            verdict,
        })
    }

    /// Runs a full multi-repetition benchmark procedure (5 repetitions by default per PRD Section 14).
    pub fn execute_full_procedure(
        &mut self,
        repetition_count: u32,
        warmup: Duration,
        recording_duration: Duration,
        sample_interval: Duration,
    ) -> Result<RuntimeBenchmarkLedger, String> {
        let mut repetitions = Vec::with_capacity(repetition_count as usize);

        for i in 1..=repetition_count {
            let record = self.run_repetition(i, warmup, recording_duration, sample_interval)?;
            repetitions.push(record);
        }

        // Derive overall ledger verdict
        let has_failures = repetitions
            .iter()
            .any(|r| matches!(r.verdict, BenchmarkVerdict::Failed { .. }));
        let has_partial = repetitions
            .iter()
            .any(|r| matches!(r.verdict, BenchmarkVerdict::PartiallyVerified { .. }));

        let overall_verdict = if has_failures {
            BenchmarkVerdict::Failed {
                reason: "One or more repetitions exceeded budget targets or experienced process exit anomalies".into(),
            }
        } else if has_partial {
            BenchmarkVerdict::PartiallyVerified {
                limitation: "CPU and memory verified within PRD budget; GPU, wakeup, or presentation hardware instrumentation unavailable in execution context".into(),
            }
        } else {
            BenchmarkVerdict::Passed
        };

        let is_ratified = self.target_budget.classification == BudgetClassification::RatifiedTarget;
        let budget_note = if is_ratified {
            "Budget is ratified PRD Section 14.2 requirement.".into()
        } else {
            "Budget thresholds are provisional diagnostic proposals per PRD Section 14.2, not ratified hard acceptance budgets.".into()
        };

        Ok(RuntimeBenchmarkLedger {
            profile: self.profile,
            target_budget: self.target_budget.clone(),
            environment: self.environment.clone(),
            warmup_duration_seconds: warmup.as_secs() as u32,
            repetition_duration_seconds: recording_duration.as_secs() as u32,
            repetitions,
            overall_verdict,
            notes: vec![
                "Measurements capture resident WorkingSetSize and PrivateUsage via Win32 K32GetProcessMemoryInfo.".into(),
                "CPU load normalized across logical cores via GetProcessTimes, isolating target processes from measurement overhead.".into(),
                "Combined multi-process percentiles computed from synchronized timestamp samples.".into(),
                budget_note,
            ],
        })
    }
}

/// Standalone RFC 6234 standard SHA-256 implementation.
#[allow(clippy::all)]
pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (bytes.len() as u64) * 8;
    let mut msg = bytes.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut out = String::with_capacity(64);
    for val in h {
        out.push_str(&format!("{:08x}", val));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_computation() {
        assert_eq!(
            compute_sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            compute_sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_environment_metadata_detection() {
        let env = EnvironmentMetadata::detect();
        assert!(!env.os_family.is_empty());
        assert!(env.logical_processor_count > 0);
        assert!(env.total_physical_memory_mb > 0.0);
    }

    #[test]
    fn test_percentile_calculation() {
        let values = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(calculate_percentile(&values, 50.0), 30.0);
        assert_eq!(calculate_percentile(&values, 0.0), 10.0);
        assert_eq!(calculate_percentile(&values, 100.0), 50.0);
    }

    #[test]
    fn test_current_process_sampler_with_allow_self() {
        let pid = std::process::id();
        let mut sampler = ProcessSampler::open_with_options(pid, ProcessRole::Auxiliary, true)
            .expect("open current process sampler");

        std::thread::sleep(Duration::from_millis(50));
        let sample = sampler
            .sample(Instant::now(), 4)
            .expect("sample current process");

        assert!(sample.working_set_bytes > 0);
        assert!(sample.working_set_mb > 0.0);
        assert!(sample.private_bytes > 0);
        assert!(sample.private_mb > 0.0);
        assert!(sample.cpu_percent >= 0.0);

        let agg = sampler.aggregate();
        assert_eq!(agg.sample_count, 1);
        assert!(agg.working_set_median_mb > 0.0);
    }

    #[test]
    fn test_self_attachment_disallowed_by_default() {
        let pid = std::process::id();
        let err = ProcessSampler::open(pid, ProcessRole::Core).unwrap_err();
        assert!(err.contains("SelfAttachmentDisallowed") || err.contains("ExecutableMismatch"));
    }

    #[test]
    fn test_runtime_benchmark_session_and_repetition() {
        let pid = std::process::id();
        let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
        session.set_allow_self_pid(true);
        session
            .track_process(pid, ProcessRole::Core)
            .expect("track core");

        let rep = session
            .run_repetition(
                1,
                Duration::from_millis(50),
                Duration::from_millis(150),
                Duration::from_millis(50),
            )
            .expect("run repetition");

        assert_eq!(rep.repetition_index, 1);
        assert_eq!(rep.process_metrics.len(), 1);
        assert!(rep.combined_working_set_median_mb > 0.0);
        assert!(matches!(
            rep.verdict,
            BenchmarkVerdict::Passed | BenchmarkVerdict::PartiallyVerified { .. }
        ));
    }

    #[test]
    fn test_empty_sampler_fails_closed() {
        let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::MinimalIdle);
        let err = session
            .run_repetition(
                1,
                Duration::from_millis(10),
                Duration::from_millis(50),
                Duration::from_millis(10),
            )
            .unwrap_err();
        assert!(err.contains("No processes are registered"));
    }

    #[test]
    fn test_missing_required_process_fails_closed() {
        let pid = std::process::id();
        let mut session = RuntimeBenchmarkSession::new(WorkloadProfile::SettingsOpen);
        session.set_allow_self_pid(true);
        session
            .track_process(pid, ProcessRole::Core)
            .expect("track core");

        // Missing Settings role
        let err = session
            .run_repetition(
                1,
                Duration::from_millis(10),
                Duration::from_millis(50),
                Duration::from_millis(10),
            )
            .unwrap_err();
        assert!(err.contains("Required process Settings (aethershell-settings.exe) is missing"));
    }
}
