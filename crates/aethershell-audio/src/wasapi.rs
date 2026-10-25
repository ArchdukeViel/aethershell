use crate::spectrum::AudioSpectrumProcessor;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Adaptive presentation frame rate modes per PRD Section 7.3 and Q36:A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FpsMode {
    Performance,  // 60 FPS
    AcBalanced,   // 30 FPS (default on AC power)
    BatterySaver, // 15 FPS (automatic on battery power)
}

impl FpsMode {
    pub fn target_fps(&self) -> u32 {
        match self {
            FpsMode::Performance => 60,
            FpsMode::AcBalanced => 30,
            FpsMode::BatterySaver => 15,
        }
    }

    pub fn frame_interval(&self) -> Duration {
        Duration::from_millis((1000.0 / self.target_fps() as f64).round() as u64)
    }
}

/// WASAPI shared loopback audio capture controller.
pub struct WasapiLoopbackCapture {
    fps_mode: FpsMode,
    is_paused: Arc<AtomicBool>,
    is_running: Arc<AtomicBool>,
    processor: AudioSpectrumProcessor,
}

impl WasapiLoopbackCapture {
    pub fn new(band_count: usize, fps_mode: FpsMode) -> Self {
        Self {
            fps_mode,
            is_paused: Arc::new(AtomicBool::new(false)),
            is_running: Arc::new(AtomicBool::new(false)),
            processor: AudioSpectrumProcessor::new(band_count),
        }
    }

    pub fn fps_mode(&self) -> FpsMode {
        self.fps_mode
    }

    pub fn set_fps_mode(&mut self, mode: FpsMode) {
        self.fps_mode = mode;
    }

    pub fn set_paused(&self, paused: bool) {
        self.is_paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Feeds audio samples in volatile memory only; zero raw audio persisted (PRD [S08]).
    pub fn feed_samples(&mut self, samples: &[f32]) -> Vec<f32> {
        if self.is_paused() {
            return vec![0.0; self.processor.band_count()];
        }
        self.processor.process_samples(samples)
    }

    pub fn current_bands(&self) -> &[f32] {
        self.processor.current_bands()
    }

    pub fn band_count(&self) -> usize {
        self.processor.band_count()
    }

    pub fn is_silent(&self) -> bool {
        self.processor.is_silent()
    }

    pub fn should_auto_hide(&self) -> bool {
        self.processor.should_auto_hide()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_fps_intervals() {
        assert_eq!(FpsMode::Performance.target_fps(), 60);
        assert_eq!(FpsMode::AcBalanced.target_fps(), 30);
        assert_eq!(FpsMode::BatterySaver.target_fps(), 15);

        assert!(FpsMode::Performance.frame_interval() < FpsMode::AcBalanced.frame_interval());
        assert!(FpsMode::AcBalanced.frame_interval() < FpsMode::BatterySaver.frame_interval());
    }

    #[test]
    fn test_wasapi_capture_lifecycle_and_pause() {
        let mut capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
        assert_eq!(capture.fps_mode(), FpsMode::AcBalanced);
        assert!(!capture.is_paused());

        let samples = vec![0.5f32; 128];
        let bands = capture.feed_samples(&samples);
        assert_eq!(bands.len(), 32);

        // Pause audio capture (e.g. from tray toggle)
        capture.set_paused(true);
        assert!(capture.is_paused());

        let muted = capture.feed_samples(&samples);
        assert_eq!(muted, vec![0.0; 32]);
    }
}
