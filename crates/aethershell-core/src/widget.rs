use serde::{Deserialize, Serialize};

/// Canonical widget types supported by AetherShell (PRD Section 6.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetType {
    ClockDate,
    SystemMetrics,
    MediaSession,
    AudioSpectrum,
    StatusPill,
    WeatherCurrent,
    NetworkThroughput,
    SystemResources,
    Custom(String),
}

impl WidgetType {
    pub fn canonical_id(&self) -> &str {
        match self {
            WidgetType::ClockDate => "clock.date",
            WidgetType::SystemMetrics => "system.metrics",
            WidgetType::MediaSession => "media.session",
            WidgetType::AudioSpectrum => "audio.spectrum",
            WidgetType::StatusPill => "status.pill",
            WidgetType::WeatherCurrent => "weather.current",
            WidgetType::NetworkThroughput => "network.throughput",
            WidgetType::SystemResources => "system.resources",
            WidgetType::Custom(ref id) => id.as_str(),
        }
    }

    pub fn from_canonical_id(id: &str) -> Self {
        match id {
            "clock.date" => WidgetType::ClockDate,
            "system.metrics" => WidgetType::SystemMetrics,
            "media.session" => WidgetType::MediaSession,
            "audio.spectrum" => WidgetType::AudioSpectrum,
            "status.pill" => WidgetType::StatusPill,
            "weather.current" => WidgetType::WeatherCurrent,
            "network.throughput" => WidgetType::NetworkThroughput,
            "system.resources" => WidgetType::SystemResources,
            other => WidgetType::Custom(other.to_string()),
        }
    }
}

/// Observable widget lifecycle states (PRD Section 6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetLifecycleState {
    Discovered,
    Validated,
    Available,
    Enabled,
    Visible,
    Suspended,
    Disabled,
    Error(String),
}

/// Binding availability state and reason (PRD Section 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingState {
    Ok,
    Warmup,
    TemporarilyUnavailable,
    Unsupported,
    PermissionDenied,
    SensorAsleep,
    Error,
}

/// Quality status of a sensor binding value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingQuality {
    Good,
    Stale,
    Estimated,
    Missing,
}

/// Strongly-typed sensor and telemetry binding value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BindingValue {
    String(String),
    Float(f64),
    Integer(i64),
    Percent(f32),
    Bytes(u64),
    /// Array of floating-point values (e.g. audio spectrum bands).
    FloatArray(Vec<f32>),
    Unavailable(BindingState),
}

impl BindingValue {
    /// Formats the binding value according to an optional format specifier (e.g. "0.0", "0.1").
    pub fn format_with(&self, fmt: Option<&str>, fallback: &str) -> String {
        match self {
            BindingValue::String(s) => s.clone(),
            BindingValue::Float(f) => match fmt {
                Some("0.0") => format!("{:.0}", f),
                Some("0.1") => format!("{:.1}", f),
                Some("0.2") => format!("{:.2}", f),
                _ => format!("{}", f),
            },
            BindingValue::Integer(i) => format!("{}", i),
            BindingValue::Percent(p) => match fmt {
                Some("0.0") => format!("{:.0}", p),
                Some("0.1") => format!("{:.1}", p),
                _ => format!("{:.1}", p),
            },
            BindingValue::Bytes(b) => {
                let gib = *b as f64 / (1024.0 * 1024.0 * 1024.0);
                match fmt {
                    Some("0.1") => format!("{:.1}", gib),
                    Some("0.2") => format!("{:.2}", gib),
                    _ => format!("{:.1}", gib),
                }
            }
            BindingValue::FloatArray(arr) => format!("[{} values]", arr.len()),
            BindingValue::Unavailable(_) => fallback.to_string(),
        }
    }

    /// Returns the float array if this is a `FloatArray` variant.
    pub fn as_float_array(&self) -> Option<&[f32]> {
        match self {
            BindingValue::FloatArray(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            BindingValue::Float(f) => Some(*f as f32),
            BindingValue::Integer(i) => Some(*i as f32),
            BindingValue::Percent(p) => Some(*p),
            BindingValue::Bytes(b) => Some(*b as f32),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_type_canonical_id_roundtrip() {
        let types = vec![
            WidgetType::ClockDate,
            WidgetType::SystemMetrics,
            WidgetType::MediaSession,
            WidgetType::AudioSpectrum,
            WidgetType::StatusPill,
            WidgetType::WeatherCurrent,
            WidgetType::NetworkThroughput,
            WidgetType::SystemResources,
            WidgetType::Custom("custom.plugin".into()),
        ];

        for t in types {
            let id = t.canonical_id();
            let parsed = WidgetType::from_canonical_id(id);
            assert_eq!(t, parsed);
        }
    }

    #[test]
    fn test_binding_value_formatting() {
        let cpu = BindingValue::Percent(42.56);
        assert_eq!(cpu.format_with(Some("0.0"), "—"), "43");
        assert_eq!(cpu.format_with(Some("0.1"), "—"), "42.6");

        let ram_bytes = BindingValue::Bytes(8 * 1024 * 1024 * 1024); // 8 GiB
        assert_eq!(ram_bytes.format_with(Some("0.1"), "—"), "8.0");

        let unavail = BindingValue::Unavailable(BindingState::TemporarilyUnavailable);
        assert_eq!(unavail.format_with(Some("0.1"), "—"), "—");
    }

    #[test]
    fn test_binding_value_float_array() {
        let arr = vec![0.1f32, 0.4, 0.9, 0.2];
        let val = BindingValue::FloatArray(arr.clone());
        assert_eq!(val.as_float_array(), Some(arr.as_slice()));
        assert_eq!(val.format_with(None, "—"), "[4 values]");
        assert_eq!(val.as_f32(), None);

        // Serialization & deserialization roundtrip
        let json = serde_json::to_string(&val).expect("serialize");
        let deserialized: BindingValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(val, deserialized);
    }
}
