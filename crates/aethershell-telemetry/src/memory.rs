use aethershell_core::{BindingState, BindingValue};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Physical RAM telemetry sampler using native Win32 GlobalMemoryStatusEx.
#[derive(Debug, Default)]
pub struct MemorySampler;

impl MemorySampler {
    pub fn new() -> Self {
        Self
    }

    /// Samples RAM metrics returning used GiB, total GiB, and usage percent.
    pub fn sample(&self) -> (BindingValue, BindingValue, BindingValue) {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };

        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok.is_err() {
            return (
                BindingValue::Unavailable(BindingState::Error),
                BindingValue::Unavailable(BindingState::Error),
                BindingValue::Unavailable(BindingState::Error),
            );
        }

        let total_bytes = status.ullTotalPhys;
        let avail_bytes = status.ullAvailPhys;
        let used_bytes = total_bytes.saturating_sub(avail_bytes);

        let gib = 1024.0 * 1024.0 * 1024.0;
        let used_gib = used_bytes as f64 / gib;
        let total_gib = total_bytes as f64 / gib;
        let usage_pct = status.dwMemoryLoad as f32;

        (
            BindingValue::Float(used_gib),
            BindingValue::Float(total_gib),
            BindingValue::Percent(usage_pct),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_sampler_reading() {
        let sampler = MemorySampler::new();
        let (used, total, pct) = sampler.sample();

        match (used, total, pct) {
            (BindingValue::Float(u), BindingValue::Float(t), BindingValue::Percent(p)) => {
                assert!(t > 0.0);
                assert!(u > 0.0 && u <= t);
                assert!((0.0..=100.0).contains(&p));
            }
            _ => panic!("Expected valid float and percent bindings"),
        }
    }
}
