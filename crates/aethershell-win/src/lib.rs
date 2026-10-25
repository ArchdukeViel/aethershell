//! Native Windows API wrappers, OS capability detection, and non-invasive probes.

pub mod conflict_detector;
pub mod desktop_probe;
pub mod monitor;
pub mod os_version;
pub mod shell_events;
pub mod single_instance;
pub mod taskbar_probe;
pub mod window_state;

pub use conflict_detector::{detect_taskbar_conflicts, ConflictInfo};
pub use desktop_probe::{probe_desktop_hierarchy, DesktopHierarchyProbe};
pub use monitor::{enumerate_monitors, MonitorInfo};
pub use os_version::{WindowsBuildFamily, WindowsVersionInfo};
pub use shell_events::{ShellEvent, ShellEventMonitor};
pub use single_instance::SessionSingletonGuard;
pub use taskbar_probe::{probe_taskbars, TaskbarWindowInfo};
pub use window_state::{
    get_window_class_name, is_cloaked, is_flyout_class, is_fullscreen_rect, is_maximized_rect,
    WindowStateEvaluator,
};
