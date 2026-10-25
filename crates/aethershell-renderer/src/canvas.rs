use crate::layout::ResolvedNode;
use crate::theme_resolver::ColorRgba;
use aethershell_core::NodeType;

/// Low-level render command ready for rasterization or DirectX drawing.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderCommand {
    DrawRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: ColorRgba,
        radius: f32,
        border_color: Option<ColorRgba>,
    },
    DrawText {
        text: String,
        x: f32,
        y: f32,
        font_size_dip: f32,
        color: ColorRgba,
    },
    DrawProgressBar {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        progress: f32,
        fill: ColorRgba,
        track: ColorRgba,
        radius: f32,
    },
    DrawSpectrum {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        bands: Vec<f32>,
        color: ColorRgba,
    },
}

/// Compiled render scene containing ordered draw operations.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneGraph {
    pub commands: Vec<RenderCommand>,
}

impl SceneGraph {
    /// Compiles resolved layout nodes into low-level render commands.
    pub fn compile(nodes: &[ResolvedNode]) -> Self {
        let mut commands = Vec::new();

        for node in nodes {
            match node.node_type {
                NodeType::Container => {
                    if let Some(color) = node.background {
                        commands.push(RenderCommand::DrawRect {
                            x: node.x,
                            y: node.y,
                            width: node.width,
                            height: node.height,
                            color,
                            radius: node.corner_radius,
                            border_color: node.border_color,
                        });
                    }
                }
                NodeType::Text => {
                    if let (Some(text), Some(color)) = (&node.text, node.text_color) {
                        commands.push(RenderCommand::DrawText {
                            text: text.clone(),
                            x: node.x,
                            y: node.y,
                            font_size_dip: node.font_size_dip,
                            color,
                        });
                    }
                }
                NodeType::Progress => {
                    let fill = node.fill_color.unwrap_or(ColorRgba::WHITE);
                    let track = node.background.unwrap_or(ColorRgba::new(255, 255, 255, 40));
                    let progress = node.progress.unwrap_or(0.0);

                    commands.push(RenderCommand::DrawProgressBar {
                        x: node.x,
                        y: node.y,
                        width: node.width,
                        height: node.height,
                        progress,
                        fill,
                        track,
                        radius: node.corner_radius,
                    });
                }
                NodeType::SpectrumVisualizer => {
                    let color = node.fill_color.unwrap_or(ColorRgba::WHITE);
                    // Default mock bands if not supplied
                    let bands = vec![0.2, 0.4, 0.7, 0.9, 0.6, 0.3];
                    commands.push(RenderCommand::DrawSpectrum {
                        x: node.x,
                        y: node.y,
                        width: node.width,
                        height: node.height,
                        bands,
                        color,
                    });
                }
                _ => {}
            }
        }

        Self { commands }
    }
}

/// Abstract rendering interface for hardware Direct2D and software test harnesses.
pub trait CanvasRenderer {
    fn render_scene(&mut self, scene: &SceneGraph) -> Result<usize, String>;
}

/// In-memory mock canvas capturing draw calls for 100% deterministic testing.
#[derive(Debug, Default)]
pub struct MockCanvasRenderer {
    pub executed_commands: Vec<RenderCommand>,
}

impl MockCanvasRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CanvasRenderer for MockCanvasRenderer {
    fn render_scene(&mut self, scene: &SceneGraph) -> Result<usize, String> {
        let count = scene.commands.len();
        self.executed_commands.extend_from_slice(&scene.commands);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_canvas_rendering() {
        let nodes = vec![
            ResolvedNode {
                id: "c1".into(),
                node_type: NodeType::Container,
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                background: Some(ColorRgba::BLACK),
                border_color: None,
                corner_radius: 8.0,
                text: None,
                text_color: None,
                font_size_dip: 12.0,
                progress: None,
                fill_color: None,
            },
            ResolvedNode {
                id: "t1".into(),
                node_type: NodeType::Text,
                x: 10.0,
                y: 10.0,
                width: 0.0,
                height: 0.0,
                background: None,
                border_color: None,
                corner_radius: 0.0,
                text: Some("Hello AetherShell".into()),
                text_color: Some(ColorRgba::WHITE),
                font_size_dip: 14.0,
                progress: None,
                fill_color: None,
            },
        ];

        let scene = SceneGraph::compile(&nodes);
        assert_eq!(scene.commands.len(), 2);

        let mut renderer = MockCanvasRenderer::new();
        let rendered_count = renderer.render_scene(&scene).unwrap();
        assert_eq!(rendered_count, 2);
        assert_eq!(renderer.executed_commands.len(), 2);
    }
}
