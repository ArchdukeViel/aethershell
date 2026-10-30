//! Desktop host, stacking order enforcement, and interaction mode controller.

pub mod accessibility;
pub mod host;
pub mod interaction;
pub mod window;

pub use accessibility::{
    create_root_provider, role_to_control_type_id, DesktopAccessibilityState,
    DesktopHostRootProvider, DesktopWidgetFragmentProvider, EVENT_OBJECT_FOCUS, OBJID_CLIENT,
};
pub use host::{DesktopHost, DesktopHostError, DesktopLayeringVerdict, DesktopStackingTier};
pub use interaction::{
    DismissalReason, InteractionController, InteractiveRegion, KeyboardNavigationResult,
    PointerHitResult,
};
pub use window::{
    safe_stacking_target, shellhook_msg, taskbar_created_msg, DesktopHostWindow,
    DesktopWindowManager, HOTKEY_ID_INTERACTIVE_MODE, HSHELL_ACTIVATESHELLWINDOW, HSHELL_REDRAW,
    HSHELL_RUDEAPPACTIVATED, HSHELL_WINDOWACTIVATED,
};
