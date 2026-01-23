use anyhow::{anyhow, Result};
use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::fft::FrequencyAnalyzer;
use super::AudioData;

pub struct AudioCapture {
    // Keep the thread handle to ensure it stays alive
    _capture_thread: thread::JoinHandle<()>,
}

impl AudioCapture {
    pub fn new(
        num_bars: usize,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
    ) -> Result<Self> {
        // PulseAudio sample specification
        let sample_rate = 44100;
        let spec = Spec {
            format: Format::F32le,
            channels: 2,
            rate: sample_rate,
        };

        if !spec.is_valid() {
            return Err(anyhow!("Invalid PulseAudio sample spec"));
        }

        // Try to find a monitor source for loopback capture
        let device = Self::find_monitor_source();
        info!("Using audio device: {}", device.as_deref().unwrap_or("default"));

        // Create PulseAudio simple connection for recording
        let pulse = psimple::Simple::new(
            None,                // Use default server
            "cavibe",            // Application name
            Direction::Record,   // Recording stream
            device.as_deref(),   // Device name (None = default)
            "audio-visualizer",  // Stream description
            &spec,               // Sample format
            None,                // Default channel map
            None,                // Default buffering attributes
        )
        .map_err(|e| anyhow!("Failed to connect to PulseAudio: {:?}", e))?;

        info!("Connected to PulseAudio, sensitivity: {}", sensitivity);

        // Spawn capture thread
        let capture_thread = thread::spawn(move || {
            Self::capture_loop(pulse, num_bars, sample_rate as f32, smoothing, sensitivity, sender);
        });

        Ok(Self {
            _capture_thread: capture_thread,
        })
    }

    fn capture_loop(
        pulse: psimple::Simple,
        num_bars: usize,
        sample_rate: f32,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
    ) {
        let mut analyzer = FrequencyAnalyzer::new(num_bars, sample_rate, smoothing, sensitivity);

        // Buffer for audio samples (stereo f32)
        // Read enough samples for FFT processing (~46ms at 44100Hz)
        let buffer_size = 2048 * 2; // stereo
        let mut buffer = vec![0.0f32; buffer_size];

        loop {
            // Read audio data from PulseAudio
            let byte_slice = unsafe {
                std::slice::from_raw_parts_mut(
                    buffer.as_mut_ptr() as *mut u8,
                    buffer.len() * std::mem::size_of::<f32>(),
                )
            };

            if let Err(e) = pulse.read(byte_slice) {
                warn!("PulseAudio read error: {:?}", e);
                continue;
            }

            // Convert stereo to mono
            let mono: Vec<f32> = buffer
                .chunks(2)
                .map(|chunk| (chunk[0] + chunk[1]) / 2.0)
                .collect();

            // Process through FFT
            let audio_data = analyzer.process(&mono);

            // Send to visualizer (ignore errors if receiver is dropped)
            if sender.send(Arc::new(audio_data)).is_err() {
                debug!("Audio receiver dropped, stopping capture");
                break;
            }
        }
    }

    /// Find a monitor source for capturing system audio output
    fn find_monitor_source() -> Option<String> {
        // Try to get list of sources from PulseAudio
        // For now, use a common default - the default output's monitor
        // This works because PipeWire exposes monitors as "<sink-name>.monitor"

        // Try common monitor source names
        let candidates = [
            "alsa_output.pci-0000_80_1f.3.analog-stereo.monitor",
            "default_output.monitor",
        ];

        // For now, just use the first known working one
        // A more robust solution would query PulseAudio for available sources
        for candidate in &candidates {
            info!("Trying monitor source: {}", candidate);
            return Some(candidate.to_string());
        }

        None
    }
}
