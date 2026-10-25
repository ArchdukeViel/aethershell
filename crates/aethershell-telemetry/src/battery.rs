use aethershell_core::{BindingState, BindingValue};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// System battery and AC power sampler using native Windows power APIs (PRD Section 7.1).
#[derive(Debug, Default)]
pub struct BatterySampler;

impl BatterySampler {
    pub fn new() -> Self {
        Self
    }

    /// Samples battery and power state.
    ///
    /// Per Gate G08 and PRD Section 7.1: If running on a desktop without a battery,
    /// returns honest `BindingState::Unsupported`, never a fabricated 0% charge.
    pub fn sample(&self) -> (BindingValue, BindingValue) {
        let mut status = SYSTEM_POWER_STATUS::default();
        let ok = unsafe { GetSystemPowerStatus(&mut status) };

        if ok.is_err() {
            return (
                BindingValue::Unavailable(BindingState::Error),
                BindingValue::Unavailable(BindingState::Error),
            );
        }

        // Check if AC is connected
        let ac_connected = match status.ACLineStatus {
            0 => BindingValue::String("Battery".into()),
            1 => BindingValue::String("AC Powered".into()),
            _ => BindingValue::Unavailable(BindingState::TemporarilyUnavailable),
        };

        // BatteryFlag 128 means no system battery installed
        if (status.BatteryFlag & 128) != 0 || status.BatteryLifePercent == 255 {
            return (
                ac_connected,
                BindingValue::Unavailable(BindingState::Unsupported),
            );
        }

        let pct = status.BatteryLifePercent as f32;
        (ac_connected, BindingValue::Percent(pct))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_sampler_safe_execution() {
        let sampler = BatterySampler::new();
        let (ac, batt) = sampler.sample();

        println!("Power sample: AC={:?}, Battery={:?}", ac, batt);
        // Ensure that desktop environments without battery don't panic or fabricate 0%
        match batt {
            BindingValue::Percent(p) => assert!((0.0..=100.0).contains(&p)),
            BindingValue::Unavailable(reason) => {
                assert!(
                    reason == BindingState::Unsupported
                        || reason == BindingState::TemporarilyUnavailable
                );
            }
            _ => panic!("Unexpected battery value"),
        }
    }
}
