use rustfft::{num_complex::Complex, FftPlanner};

use super::AudioData;

pub struct FrequencyAnalyzer {
    fft_size: usize,
    num_bars: usize,
    sample_rate: f32,
    smoothing: f32,
    planner: FftPlanner<f32>,
    buffer: Vec<Complex<f32>>,
    window: Vec<f32>,
    previous_magnitudes: Vec<f32>,
    sensitivity: f32,
}

impl FrequencyAnalyzer {
    pub fn new(num_bars: usize, sample_rate: f32, smoothing: f32, sensitivity: f32) -> Self {
        let fft_size = 2048; // Good balance of frequency resolution and responsiveness
        let planner = FftPlanner::new();

        // Hann window for smoother frequency response
        let window: Vec<f32> = (0..fft_size)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
            })
            .collect();

        Self {
            fft_size,
            num_bars,
            sample_rate,
            smoothing,
            planner,
            buffer: vec![Complex::new(0.0, 0.0); fft_size],
            window,
            previous_magnitudes: vec![0.0; num_bars],
            sensitivity,
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> AudioData {
        // Fill buffer with windowed samples
        for (i, sample) in samples.iter().take(self.fft_size).enumerate() {
            self.buffer[i] = Complex::new(sample * self.window[i], 0.0);
        }

        // Zero-pad if needed
        for i in samples.len()..self.fft_size {
            self.buffer[i] = Complex::new(0.0, 0.0);
        }

        // Perform FFT
        let fft = self.planner.plan_fft_forward(self.fft_size);
        fft.process(&mut self.buffer);

        // Calculate magnitudes and map to bars
        let frequencies = self.calculate_bar_magnitudes();

        // Apply smoothing
        let smoothed: Vec<f32> = frequencies
            .iter()
            .zip(self.previous_magnitudes.iter())
            .map(|(&new, &old)| old * self.smoothing + new * (1.0 - self.smoothing))
            .collect();

        self.previous_magnitudes = smoothed.clone();

        // Calculate overall metrics
        let intensity = smoothed.iter().sum::<f32>() / smoothed.len() as f32;

        let third = smoothed.len() / 3;
        let bass = smoothed[..third].iter().sum::<f32>() / third as f32;

        AudioData {
            frequencies: smoothed,
            intensity,
            bass,
        }
    }

    fn calculate_bar_magnitudes(&self) -> Vec<f32> {
        // Use only positive frequencies (first half of FFT output)
        let useful_bins = self.fft_size / 2;

        // Logarithmic frequency scaling for better visualization
        // Human hearing is logarithmic, so we want more bars for lower frequencies
        let min_freq = 50.0; // Hz - start a bit higher for better bass response
        let max_freq = 10000.0_f32.min(self.sample_rate / 2.0); // Hz - cap lower for more activity

        // Calculate half the bars, then mirror for symmetric display (like cava)
        let half_bars = self.num_bars / 2;
        let mut half_magnitudes = vec![0.0; half_bars];

        for bar in 0..half_bars {
            // Calculate frequency range for this bar (logarithmic scale)
            let bar_start = (bar as f32) / (half_bars as f32);
            let bar_end = ((bar + 1) as f32) / (half_bars as f32);

            let freq_start = min_freq * (max_freq / min_freq).powf(bar_start);
            let freq_end = min_freq * (max_freq / min_freq).powf(bar_end);

            // Convert to bin indices
            let bin_start =
                ((freq_start * self.fft_size as f32) / self.sample_rate).floor() as usize;
            let bin_end = ((freq_end * self.fft_size as f32) / self.sample_rate).ceil() as usize;

            let bin_start = bin_start.min(useful_bins - 1);
            let bin_end = bin_end.min(useful_bins).max(bin_start + 1);

            // Average magnitude across bins
            let mut sum = 0.0;
            for bin in bin_start..bin_end {
                let magnitude = self.buffer[bin].norm();
                sum += magnitude;
            }

            let avg = sum / (bin_end - bin_start) as f32;
            // Apply sensitivity scaling (base scale + user sensitivity multiplier)
            half_magnitudes[bar] = (avg * 0.02 * self.sensitivity).min(1.0);
        }

        // Mirror: bass on edges, treble in middle
        // Layout: [bass...treble | treble...bass]
        let mut bar_magnitudes = vec![0.0; self.num_bars];
        for i in 0..half_bars {
            // Left side: bass to treble (reversed so bass is on left edge)
            bar_magnitudes[half_bars - 1 - i] = half_magnitudes[i];
            // Right side: treble to bass (normal order so bass is on right edge)
            bar_magnitudes[half_bars + i] = half_magnitudes[i];
        }

        bar_magnitudes
    }
}
