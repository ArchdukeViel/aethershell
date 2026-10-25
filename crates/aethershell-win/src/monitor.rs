use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};

const MONITORINFOF_PRIMARY: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub hmonitor: usize,
    pub is_primary: bool,
    pub bounds: [i32; 4],    // [left, top, right, bottom]
    pub work_area: [i32; 4], // [left, top, right, bottom]
}

/// Enumerates connected physical monitors and their work areas.
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors = Vec::new();

    unsafe extern "system" fn monitor_enum_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _lprect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<MonitorInfo>);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            let is_primary = (info.dwFlags & MONITORINFOF_PRIMARY) != 0;
            list.push(MonitorInfo {
                hmonitor: hmon.0 as usize,
                is_primary,
                bounds: [
                    info.rcMonitor.left,
                    info.rcMonitor.top,
                    info.rcMonitor.right,
                    info.rcMonitor.bottom,
                ],
                work_area: [
                    info.rcWork.left,
                    info.rcWork.top,
                    info.rcWork.right,
                    info.rcWork.bottom,
                ],
            });
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }

    monitors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enumerate_monitors_safe() {
        let monitors = enumerate_monitors();
        println!("Found {} monitors", monitors.len());
        if !monitors.is_empty() {
            let has_primary = monitors.iter().any(|m| m.is_primary);
            assert!(has_primary, "At least one monitor must be primary");
        }
    }
}
