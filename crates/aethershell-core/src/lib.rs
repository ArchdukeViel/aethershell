//! Core data structures, state models, and contracts for AetherShell.
//!
//! This crate contains pure domain logic and is free of platform-specific Win32 or GUI bindings.

pub mod capabilities;
pub mod desktop;
pub mod lifecycle;
pub mod skin;
pub mod status;
pub mod taskbar;
pub mod theme;
pub mod widget;

pub use capabilities::{FeatureCapability, RiskTier};
pub use desktop::{DesktopLayer, DipRect, InteractionMode, LayoutAnchor};
pub use lifecycle::{LifecycleState, RecoveryPrecedence};
pub use skin::{
    BindingMetricConfig, BindingsDefinition, LayoutDefinition, LayoutNode, NodeType,
    ProfileDefinition, SkinGeometry, SkinInteractions, SkinManifest, SkinRendering,
    WidgetInstanceConfig,
};
pub use status::{TestDisposition, VerificationStatus};
pub use taskbar::{
    resolve_taskbar_rule, MonitorTaskbarState, TaskbarEffect, TaskbarResolution, TaskbarRule,
};
pub use theme::{
    AccentColors, HighContrastSettings, ReducedMotionSettings, StatusColors, SurfaceColors,
    TextColors, ThemeColors, ThemeDefinition, ThemeEffects, ThemeMotion, ThemeShape,
    ThemeTypography,
};
pub use widget::{BindingQuality, BindingState, BindingValue, WidgetLifecycleState, WidgetType};
