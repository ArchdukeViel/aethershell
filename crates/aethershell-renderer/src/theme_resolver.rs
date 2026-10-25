use aethershell_core::ThemeDefinition;
use serde::{Deserialize, Serialize};

/// Unpremultiplied 8-bit sRGB color with alpha channel (PRD Section 9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorRgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ColorRgba {
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses a standard #RRGGBB or #RRGGBBAA hex string into ColorRgba.
    pub fn from_hex(hex: &str, alpha_override: Option<f32>) -> Result<Self, String> {
        let clean = hex.trim().trim_start_matches('#');
        if clean.len() == 6 {
            let r = u8::from_str_radix(&clean[0..2], 16)
                .map_err(|e| format!("Invalid red hex: {}", e))?;
            let g = u8::from_str_radix(&clean[2..4], 16)
                .map_err(|e| format!("Invalid green hex: {}", e))?;
            let b = u8::from_str_radix(&clean[4..6], 16)
                .map_err(|e| format!("Invalid blue hex: {}", e))?;
            let a = alpha_override
                .map(|a| (a.clamp(0.0, 1.0) * 255.0).round() as u8)
                .unwrap_or(255);
            Ok(Self { r, g, b, a })
        } else if clean.len() == 8 {
            let r = u8::from_str_radix(&clean[0..2], 16)
                .map_err(|e| format!("Invalid red hex: {}", e))?;
            let g = u8::from_str_radix(&clean[2..4], 16)
                .map_err(|e| format!("Invalid green hex: {}", e))?;
            let b = u8::from_str_radix(&clean[4..6], 16)
                .map_err(|e| format!("Invalid blue hex: {}", e))?;
            let parsed_a = u8::from_str_radix(&clean[6..8], 16)
                .map_err(|e| format!("Invalid alpha hex: {}", e))?;
            let a = alpha_override
                .map(|a| (a.clamp(0.0, 1.0) * 255.0).round() as u8)
                .unwrap_or(parsed_a);
            Ok(Self { r, g, b, a })
        } else {
            Err(format!(
                "Invalid hex length '{}': expected 6 or 8 hex digits",
                hex
            ))
        }
    }
}

/// Resolves semantic theme tokens (e.g. `@surface.raised`) into concrete values.
pub struct ThemeResolver;

impl ThemeResolver {
    /// Resolves a color string which may be a token `@...` or a direct `#RRGGBB` hex string.
    pub fn resolve_color(
        token_or_hex: &str,
        theme: &ThemeDefinition,
        alpha_override: Option<f32>,
    ) -> Result<ColorRgba, String> {
        let trimmed = token_or_hex.trim();

        if let Some(token) = trimmed.strip_prefix('@') {
            let hex = match token {
                "surface.bg" => &theme.colors.surface.bg,
                "surface.raised" => &theme.colors.surface.raised,
                "surface.overlay" => &theme.colors.surface.overlay,
                "surface.border" => &theme.colors.surface.border,
                "text.primary" => &theme.colors.text.primary,
                "text.secondary" => &theme.colors.text.secondary,
                "text.disabled" => &theme.colors.text.disabled,
                "accent.primary" => &theme.colors.accent.primary,
                "accent.secondary" => &theme.colors.accent.secondary,
                "status.success" => &theme.colors.status.success,
                "status.warning" => &theme.colors.status.warning,
                "status.danger" => &theme.colors.status.danger,
                other => return Err(format!("Unknown color token: '@{}'", other)),
            };

            let alpha = alpha_override.unwrap_or(theme.effects.surface_opacity);
            ColorRgba::from_hex(hex, Some(alpha))
        } else if trimmed.starts_with('#') {
            ColorRgba::from_hex(trimmed, alpha_override)
        } else {
            Err(format!(
                "Unrecognized color specification: '{}'. Must start with '@' or '#'",
                token_or_hex
            ))
        }
    }

    /// Resolves a corner radius token (e.g. `@radius.lg`) into DIP units.
    pub fn resolve_radius(token_or_num: &str, theme: &ThemeDefinition) -> Result<f32, String> {
        let trimmed = token_or_num.trim();
        if let Some(token) = trimmed.strip_prefix('@') {
            match token {
                "radius.none" => Ok(0.0),
                "radius.sm" => Ok(theme.shape.corner_radius_dip * 0.5),
                "radius.md" => Ok(theme.shape.corner_radius_dip * 0.75),
                "radius.lg" => Ok(theme.shape.corner_radius_dip),
                "radius.pill" => Ok(999.0),
                other => Err(format!("Unknown radius token: '@{}'", other)),
            }
        } else {
            trimmed
                .parse::<f32>()
                .map_err(|e| format!("Invalid numeric radius '{}': {}", trimmed, e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_hex_parsing() {
        let white = ColorRgba::from_hex("#FFFFFF", None).unwrap();
        assert_eq!(white, ColorRgba::new(255, 255, 255, 255));

        let half_black = ColorRgba::from_hex("#000000", Some(0.5)).unwrap();
        assert_eq!(half_black, ColorRgba::new(0, 0, 0, 128));
    }

    #[test]
    fn test_theme_resolver_obsidian_tokens() {
        let theme = ThemeDefinition::obsidian();

        let raised = ThemeResolver::resolve_color("@surface.raised", &theme, None).unwrap();
        assert_eq!(raised.r, 0x1C);
        assert_eq!(raised.g, 0x23);
        assert_eq!(raised.b, 0x30);
        assert_eq!(
            raised.a,
            (theme.effects.surface_opacity * 255.0).round() as u8
        );

        let text_prim = ThemeResolver::resolve_color("@text.primary", &theme, Some(1.0)).unwrap();
        assert_eq!(text_prim.r, 0xF3);
        assert_eq!(text_prim.g, 0xF6);
        assert_eq!(text_prim.b, 0xFC);
        assert_eq!(text_prim.a, 255);

        let radius_lg = ThemeResolver::resolve_radius("@radius.lg", &theme).unwrap();
        assert_eq!(radius_lg, 14.0);
    }
}
