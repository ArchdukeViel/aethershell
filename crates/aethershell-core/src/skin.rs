use crate::desktop::LayoutAnchor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Geometry parameters for a widget skin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkinGeometry {
    pub width_dip: f32,
    pub height_dip: f32,
    pub min_width_dip: Option<f32>,
    pub max_width_dip: Option<f32>,
    pub min_scale: f32,
    pub max_scale: f32,
}

/// Rendering options for a widget skin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkinRendering {
    pub preferred_mode: String,
    pub max_fps: u32,
    pub needs_alpha: bool,
}

/// Interactive permissions for a widget skin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkinInteractions {
    pub allowed: bool,
}

/// Declarative skin package manifest conforming to PRD Section 10.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkinManifest {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub license: String,
    pub engine_min: String,
    pub supported_widget_types: Vec<String>,
    pub layout_file: String,
    pub bindings_file: String,
    pub preview_image: Option<String>,
    pub script: Option<String>,
    pub geometry: SkinGeometry,
    pub rendering: SkinRendering,
    pub interactions: SkinInteractions,
}

/// Supported declarative node types (PRD Section 10.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Container,
    Text,
    Progress,
    SpectrumVisualizer,
    Button,
    Image,
}

/// A single visual element in a declarative layout scene graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub parent: Option<String>,
    pub x: f32,
    pub y: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub background: Option<String>,
    pub radius: Option<String>,
    pub value: Option<String>,
    pub color: Option<String>,
    pub font_size_dip: Option<f32>,
    pub value_source: Option<String>,
    pub min_value: Option<f32>,
    pub max_value: Option<f32>,
    pub fill: Option<String>,
}

/// Declarative layout definition (layout.toml).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutDefinition {
    pub schema_version: String,
    #[serde(default = "default_node_limit")]
    pub node_limit: u32,
    pub nodes: Vec<LayoutNode>,
}

fn default_node_limit() -> u32 {
    32
}

/// Metric configuration inside bindings.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingMetricConfig {
    pub source: String,
    pub unit: Option<String>,
    pub fallback_text: String,
    #[serde(default = "default_max_age_ms")]
    pub max_age_ms: u32,
}

fn default_max_age_ms() -> u32 {
    5000
}

/// Declarative data bindings definition (bindings.toml).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingsDefinition {
    pub schema_version: String,
    pub metrics: HashMap<String, BindingMetricConfig>,
}

/// User placement and runtime binding for a widget instance inside a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetInstanceConfig {
    pub instance_id: String,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub skin: String,
    pub monitor: String,
    pub anchor: LayoutAnchor,
    pub offset_x_dip: f32,
    pub offset_y_dip: f32,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub auto_hide_when_inactive: bool,
}

fn default_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

/// User profile specifying active widgets, themes, and monitor placements (PRD Section 10.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileDefinition {
    pub schema_version: String,
    pub id: String,
    pub display_name: String,
    pub theme: String,
    pub widgets: Vec<WidgetInstanceConfig>,
}

impl ProfileDefinition {
    /// Authoritative default first-run profile per PRD Section 9.5 & 10.3.
    pub fn signature_minimal() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "signature_minimal".into(),
            display_name: "Signature Minimal".into(),
            theme: "obsidian".into(),
            widgets: vec![
                WidgetInstanceConfig {
                    instance_id: "clock_main".into(),
                    widget_type: "clock.date".into(),
                    skin: "minimal_type".into(),
                    monitor: "primary".into(),
                    anchor: LayoutAnchor::TopCenter,
                    offset_x_dip: 0.0,
                    offset_y_dip: 56.0,
                    scale: 1.0,
                    visible: true,
                    auto_hide_when_inactive: false,
                },
                WidgetInstanceConfig {
                    instance_id: "metrics_main".into(),
                    widget_type: "system.metrics".into(),
                    skin: "glass_bento".into(),
                    monitor: "primary".into(),
                    anchor: LayoutAnchor::BottomRight,
                    offset_x_dip: -24.0,
                    offset_y_dip: -68.0,
                    scale: 1.0,
                    visible: true,
                    auto_hide_when_inactive: false,
                },
                WidgetInstanceConfig {
                    instance_id: "player_main".into(),
                    widget_type: "media.session".into(),
                    skin: "spectrum_dynamic".into(),
                    monitor: "primary".into(),
                    anchor: LayoutAnchor::BottomLeft,
                    offset_x_dip: 24.0,
                    offset_y_dip: -168.0,
                    scale: 1.0,
                    visible: true,
                    auto_hide_when_inactive: true,
                },
                WidgetInstanceConfig {
                    instance_id: "spectrum_main".into(),
                    widget_type: "audio.spectrum".into(),
                    skin: "spectrum_dynamic".into(),
                    monitor: "primary".into(),
                    anchor: LayoutAnchor::BottomLeft,
                    offset_x_dip: 24.0,
                    offset_y_dip: -64.0,
                    scale: 1.0,
                    visible: true,
                    auto_hide_when_inactive: true,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_minimal_first_run_profile() {
        let profile = ProfileDefinition::signature_minimal();
        assert_eq!(profile.schema_version, "1.0.0");
        assert_eq!(profile.id, "signature_minimal");
        assert_eq!(profile.theme, "obsidian");
        assert_eq!(profile.widgets.len(), 4);

        let clock = &profile.widgets[0];
        assert_eq!(clock.instance_id, "clock_main");
        assert_eq!(clock.widget_type, "clock.date");
        assert_eq!(clock.anchor, LayoutAnchor::TopCenter);

        let metrics = &profile.widgets[1];
        assert_eq!(metrics.instance_id, "metrics_main");
        assert_eq!(metrics.skin, "glass_bento");
        assert_eq!(metrics.anchor, LayoutAnchor::BottomRight);
    }
}
