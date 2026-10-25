use aethershell_core::{BindingState, BindingValue};
use std::time::Instant;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

/// Network throughput sampler computing delta octets per second.
#[derive(Debug)]
pub struct NetworkSampler {
    prev_in_octets: u64,
    prev_out_octets: u64,
    prev_time: Option<Instant>,
    has_sampled_once: bool,
}

impl Default for NetworkSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkSampler {
    pub fn new() -> Self {
        Self {
            prev_in_octets: 0,
            prev_out_octets: 0,
            prev_time: None,
            has_sampled_once: false,
        }
    }

    /// Samples network throughput (MB/s down, MB/s up).
    pub fn sample(&mut self) -> (BindingValue, BindingValue) {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let status = unsafe { GetIfTable2(&mut table_ptr) };

        if status.is_err() || table_ptr.is_null() {
            return (
                BindingValue::Unavailable(BindingState::Error),
                BindingValue::Unavailable(BindingState::Error),
            );
        }

        let mut total_in: u64 = 0;
        let mut total_out: u64 = 0;

        unsafe {
            let table = &*table_ptr;
            let rows = std::slice::from_raw_parts(table.Table.as_ptr(), table.NumEntries as usize);

            for row in rows {
                // Ignore loopback adapters (IF_TYPE_SOFTWARE_LOOPBACK = 24)
                if row.Type != 24 {
                    total_in += row.InOctets;
                    total_out += row.OutOctets;
                }
            }

            FreeMibTable(table_ptr as *mut _);
        }

        let now = Instant::now();

        if !self.has_sampled_once {
            self.prev_in_octets = total_in;
            self.prev_out_octets = total_out;
            self.prev_time = Some(now);
            self.has_sampled_once = true;
            return (
                BindingValue::Unavailable(BindingState::Warmup),
                BindingValue::Unavailable(BindingState::Warmup),
            );
        }

        let elapsed_secs = match self.prev_time {
            Some(t) => now.duration_since(t).as_secs_f64(),
            None => 1.0,
        };

        if elapsed_secs <= 0.001 {
            return (
                BindingValue::Unavailable(BindingState::TemporarilyUnavailable),
                BindingValue::Unavailable(BindingState::TemporarilyUnavailable),
            );
        }

        let delta_in = total_in.saturating_sub(self.prev_in_octets);
        let delta_out = total_out.saturating_sub(self.prev_out_octets);

        self.prev_in_octets = total_in;
        self.prev_out_octets = total_out;
        self.prev_time = Some(now);

        let mb = 1024.0 * 1024.0;
        let down_mbps = (delta_in as f64 / elapsed_secs) / mb;
        let up_mbps = (delta_out as f64 / elapsed_secs) / mb;

        (
            BindingValue::Float(down_mbps.max(0.0)),
            BindingValue::Float(up_mbps.max(0.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_sampler_warmup_and_delta() {
        let mut sampler = NetworkSampler::new();
        let (first_down, first_up) = sampler.sample();
        assert_eq!(first_down, BindingValue::Unavailable(BindingState::Warmup));
        assert_eq!(first_up, BindingValue::Unavailable(BindingState::Warmup));

        std::thread::sleep(std::time::Duration::from_millis(50));
        let (second_down, second_up) = sampler.sample();
        if let (BindingValue::Float(d), BindingValue::Float(u)) = (second_down, second_up) {
            assert!(d >= 0.0);
            assert!(u >= 0.0);
        }
    }
}
