use crate::battery::BatterySampler;
use crate::cpu::CpuSampler;
use crate::memory::MemorySampler;
use crate::network::NetworkSampler;
use crate::storage::StorageSampler;
use aethershell_core::BindingValue;
use std::collections::HashMap;

/// Central telemetry registry coordinating native Windows sensor sampling.
#[derive(Debug, Default)]
pub struct TelemetryRegistry {
    cpu: CpuSampler,
    memory: MemorySampler,
    storage: StorageSampler,
    battery: BatterySampler,
    network: NetworkSampler,
}

impl TelemetryRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Polls all active native Windows telemetry providers and returns a dictionary of metrics.
    pub fn poll_all(&mut self) -> HashMap<String, BindingValue> {
        let mut map = HashMap::new();

        // 1. CPU
        let cpu_val = self.cpu.sample();
        map.insert("cpu.usage_percent".into(), cpu_val.clone());
        map.insert("metrics.cpu".into(), cpu_val);

        // 2. Memory
        let (used_gib, total_gib, mem_pct) = self.memory.sample();
        map.insert("memory.used_gib".into(), used_gib.clone());
        map.insert("metrics.ram".into(), used_gib);
        map.insert("memory.total_gib".into(), total_gib);
        map.insert("memory.usage_percent".into(), mem_pct);

        // 3. Storage
        let (free_gib, total_storage_gib) = self.storage.sample_system_drive();
        map.insert("storage.free_gib".into(), free_gib.clone());
        map.insert("storage.free".into(), free_gib);
        map.insert("storage.total_gib".into(), total_storage_gib);

        // 4. Battery / Power
        let (ac_state, battery_pct) = self.battery.sample();
        map.insert("power.ac_state".into(), ac_state);
        map.insert("power.battery_percent".into(), battery_pct);

        // 5. Network
        let (net_down, net_up) = self.network.sample();
        map.insert("network.down".into(), net_down);
        map.insert("network.up".into(), net_up);

        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_registry_poll_all() {
        let mut registry = TelemetryRegistry::new();
        let map = registry.poll_all();

        assert!(map.contains_key("cpu.usage_percent"));
        assert!(map.contains_key("metrics.cpu"));
        assert!(map.contains_key("memory.used_gib"));
        assert!(map.contains_key("metrics.ram"));
        assert!(map.contains_key("storage.free"));
        assert!(map.contains_key("power.battery_percent"));
        assert!(map.contains_key("network.down"));
    }
}
