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
    half_magnitudes: Vec<f32>,
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
            half_magnitudes: vec![0.0; num_bars / 2],
            sensitivity,
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> AudioData {
        // Capture raw waveform before windowing (for oscilloscope display).
        // This still allocates because waveform is owned by AudioData.
        let waveform: Vec<f32> = samples
            .iter()
            .take(self.fft_size)
            .map(|&s| (s * self.sensitivity * 2.0).clamp(-1.0, 1.0))
            .collect();

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

        // Calculate magnitudes into the reusable half_magnitudes buffer
        self.calculate_bar_magnitudes();

        // Smooth in-place into previous_magnitudes, and emit the result as a
        // single allocation (AudioData owns this Vec).
        let mut frequencies = vec![0.0f32; self.num_bars];
        let half_bars = self.num_bars / 2;
        for i in 0..half_bars {
            // Mirror layout: bass on edges, treble in middle
            let new_left = self.half_magnitudes[i];
            let new_right = self.half_magnitudes[i];

            let li = half_bars - 1 - i;
            let ri = half_bars + i;

            let smoothed_l = self.previous_magnitudes[li] * self.smoothing + new_left * (1.0 - self.smoothing);
            let smoothed_r = self.previous_magnitudes[ri] * self.smoothing + new_right * (1.0 - self.smoothing);

            self.previous_magnitudes[li] = smoothed_l;
            self.previous_magnitudes[ri] = smoothed_r;
            frequencies[li] = smoothed_l;
            frequencies[ri] = smoothed_r;
        }

        let intensity = frequencies.iter().sum::<f32>() / frequencies.len() as f32;

        AudioData {
            frequencies,
            intensity,
            waveform,
        }
    }

    /// Fill `self.half_magnitudes` from the current FFT buffer.
    fn calculate_bar_magnitudes(&mut self) {
        let useful_bins = self.fft_size / 2;

        // Logarithmic frequency scaling: human hearing is logarithmic
        let min_freq = 50.0_f32;
        let max_freq = 10000.0_f32.min(self.sample_rate / 2.0);

        let half_bars = self.num_bars / 2;
        let denom = half_bars.max(1) as f32;

        for bar in 0..half_bars {
            let bar_start = bar as f32 / denom;
            let bar_end = (bar + 1) as f32 / denom;

            let freq_start = min_freq * (max_freq / min_freq).powf(bar_start);
            let freq_end = min_freq * (max_freq / min_freq).powf(bar_end);

            let bin_start =
                ((freq_start * self.fft_size as f32) / self.sample_rate).floor() as usize;
            let bin_end = ((freq_end * self.fft_size as f32) / self.sample_rate).ceil() as usize;

            let bin_start = bin_start.min(useful_bins - 1);
            let bin_end = bin_end.min(useful_bins).max(bin_start + 1);

            let mut sum = 0.0;
            for bin in bin_start..bin_end {
                sum += self.buffer[bin].norm();
            }
            let avg = sum / (bin_end - bin_start) as f32;
            self.half_magnitudes[bar] = (avg * 0.02 * self.sensitivity).min(1.0);
        }
    }
}
