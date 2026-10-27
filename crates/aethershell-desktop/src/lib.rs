//! Desktop host, stacking order enforcement, and interaction mode controller.

pub mod host;
pub mod interaction;
pub mod window;

pub use host::{DesktopHost, DesktopHostError};
pub use interaction::{
    DismissalReason, InteractionController, InteractiveRegion, PointerHitResult,
};
pub use window::{DesktopHostWindow, DesktopWindowManager, HOTKEY_ID_INTERACTIVE_MODE};
