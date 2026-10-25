//! Desktop host, stacking order enforcement, and interaction mode controller.

pub mod host;
pub mod interaction;

pub use host::{DesktopHost, DesktopHostError};
pub use interaction::{DismissalReason, InteractionController};
