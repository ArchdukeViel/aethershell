use aethershell_core::TaskbarEffect;
use serde::{Deserialize, Serialize};
use std::fmt;
use windows::core::w;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    REG_VALUE_TYPE,
};

/// Taskbar shell architecture across Windows versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskbarArchitecture {
    /// Classic Win32 taskbar (Shell_TrayWnd without XAML island content bridge).
    LegacyWin32,
    /// Modern Win11 taskbar hosting Windows.UI.Composition.DesktopWindowContentBridge XAML island.
    XamlIsland,
    /// Unknown or unclassified shell architecture.
    Unknown,
}

impl fmt::Display for TaskbarArchitecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LegacyWin32 => write!(f, "Legacy Win32 Taskbar"),
            Self::XamlIsland => write!(f, "XAML Island Taskbar"),
            Self::Unknown => write!(f, "Unknown Taskbar Architecture"),
        }
    }
}

/// Evaluation verdict for requesting an appearance effect on an OS tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectSupportVerdict {
    /// Effect is supported and verified.
    Supported,
    /// Effect cannot be applied safely; honest fallback to NativeNoop with reason.
    UnsupportedFallback {
        reason: String,
        target_effect: TaskbarEffect,
    },
}

/// Authoritative capabilities of a Windows OS matrix tier per PRD Section 2.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsTierCapabilities {
    pub tier_id: &'static str,
    pub family: WindowsBuildFamily,
    pub build_number: u32,
    pub display_name: &'static str,
    pub policy_notes: &'static str,
    pub taskbar_architecture: TaskbarArchitecture,
    pub is_v1_supported: bool,
    pub is_special_validation_tier: bool,
    pub supports_per_monitor_v2: bool,
    pub supports_direct2d_1_1: bool,
    pub supports_job_object_quotas: bool,
    pub supports_named_pipe_security: bool,
    pub supports_shell_hook_stacking: bool,
}

/// Detected Windows Build Family matching PRD Section 2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowsBuildFamily {
    Win10_22H2, // 19045
    Win11_21H2, // 22000
    Win11_22H2, // 22621
    Win11_23H2, // 22631
    Win11_24H2, // 26100
    Win11_25H2, // 26200
    Win11_26H2, // 26300 (October 2026 GA target)
    Win11_26H1, // 28000 (Special validation tier, PRD Section 2.1)
    Unknown(u32),
}

impl WindowsBuildFamily {
    pub fn from_build(build: u32) -> Self {
        match build {
            19045 => Self::Win10_22H2,
            22000 => Self::Win11_21H2,
            22621 => Self::Win11_22H2,
            22631 => Self::Win11_23H2,
            26100 => Self::Win11_24H2,
            26200 => Self::Win11_25H2,
            26300 => Self::Win11_26H2,
            28000 => Self::Win11_26H1,
            other => Self::Unknown(other),
        }
    }

    pub fn build_number_or_default(&self) -> u32 {
        match self {
            Self::Win10_22H2 => 19045,
            Self::Win11_21H2 => 22000,
            Self::Win11_22H2 => 22621,
            Self::Win11_23H2 => 22631,
            Self::Win11_24H2 => 26100,
            Self::Win11_25H2 => 26200,
            Self::Win11_26H2 => 26300,
            Self::Win11_26H1 => 28000,
            Self::Unknown(b) => *b,
        }
    }

    pub fn tier_id(&self) -> &'static str {
        match self {
            Self::Win10_22H2 => "VM-WIN10-22H2",
            Self::Win11_21H2 => "VM-WIN11-21H2",
            Self::Win11_22H2 => "VM-WIN11-22H2",
            Self::Win11_23H2 => "VM-WIN11-23H2",
            Self::Win11_24H2 => "VM-WIN11-24H2",
            Self::Win11_25H2 => "VM-WIN11-25H2",
            Self::Win11_26H2 => "VM-WIN11-26H2",
            Self::Win11_26H1 => "VM-WIN11-26H1",
            Self::Unknown(_) => "UNKNOWN",
        }
    }

    pub fn is_windows_11(&self) -> bool {
        match self {
            Self::Win10_22H2 => false,
            Self::Unknown(b) if *b < 22000 => false,
            _ => true,
        }
    }

    /// Checks if this build family is in the authoritative v1.0.0 target matrix (PRD Section 2.1).
    pub fn is_v1_supported(&self) -> bool {
        match self {
            Self::Win10_22H2
            | Self::Win11_22H2
            | Self::Win11_23H2
            | Self::Win11_24H2
            | Self::Win11_25H2
            | Self::Win11_26H2 => true,
            // 28000 is special validation tier; 22000 is below initial Win11 baseline
            Self::Win11_26H1 | Self::Win11_21H2 | Self::Unknown(_) => false,
        }
    }

    pub fn is_special_validation_tier(&self) -> bool {
        matches!(self, Self::Win11_26H1)
    }

    pub fn taskbar_architecture(&self) -> TaskbarArchitecture {
        match self {
            Self::Win10_22H2 => TaskbarArchitecture::LegacyWin32,
            Self::Win11_21H2
            | Self::Win11_22H2
            | Self::Win11_23H2
            | Self::Win11_24H2
            | Self::Win11_25H2
            | Self::Win11_26H2
            | Self::Win11_26H1 => TaskbarArchitecture::XamlIsland,
            Self::Unknown(b) if *b >= 22000 => TaskbarArchitecture::XamlIsland,
            Self::Unknown(b) if *b >= 10240 => TaskbarArchitecture::LegacyWin32,
            Self::Unknown(_) => TaskbarArchitecture::Unknown,
        }
    }

    /// Returns the 7 authoritative PRD Section 2.1 OS matrix tiers.
    pub fn all_authoritative_tiers() -> &'static [WindowsBuildFamily] {
        &[
            Self::Win10_22H2,
            Self::Win11_22H2,
            Self::Win11_23H2,
            Self::Win11_24H2,
            Self::Win11_25H2,
            Self::Win11_26H2,
            Self::Win11_26H1,
        ]
    }

    pub fn tier_capabilities(&self) -> OsTierCapabilities {
        match self {
            Self::Win10_22H2 => OsTierCapabilities {
                tier_id: "VM-WIN10-22H2",
                family: *self,
                build_number: 19045,
                display_name: "Windows 10 22H2 x64 (Build 19045)",
                policy_notes: "v1 minimum; required compatibility tests; legacy taskbar fallback",
                taskbar_architecture: TaskbarArchitecture::LegacyWin32,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_21H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-21H2",
                family: *self,
                build_number: 22000,
                display_name: "Windows 11 21H2 x64 (Build 22000)",
                policy_notes: "below initial Windows 11 baseline; evaluate separately",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: false,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_22H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-22H2",
                family: *self,
                build_number: 22621,
                display_name: "Windows 11 22H2 x64 (Build 22621)",
                policy_notes: "required legacy Win11 compatibility tier; XAML taskbar detection",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_23H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-23H2",
                family: *self,
                build_number: 22631,
                display_name: "Windows 11 23H2 x64 (Build 22631)",
                policy_notes:
                    "required compatibility tier; Named Pipe DACL & session token lifecycle",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_24H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-24H2",
                family: *self,
                build_number: 26100,
                display_name: "Windows 11 24H2 x64 (Build 26100)",
                policy_notes: "required compatibility tier; Job Object containment & UTF-8 storage",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_25H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-25H2",
                family: *self,
                build_number: 26200,
                display_name: "Windows 11 25H2 x64 (Build 26200)",
                policy_notes: "primary test tier; mid-gen shell changes & visual tree stability",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_26H2 => OsTierCapabilities {
                tier_id: "VM-WIN11-26H2",
                family: *self,
                build_number: 26300,
                display_name: "Windows 11 26H2 x64 (Build 26300 - October 2026 GA Baseline)",
                policy_notes: "primary compatibility tier baseline (October 2026 GA)",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: true,
                is_special_validation_tier: false,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Win11_26H1 => OsTierCapabilities {
                tier_id: "VM-WIN11-26H1",
                family: *self,
                build_number: 28000,
                display_name: "Windows 11 26H1 x64 (Build 28000 - Special Validation Tier)",
                policy_notes: "special validation tier (distinct hardware-focused branch)",
                taskbar_architecture: TaskbarArchitecture::XamlIsland,
                is_v1_supported: false,
                is_special_validation_tier: true,
                supports_per_monitor_v2: true,
                supports_direct2d_1_1: true,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
            Self::Unknown(b) => OsTierCapabilities {
                tier_id: "UNKNOWN",
                family: *self,
                build_number: *b,
                display_name: "Unknown Windows Build",
                policy_notes: "outside authoritative PRD Section 2.1 matrix",
                taskbar_architecture: self.taskbar_architecture(),
                is_v1_supported: false,
                is_special_validation_tier: false,
                supports_per_monitor_v2: *b >= 1703,
                supports_direct2d_1_1: *b >= 9600,
                supports_job_object_quotas: true,
                supports_named_pipe_security: true,
                supports_shell_hook_stacking: true,
            },
        }
    }

    /// Evaluates honest effect support for this build family.
    pub fn evaluate_taskbar_effect(
        &self,
        effect: TaskbarEffect,
        is_capability_verified: bool,
    ) -> EffectSupportVerdict {
        if effect == TaskbarEffect::NativeNoop {
            return EffectSupportVerdict::Supported;
        }

        match self {
            Self::Win10_22H2 => match effect {
                TaskbarEffect::AcrylicLike => EffectSupportVerdict::UnsupportedFallback {
                    reason: "Acrylic / Mica materials are not supported on Windows 10 (Build 19045); honestly routed to native_noop per PRD Section 2.2 [S05]".into(),
                    target_effect: TaskbarEffect::NativeNoop,
                },
                TaskbarEffect::BlurLike => EffectSupportVerdict::UnsupportedFallback {
                    reason: "Blur composition on taskbar is not supported via external APIs on Windows 10 (Build 19045); honestly routed to native_noop per PRD Section 2.2 / TB-03".into(),
                    target_effect: TaskbarEffect::NativeNoop,
                },
                TaskbarEffect::Clear | TaskbarEffect::Tint | TaskbarEffect::Solid => {
                    if is_capability_verified {
                        EffectSupportVerdict::Supported
                    } else {
                        EffectSupportVerdict::UnsupportedFallback {
                            reason: "External composition unverified on Windows 10 (Build 19045); safely routed to native_noop per PRD Section 4.2".into(),
                            target_effect: TaskbarEffect::NativeNoop,
                        }
                    }
                }
                TaskbarEffect::NativeNoop => EffectSupportVerdict::Supported,
            },
            Self::Win11_22H2
            | Self::Win11_23H2
            | Self::Win11_24H2
            | Self::Win11_25H2
            | Self::Win11_26H2
            | Self::Win11_21H2 => {
                if is_capability_verified {
                    EffectSupportVerdict::Supported
                } else {
                    EffectSupportVerdict::UnsupportedFallback {
                        reason: format!(
                            "External composition unverified on modern Windows 11 XAML taskbar (Build {}); safely routed to native_noop per PRD Section 4.2",
                            self.build_number_or_default()
                        ),
                        target_effect: TaskbarEffect::NativeNoop,
                    }
                }
            }
            Self::Win11_26H1 => EffectSupportVerdict::UnsupportedFallback {
                reason: "Windows 11 26H1 (Build 28000) is a special validation tier outside v1.0.0 production baseline; safely routed to native_noop per PRD Section 2.1".into(),
                target_effect: TaskbarEffect::NativeNoop,
            },
            Self::Unknown(b) => EffectSupportVerdict::UnsupportedFallback {
                reason: format!(
                    "Windows build {} is outside authoritative PRD Section 2.1 matrix; safely routed to native_noop",
                    b
                ),
                target_effect: TaskbarEffect::NativeNoop,
            },
        }
    }
}

impl fmt::Display for WindowsBuildFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win10_22H2 => write!(f, "Windows 10 22H2 (Build 19045)"),
            Self::Win11_21H2 => write!(f, "Windows 11 21H2 (Build 22000)"),
            Self::Win11_22H2 => write!(f, "Windows 11 22H2 (Build 22621)"),
            Self::Win11_23H2 => write!(f, "Windows 11 23H2 (Build 22631)"),
            Self::Win11_24H2 => write!(f, "Windows 11 24H2 (Build 26100)"),
            Self::Win11_25H2 => write!(f, "Windows 11 25H2 (Build 26200)"),
            Self::Win11_26H2 => write!(f, "Windows 11 26H2 (Build 26300)"),
            Self::Win11_26H1 => write!(f, "Windows 11 26H1 (Build 28000 - Special Validation)"),
            Self::Unknown(b) => write!(f, "Windows Build {}", b),
        }
    }
}

/// Detailed Windows Version snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsVersionInfo {
    pub major: u32,
    pub minor: u32,
    pub build_number: u32,
    pub ubr: Option<u32>,
    pub family: WindowsBuildFamily,
    pub is_supported_v1: bool,
}

impl WindowsVersionInfo {
    /// Detects Windows version from Windows API / ntdll, with registry UBR lookup.
    ///
    /// Resolves Finding IVA-003: Honest fallback to 0 on detection failure and explicit UBR query.
    pub fn detect() -> Self {
        #[repr(C)]
        #[allow(non_snake_case, clippy::upper_case_acronyms)]
        struct OSVERSIONINFOEXW {
            dwOSVersionInfoSize: u32,
            dwMajorVersion: u32,
            dwMinorVersion: u32,
            dwBuildNumber: u32,
            dwPlatformId: u32,
            szCSDVersion: [u16; 128],
            wServicePackMajor: u16,
            wServicePackMinor: u16,
            wSuiteMask: u16,
            wProductType: u8,
            wReserved: u8,
        }

        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32;

        let mut major = 0;
        let mut minor = 0;
        let mut build_number = 0; // Honest unknown fallback (IVA-003)

        unsafe {
            let ntdll = windows::Win32::System::LibraryLoader::GetModuleHandleA(windows::core::s!(
                "ntdll.dll"
            ));
            if let Ok(h_ntdll) = ntdll {
                let proc = windows::Win32::System::LibraryLoader::GetProcAddress(
                    h_ntdll,
                    windows::core::s!("RtlGetVersion"),
                );
                if let Some(p) = proc {
                    let rtl_get_version: RtlGetVersionFn = std::mem::transmute(p);
                    let mut info = std::mem::zeroed::<OSVERSIONINFOEXW>();
                    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOEXW>() as u32;
                    if rtl_get_version(&mut info) == 0 {
                        major = info.dwMajorVersion;
                        minor = info.dwMinorVersion;
                        build_number = info.dwBuildNumber;
                    }
                }
            }
        }

        let ubr = Self::query_ubr();
        let family = WindowsBuildFamily::from_build(build_number);
        let is_supported_v1 = family.is_v1_supported();

        Self {
            major,
            minor,
            build_number,
            ubr,
            family,
            is_supported_v1,
        }
    }

    /// Queries the Update Build Revision (UBR) from HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion.
    fn query_ubr() -> Option<u32> {
        unsafe {
            let mut hkey = HKEY::default();
            let subkey = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, 0, KEY_READ, &mut hkey).is_ok() {
                let mut data: u32 = 0;
                let mut data_len = std::mem::size_of::<u32>() as u32;
                let mut val_type = REG_VALUE_TYPE::default();

                let res = RegQueryValueExW(
                    hkey,
                    w!("UBR"),
                    None,
                    Some(&mut val_type),
                    Some(&mut data as *mut u32 as *mut u8),
                    Some(&mut data_len),
                );
                let _ = RegCloseKey(hkey);

                if res.is_ok() && val_type == REG_DWORD {
                    return Some(data);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_family_classification() {
        assert_eq!(
            WindowsBuildFamily::from_build(19045),
            WindowsBuildFamily::Win10_22H2
        );
        assert!(!WindowsBuildFamily::from_build(19045).is_windows_11());
        assert!(WindowsBuildFamily::from_build(19045).is_v1_supported());

        assert_eq!(
            WindowsBuildFamily::from_build(22621),
            WindowsBuildFamily::Win11_22H2
        );
        assert!(WindowsBuildFamily::from_build(22621).is_windows_11());
        assert!(WindowsBuildFamily::from_build(22621).is_v1_supported());

        assert_eq!(
            WindowsBuildFamily::from_build(26300),
            WindowsBuildFamily::Win11_26H2
        );
        assert!(WindowsBuildFamily::from_build(26300).is_windows_11());
        assert!(WindowsBuildFamily::from_build(26300).is_v1_supported());

        // Build 28000 (26H1) must NOT be marked as an unconditional supported v1 build (IVA-003)
        assert_eq!(
            WindowsBuildFamily::from_build(28000),
            WindowsBuildFamily::Win11_26H1
        );
        assert!(!WindowsBuildFamily::from_build(28000).is_v1_supported());
        assert!(WindowsBuildFamily::from_build(28000).is_special_validation_tier());
    }

    #[test]
    fn test_current_detection_with_ubr_iva_003() {
        let detected = WindowsVersionInfo::detect();
        assert!(detected.build_number >= 19045);
        assert!(detected.is_supported_v1);
        println!("Detected OS: {} (UBR: {:?})", detected.family, detected.ubr);
    }

    #[test]
    fn test_all_seven_authoritative_tiers_capabilities() {
        let tiers = WindowsBuildFamily::all_authoritative_tiers();
        assert_eq!(
            tiers.len(),
            7,
            "Exactly 7 authoritative tiers per PRD Section 2.1"
        );

        let expected_tier_ids = [
            "VM-WIN10-22H2",
            "VM-WIN11-22H2",
            "VM-WIN11-23H2",
            "VM-WIN11-24H2",
            "VM-WIN11-25H2",
            "VM-WIN11-26H2",
            "VM-WIN11-26H1",
        ];

        for (i, family) in tiers.iter().enumerate() {
            assert_eq!(family.tier_id(), expected_tier_ids[i]);
            let caps = family.tier_capabilities();
            assert_eq!(caps.tier_id, expected_tier_ids[i]);
            assert!(caps.supports_per_monitor_v2);
            assert!(caps.supports_direct2d_1_1);
            assert!(caps.supports_job_object_quotas);
            assert!(caps.supports_named_pipe_security);
            assert!(caps.supports_shell_hook_stacking);
        }

        // Win10 is LegacyWin32
        assert_eq!(
            WindowsBuildFamily::Win10_22H2.taskbar_architecture(),
            TaskbarArchitecture::LegacyWin32
        );
        // Win11 tiers are XamlIsland
        assert_eq!(
            WindowsBuildFamily::Win11_22H2.taskbar_architecture(),
            TaskbarArchitecture::XamlIsland
        );
        assert_eq!(
            WindowsBuildFamily::Win11_26H2.taskbar_architecture(),
            TaskbarArchitecture::XamlIsland
        );
        assert_eq!(
            WindowsBuildFamily::Win11_26H1.taskbar_architecture(),
            TaskbarArchitecture::XamlIsland
        );

        // 6 tiers v1 supported, 1 special validation
        let v1_supported_count = tiers.iter().filter(|f| f.is_v1_supported()).count();
        let special_count = tiers
            .iter()
            .filter(|f| f.is_special_validation_tier())
            .count();
        assert_eq!(v1_supported_count, 6);
        assert_eq!(special_count, 1);
    }

    #[test]
    fn test_honest_effect_fallback_evaluation() {
        // 1. NativeNoop is universally supported
        for family in WindowsBuildFamily::all_authoritative_tiers() {
            assert_eq!(
                family.evaluate_taskbar_effect(TaskbarEffect::NativeNoop, false),
                EffectSupportVerdict::Supported
            );
            assert_eq!(
                family.evaluate_taskbar_effect(TaskbarEffect::NativeNoop, true),
                EffectSupportVerdict::Supported
            );
        }

        // 2. Win10 Acrylic / Mica honest fallback
        let win10 = WindowsBuildFamily::Win10_22H2;
        let acrylic_res = win10.evaluate_taskbar_effect(TaskbarEffect::AcrylicLike, true);
        assert!(matches!(
            acrylic_res,
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        if let EffectSupportVerdict::UnsupportedFallback { reason, .. } = acrylic_res {
            assert!(reason.contains("Acrylic / Mica materials are not supported on Windows 10"));
        }

        // 3. Win10 Blur honest fallback
        let blur_res = win10.evaluate_taskbar_effect(TaskbarEffect::BlurLike, true);
        assert!(matches!(
            blur_res,
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        if let EffectSupportVerdict::UnsupportedFallback { reason, .. } = blur_res {
            assert!(
                reason.contains("Blur composition on taskbar is not supported via external APIs")
            );
        }

        // 4. Win10 Clear: unverified falls back, verified passes
        assert!(matches!(
            win10.evaluate_taskbar_effect(TaskbarEffect::Clear, false),
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        assert_eq!(
            win10.evaluate_taskbar_effect(TaskbarEffect::Clear, true),
            EffectSupportVerdict::Supported
        );

        // 5. Win11 XAML taskbar unverified falls back honestly
        let win11 = WindowsBuildFamily::Win11_26H2;
        let unverified_res = win11.evaluate_taskbar_effect(TaskbarEffect::Clear, false);
        assert!(matches!(
            unverified_res,
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        if let EffectSupportVerdict::UnsupportedFallback { reason, .. } = unverified_res {
            assert!(reason
                .contains("External composition unverified on modern Windows 11 XAML taskbar"));
        }
        assert_eq!(
            win11.evaluate_taskbar_effect(TaskbarEffect::Clear, true),
            EffectSupportVerdict::Supported
        );

        // 6. Win11 26H1 special validation tier honestly falls back even if verified
        let win11_special = WindowsBuildFamily::Win11_26H1;
        let special_res = win11_special.evaluate_taskbar_effect(TaskbarEffect::Clear, true);
        assert!(matches!(
            special_res,
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        if let EffectSupportVerdict::UnsupportedFallback { reason, .. } = special_res {
            assert!(reason.contains("special validation tier outside v1.0.0 production baseline"));
        }

        // 7. Unknown build falls back honestly
        let unknown = WindowsBuildFamily::Unknown(99999);
        let unknown_res = unknown.evaluate_taskbar_effect(TaskbarEffect::Clear, true);
        assert!(matches!(
            unknown_res,
            EffectSupportVerdict::UnsupportedFallback { .. }
        ));
        if let EffectSupportVerdict::UnsupportedFallback { reason, .. } = unknown_res {
            assert!(reason.contains("outside authoritative PRD Section 2.1 matrix"));
        }
    }
}
