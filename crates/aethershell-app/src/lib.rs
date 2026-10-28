//! AetherShell primary application engine and services.

pub mod engine;
pub mod tray;

pub use engine::{
    compile_first_run_scene, compile_live_scene, compile_live_scene_and_regions,
    compile_live_scene_with_scale, extract_interactive_regions, find_script_host_binary,
    normalize_audio_spectrum, should_render_frame, sync_script_worker_internal, AppEngine,
};
pub use tray::{taskbar_created_msg, SystemTray, TrayShutdownHandle};
