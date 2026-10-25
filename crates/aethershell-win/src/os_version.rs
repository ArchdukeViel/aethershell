use serde::{Deserialize, Serialize};
use std::fmt;
use windows::core::w;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    REG_VALUE_TYPE,
};

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
    }

    #[test]
    fn test_current_detection_with_ubr_iva_003() {
        let detected = WindowsVersionInfo::detect();
        assert!(detected.build_number >= 19045);
        assert!(detected.is_supported_v1);
        println!("Detected OS: {} (UBR: {:?})", detected.family, detected.ubr);
    }
}
