//! AetherShell primary application engine and services.

pub mod engine;
pub mod tray;

pub use engine::{
    compile_first_run_scene, compile_live_scene, compile_live_scene_and_regions,
    extract_interactive_regions, AppEngine,
};
pub use tray::{taskbar_created_msg, SystemTray};
