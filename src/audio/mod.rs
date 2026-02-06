mod capture;
mod fft;

pub use capture::AudioCapture;

use std::sync::Arc;
use tokio::sync::watch;

/// Audio data shared between capture and visualization
#[derive(Debug, Clone)]
pub struct AudioData {
    /// Frequency magnitudes (0.0 to 1.0 for each bar)
    pub frequencies: Vec<f32>,
    /// Overall volume/intensity
    pub intensity: f32,
    /// Bass intensity (low frequencies)
    pub bass: f32,
}

impl Default for AudioData {
    fn default() -> Self {
        Self {
            frequencies: vec![0.0; 64],
            intensity: 0.0,
            bass: 0.0,
        }
    }
}

/// Create an audio processing pipeline
pub fn create_audio_pipeline(
    num_bars: usize,
    smoothing: f32,
    sensitivity: f32,
    device: Option<String>,
) -> anyhow::Result<(AudioCapture, watch::Receiver<Arc<AudioData>>)> {
    let (tx, rx) = watch::channel(Arc::new(AudioData::default()));
    let capture = AudioCapture::new(num_bars, smoothing, sensitivity, tx, device)?;
    Ok((capture, rx))
}
