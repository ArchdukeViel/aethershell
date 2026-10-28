use crate::fft::FftEngine;
use std::time::Instant;

/// Audio spectrum processor handling FFT, smoothing, peak decay, and silence auto-hide.
#[derive(Debug)]
pub struct AudioSpectrumProcessor {
    band_count: usize,
    prev_bands: Vec<f32>,
    silence_threshold: f32,
    silence_duration_secs: f32,
    last_audio_activity: Instant,
    is_silent: bool,
}

impl AudioSpectrumProcessor {
    pub fn new(band_count: usize) -> Self {
        Self {
            band_count,
            prev_bands: vec![0.0; band_count],
            silence_threshold: 0.002,
            silence_duration_secs: 5.0,
            last_audio_activity: Instant::now(),
            is_silent: true,
        }
    }

    pub fn band_count(&self) -> usize {
        self.band_count
    }

    pub fn current_bands(&self) -> &[f32] {
        &self.prev_bands
    }

    pub fn is_silent(&self) -> bool {
        self.is_silent
    }

    pub fn should_auto_hide(&self) -> bool {
        self.is_silent
    }

    /// Processes a slice of audio PCM float samples (strictly in-memory; zero raw audio persisted).
    ///
    /// Resolves PRD Section 7.3 and Gate G09 / G16.
    pub fn process_samples(&mut self, samples: &[f32]) -> Vec<f32> {
        let now = Instant::now();
        if samples.is_empty() {
            if now.duration_since(self.last_audio_activity).as_secs_f32()
                >= self.silence_duration_secs
            {
                self.is_silent = true;
            }
            self.decay_bands();
            return self.prev_bands.clone();
        }

        // 1. Calculate RMS energy for silence detection
        let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
        let rms = (sum_squares / samples.len() as f32).sqrt();

        if rms > self.silence_threshold {
            self.last_audio_activity = now;
            self.is_silent = false;
        } else if now.duration_since(self.last_audio_activity).as_secs_f32()
            >= self.silence_duration_secs
        {
            self.is_silent = true;
            self.decay_bands();
            return self.prev_bands.clone();
        }

        // 2. Compute FFT magnitudes
        let magnitudes = FftEngine::compute_magnitudes(samples);

        // 3. Group into logarithmic bands
        let raw_bands = FftEngine::group_logarithmic_bands(&magnitudes, self.band_count);

        // 4. Temporal smoothing (attack fast, release slow)
        let mut smoothed = vec![0.0f32; self.band_count];
        for i in 0..self.band_count {
            let prev = self.prev_bands[i];
            let target = raw_bands[i];

            let alpha = if target > prev {
                0.25 // Fast attack
            } else {
                0.70 // Smooth release
            };

            smoothed[i] = (prev * alpha + target * (1.0 - alpha)).clamp(0.0, 1.0);
        }

        self.prev_bands = smoothed.clone();
        smoothed
    }

    /// Decays existing spectrum bands toward zero during silence.
    fn decay_bands(&mut self) {
        for band in &mut self.prev_bands {
            let next = *band * 0.85;
            *band = if next < 0.0005 { 0.0 } else { next };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spectrum_processor_silence_detection() {
        let mut processor = AudioSpectrumProcessor::new(16);
        assert!(processor.is_silent());

        // Process silence
        let silent_samples = vec![0.0f32; 128];
        let bands = processor.process_samples(&silent_samples);
        assert_eq!(bands.len(), 16);
        assert!(processor.is_silent());

        // Process active audio signal
        let mut active_samples = vec![0.0f32; 128];
        for (i, s) in active_samples.iter_mut().enumerate() {
            *s = (i as f32 * 0.1).sin() * 0.5;
        }

        let bands_active = processor.process_samples(&active_samples);
        assert!(!processor.is_silent());
        assert!(!processor.should_auto_hide());
        assert!(bands_active.iter().any(|&b| b > 0.01));

        // When loopback produces 0 packets (audio ended), empty samples decay bands
        let decayed = processor.process_samples(&[]);
        assert!(decayed.iter().sum::<f32>() < bands_active.iter().sum::<f32>());

        // Manually trigger silence duration expiration
        processor.last_audio_activity = Instant::now() - std::time::Duration::from_secs(6);
        let _ = processor.process_samples(&[]);
        assert!(processor.is_silent());
        assert!(processor.should_auto_hide());
    }
}
