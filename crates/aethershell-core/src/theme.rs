use serde::{Deserialize, Serialize};

/// Semantic color tokens for surface layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceColors {
    pub bg: String,
    pub raised: String,
    pub overlay: String,
    pub border: String,
}

/// Semantic color tokens for text and readability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextColors {
    pub primary: String,
    pub secondary: String,
    pub disabled: String,
}

/// Semantic color tokens for primary and secondary accents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccentColors {
    pub primary: String,
    pub secondary: String,
}

/// Semantic status colors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusColors {
    pub success: String,
    pub warning: String,
    pub danger: String,
}

/// Bundled semantic color palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    pub surface: SurfaceColors,
    pub text: TextColors,
    pub accent: AccentColors,
    pub status: StatusColors,
}

/// Typography tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeTypography {
    pub family: String,
    pub mono_family: String,
    pub scale: f32,
}

/// Shape and layout tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeShape {
    pub corner_radius_dip: f32,
    pub border_width_dip: f32,
}

/// Motion and transition duration tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeMotion {
    pub fast_ms: u32,
    pub normal_ms: u32,
    pub slow_ms: u32,
}

/// Visual effect parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeEffects {
    pub surface_opacity: f32,
    pub glow_intensity: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighContrastSettings {
    pub use_system_colors: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReducedMotionSettings {
    pub disable_nonessential_animation: bool,
}

/// Authoritative Theme definition conforming to PRD Section 9.3 & 10.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub schema_version: String,
    pub id: String,
    pub label: String,
    pub color_space: String,
    pub colors: ThemeColors,
    pub typography: ThemeTypography,
    pub shape: ThemeShape,
    pub motion: ThemeMotion,
    pub effects: ThemeEffects,
    pub high_contrast: HighContrastSettings,
    pub reduced_motion: ReducedMotionSettings,
}

impl ThemeDefinition {
    /// 1. Obsidian theme (PRD Section 9.3 default)
    pub fn obsidian() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "obsidian".into(),
            label: "Obsidian".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#090B10".into(),
                    raised: "#1C2330".into(),
                    overlay: "#242E40".into(),
                    border: "#354257".into(),
                },
                text: TextColors {
                    primary: "#F3F6FC".into(),
                    secondary: "#A9B5C7".into(),
                    disabled: "#8D9AAA".into(),
                },
                accent: AccentColors {
                    primary: "#A78BFA".into(),
                    secondary: "#6EE7B7".into(),
                },
                status: StatusColors {
                    success: "#6EE7B7".into(),
                    warning: "#F5BE68".into(),
                    danger: "#F58F9D".into(),
                },
            },
            typography: ThemeTypography {
                family: "Segoe UI Variable".into(),
                mono_family: "Cascadia Mono".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 14.0,
                border_width_dip: 1.0,
            },
            motion: ThemeMotion {
                fast_ms: 120,
                normal_ms: 220,
                slow_ms: 350,
            },
            effects: ThemeEffects {
                surface_opacity: 0.82,
                glow_intensity: 0.0,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// 2. Arctic theme (PRD Section 9.3)
    pub fn arctic() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "arctic".into(),
            label: "Arctic".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#182233".into(),
                    raised: "#2B3D53".into(),
                    overlay: "#384E69".into(),
                    border: "#4E647B".into(),
                },
                text: TextColors {
                    primary: "#F1F5F9".into(),
                    secondary: "#B4C7D8".into(),
                    disabled: "#7E95AC".into(),
                },
                accent: AccentColors {
                    primary: "#38BDF8".into(),
                    secondary: "#88D0E5".into(),
                },
                status: StatusColors {
                    success: "#88D0E5".into(),
                    warning: "#F5BE68".into(),
                    danger: "#F58F9D".into(),
                },
            },
            typography: ThemeTypography {
                family: "Segoe UI Variable".into(),
                mono_family: "Cascadia Mono".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 12.0,
                border_width_dip: 1.0,
            },
            motion: ThemeMotion {
                fast_ms: 120,
                normal_ms: 220,
                slow_ms: 350,
            },
            effects: ThemeEffects {
                surface_opacity: 0.85,
                glow_intensity: 0.0,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// 3. Aurora theme (PRD Section 9.3)
    pub fn aurora() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "aurora".into(),
            label: "Aurora".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#111827".into(),
                    raised: "#28354A".into(),
                    overlay: "#34455E".into(),
                    border: "#43566E".into(),
                },
                text: TextColors {
                    primary: "#F7F7FF".into(),
                    secondary: "#ABB9D1".into(),
                    disabled: "#7E8DA5".into(),
                },
                accent: AccentColors {
                    primary: "#6EE7B7".into(),
                    secondary: "#C4B5FD".into(),
                },
                status: StatusColors {
                    success: "#6EE7B7".into(),
                    warning: "#FCD34D".into(),
                    danger: "#F87171".into(),
                },
            },
            typography: ThemeTypography {
                family: "Segoe UI Variable".into(),
                mono_family: "Cascadia Mono".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 16.0,
                border_width_dip: 1.0,
            },
            motion: ThemeMotion {
                fast_ms: 100,
                normal_ms: 200,
                slow_ms: 320,
            },
            effects: ThemeEffects {
                surface_opacity: 0.80,
                glow_intensity: 0.1,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// 4. Solar theme (PRD Section 9.3)
    pub fn solar() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "solar".into(),
            label: "Solar".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#FAF7F0".into(),
                    raised: "#E8DED0".into(),
                    overlay: "#D8CCBC".into(),
                    border: "#C6BAAC".into(),
                },
                text: TextColors {
                    primary: "#302B28".into(),
                    secondary: "#675C52".into(),
                    disabled: "#9A8C7F".into(),
                },
                accent: AccentColors {
                    primary: "#A45C2F".into(),
                    secondary: "#56767B".into(),
                },
                status: StatusColors {
                    success: "#4E7B56".into(),
                    warning: "#C7862C".into(),
                    danger: "#BA3C3C".into(),
                },
            },
            typography: ThemeTypography {
                family: "Segoe UI Variable".into(),
                mono_family: "Cascadia Mono".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 10.0,
                border_width_dip: 1.0,
            },
            motion: ThemeMotion {
                fast_ms: 140,
                normal_ms: 240,
                slow_ms: 380,
            },
            effects: ThemeEffects {
                surface_opacity: 0.90,
                glow_intensity: 0.0,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// 5. Neon Grid theme (PRD Section 9.3)
    pub fn neon_grid() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "neon_grid".into(),
            label: "Neon Grid".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#080D18".into(),
                    raised: "#17263C".into(),
                    overlay: "#233959".into(),
                    border: "#376B83".into(),
                },
                text: TextColors {
                    primary: "#F1FAFF".into(),
                    secondary: "#ADC0D2".into(),
                    disabled: "#6A7F93".into(),
                },
                accent: AccentColors {
                    primary: "#00D9EE".into(),
                    secondary: "#FF4FD8".into(),
                },
                status: StatusColors {
                    success: "#00FFC2".into(),
                    warning: "#FFDE59".into(),
                    danger: "#FF3366".into(),
                },
            },
            typography: ThemeTypography {
                family: "Cascadia Code".into(),
                mono_family: "Cascadia Mono".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 6.0,
                border_width_dip: 1.5,
            },
            motion: ThemeMotion {
                fast_ms: 80,
                normal_ms: 160,
                slow_ms: 260,
            },
            effects: ThemeEffects {
                surface_opacity: 0.85,
                glow_intensity: 0.25,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// 6. Monochrome theme (PRD Section 9.3)
    pub fn monochrome() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            id: "monochrome".into(),
            label: "Monochrome".into(),
            color_space: "srgb".into(),
            colors: ThemeColors {
                surface: SurfaceColors {
                    bg: "#121212".into(),
                    raised: "#333333".into(),
                    overlay: "#454545".into(),
                    border: "#737373".into(),
                },
                text: TextColors {
                    primary: "#FFFFFF".into(),
                    secondary: "#CBCBCB".into(),
                    disabled: "#737373".into(),
                },
                accent: AccentColors {
                    primary: "#E4E4E4".into(),
                    secondary: "#A8A8A8".into(),
                },
                status: StatusColors {
                    success: "#E4E4E4".into(),
                    warning: "#C2C2C2".into(),
                    danger: "#8A8A8A".into(),
                },
            },
            typography: ThemeTypography {
                family: "Segoe UI".into(),
                mono_family: "Consolas".into(),
                scale: 1.0,
            },
            shape: ThemeShape {
                corner_radius_dip: 4.0,
                border_width_dip: 1.0,
            },
            motion: ThemeMotion {
                fast_ms: 120,
                normal_ms: 200,
                slow_ms: 300,
            },
            effects: ThemeEffects {
                surface_opacity: 0.95,
                glow_intensity: 0.0,
            },
            high_contrast: HighContrastSettings {
                use_system_colors: true,
            },
            reduced_motion: ReducedMotionSettings {
                disable_nonessential_animation: true,
            },
        }
    }

    /// Returns all six official first-party launch themes.
    pub fn all_themes() -> Vec<Self> {
        vec![
            Self::obsidian(),
            Self::arctic(),
            Self::aurora(),
            Self::solar(),
            Self::neon_grid(),
            Self::monochrome(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_six_launch_themes_defined() {
        let themes = ThemeDefinition::all_themes();
        assert_eq!(themes.len(), 6);

        let ids: Vec<&str> = themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "obsidian",
                "arctic",
                "aurora",
                "solar",
                "neon_grid",
                "monochrome"
            ]
        );

        for theme in themes {
            assert_eq!(theme.schema_version, "1.0.0");
            assert!(theme.colors.surface.bg.starts_with('#'));
            assert!(theme.colors.surface.raised.starts_with('#'));
            assert!(theme.colors.text.primary.starts_with('#'));
            assert!(theme.colors.accent.primary.starts_with('#'));
            assert!(theme.shape.corner_radius_dip >= 0.0);
            assert!(theme.effects.surface_opacity > 0.0 && theme.effects.surface_opacity <= 1.0);
        }
    }
}
