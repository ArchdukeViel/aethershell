use crate::formatter::BindingFormatter;
use crate::theme_resolver::{ColorRgba, ThemeResolver};
use aethershell_core::{BindingValue, LayoutDefinition, NodeType, ThemeDefinition};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum LayoutError {
    #[error("Node limit exceeded: {0} nodes exceeds limit of {1}")]
    NodeLimitExceeded(usize, u32),
    #[error("Duplicate node ID detected: '{0}'")]
    DuplicateNodeId(String),
    #[error("Parent node '{0}' referenced by '{1}' does not exist")]
    MissingParent(String, String),
    #[error("Cyclic parent relationship detected at node '{0}'")]
    CyclicParent(String),
    #[error("Theme resolution error: {0}")]
    ThemeError(String),
}

/// A fully resolved visual node ready for rasterization or rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedNode {
    pub id: String,
    pub node_type: NodeType,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub background: Option<ColorRgba>,
    pub border_color: Option<ColorRgba>,
    pub corner_radius: f32,
    pub text: Option<String>,
    pub text_color: Option<ColorRgba>,
    pub font_size_dip: f32,
    pub progress: Option<f32>,
    pub fill_color: Option<ColorRgba>,
    /// Live audio spectrum bands for SpectrumVisualizer nodes.
    pub bands: Option<Vec<f32>>,
}

/// Layout engine compiling declarative LayoutDefinitions into resolved spatial trees.
pub struct LayoutEngine;

impl LayoutEngine {
    pub fn resolve(
        layout: &LayoutDefinition,
        theme: &ThemeDefinition,
        bindings: &HashMap<String, BindingValue>,
        base_x: f32,
        base_y: f32,
        scale: f32,
    ) -> Result<Vec<ResolvedNode>, LayoutError> {
        if layout.nodes.len() > layout.node_limit as usize {
            return Err(LayoutError::NodeLimitExceeded(
                layout.nodes.len(),
                layout.node_limit,
            ));
        }

        // Validate uniqueness of IDs
        let mut seen_ids = std::collections::HashSet::new();
        for node in &layout.nodes {
            if !seen_ids.insert(&node.id) {
                return Err(LayoutError::DuplicateNodeId(node.id.clone()));
            }
        }

        // Compute resolved node positions considering parent offsets
        let mut resolved_positions: HashMap<String, (f32, f32)> = HashMap::new();
        let mut resolved_nodes = Vec::with_capacity(layout.nodes.len());

        for node in &layout.nodes {
            let (parent_x, parent_y) = match &node.parent {
                Some(parent_id) => {
                    let pos = resolved_positions.get(parent_id).ok_or_else(|| {
                        LayoutError::MissingParent(parent_id.clone(), node.id.clone())
                    })?;
                    *pos
                }
                None => (base_x, base_y),
            };

            let abs_x = parent_x + (node.x * scale);
            let abs_y = parent_y + (node.y * scale);
            resolved_positions.insert(node.id.clone(), (abs_x, abs_y));

            let width = node.width.unwrap_or(0.0) * scale;
            let height = node.height.unwrap_or(0.0) * scale;

            // Resolve styling
            let background = match &node.background {
                Some(bg) => Some(
                    ThemeResolver::resolve_color(bg, theme, None)
                        .map_err(LayoutError::ThemeError)?,
                ),
                None => None,
            };

            let corner_radius = match &node.radius {
                Some(r) => {
                    ThemeResolver::resolve_radius(r, theme).map_err(LayoutError::ThemeError)?
                        * scale
                }
                None => 0.0,
            };

            // Format text
            let text = node
                .value
                .as_ref()
                .map(|tmpl| BindingFormatter::format_template(tmpl, bindings, "—"));

            let text_color = match &node.color {
                Some(c) => Some(
                    ThemeResolver::resolve_color(c, theme, Some(1.0))
                        .map_err(LayoutError::ThemeError)?,
                ),
                None => None,
            };

            let font_size_dip = node.font_size_dip.unwrap_or(13.0) * scale;

            // Resolve progress bar value
            let progress = if node.node_type == NodeType::Progress {
                let current = match &node.value_source {
                    Some(src) => bindings.get(src).and_then(|v| v.as_f32()).unwrap_or(0.0),
                    None => 0.0,
                };
                let min = node.min_value.unwrap_or(0.0);
                let max = node.max_value.unwrap_or(100.0);
                let normalized = if max > min {
                    ((current - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Some(normalized)
            } else {
                None
            };

            let fill_color = match &node.fill {
                Some(f) => Some(
                    ThemeResolver::resolve_color(f, theme, Some(1.0))
                        .map_err(LayoutError::ThemeError)?,
                ),
                None => None,
            };

            let border_color = match background {
                Some(_) => Some(
                    ThemeResolver::resolve_color("@surface.border", theme, Some(0.6))
                        .map_err(LayoutError::ThemeError)?,
                ),
                None => None,
            };

            // Resolve audio spectrum bands for SpectrumVisualizer nodes
            let bands = if node.node_type == NodeType::SpectrumVisualizer {
                // Look for "audio.spectrum" binding, or the node's value_source
                let source = node.value_source.as_deref().unwrap_or("audio.spectrum");
                bindings
                    .get(source)
                    .and_then(|v| v.as_float_array())
                    .map(|arr| arr.to_vec())
            } else {
                None
            };

            resolved_nodes.push(ResolvedNode {
                id: node.id.clone(),
                node_type: node.node_type.clone(),
                x: abs_x,
                y: abs_y,
                width,
                height,
                background,
                border_color,
                corner_radius,
                text,
                text_color,
                font_size_dip,
                progress,
                fill_color,
                bands,
            });
        }

        Ok(resolved_nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethershell_core::LayoutNode;

    #[test]
    fn test_layout_hierarchy_resolution() {
        let theme = ThemeDefinition::obsidian();
        let mut bindings = HashMap::new();
        bindings.insert("metrics.cpu".into(), BindingValue::Percent(75.0));

        let layout = LayoutDefinition {
            schema_version: "1.0.0".into(),
            node_limit: 10,
            nodes: vec![
                LayoutNode {
                    id: "card".into(),
                    node_type: NodeType::Container,
                    parent: None,
                    x: 10.0,
                    y: 20.0,
                    width: Some(200.0),
                    height: Some(100.0),
                    background: Some("@surface.raised".into()),
                    radius: Some("@radius.lg".into()),
                    value: None,
                    color: None,
                    font_size_dip: None,
                    value_source: None,
                    min_value: None,
                    max_value: None,
                    fill: None,
                },
                LayoutNode {
                    id: "label".into(),
                    node_type: NodeType::Text,
                    parent: Some("card".into()),
                    x: 15.0,
                    y: 10.0,
                    width: None,
                    height: None,
                    background: None,
                    radius: None,
                    value: Some("CPU: ${metrics.cpu:0.0}%".into()),
                    color: Some("@text.primary".into()),
                    font_size_dip: Some(14.0),
                    value_source: None,
                    min_value: None,
                    max_value: None,
                    fill: None,
                },
                LayoutNode {
                    id: "bar".into(),
                    node_type: NodeType::Progress,
                    parent: Some("card".into()),
                    x: 15.0,
                    y: 40.0,
                    width: Some(170.0),
                    height: Some(8.0),
                    background: None,
                    radius: Some("4.0".into()),
                    value: None,
                    color: None,
                    font_size_dip: None,
                    value_source: Some("metrics.cpu".into()),
                    min_value: Some(0.0),
                    max_value: Some(100.0),
                    fill: Some("@accent.primary".into()),
                },
            ],
        };

        let resolved = LayoutEngine::resolve(&layout, &theme, &bindings, 0.0, 0.0, 1.0).unwrap();
        assert_eq!(resolved.len(), 3);

        // Card container at (10, 20)
        assert_eq!(resolved[0].x, 10.0);
        assert_eq!(resolved[0].y, 20.0);
        assert_eq!(resolved[0].width, 200.0);
        assert_eq!(resolved[0].corner_radius, 14.0);

        // Child label at (10 + 15 = 25, 20 + 10 = 30)
        assert_eq!(resolved[1].x, 25.0);
        assert_eq!(resolved[1].y, 30.0);
        assert_eq!(resolved[1].text.as_deref(), Some("CPU: 75%"));

        // Progress bar at (10 + 15 = 25, 20 + 40 = 60)
        assert_eq!(resolved[2].x, 25.0);
        assert_eq!(resolved[2].y, 60.0);
        assert_eq!(resolved[2].progress, Some(0.75));
    }

    #[test]
    fn test_node_limit_enforcement() {
        let theme = ThemeDefinition::obsidian();
        let bindings = HashMap::new();

        let layout = LayoutDefinition {
            schema_version: "1.0.0".into(),
            node_limit: 1,
            nodes: vec![
                LayoutNode {
                    id: "n1".into(),
                    node_type: NodeType::Container,
                    parent: None,
                    x: 0.0,
                    y: 0.0,
                    width: None,
                    height: None,
                    background: None,
                    radius: None,
                    value: None,
                    color: None,
                    font_size_dip: None,
                    value_source: None,
                    min_value: None,
                    max_value: None,
                    fill: None,
                },
                LayoutNode {
                    id: "n2".into(),
                    node_type: NodeType::Container,
                    parent: None,
                    x: 0.0,
                    y: 0.0,
                    width: None,
                    height: None,
                    background: None,
                    radius: None,
                    value: None,
                    color: None,
                    font_size_dip: None,
                    value_source: None,
                    min_value: None,
                    max_value: None,
                    fill: None,
                },
            ],
        };

        let res = LayoutEngine::resolve(&layout, &theme, &bindings, 0.0, 0.0, 1.0);
        assert_eq!(res, Err(LayoutError::NodeLimitExceeded(2, 1)));
    }

    #[test]
    fn test_spectrum_visualizer_bands_binding_resolution() {
        let theme = ThemeDefinition::obsidian();
        let mut bindings = HashMap::new();
        let test_bands = vec![0.1f32, 0.3, 0.5, 0.7, 0.9];
        bindings.insert(
            "audio.spectrum".into(),
            BindingValue::FloatArray(test_bands.clone()),
        );

        let layout = LayoutDefinition {
            schema_version: "1.0.0".into(),
            node_limit: 10,
            nodes: vec![LayoutNode {
                id: "spec".into(),
                node_type: NodeType::SpectrumVisualizer,
                parent: None,
                x: 0.0,
                y: 0.0,
                width: Some(120.0),
                height: Some(30.0),
                background: None,
                radius: None,
                value: None,
                color: None,
                font_size_dip: None,
                value_source: Some("audio.spectrum".into()),
                min_value: None,
                max_value: None,
                fill: Some("@accent.primary".into()),
            }],
        };

        let resolved = LayoutEngine::resolve(&layout, &theme, &bindings, 0.0, 0.0, 1.0).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].bands, Some(test_bands));
    }
}
