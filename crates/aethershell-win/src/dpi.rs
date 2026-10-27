//! Per-monitor DPI awareness and multi-monitor scaling primitives.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, GetWindowDpiAwarenessContext, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

pub const DEFAULT_DPI: u32 = 96;

/// Initializes Per-Monitor V2 DPI awareness for the current process.
/// Falls back to Per-Monitor V1 if V2 is unsupported.
pub fn set_per_monitor_v2_dpi_awareness() -> Result<bool, String> {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok() {
            return Ok(true);
        }
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE).is_ok() {
            return Ok(false);
        }
    }
    Err("Failed to set per-monitor DPI awareness context".into())
}

/// Retrieves the DPI awareness context for a specific window.
pub fn get_window_dpi_awareness_context(hwnd: HWND) -> DPI_AWARENESS_CONTEXT {
    unsafe { GetWindowDpiAwarenessContext(hwnd) }
}

/// Checks whether a window is running with PerMonitorV2 awareness.
pub fn is_window_per_monitor_v2_aware(hwnd: HWND) -> bool {
    let ctx = get_window_dpi_awareness_context(hwnd);
    ctx == DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
}

/// Retrieves the DPI for a specific window handle, defaulting to 96 if query fails.
pub fn get_window_dpi(hwnd: HWND) -> u32 {
    if hwnd.0.is_null() {
        return DEFAULT_DPI;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        DEFAULT_DPI
    } else {
        dpi
    }
}

/// Converts a DPI value (e.g. 96, 120, 144, 192) into a layout scaling factor (1.0, 1.25, 1.5, 2.0).
pub fn dpi_to_scale_factor(dpi: u32) -> f32 {
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / DEFAULT_DPI as f32
    }
}

/// Scales device-independent pixels (DIPs) to physical pixels at a given DPI.
pub fn scale_dip_to_pixels(dips: f32, dpi: u32) -> f32 {
    dips * dpi_to_scale_factor(dpi)
}

/// Scales physical pixels to device-independent pixels (DIPs) at a given DPI.
pub fn scale_pixels_to_dip(pixels: f32, dpi: u32) -> f32 {
    let scale = dpi_to_scale_factor(dpi);
    if scale == 0.0 {
        pixels
    } else {
        pixels / scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpi_scale_factor_calculations() {
        assert_eq!(dpi_to_scale_factor(96), 1.0);
        assert_eq!(dpi_to_scale_factor(120), 1.25);
        assert_eq!(dpi_to_scale_factor(144), 1.5);
        assert_eq!(dpi_to_scale_factor(192), 2.0);
        assert_eq!(dpi_to_scale_factor(288), 3.0);
    }

    #[test]
    fn test_scale_dip_and_pixels_roundtrip() {
        let dpi = 144; // 150% scaling
        let dip_val = 160.0f32;
        let px = scale_dip_to_pixels(dip_val, dpi);
        assert_eq!(px, 240.0f32);
        let back = scale_pixels_to_dip(px, dpi);
        assert_eq!(back, dip_val);
    }

    #[test]
    fn test_get_window_dpi_null_hwnd_defaults_to_96() {
        assert_eq!(get_window_dpi(HWND::default()), DEFAULT_DPI);
    }

    #[test]
    fn test_set_per_monitor_v2_dpi_awareness_safe_execution() {
        // Can be called safely; even if already initialized, executes without panic
        let _ = set_per_monitor_v2_dpi_awareness();
    }
}
