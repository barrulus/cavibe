use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::fft::FrequencyAnalyzer;
use super::AudioData;

pub struct AudioCapture {
    stream: Stream,
}

impl AudioCapture {
    pub fn new(
        num_bars: usize,
        smoothing: f32,
        sender: watch::Sender<Arc<AudioData>>,
    ) -> Result<Self> {
        let host = cpal::default_host();

        // Try to get default input device, fall back to output device for loopback
        let device = host
            .default_input_device()
            .or_else(|| {
                warn!("No input device found, trying output device for loopback");
                host.default_output_device()
            })
            .ok_or_else(|| anyhow!("No audio device available"))?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        info!("Using audio device: {}", device_name);

        let config = device.default_input_config()?;
        debug!("Audio config: {:?}", config);

        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let stream = Self::build_stream(
            &device,
            &config.into(),
            num_bars,
            sample_rate,
            channels,
            smoothing,
            sender,
        )?;

        stream.play()?;

        Ok(Self { stream })
    }

    fn build_stream(
        device: &Device,
        config: &StreamConfig,
        num_bars: usize,
        sample_rate: f32,
        channels: usize,
        smoothing: f32,
        sender: watch::Sender<Arc<AudioData>>,
    ) -> Result<Stream> {
        let mut analyzer = FrequencyAnalyzer::new(num_bars, sample_rate, smoothing);

        let err_fn = |err| {
            warn!("Audio stream error: {}", err);
        };

        let stream = device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert to mono if stereo
                let mono: Vec<f32> = if channels > 1 {
                    data.chunks(channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                        .collect()
                } else {
                    data.to_vec()
                };

                // Process through FFT
                let audio_data = analyzer.process(&mono);

                // Send to visualizer (ignore errors if receiver is dropped)
                let _ = sender.send(Arc::new(audio_data));
            },
            err_fn,
            None,
        )?;

        Ok(stream)
    }

    pub fn pause(&self) -> Result<()> {
        self.stream.pause()?;
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        self.stream.play()?;
        Ok(())
    }
}
