use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// Information about a detected third-party taskbar customization tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub name: String,
    pub description: String,
    pub recommendation: String,
}

/// Known competing taskbar customization signatures per PRD Q21:A.
struct KnownCustomizer {
    class_name: &'static [u16],
    name: &'static str,
    description: &'static str,
}

// UTF-16 null-terminated strings for Win32 class queries
const TRANSLUCENT_TB: &[u16] = &[
    'T' as u16, 'r' as u16, 'a' as u16, 'n' as u16, 's' as u16, 'l' as u16, 'u' as u16, 'c' as u16,
    'e' as u16, 'n' as u16, 't' as u16, 'T' as u16, 'B' as u16, 0,
];

const EXPLORER_PATCHER: &[u16] = &[
    'e' as u16, 'p' as u16, '_' as u16, 't' as u16, 'a' as u16, 's' as u16, 'k' as u16, 'b' as u16,
    'a' as u16, 'r' as u16, 0,
];

const RETRO_BAR: &[u16] = &[
    'R' as u16, 'e' as u16, 't' as u16, 'r' as u16, 'o' as u16, 'B' as u16, 'a' as u16, 'r' as u16,
    0,
];

const START_ALL_BACK: &[u16] = &[
    'S' as u16, 't' as u16, 'a' as u16, 'r' as u16, 't' as u16, 'A' as u16, 'l' as u16, 'l' as u16,
    'B' as u16, 'a' as u16, 'c' as u16, 'k' as u16, 0,
];

const KNOWN_CUSTOMIZERS: &[KnownCustomizer] = &[
    KnownCustomizer {
        class_name: TRANSLUCENT_TB,
        name: "TranslucentTB",
        description: "TranslucentTB taskbar styling application detected.",
    },
    KnownCustomizer {
        class_name: EXPLORER_PATCHER,
        name: "ExplorerPatcher",
        description: "ExplorerPatcher taskbar hook detected.",
    },
    KnownCustomizer {
        class_name: RETRO_BAR,
        name: "RetroBar",
        description: "RetroBar custom taskbar shell detected.",
    },
    KnownCustomizer {
        class_name: START_ALL_BACK,
        name: "StartAllBack",
        description: "StartAllBack custom taskbar framework detected.",
    },
];

/// Non-invasively inspects for active competing taskbar customizers (PRD Section 4.2 / Q21:A).
///
/// Returns `Some(ConflictInfo)` if a competing tool is active, directing AetherShell
/// to yield and use `native_noop` fallback without fighting or terminating the other app.
pub fn detect_taskbar_conflicts() -> Option<ConflictInfo> {
    for customizer in KNOWN_CUSTOMIZERS {
        if let Ok(hwnd) =
            unsafe { FindWindowW(PCWSTR(customizer.class_name.as_ptr()), PCWSTR::null()) }
        {
            if !hwnd.0.is_null() {
                return Some(ConflictInfo {
                    name: customizer.name.to_string(),
                    description: customizer.description.to_string(),
                    recommendation: format!(
                        "Per PRD Q21:A, AetherShell yielded taskbar customization to {} to prevent shell instability.",
                        customizer.name
                    ),
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_detection_safe_execution() {
        // Safe read-only execution; returns None on clean test system
        let conflict = detect_taskbar_conflicts();
        if let Some(ref c) = conflict {
            println!("Detected active conflict: {} ({})", c.name, c.description);
        } else {
            println!("No competing taskbar customizers active.");
        }
    }
}
