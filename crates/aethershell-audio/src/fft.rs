use std::f32::consts::PI;

/// Fast Fourier Transform and logarithmic band grouping engine.
pub struct FftEngine;

impl FftEngine {
    /// Computes power-of-two FFT on real input samples and returns magnitude spectrum.
    pub fn compute_magnitudes(samples: &[f32]) -> Vec<f32> {
        if samples.len() < 2 {
            return Vec::new();
        }

        // Pad to next power of two
        let n = samples.len().next_power_of_two();
        if n < 2 {
            return Vec::new();
        }
        let mut re = vec![0.0f32; n];
        let mut im = vec![0.0f32; n];

        // Apply Hann window
        for (i, &s) in samples.iter().enumerate() {
            let window = 0.5 * (1.0 - (2.0 * PI * i as f32 / (n as f32 - 1.0)).cos());
            re[i] = s * window;
        }

        // Cooley-Tukey Radix-2 FFT (in-place)
        cooley_tukey_fft(&mut re, &mut im);

        // Compute magnitude for positive frequencies (0 .. n/2)
        let half = n / 2;
        let mut magnitudes = Vec::with_capacity(half);
        for i in 0..half {
            let mag = (re[i] * re[i] + im[i] * im[i]).sqrt() / (n as f32 * 0.5);
            magnitudes.push(mag);
        }

        magnitudes
    }

    /// Groups raw FFT bins into logarithmically spaced visual frequency bands (e.g. 16, 32, 64).
    pub fn group_logarithmic_bands(magnitudes: &[f32], band_count: usize) -> Vec<f32> {
        if magnitudes.is_empty() || band_count == 0 {
            return vec![0.0; band_count];
        }

        let num_bins = magnitudes.len();
        let mut bands = vec![0.0f32; band_count];

        for (i, band) in bands.iter_mut().enumerate() {
            // Logarithmic mapping: lower bands aggregate fewer bins, higher bands aggregate more
            let start_fraction = (i as f32 / band_count as f32).powf(2.0);
            let end_fraction = ((i + 1) as f32 / band_count as f32).powf(2.0);

            let start_bin = (start_fraction * num_bins as f32) as usize;
            let mut end_bin = (end_fraction * num_bins as f32).ceil() as usize;
            if end_bin <= start_bin {
                end_bin = start_bin + 1;
            }
            let end_bin = end_bin.min(num_bins);

            let mut sum = 0.0f32;
            let mut count = 0;
            for mag in magnitudes.iter().take(end_bin).skip(start_bin) {
                sum += *mag;
                count += 1;
            }

            let avg = if count > 0 { sum / count as f32 } else { 0.0 };
            // Apply perceptual boost to high frequencies and clamp
            let frequency_weight = 1.0 + (i as f32 / band_count as f32) * 1.5;
            *band = (avg * frequency_weight * 3.0).clamp(0.0, 1.0);
        }

        bands
    }
}

/// In-place Cooley-Tukey FFT algorithm
fn cooley_tukey_fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n <= 1 {
        return;
    }

    // Bit-reversal permutation
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;

        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Butterfly computations
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * PI / len as f32;
        let w_step_re = angle.cos();
        let w_step_im = angle.sin();

        let mut i = 0;
        while i < n {
            let mut w_re = 1.0;
            let mut w_im = 0.0;

            for k in 0..half {
                let u_re = re[i + k];
                let u_im = im[i + k];

                let v_re = re[i + k + half] * w_re - im[i + k + half] * w_im;
                let v_im = re[i + k + half] * w_im + im[i + k + half] * w_re;

                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;

                let next_w_re = w_re * w_step_re - w_im * w_step_im;
                let next_w_im = w_re * w_step_im + w_im * w_step_re;
                w_re = next_w_re;
                w_im = next_w_im;
            }
            i += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fft_impulse_response() {
        let mut samples = vec![0.0f32; 64];
        samples[0] = 1.0; // Impulse

        let mags = FftEngine::compute_magnitudes(&samples);
        assert_eq!(mags.len(), 32);

        let bands = FftEngine::group_logarithmic_bands(&mags, 16);
        assert_eq!(bands.len(), 16);
        for band in bands {
            assert!((0.0..=1.0).contains(&band));
        }
    }

    #[test]
    fn test_fft_sine_wave_peak_detection() {
        let n = 256;
        let mut samples = vec![0.0f32; n];
        // Generate pure sine wave at bin 16
        for (i, sample) in samples.iter_mut().enumerate() {
            *sample = (2.0 * PI * 16.0 * i as f32 / n as f32).sin();
        }

        let mags = FftEngine::compute_magnitudes(&samples);
        assert_eq!(mags.len(), 128);

        // Find max magnitude bin
        let (max_bin, max_mag) = mags
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();

        // Bin 16 should be the peak (with +/- 1 bin window leakage)
        assert!((max_bin as i32 - 16).abs() <= 1);
        assert!(*max_mag > 0.1);
    }
}
