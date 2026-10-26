//! Desktop host, stacking order enforcement, and interaction mode controller.

pub mod host;
pub mod interaction;
pub mod window;

pub use host::{DesktopHost, DesktopHostError};
pub use interaction::{DismissalReason, InteractionController};
pub use window::{DesktopHostWindow, DesktopWindowManager};
