//! Native Windows API wrappers, OS capability detection, and non-invasive probes.

pub mod desktop_probe;
pub mod monitor;
pub mod os_version;
pub mod single_instance;
pub mod taskbar_probe;

pub use desktop_probe::{probe_desktop_hierarchy, DesktopHierarchyProbe};
pub use monitor::{enumerate_monitors, MonitorInfo};
pub use os_version::{WindowsBuildFamily, WindowsVersionInfo};
pub use single_instance::SessionSingletonGuard;
pub use taskbar_probe::{probe_taskbars, TaskbarWindowInfo};
