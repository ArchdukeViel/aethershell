use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// Performance budgets specified in PRD Section 17 & Gate G13.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerformanceBudgets {
    pub idle_cpu_limit_pct: f64,
    pub active_cpu_limit_pct: f64,
    pub idle_ram_limit_mb: f64,
    pub active_ram_limit_mb: f64,
}

impl Default for PerformanceBudgets {
    fn default() -> Self {
        Self {
            idle_cpu_limit_pct: 0.5,
            active_cpu_limit_pct: 2.0,
            idle_ram_limit_mb: 40.0,
            active_ram_limit_mb: 65.0,
        }
    }
}

/// Truthful, evidence-backed benchmark report (PRD Section 17 / Gate G13).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkReport {
    pub duration_seconds: u32,
    pub idle_cpu_avg_pct: f64,
    pub idle_cpu_peak_pct: f64,
    pub idle_ram_mb: f64,
    pub active_cpu_avg_pct: f64,
    pub active_cpu_peak_pct: f64,
    pub active_ram_mb: f64,
    pub budgets: PerformanceBudgets,
    pub is_within_budget: bool,
    pub verdict: String,
}

/// Queries current process private working set RAM in megabytes (Gate G13).
pub fn query_process_memory_mb() -> f64 {
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let handle = GetCurrentProcess();
        if K32GetProcessMemoryInfo(
            handle,
            &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
        .as_bool()
        {
            counters.PrivateUsage as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// Queries process CPU time (kernel + user) in 100ns units.
fn query_process_cpu_time() -> (u64, Instant) {
    unsafe {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let handle = GetCurrentProcess();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
            let total = filetime_to_u64(kernel) + filetime_to_u64(user);
            (total, Instant::now())
        } else {
            (0, Instant::now())
        }
    }
}

/// Standardized benchmark harness measuring idle and active profiles over `duration_secs`.
pub fn run_benchmark(duration_secs: u32) -> BenchmarkReport {
    let half_duration = Duration::from_secs((duration_secs.max(2) / 2) as u64);
    let sample_interval = Duration::from_millis(100);

    // 1. Idle Profile Measurement
    std::thread::sleep(Duration::from_millis(200)); // Warmup stabilization
    let mut idle_cpu_samples = Vec::new();
    let mut idle_ram_samples = Vec::new();
    let (mut prev_cpu, mut prev_time) = query_process_cpu_time();

    let idle_start = Instant::now();
    while idle_start.elapsed() < half_duration {
        std::thread::sleep(sample_interval);
        let (curr_cpu, curr_time) = query_process_cpu_time();
        let elapsed_wall = curr_time.duration_since(prev_time).as_secs_f64();
        if elapsed_wall > 0.0 {
            let cpu_delta_100ns = curr_cpu.saturating_sub(prev_cpu) as f64;
            let cpu_delta_secs = cpu_delta_100ns / 10_000_000.0;
            let cpu_pct = (cpu_delta_secs / elapsed_wall) * 100.0;
            idle_cpu_samples.push(cpu_pct);
        }
        idle_ram_samples.push(query_process_memory_mb());
        prev_cpu = curr_cpu;
        prev_time = curr_time;
    }

    // 2. Active Profile Measurement (Active workload: scene graph compilation,
    // layout evaluation, FFT frequency spectrum processing per PRD Table 17-1 / Gate G13)
    let mut active_cpu_samples = Vec::new();
    let mut active_ram_samples = Vec::new();
    let active_start = Instant::now();

    let mut spectrum_processor = aethershell_audio::AudioSpectrumProcessor::new(32);
    let sample_count = 512;
    let mut frame_index: usize = 0;

    let layout_def = aethershell_core::LayoutDefinition {
        schema_version: "1.0.0".into(),
        node_limit: 128,
        nodes: vec![
            aethershell_core::LayoutNode {
                id: "root_container".into(),
                node_type: aethershell_core::NodeType::Container,
                parent: None,
                x: 0.0,
                y: 0.0,
                width: Some(380.0),
                height: Some(180.0),
                background: Some("surface.raised".into()),
                radius: Some("8.0".into()),
                value: None,
                color: None,
                font_size_dip: None,
                value_source: None,
                min_value: None,
                max_value: None,
                fill: None,
                action: None,
            },
            aethershell_core::LayoutNode {
                id: "title_text".into(),
                node_type: aethershell_core::NodeType::Text,
                parent: Some("root_container".into()),
                x: 16.0,
                y: 16.0,
                width: Some(200.0),
                height: Some(24.0),
                background: None,
                radius: None,
                value: Some("Active Benchmark Frame".into()),
                color: Some("text.primary".into()),
                font_size_dip: Some(14.0),
                value_source: Some("title_text".into()),
                min_value: None,
                max_value: None,
                fill: None,
                action: None,
            },
        ],
    };

    let theme_def = aethershell_core::ThemeDefinition::obsidian();

    while active_start.elapsed() < half_duration {
        let frame_start = Instant::now();

        // 1. Synthesize 512-sample PCM buffer with harmonic frequency tones
        frame_index = frame_index.wrapping_add(1);
        let mut pcm_samples = Vec::with_capacity(sample_count);
        let base_freq = 440.0 + (frame_index as f32 * 2.0).sin() * 200.0;
        for i in 0..sample_count {
            let t = i as f32 / 44100.0;
            let sample = (t * base_freq * 2.0 * std::f32::consts::PI).sin() * 0.5
                + (t * base_freq * 4.0 * std::f32::consts::PI).sin() * 0.25;
            pcm_samples.push(sample);
        }

        // 2. Active FFT audio spectrum processing
        let bands = spectrum_processor.process_samples(&pcm_samples);
        std::hint::black_box(&bands);

        // 3. Layout evaluation
        let mut bindings = std::collections::HashMap::new();
        bindings.insert(
            "title_text".into(),
            aethershell_core::BindingValue::String(format!("Frame {}", frame_index)),
        );
        let resolved = aethershell_renderer::LayoutEngine::resolve(
            &layout_def,
            &theme_def,
            &bindings,
            0.0,
            0.0,
            1.0,
        )
        .unwrap_or_default();

        // 4. Scene graph compilation
        let scene = aethershell_renderer::SceneGraph::compile(&resolved);
        std::hint::black_box(scene);

        // Target ~30 FPS cadence
        let compute_elapsed = frame_start.elapsed();
        let target_frame = Duration::from_millis(33);
        if compute_elapsed < target_frame {
            std::thread::sleep(target_frame - compute_elapsed);
        }

        let (curr_cpu, curr_time) = query_process_cpu_time();
        let elapsed_wall = curr_time.duration_since(prev_time).as_secs_f64();
        if elapsed_wall > 0.0 {
            let cpu_delta_100ns = curr_cpu.saturating_sub(prev_cpu) as f64;
            let cpu_delta_secs = cpu_delta_100ns / 10_000_000.0;
            let cpu_pct = (cpu_delta_secs / elapsed_wall) * 100.0;
            active_cpu_samples.push(cpu_pct);
        }
        active_ram_samples.push(query_process_memory_mb());
        prev_cpu = curr_cpu;
        prev_time = curr_time;
    }

    // Aggregate metrics
    let idle_cpu_avg = if idle_cpu_samples.is_empty() {
        0.0
    } else {
        idle_cpu_samples.iter().sum::<f64>() / idle_cpu_samples.len() as f64
    };
    let idle_cpu_peak = idle_cpu_samples.iter().copied().fold(0.0, f64::max);

    let idle_ram = if idle_ram_samples.is_empty() {
        0.0
    } else {
        idle_ram_samples.iter().copied().fold(0.0, f64::max)
    };

    let active_cpu_avg = if active_cpu_samples.is_empty() {
        0.0
    } else {
        active_cpu_samples.iter().sum::<f64>() / active_cpu_samples.len() as f64
    };
    let active_cpu_peak = active_cpu_samples.iter().copied().fold(0.0, f64::max);

    let active_ram = if active_ram_samples.is_empty() {
        0.0
    } else {
        active_ram_samples.iter().copied().fold(0.0, f64::max)
    };

    let budgets = PerformanceBudgets::default();
    let is_within_budget = idle_cpu_avg <= budgets.idle_cpu_limit_pct
        && active_cpu_avg <= budgets.active_cpu_limit_pct
        && idle_ram <= budgets.idle_ram_limit_mb
        && active_ram <= budgets.active_ram_limit_mb;

    BenchmarkReport {
        duration_seconds: duration_secs,
        idle_cpu_avg_pct: (idle_cpu_avg * 100.0).round() / 100.0,
        idle_cpu_peak_pct: (idle_cpu_peak * 100.0).round() / 100.0,
        idle_ram_mb: (idle_ram * 100.0).round() / 100.0,
        active_cpu_avg_pct: (active_cpu_avg * 100.0).round() / 100.0,
        active_cpu_peak_pct: (active_cpu_peak * 100.0).round() / 100.0,
        active_ram_mb: (active_ram * 100.0).round() / 100.0,
        budgets,
        is_within_budget,
        verdict: if is_within_budget {
            "passed".into()
        } else {
            "exceeded".into()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_memory_query() {
        let mem = query_process_memory_mb();
        assert!(mem > 0.0, "Working set memory must be greater than 0");
    }

    #[test]
    fn test_benchmark_harness_execution() {
        // Run a short 2-second benchmark in unit test
        let report = run_benchmark(2);
        assert_eq!(report.duration_seconds, 2);
        assert!(report.idle_ram_mb > 0.0);
        assert!(report.active_ram_mb > 0.0);
        println!("Benchmark report: {:?}", report);
    }
}
