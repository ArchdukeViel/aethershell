use aethershell_core::{TaskbarEffect, TaskbarRule};
use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: &str = "1.0.0";

/// Master configuration model matching PRD Section 10.2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub schema_version: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub taskbar: TaskbarConfig,
    #[serde(default)]
    pub desktop: DesktopConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub power: PowerConfig,
    #[serde(default)]
    pub gaming: GamingConfig,
    #[serde(default)]
    pub scripting: ScriptingConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub updates: UpdatesConfig,
}

fn default_profile() -> String {
    "signature_minimal".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION.into(),
            profile: default_profile(),
            general: GeneralConfig::default(),
            taskbar: TaskbarConfig::default(),
            desktop: DesktopConfig::default(),
            audio: AudioConfig::default(),
            telemetry: TelemetryConfig::default(),
            power: PowerConfig::default(),
            gaming: GamingConfig::default(),
            scripting: ScriptingConfig::default(),
            privacy: PrivacyConfig::default(),
            updates: UpdatesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralConfig {
    #[serde(default = "default_true")]
    pub start_with_windows: bool,
    #[serde(default = "default_system")]
    pub language: String,
    #[serde(default = "default_true")]
    pub check_updates_manually: bool,
    #[serde(default)]
    pub skin_library_path: String,
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    #[serde(default = "default_false")]
    pub safe_mode: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            language: "system".into(),
            check_updates_manually: true,
            skin_library_path: String::new(),
            show_tray_icon: true,
            safe_mode: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskbarConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_external_appearance")]
    pub engine: String,
    #[serde(default = "default_native_noop")]
    pub on_unsupported: String,
    #[serde(default = "default_yield_and_notify")]
    pub conflict_policy: String,
    #[serde(default = "default_true")]
    pub per_monitor_rules: bool,
    #[serde(default = "default_false")]
    pub allow_experimental_explorer_bridge: bool,
    #[serde(default)]
    pub states: TaskbarStateSettings,
    #[serde(default)]
    pub app_rules: Vec<TaskbarRule>,
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            engine: "external_appearance".into(),
            on_unsupported: "native_noop".into(),
            conflict_policy: "yield_and_notify".into(),
            per_monitor_rules: true,
            allow_experimental_explorer_bridge: false,
            states: TaskbarStateSettings::default(),
            app_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskbarStateSettings {
    #[serde(default)]
    pub desktop: TaskbarEffectState,
    #[serde(default)]
    pub maximized: TaskbarEffectState,
    #[serde(default)]
    pub flyout: TaskbarEffectState,
    #[serde(default)]
    pub battery_saver: TaskbarEffectState,
}

impl Default for TaskbarStateSettings {
    fn default() -> Self {
        Self {
            desktop: TaskbarEffectState {
                effect: TaskbarEffect::Clear,
                color: None,
                opacity: 0.0,
            },
            maximized: TaskbarEffectState {
                effect: TaskbarEffect::Tint,
                color: Some("#141925".into()),
                opacity: 0.85,
            },
            flyout: TaskbarEffectState {
                effect: TaskbarEffect::Solid,
                color: Some("#171D2A".into()),
                opacity: 1.0,
            },
            battery_saver: TaskbarEffectState {
                effect: TaskbarEffect::Solid,
                color: Some("#171D2A".into()),
                opacity: 1.0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskbarEffectState {
    #[serde(default)]
    pub effect: TaskbarEffect,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_one_float")]
    pub opacity: f32,
}

impl Default for TaskbarEffectState {
    fn default() -> Self {
        Self {
            effect: TaskbarEffect::NativeNoop,
            color: None,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopConfig {
    #[serde(default = "default_placement")]
    pub placement: String,
    #[serde(default = "default_false")]
    pub allow_always_on_top: bool,
    #[serde(default = "default_false")]
    pub allow_behind_wallpaper: bool,
    #[serde(default = "default_true")]
    pub locked: bool,
    #[serde(default = "default_temporary_only")]
    pub interaction_mode: String,
    #[serde(default = "default_interactive_timeout")]
    pub interactive_timeout_seconds: u32,
    #[serde(default = "default_snap_grid")]
    pub snap_grid_dip: f32,
    #[serde(default = "default_autohide_policy")]
    pub autohide_taskbar_policy: String,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            placement: "between_wallpaper_and_icons".into(),
            allow_always_on_top: false,
            allow_behind_wallpaper: false,
            locked: true,
            interaction_mode: "temporary_only".into(),
            interactive_timeout_seconds: 8,
            snap_grid_dip: 8.0,
            autohide_taskbar_policy: "reserve_expanded_safe_area".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    #[serde(default = "default_true")]
    pub visualizer_enabled: bool,
    #[serde(default = "default_playback_source")]
    pub capture_source: String,
    #[serde(default = "default_false")]
    pub store_audio: bool,
    #[serde(default = "default_fps")]
    pub render_fps: u32,
    #[serde(default = "default_bands")]
    pub spectrum_bands: u32,
    #[serde(default = "default_auto_hide_delay")]
    pub auto_hide_delay_seconds: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            visualizer_enabled: true,
            capture_source: "default_playback_endpoint".into(),
            store_audio: false,
            render_fps: 30,
            spectrum_bands: 64,
            auto_hide_delay_seconds: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConfig {
    #[serde(default = "default_sample_interval")]
    pub sample_interval_ms: u64,
    #[serde(default = "default_battery_sample_interval")]
    pub on_battery_sample_interval_ms: u64,
    #[serde(default = "default_true")]
    pub allow_optional_vendor_providers: bool,
    #[serde(default = "default_true")]
    pub avoid_waking_discrete_gpu: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            sample_interval_ms: 1000,
            on_battery_sample_interval_ms: 5000,
            allow_optional_vendor_providers: true,
            avoid_waking_discrete_gpu: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PowerConfig {
    #[serde(default = "default_power_profile")]
    pub profile: String,
    #[serde(default = "default_true")]
    pub pause_on_session_lock: bool,
    #[serde(default = "default_true")]
    pub pause_on_monitor_off: bool,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            pause_on_session_lock: true,
            pause_on_monitor_off: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GamingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_gaming_mode")]
    pub mode: String,
    #[serde(default = "default_false")]
    pub force_global_dormancy: bool,
}

impl Default for GamingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "per_monitor_conservative".into(),
            force_global_dormancy: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default = "default_true")]
    pub allow_standard_libraries: bool,
    #[serde(default = "default_worker_exe")]
    pub worker_executable: String,
    #[serde(default = "default_crash_limit")]
    pub restart_after_crash_limit: u32,
    #[serde(default = "default_true")]
    pub require_explicit_trust_for_imported_scripts: bool,
}

impl Default for ScriptingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            runtime: "lua54_separate_worker".into(),
            allow_standard_libraries: true,
            worker_executable: "aethershell-script-host.exe".into(),
            restart_after_crash_limit: 2,
            require_explicit_trust_for_imported_scripts: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivacyConfig {
    #[serde(default = "default_true")]
    pub offline_first: bool,
    #[serde(default = "default_false")]
    pub automatic_telemetry_upload: bool,
    #[serde(default = "default_false")]
    pub crash_upload: bool,
    #[serde(default = "default_false")]
    pub network_widgets_enabled_by_default: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            offline_first: true,
            automatic_telemetry_upload: false,
            crash_upload: false,
            network_widgets_enabled_by_default: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdatesConfig {
    #[serde(default = "default_channel")]
    pub channel: String,
    #[serde(default = "default_false")]
    pub auto_apply_without_consent: bool,
    #[serde(default = "default_true")]
    pub retain_rollback_release: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            channel: "stable".into(),
            auto_apply_without_consent: false,
            retain_rollback_release: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_system() -> String {
    "system".into()
}
fn default_external_appearance() -> String {
    "external_appearance".into()
}
fn default_native_noop() -> String {
    "native_noop".into()
}
fn default_yield_and_notify() -> String {
    "yield_and_notify".into()
}
fn default_placement() -> String {
    "between_wallpaper_and_icons".into()
}
fn default_temporary_only() -> String {
    "temporary_only".into()
}
fn default_interactive_timeout() -> u32 {
    8
}
fn default_snap_grid() -> f32 {
    8.0
}
fn default_autohide_policy() -> String {
    "reserve_expanded_safe_area".into()
}
fn default_playback_source() -> String {
    "default_playback_endpoint".into()
}
fn default_fps() -> u32 {
    30
}
fn default_bands() -> u32 {
    64
}
fn default_auto_hide_delay() -> u32 {
    10
}
fn default_sample_interval() -> u64 {
    1000
}
fn default_battery_sample_interval() -> u64 {
    5000
}
fn default_power_profile() -> String {
    "balanced".into()
}
fn default_gaming_mode() -> String {
    "per_monitor_conservative".into()
}
fn default_runtime() -> String {
    "lua54_separate_worker".into()
}
fn default_worker_exe() -> String {
    "aethershell-script-host.exe".into()
}
fn default_crash_limit() -> u32 {
    2
}
fn default_channel() -> String {
    "stable".into()
}
fn default_one_float() -> f32 {
    1.0
}
