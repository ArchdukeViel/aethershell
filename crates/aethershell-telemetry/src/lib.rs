//! Native Windows telemetry providers, sensors, and hardware samplers.

pub mod battery;
pub mod cpu;
pub mod memory;
pub mod network;
pub mod provider;
pub mod storage;

pub use battery::BatterySampler;
pub use cpu::CpuSampler;
pub use memory::MemorySampler;
pub use network::NetworkSampler;
pub use provider::TelemetryRegistry;
pub use storage::StorageSampler;
