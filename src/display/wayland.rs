//! Wayland layer-shell backend for wallpaper mode
//!
//! Uses wlr-layer-shell protocol to render the visualizer as a desktop background
//! on Wayland compositors like Niri, Sway, Hyprland, etc.

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::SlotPool,
        Shm, ShmHandler,
    },
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::audio::{self, AudioData};
use crate::color::ColorScheme;
use crate::config::Config;
use crate::metadata::{self, TrackInfo};
use crate::visualizer::VisualizerState;

/// Wayland layer-shell wallpaper renderer
struct WallpaperState {
    // Wayland state
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,

    // Surface state
    layer_surface: Option<LayerSurface>,
    pool: Option<SlotPool>,
    width: u32,
    height: u32,
    configured: bool,

    // Visualizer state
    visualizer: VisualizerState,
    color_scheme: ColorScheme,
    audio_data: Arc<AudioData>,
    track_info: Arc<TrackInfo>,
    last_frame: Instant,
    time: f32,

    // Control
    running: bool,
    config: Config,
}

impl WallpaperState {
    fn new(
        registry_state: RegistryState,
        output_state: OutputState,
        compositor_state: CompositorState,
        shm: Shm,
        layer_shell: LayerShell,
        config: Config,
    ) -> Self {
        let visualizer = VisualizerState::new(config.visualizer.clone(), config.text.clone());
        let color_scheme = config.visualizer.color_scheme;

        Self {
            registry_state,
            output_state,
            compositor_state,
            shm,
            layer_shell,
            layer_surface: None,
            pool: None,
            width: 0,
            height: 0,
            configured: false,
            visualizer,
            color_scheme,
            audio_data: Arc::new(AudioData::default()),
            track_info: Arc::new(TrackInfo::default()),
            last_frame: Instant::now(),
            time: 0.0,
            running: true,
            config,
        }
    }

    fn create_layer_surface(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);

        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Background,
            Some("cavibe-wallpaper"),
            None, // Use default output
        );

        // Configure the layer surface
        layer_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer_surface.set_exclusive_zone(-1); // Don't reserve space
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

        // Commit to get the configure event
        layer_surface.commit();

        self.layer_surface = Some(layer_surface);
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }

        if self.layer_surface.is_none() {
            return;
        }

        // Ensure we have a pool
        if self.pool.is_none() {
            let pool = SlotPool::new(
                (self.width * self.height * 4) as usize,
                &self.shm,
            )
            .expect("Failed to create slot pool");
            self.pool = Some(pool);
        }

        let pool = self.pool.as_mut().unwrap();

        // Get a buffer from the pool
        let (buffer, canvas) = pool
            .create_buffer(
                self.width as i32,
                self.height as i32,
                (self.width * 4) as i32,
                wl_shm::Format::Argb8888,
            )
            .expect("Failed to create buffer");

        // Render the visualizer to the canvas
        // Extract values needed for rendering to avoid borrow issues
        let width = self.width as usize;
        let height = self.height as usize;
        let frequencies = self.audio_data.frequencies.clone();
        let intensity = self.audio_data.intensity;
        let track_title = self.track_info.title.clone();
        let track_artist = self.track_info.artist.clone();
        let color_scheme = self.color_scheme;
        let bar_width = self.config.visualizer.bar_width as usize;
        let bar_spacing = self.config.visualizer.bar_spacing as usize;
        let time = self.time;

        render_to_buffer(
            canvas,
            width,
            height,
            &frequencies,
            intensity,
            &track_title,
            &track_artist,
            &color_scheme,
            bar_width,
            bar_spacing,
            time,
        );

        // Attach and commit
        let layer_surface = self.layer_surface.as_ref().unwrap();
        let surface = layer_surface.wl_surface();
        buffer.attach_to(surface).expect("Failed to attach buffer");
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        surface.commit();
    }

    fn update(&mut self, dt: f32) {
        self.time += dt;
        self.visualizer.update(dt);
    }
}

/// Render visualizer to an ARGB8888 buffer
fn render_to_buffer(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    frequencies: &[f32],
    intensity: f32,
    track_title: &Option<String>,
    track_artist: &Option<String>,
    color_scheme: &ColorScheme,
    bar_width: usize,
    bar_spacing: usize,
    time: f32,
) {
    // Clear to transparent/black
    for pixel in canvas.chunks_exact_mut(4) {
        pixel[0] = 0; // B
        pixel[1] = 0; // G
        pixel[2] = 0; // R
        pixel[3] = 0; // A (transparent)
    }

    // Render bars
    render_bars(canvas, width, height, frequencies, color_scheme, bar_width, bar_spacing);

    // Render text
    render_text(canvas, width, height, track_title, track_artist, color_scheme, intensity, time);
}

fn render_bars(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    frequencies: &[f32],
    color_scheme: &ColorScheme,
    bar_width: usize,
    bar_spacing: usize,
) {
    if frequencies.is_empty() {
        return;
    }

    let bar_count = frequencies.len().min(width);
    let text_height = 60; // Reserve space for text
    let bars_height = height.saturating_sub(text_height);

    if bars_height == 0 {
        return;
    }

    let slot_width = bar_width + bar_spacing;

    // Calculate how many bars fit
    let max_bars = width / slot_width.max(1);
    let displayable = max_bars.min(bar_count);

    if displayable == 0 {
        return;
    }

    // Center the bars
    let total_width = displayable * slot_width;
    let start_x = (width.saturating_sub(total_width)) / 2;

    for i in 0..displayable {
        let freq_idx = (i * frequencies.len()) / displayable;
        let magnitude = frequencies[freq_idx];

        let bar_height = (magnitude * bars_height as f32) as usize;
        let x_start = start_x + i * slot_width;
        let position = i as f32 / displayable as f32;

        // Draw bar from bottom up
        for y_offset in 0..bar_height.min(bars_height) {
            let y = bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / bars_height as f32;
            let (r, g, b) = color_scheme.get_color(position, intensity);

            for bx in 0..bar_width {
                let x = x_start + bx;
                if x < width {
                    let idx = (y * width + x) * 4;
                    if idx + 3 < canvas.len() {
                        canvas[idx] = b;     // B
                        canvas[idx + 1] = g; // G
                        canvas[idx + 2] = r; // R
                        canvas[idx + 3] = 255; // A
                    }
                }
            }
        }
    }
}

fn render_text(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    track_title: &Option<String>,
    track_artist: &Option<String>,
    color_scheme: &ColorScheme,
    intensity: f32,
    time: f32,
) {
    let text_area_height = 60;
    let text_y = height.saturating_sub(text_area_height);

    // Build display text
    let text = match (track_title, track_artist) {
        (Some(title), Some(artist)) => format!("{} - {}", title, artist),
        (Some(title), None) => title.clone(),
        (None, Some(artist)) => artist.clone(),
        (None, None) => "cavibe".to_string(),
    };

    // Scale factor for the bitmap font (2x for better visibility on high-res displays)
    let scale = 3;
    let char_width = 8 * scale;
    let char_height = 8 * scale;
    let char_spacing = 1 * scale; // Small gap between characters

    let text_width = text.len() * (char_width + char_spacing);
    let start_x = (width.saturating_sub(text_width)) / 2;
    let y = text_y + (text_area_height - char_height) / 2;

    // Get gradient colors for text
    let colors = color_scheme.get_text_gradient(text.len(), intensity, time);

    for (i, ch) in text.chars().enumerate() {
        let x = start_x + i * (char_width + char_spacing);
        let (r, g, b) = colors.get(i).copied().unwrap_or((255, 255, 255));

        render_char(canvas, width, height, x, y, ch, r, g, b, scale);
    }
}

/// Simple 8x8 bitmap font for basic text rendering
/// Each character is represented as 8 bytes, one per row
fn get_char_bitmap(ch: char) -> Option<[u8; 8]> {
    let ch = ch.to_ascii_uppercase();
    Some(match ch {
        'A' => [0x18, 0x24, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x00],
        'B' => [0x7C, 0x42, 0x7C, 0x42, 0x42, 0x42, 0x7C, 0x00],
        'C' => [0x3C, 0x42, 0x40, 0x40, 0x40, 0x42, 0x3C, 0x00],
        'D' => [0x78, 0x44, 0x42, 0x42, 0x42, 0x44, 0x78, 0x00],
        'E' => [0x7E, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x7E, 0x00],
        'F' => [0x7E, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x40, 0x00],
        'G' => [0x3C, 0x42, 0x40, 0x4E, 0x42, 0x42, 0x3C, 0x00],
        'H' => [0x42, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x00],
        'I' => [0x3E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x3E, 0x00],
        'J' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x44, 0x38, 0x00],
        'K' => [0x42, 0x44, 0x78, 0x48, 0x44, 0x42, 0x42, 0x00],
        'L' => [0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7E, 0x00],
        'M' => [0x42, 0x66, 0x5A, 0x42, 0x42, 0x42, 0x42, 0x00],
        'N' => [0x42, 0x62, 0x52, 0x4A, 0x46, 0x42, 0x42, 0x00],
        'O' => [0x3C, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'P' => [0x7C, 0x42, 0x42, 0x7C, 0x40, 0x40, 0x40, 0x00],
        'Q' => [0x3C, 0x42, 0x42, 0x42, 0x4A, 0x44, 0x3A, 0x00],
        'R' => [0x7C, 0x42, 0x42, 0x7C, 0x48, 0x44, 0x42, 0x00],
        'S' => [0x3C, 0x42, 0x30, 0x0C, 0x02, 0x42, 0x3C, 0x00],
        'T' => [0x7F, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00],
        'U' => [0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'V' => [0x42, 0x42, 0x42, 0x42, 0x24, 0x24, 0x18, 0x00],
        'W' => [0x42, 0x42, 0x42, 0x5A, 0x5A, 0x66, 0x42, 0x00],
        'X' => [0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x42, 0x00],
        'Y' => [0x41, 0x22, 0x14, 0x08, 0x08, 0x08, 0x08, 0x00],
        'Z' => [0x7E, 0x04, 0x08, 0x10, 0x20, 0x40, 0x7E, 0x00],
        '0' => [0x3C, 0x42, 0x46, 0x5A, 0x62, 0x42, 0x3C, 0x00],
        '1' => [0x08, 0x18, 0x28, 0x08, 0x08, 0x08, 0x3E, 0x00],
        '2' => [0x3C, 0x42, 0x02, 0x0C, 0x30, 0x40, 0x7E, 0x00],
        '3' => [0x3C, 0x42, 0x02, 0x1C, 0x02, 0x42, 0x3C, 0x00],
        '4' => [0x04, 0x0C, 0x14, 0x24, 0x7E, 0x04, 0x04, 0x00],
        '5' => [0x7E, 0x40, 0x7C, 0x02, 0x02, 0x42, 0x3C, 0x00],
        '6' => [0x1C, 0x20, 0x40, 0x7C, 0x42, 0x42, 0x3C, 0x00],
        '7' => [0x7E, 0x02, 0x04, 0x08, 0x10, 0x10, 0x10, 0x00],
        '8' => [0x3C, 0x42, 0x42, 0x3C, 0x42, 0x42, 0x3C, 0x00],
        '9' => [0x3C, 0x42, 0x42, 0x3E, 0x02, 0x04, 0x38, 0x00],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x08, 0x10],
        '!' => [0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x08, 0x00],
        '?' => [0x3C, 0x42, 0x02, 0x0C, 0x10, 0x00, 0x10, 0x00],
        ':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        '\'' => [0x08, 0x08, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00],
        '"' => [0x24, 0x24, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00],
        '(' => [0x04, 0x08, 0x10, 0x10, 0x10, 0x08, 0x04, 0x00],
        ')' => [0x20, 0x10, 0x08, 0x08, 0x08, 0x10, 0x20, 0x00],
        '&' => [0x30, 0x48, 0x30, 0x50, 0x4A, 0x44, 0x3A, 0x00],
        _ => return None,
    })
}

fn render_char(canvas: &mut [u8], width: usize, height: usize, x: usize, y: usize, ch: char, r: u8, g: u8, b: u8, scale: usize) {
    let bitmap = match get_char_bitmap(ch) {
        Some(b) => b,
        None => return,
    };

    for (row_idx, &row) in bitmap.iter().enumerate() {
        for col in 0..8 {
            if (row >> (7 - col)) & 1 == 1 {
                // Draw scaled pixel
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row_idx * scale + sy;
                        if px < width && py < height {
                            let idx = (py * width + px) * 4;
                            if idx + 3 < canvas.len() {
                                canvas[idx] = b;
                                canvas[idx + 1] = g;
                                canvas[idx + 2] = r;
                                canvas[idx + 3] = 255;
                            }
                        }
                    }
                }
            }
        }
    }
}

// Implement required traits for smithay-client-toolkit

impl CompositorHandler for WallpaperState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WallpaperState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for WallpaperState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.running = false;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (width, height) = configure.new_size;

        // If compositor suggests 0 dimensions, use fallback
        self.width = if width > 0 { width } else { 1920 };
        self.height = if height > 0 { height } else { 1080 };

        info!("Layer surface configured: {}x{}", self.width, self.height);

        // Recreate the pool for new size
        self.pool = None;
        self.configured = true;

        // Request frame callback for animation
        layer.wl_surface().frame(qh, layer.wl_surface().clone());

        // Initial draw
        self.draw(qh);
    }
}

impl ShmHandler for WallpaperState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for WallpaperState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

delegate_compositor!(WallpaperState);
delegate_output!(WallpaperState);
delegate_layer!(WallpaperState);
delegate_shm!(WallpaperState);
delegate_registry!(WallpaperState);

/// Run the Wayland layer-shell wallpaper mode
pub async fn run(config: Config) -> Result<()> {
    info!("Starting Wayland layer-shell wallpaper mode");

    // Connect to Wayland
    let conn = Connection::connect_to_env().context("Failed to connect to Wayland display")?;

    let (globals, mut event_queue) =
        registry_queue_init(&conn).context("Failed to initialize Wayland registry")?;

    let qh = event_queue.handle();

    // Initialize required globals
    let compositor_state =
        CompositorState::bind(&globals, &qh).context("wl_compositor not available")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context(
        "wlr-layer-shell not available. Your compositor may not support this protocol.",
    )?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm not available")?;
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    // Create state
    let mut state = WallpaperState::new(
        registry_state,
        output_state,
        compositor_state,
        shm,
        layer_shell,
        config.clone(),
    );

    // Create the layer surface
    state.create_layer_surface(&qh);

    // Do a roundtrip to ensure the layer surface gets configured
    event_queue
        .roundtrip(&mut state)
        .context("Failed initial Wayland roundtrip")?;

    // Start audio capture
    let (_audio_capture, audio_rx) = audio::create_audio_pipeline(
        config.visualizer.bars,
        config.audio.smoothing,
        config.audio.sensitivity,
    )?;

    // Start metadata watcher
    let metadata_rx = metadata::start_watcher();

    info!("Wayland wallpaper mode running. Press Ctrl+C to stop.");

    let target_fps = Duration::from_secs_f64(1.0 / 60.0);

    // Main loop
    while state.running {
        let frame_start = Instant::now();

        // Update audio and metadata
        state.audio_data = audio_rx.borrow().clone();
        state.track_info = metadata_rx.borrow().clone();

        // Calculate delta time
        let dt = state.last_frame.elapsed().as_secs_f32();
        state.last_frame = Instant::now();
        state.update(dt);

        // Flush outgoing requests
        if event_queue.flush().is_err() {
            // Connection lost
            break;
        }

        // Read and dispatch incoming events (non-blocking)
        if let Some(guard) = event_queue.prepare_read() {
            // Non-blocking read
            let _ = guard.read();
        }
        event_queue
            .dispatch_pending(&mut state)
            .context("Wayland dispatch failed")?;

        // Request next frame and draw if configured
        if state.configured {
            if let Some(ref layer_surface) = state.layer_surface {
                layer_surface.wl_surface().frame(&qh, layer_surface.wl_surface().clone());
                state.draw(&qh);
            }
        }

        // Frame rate limiting
        let elapsed = frame_start.elapsed();
        if elapsed < target_fps {
            std::thread::sleep(target_fps - elapsed);
        }
    }

    info!("Wayland wallpaper mode stopped");
    Ok(())
}
