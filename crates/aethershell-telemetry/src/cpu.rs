use aethershell_core::{BindingState, BindingValue};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::GetSystemTimes;

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// CPU utilization sampler using native user-mode Windows NT system times.
#[derive(Debug, Default)]
pub struct CpuSampler {
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    has_sampled_once: bool,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Samples current CPU usage; returns Warmup on first invocation (PRD Gate G08).
    pub fn sample(&mut self) -> BindingValue {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();

        let ok = unsafe {
            GetSystemTimes(
                Some(&mut idle as *mut _),
                Some(&mut kernel as *mut _),
                Some(&mut user as *mut _),
            )
        };

        if ok.is_err() {
            return BindingValue::Unavailable(BindingState::Error);
        }

        let idle_u64 = filetime_to_u64(idle);
        let kernel_u64 = filetime_to_u64(kernel);
        let user_u64 = filetime_to_u64(user);

        if !self.has_sampled_once {
            self.prev_idle = idle_u64;
            self.prev_kernel = kernel_u64;
            self.prev_user = user_u64;
            self.has_sampled_once = true;
            return BindingValue::Unavailable(BindingState::Warmup);
        }

        let delta_idle = idle_u64.saturating_sub(self.prev_idle);
        let delta_kernel = kernel_u64.saturating_sub(self.prev_kernel);
        let delta_user = user_u64.saturating_sub(self.prev_user);

        let delta_total = delta_kernel + delta_user;

        self.prev_idle = idle_u64;
        self.prev_kernel = kernel_u64;
        self.prev_user = user_u64;

        if delta_total == 0 {
            return BindingValue::Unavailable(BindingState::TemporarilyUnavailable);
        }

        let usage = if delta_idle >= delta_total {
            0.0
        } else {
            let busy = delta_total - delta_idle;
            ((busy as f64 / delta_total as f64) * 100.0) as f32
        };

        BindingValue::Percent(usage.clamp(0.0, 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_sampler_warmup_and_subsequent_reading() {
        let mut sampler = CpuSampler::new();
        // First sample should report Warmup
        let first = sampler.sample();
        assert_eq!(first, BindingValue::Unavailable(BindingState::Warmup));

        // Sleep briefly and take second sample
        std::thread::sleep(std::time::Duration::from_millis(50));
        let second = sampler.sample();
        match second {
            BindingValue::Percent(pct) => {
                assert!((0.0..=100.0).contains(&pct));
            }
            BindingValue::Unavailable(_) => {}
            other => panic!("Unexpected sample value: {:?}", other),
        }
    }
}
