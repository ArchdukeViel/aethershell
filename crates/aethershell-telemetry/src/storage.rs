use aethershell_core::{BindingState, BindingValue};
use windows::core::w;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// Disk capacity sampler (PRD Section 7.1 / [S12]).
/// Strictly reports drive capacity, not continuous I/O throughput.
#[derive(Debug, Default)]
pub struct StorageSampler;

impl StorageSampler {
    pub fn new() -> Self {
        Self
    }

    /// Samples disk capacity for the system drive (C:\).
    pub fn sample_system_drive(&self) -> (BindingValue, BindingValue) {
        let mut free_bytes: u64 = 0;
        let mut total_bytes: u64 = 0;

        let ok = unsafe {
            GetDiskFreeSpaceExW(
                w!("C:\\"),
                Some(&mut free_bytes as *mut _),
                Some(&mut total_bytes as *mut _),
                None,
            )
        };

        if ok.is_err() || total_bytes == 0 {
            return (
                BindingValue::Unavailable(BindingState::TemporarilyUnavailable),
                BindingValue::Unavailable(BindingState::TemporarilyUnavailable),
            );
        }

        let gib = 1024.0 * 1024.0 * 1024.0;
        let free_gib = free_bytes as f64 / gib;
        let total_gib = total_bytes as f64 / gib;

        (
            BindingValue::Float(free_gib),
            BindingValue::Float(total_gib),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_sampler_system_drive() {
        let sampler = StorageSampler::new();
        let (free, total) = sampler.sample_system_drive();

        match (free, total) {
            (BindingValue::Float(f), BindingValue::Float(t)) => {
                assert!(t > 0.0);
                assert!(f <= t);
            }
            _ => panic!("Expected valid float storage capacity"),
        }
    }
}
