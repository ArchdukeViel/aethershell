use aethershell_core::{
    BindingValue, LayoutDefinition, ProfileDefinition, SkinManifest, ThemeDefinition,
    WidgetLifecycleState,
};
use aethershell_renderer::{
    CanvasRenderer, LayoutEngine, MockCanvasRenderer, RenderCommand, SceneGraph,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn find_asset_path(rel_path: &str) -> String {
    if Path::new(rel_path).exists() {
        rel_path.to_string()
    } else {
        format!("../../{}", rel_path)
    }
}

#[test]
fn test_all_six_themes_load_and_resolve() {
    let theme_files = [
        "obsidian.toml",
        "arctic.toml",
        "aurora.toml",
        "solar.toml",
        "neon_grid.toml",
        "monochrome.toml",
    ];

    for file in &theme_files {
        let path = find_asset_path(&format!("assets/themes/{}", file));
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Failed to read theme file {}", path));
        let theme: ThemeDefinition = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse theme {}: {}", path, e));

        assert_eq!(theme.schema_version, "1.0.0");
        assert!(!theme.id.is_empty());
        assert!(!theme.colors.surface.bg.is_empty());
        assert!(!theme.colors.text.primary.is_empty());
        assert!(!theme.colors.accent.primary.is_empty());
    }
}

#[test]
fn test_all_six_launch_skins_manifest_and_layout_validity() {
    let skin_dirs = [
        "minimal_type",
        "glass_bento",
        "cyber_hud",
        "terminal_mono",
        "ambient_soft",
        "spectrum_dynamic",
    ];

    for dir in &skin_dirs {
        let manifest_path = find_asset_path(&format!("assets/skins/{}/manifest.toml", dir));
        let layout_path = find_asset_path(&format!("assets/skins/{}/layout.toml", dir));
        let bindings_path = find_asset_path(&format!("assets/skins/{}/bindings.toml", dir));

        let manifest_content = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|_| panic!("Missing {}", manifest_path));
        let manifest: SkinManifest = toml::from_str(&manifest_content)
            .unwrap_or_else(|e| panic!("Invalid manifest {}: {}", manifest_path, e));
        assert_eq!(manifest.schema_version, "1.0.0");
        assert!(!manifest.supported_widget_types.is_empty());

        let layout_content =
            fs::read_to_string(&layout_path).unwrap_or_else(|_| panic!("Missing {}", layout_path));
        let layout: LayoutDefinition = toml::from_str(&layout_content)
            .unwrap_or_else(|e| panic!("Invalid layout {}: {}", layout_path, e));
        assert_eq!(layout.schema_version, "1.0.0");
        assert!(!layout.nodes.is_empty());

        assert!(
            fs::metadata(&bindings_path).is_ok(),
            "Missing bindings file: {}",
            bindings_path
        );
    }
}

#[test]
fn test_first_run_profile_signature_minimal() {
    let path = find_asset_path("assets/profiles/signature_minimal.toml");
    let profile_content =
        fs::read_to_string(&path).expect("Missing signature_minimal.toml profile");
    let profile: ProfileDefinition =
        toml::from_str(&profile_content).expect("Failed to parse signature_minimal.toml");

    assert_eq!(profile.schema_version, "1.0.0");
    assert_eq!(profile.id, "signature_minimal");
    assert_eq!(profile.theme, "obsidian");
    assert_eq!(profile.widgets.len(), 4);

    // Verify canonical types in profile
    let types: Vec<&str> = profile
        .widgets
        .iter()
        .map(|w| w.widget_type.as_str())
        .collect();
    assert!(types.contains(&"clock.date"));
    assert!(types.contains(&"system.metrics"));
    assert!(types.contains(&"media.session"));
    assert!(types.contains(&"audio.spectrum"));
}

#[test]
fn test_end_to_end_glass_bento_rendering_pipeline() {
    // 1. Load layout
    let layout_path = find_asset_path("assets/skins/glass_bento/layout.toml");
    let layout_content = fs::read_to_string(&layout_path).expect("Missing glass_bento layout.toml");
    let layout: LayoutDefinition = toml::from_str(&layout_content).unwrap();

    // 2. Load theme
    let theme_path = find_asset_path("assets/themes/obsidian.toml");
    let theme_content = fs::read_to_string(&theme_path).expect("Missing obsidian.toml theme");
    let theme: ThemeDefinition = toml::from_str(&theme_content).unwrap();

    // 3. Telemetry bindings
    let mut bindings = HashMap::new();
    bindings.insert("metrics.cpu".into(), BindingValue::Percent(38.4));
    bindings.insert("metrics.ram".into(), BindingValue::Float(7.8));

    // 4. Resolve spatial layout
    let resolved_nodes =
        LayoutEngine::resolve(&layout, &theme, &bindings, 100.0, 200.0, 1.0).unwrap();
    assert_eq!(resolved_nodes.len(), 5); // card, cpu_label, cpu_bar, ram_label, ram_bar

    // 5. Compile into low-level render scene graph
    let scene = SceneGraph::compile(&resolved_nodes);
    assert_eq!(scene.commands.len(), 5);

    // Verify card background rect command
    match &scene.commands[0] {
        RenderCommand::DrawRect {
            x,
            y,
            width,
            height,
            radius,
            ..
        } => {
            assert_eq!(*x, 100.0);
            assert_eq!(*y, 200.0);
            assert_eq!(*width, 260.0);
            assert_eq!(*height, 96.0);
            assert_eq!(*radius, 14.0);
        }
        other => panic!("Expected DrawRect, got {:?}", other),
    }

    // Verify cpu text command
    match &scene.commands[1] {
        RenderCommand::DrawText { text, x, y, .. } => {
            assert_eq!(text, "CPU 38%");
            assert_eq!(*x, 116.0); // 100 + 16
            assert_eq!(*y, 216.0); // 200 + 16
        }
        other => panic!("Expected DrawText, got {:?}", other),
    }

    // Verify progress bar command
    match &scene.commands[2] {
        RenderCommand::DrawProgressBar {
            progress, x, width, ..
        } => {
            assert!((*progress - 0.384).abs() < 0.001);
            assert_eq!(*x, 200.0); // 100 + 100
            assert_eq!(*width, 140.0);
        }
        other => panic!("Expected DrawProgressBar, got {:?}", other),
    }

    // 6. Execute drawing on MockCanvasRenderer
    let mut canvas = MockCanvasRenderer::new();
    let drawn = canvas.render_scene(&scene).unwrap();
    assert_eq!(drawn, 5);
    assert_eq!(canvas.executed_commands.len(), 5);
}

#[test]
fn test_widget_lifecycle_state_machine() {
    let mut state = WidgetLifecycleState::Discovered;
    assert_eq!(state, WidgetLifecycleState::Discovered);

    state = WidgetLifecycleState::Validated;
    assert_eq!(state, WidgetLifecycleState::Validated);

    state = WidgetLifecycleState::Available;
    assert_eq!(state, WidgetLifecycleState::Available);

    state = WidgetLifecycleState::Enabled;
    assert_eq!(state, WidgetLifecycleState::Enabled);

    state = WidgetLifecycleState::Visible;
    assert_eq!(state, WidgetLifecycleState::Visible);

    state = WidgetLifecycleState::Suspended;
    assert_eq!(state, WidgetLifecycleState::Suspended);

    state = WidgetLifecycleState::Disabled;
    assert_eq!(state, WidgetLifecycleState::Disabled);
}
