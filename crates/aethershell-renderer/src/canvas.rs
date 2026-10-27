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
    DrawLine {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: ColorRgba,
        stroke_width: f32,
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
                    if node.background.is_some() || node.border_color.is_some() {
                        let color = node.background.unwrap_or(ColorRgba::new(0, 0, 0, 0));
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
                    // Use live bands from layout resolution, or fallback to default visualization
                    let bands = node
                        .bands
                        .clone()
                        .unwrap_or_else(|| vec![0.05, 0.05, 0.05, 0.05, 0.05, 0.05]);
                    commands.push(RenderCommand::DrawSpectrum {
                        x: node.x,
                        y: node.y,
                        width: node.width,
                        height: node.height,
                        bands,
                        color,
                    });
                }
                NodeType::Button => {
                    let bg = node.background.unwrap_or(ColorRgba::new(40, 40, 40, 200));
                    let border = node
                        .border_color
                        .or(Some(ColorRgba::new(255, 255, 255, 50)));
                    commands.push(RenderCommand::DrawRect {
                        x: node.x,
                        y: node.y,
                        width: node.width,
                        height: node.height,
                        color: bg,
                        radius: if node.corner_radius > 0.0 {
                            node.corner_radius
                        } else {
                            4.0
                        },
                        border_color: border,
                    });
                    if let Some(text) = &node.text {
                        let color = node.text_color.unwrap_or(ColorRgba::WHITE);
                        let text_x = node.x + (node.width * 0.15).max(4.0);
                        let text_y = node.y + (node.height * 0.15).max(2.0);
                        commands.push(RenderCommand::DrawText {
                            text: text.clone(),
                            x: text_x,
                            y: text_y,
                            font_size_dip: if node.font_size_dip > 0.0 {
                                node.font_size_dip
                            } else {
                                12.0
                            },
                            color,
                        });
                    }
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

    /// Indicates whether the underlying graphics device or target is in a lost state.
    fn is_device_lost(&self) -> bool {
        false
    }

    /// Attempts to recover or recreate the graphics device or target following device loss.
    fn recover_device(&mut self) -> Result<(), String> {
        Ok(())
    }
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
                bands: None,
                action: None,
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
                bands: None,
                action: None,
            },
        ];

        let scene = SceneGraph::compile(&nodes);
        assert_eq!(scene.commands.len(), 2);

        let mut renderer = MockCanvasRenderer::new();
        let rendered_count = renderer.render_scene(&scene).unwrap();
        assert_eq!(rendered_count, 2);
        assert_eq!(renderer.executed_commands.len(), 2);
    }

    #[test]
    fn test_spectrum_scene_compilation_with_live_bands_and_fallback() {
        let live_bands = vec![0.25f32, 0.5, 0.75, 1.0];
        let nodes = vec![
            ResolvedNode {
                id: "spec_live".into(),
                node_type: NodeType::SpectrumVisualizer,
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
                background: None,
                border_color: None,
                corner_radius: 0.0,
                text: None,
                text_color: None,
                font_size_dip: 0.0,
                progress: None,
                fill_color: Some(ColorRgba::WHITE),
                bands: Some(live_bands.clone()),
                action: None,
            },
            ResolvedNode {
                id: "spec_fallback".into(),
                node_type: NodeType::SpectrumVisualizer,
                x: 0.0,
                y: 30.0,
                width: 100.0,
                height: 20.0,
                background: None,
                border_color: None,
                corner_radius: 0.0,
                text: None,
                text_color: None,
                font_size_dip: 0.0,
                progress: None,
                fill_color: Some(ColorRgba::WHITE),
                bands: None,
                action: None,
            },
        ];

        let scene = SceneGraph::compile(&nodes);
        assert_eq!(scene.commands.len(), 2);

        match &scene.commands[0] {
            RenderCommand::DrawSpectrum { bands, .. } => {
                assert_eq!(bands, &live_bands);
            }
            other => panic!("Expected DrawSpectrum, got {:?}", other),
        }

        match &scene.commands[1] {
            RenderCommand::DrawSpectrum { bands, .. } => {
                assert_eq!(bands.len(), 6);
            }
            other => panic!("Expected DrawSpectrum, got {:?}", other),
        }
    }

    #[test]
    fn test_button_node_scene_compilation() {
        let nodes = vec![ResolvedNode {
            id: "btn_action".into(),
            node_type: NodeType::Button,
            x: 20.0,
            y: 40.0,
            width: 80.0,
            height: 30.0,
            background: Some(ColorRgba::new(50, 50, 50, 255)),
            border_color: Some(ColorRgba::WHITE),
            corner_radius: 6.0,
            text: Some("Play".into()),
            text_color: Some(ColorRgba::WHITE),
            font_size_dip: 12.0,
            progress: None,
            fill_color: None,
            bands: None,
            action: Some("play_pause".into()),
        }];

        let scene = SceneGraph::compile(&nodes);
        assert_eq!(scene.commands.len(), 2);
        match &scene.commands[0] {
            RenderCommand::DrawRect {
                width,
                height,
                radius,
                ..
            } => {
                assert_eq!(*width, 80.0);
                assert_eq!(*height, 30.0);
                assert_eq!(*radius, 6.0);
            }
            other => panic!("Expected DrawRect, got {:?}", other),
        }
        match &scene.commands[1] {
            RenderCommand::DrawText {
                text,
                font_size_dip,
                ..
            } => {
                assert_eq!(text, "Play");
                assert_eq!(*font_size_dip, 12.0);
            }
            other => panic!("Expected DrawText, got {:?}", other),
        }
    }
}
