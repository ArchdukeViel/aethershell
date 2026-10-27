//! Native Windows API wrappers, OS capability detection, and non-invasive probes.

pub mod conflict_detector;
pub mod desktop_probe;
pub mod dpi;
pub mod monitor;
pub mod os_version;
pub mod shell_events;
pub mod single_instance;
pub mod taskbar_probe;
pub mod window_state;

pub use conflict_detector::{detect_taskbar_conflicts, ConflictInfo};
pub use desktop_probe::{
    probe_desktop_hierarchy, register_taskbar_created_message, DesktopHierarchyProbe,
};
pub use dpi::{
    dpi_to_scale_factor, get_window_dpi, get_window_dpi_awareness_context,
    is_window_per_monitor_v2_aware, scale_dip_to_pixels, scale_pixels_to_dip,
    set_per_monitor_v2_dpi_awareness, DEFAULT_DPI,
};
pub use monitor::{enumerate_monitors, MonitorInfo};
pub use os_version::{WindowsBuildFamily, WindowsVersionInfo};
pub use shell_events::{ShellEvent, ShellEventMonitor};
pub use single_instance::SessionSingletonGuard;
pub use taskbar_probe::{probe_taskbars, TaskbarWindowInfo};
pub use window_state::{
    get_window_class_name, is_cloaked, is_flyout_class, is_fullscreen_rect, is_maximized_rect,
    WindowStateEvaluator,
};
