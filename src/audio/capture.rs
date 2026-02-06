use anyhow::{anyhow, Result};
use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::fft::FrequencyAnalyzer;
use super::AudioData;

pub struct AudioCapture {
    // Keep the thread handle to ensure it stays alive
    _capture_thread: thread::JoinHandle<()>,
    stop_flag: Arc<AtomicBool>,
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// List available PulseAudio/PipeWire sources via the native libpulse API.
///
/// Returns a list of `(name, state)` tuples.
pub fn list_sources() -> Result<Vec<(String, String)>> {
    use pulse::callbacks::ListResult;
    use pulse::context::{Context, State as ContextState};
    use pulse::mainloop::standard::{IterateResult, Mainloop};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let mainloop = Rc::new(RefCell::new(
        Mainloop::new().ok_or_else(|| anyhow!("Failed to create PulseAudio mainloop"))?,
    ));
    let context = Rc::new(RefCell::new(
        Context::new(&*mainloop.borrow(), "cavibe-list")
            .ok_or_else(|| anyhow!("Failed to create PulseAudio context"))?,
    ));

    context
        .borrow_mut()
        .connect(None, pulse::context::FlagSet::NOFLAGS, None)
        .map_err(|_| anyhow!("Failed to connect to PulseAudio"))?;

    // Wait for context to be ready
    loop {
        match mainloop.borrow_mut().iterate(true) {
            IterateResult::Success(_) => {}
            _ => return Err(anyhow!("PulseAudio mainloop error")),
        }
        match context.borrow().get_state() {
            ContextState::Ready => break,
            ContextState::Failed | ContextState::Terminated => {
                return Err(anyhow!("PulseAudio connection failed"));
            }
            _ => {}
        }
    }

    let sources = Rc::new(RefCell::new(Vec::new()));
    let done = Rc::new(Cell::new(false));

    let sources_clone = sources.clone();
    let done_clone = done.clone();

    let _op = context
        .borrow()
        .introspect()
        .get_source_info_list(move |result| match result {
            ListResult::Item(info) => {
                let name = info.name.as_ref().map(|n| n.to_string()).unwrap_or_default();
                let state = match info.state {
                    pulse::def::SourceState::Running => "RUNNING",
                    pulse::def::SourceState::Idle => "IDLE",
                    pulse::def::SourceState::Suspended => "SUSPENDED",
                    _ => "UNKNOWN",
                };
                sources_clone.borrow_mut().push((name, state.to_string()));
            }
            ListResult::End | ListResult::Error => {
                done_clone.set(true);
            }
        });

    while !done.get() {
        match mainloop.borrow_mut().iterate(true) {
            IterateResult::Success(_) => {}
            _ => return Err(anyhow!("PulseAudio mainloop error")),
        }
    }

    Ok(Rc::try_unwrap(sources).unwrap().into_inner())
}

impl AudioCapture {
    pub fn new(
        num_bars: usize,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
        device: Option<String>,
    ) -> Result<Self> {
        // Use explicit device if provided, otherwise auto-detect
        let source = if let Some(sink_name) = device {
            let monitor = format!("{}.monitor", sink_name);
            info!("Using explicit sink monitor: {}", monitor);
            Some(monitor)
        } else {
            Self::find_monitor_source()
        };

        Self::start_capture(num_bars, smoothing, sensitivity, sender, source)
    }

    /// Create an AudioCapture using a raw PulseAudio source name (no `.monitor` appended).
    ///
    /// Use this when the caller already has a full source name (e.g. from `list_sources()`).
    pub fn new_with_source(
        num_bars: usize,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
        source: String,
    ) -> Result<Self> {
        info!("Using explicit source: {}", source);
        Self::start_capture(num_bars, smoothing, sensitivity, sender, Some(source))
    }

    /// Common setup: connect to PulseAudio and spawn the capture thread.
    fn start_capture(
        num_bars: usize,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
        device: Option<String>,
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

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();

        // Spawn capture thread
        let capture_thread = thread::spawn(move || {
            Self::capture_loop(pulse, num_bars, sample_rate as f32, smoothing, sensitivity, sender, stop_flag_clone);
        });

        Ok(Self {
            _capture_thread: capture_thread,
            stop_flag,
        })
    }

    fn capture_loop(
        pulse: psimple::Simple,
        num_bars: usize,
        sample_rate: f32,
        smoothing: f32,
        sensitivity: f32,
        sender: watch::Sender<Arc<AudioData>>,
        stop_flag: Arc<AtomicBool>,
    ) {
        let mut analyzer = FrequencyAnalyzer::new(num_bars, sample_rate, smoothing, sensitivity);

        // Buffer for audio samples (stereo f32)
        // Read enough samples for FFT processing (~46ms at 44100Hz)
        let buffer_size = 2048 * 2; // stereo
        let mut buffer = vec![0.0f32; buffer_size];

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                debug!("Stop flag set, ending capture loop");
                break;
            }
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

    /// Find a monitor source for capturing system audio output.
    ///
    /// Queries PulseAudio/PipeWire for the default sink and uses its monitor
    /// source, so we always capture from whatever output the user is listening to.
    fn find_monitor_source() -> Option<String> {
        // Get the default sink name and append ".monitor" to capture its output
        if let Ok(output) = std::process::Command::new("pactl")
            .args(["get-default-sink"])
            .output()
        {
            if output.status.success() {
                let sink_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !sink_name.is_empty() {
                    let monitor = format!("{}.monitor", sink_name);
                    info!("Using default sink monitor: {}", monitor);
                    return Some(monitor);
                }
            }
        }

        warn!("Could not determine default sink, using PulseAudio default source");
        None
    }
}
