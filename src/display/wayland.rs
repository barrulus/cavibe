//! Wayland layer-shell backend for wallpaper mode
//!
//! Uses wlr-layer-shell protocol to render the visualizer as a desktop background
//! on Wayland compositors like Niri, Sway, Hyprland, etc.
//!
//! Supports multiple monitors: one LayerSurface per output. In "clone" mode all
//! monitors show the same visualization; in "independent" mode per-monitor
//! overrides for color scheme, style, and opacity are applied.

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
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, Proxy, QueueHandle,
};

use crate::audio::{self, AudioCapture, AudioData};
use crate::color::ColorScheme;
use crate::config::{Config, FontStyle, MultiMonitorMode, TextAlignment, TextAnimation, TextConfig, TextPosition, WallpaperAnchor};
use crate::ipc::IpcCommand;
use crate::metadata::{self, TrackInfo};
use crate::visualizer::{VisualizerState, VISUALIZER_STYLES};
use tokio::sync::{mpsc, watch};

impl WallpaperAnchor {
    /// Convert to layer-shell Anchor bitflags
    pub fn to_layer_shell_anchor(self) -> Anchor {
        match self {
            WallpaperAnchor::TopLeft => Anchor::TOP | Anchor::LEFT,
            WallpaperAnchor::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            WallpaperAnchor::TopRight => Anchor::TOP | Anchor::RIGHT,
            WallpaperAnchor::Left => Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT,
            WallpaperAnchor::Center => Anchor::empty(),
            WallpaperAnchor::Right => Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT,
            WallpaperAnchor::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
            WallpaperAnchor::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            WallpaperAnchor::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
            WallpaperAnchor::Fullscreen => Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        }
    }
}

/// Options for rendering the visualizer
struct RenderOptions<'a> {
    // Visualizer settings
    style: usize,
    bar_width: usize,
    bar_spacing: usize,
    mirror: bool,
    reverse_mirror: bool,
    opacity: f32,
    color_scheme: &'a ColorScheme,
    waveform: &'a [f32],
    spectrogram_history: &'a [Vec<f32>],

    // Text settings
    text_config: &'a TextConfig,
}

/// An audio capture pipeline with its receiver
struct AudioPipeline {
    _capture: AudioCapture,
    rx: watch::Receiver<Arc<AudioData>>,
}

/// Per-output surface state
struct OutputSurface {
    output_name: Option<String>,
    layer_surface: LayerSurface,
    pool: Option<SlotPool>,
    width: u32,
    height: u32,
    configured: bool,
    screen_width: u32,
    screen_height: u32,
    explicit_size: Option<(u32, u32)>,
    // Per-monitor overrides (None = use global)
    color_scheme_override: Option<ColorScheme>,
    style_override: Option<usize>,
    opacity_override: Option<f32>,
    // Per-monitor audio
    audio_source_key: Option<String>, // Key into audio_pipelines map
    audio_data: Arc<AudioData>,       // Cached per-surface audio data
    // Spectrogram history (rolling buffer of frequency snapshots)
    spectrogram_history: Vec<Vec<f32>>,
}

/// Wayland layer-shell wallpaper renderer with multi-monitor support
struct WallpaperState {
    // Wayland state
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,

    // Per-output surfaces, keyed by wl_output ObjectId
    surfaces: HashMap<wayland_client::backend::ObjectId, OutputSurface>,

    // Shared visualizer state
    visualizer: VisualizerState,
    color_scheme: ColorScheme,
    audio_pipelines: HashMap<Option<String>, AudioPipeline>,
    track_info: Arc<TrackInfo>,
    last_frame: Instant,
    time: f32,

    // Control
    running: bool,
    visible: bool,
    active: bool, // true when audio is playing and frames are being rendered
    config: Config,

    // IPC
    ipc_rx: mpsc::Receiver<IpcCommand>,
}

impl WallpaperState {
    fn new(
        registry_state: RegistryState,
        output_state: OutputState,
        compositor_state: CompositorState,
        shm: Shm,
        layer_shell: LayerShell,
        config: Config,
        ipc_rx: mpsc::Receiver<IpcCommand>,
    ) -> Self {
        let visualizer = VisualizerState::new(config.visualizer.clone(), config.text.clone());
        let color_scheme = config.visualizer.color_scheme;

        Self {
            registry_state,
            output_state,
            compositor_state,
            shm,
            layer_shell,
            surfaces: HashMap::new(),
            visualizer,
            color_scheme,
            audio_pipelines: HashMap::new(),
            track_info: Arc::new(TrackInfo::default()),
            last_frame: Instant::now(),
            time: 0.0,
            running: true,
            visible: true,
            active: true,
            config,
            ipc_rx,
        }
    }

    /// Check if an output should get a surface (not filtered out)
    fn should_create_surface(&self, output_name: &Option<String>) -> bool {
        // Check CLI output filter
        if let Some(ref allowed) = self.config.wallpaper.outputs {
            if let Some(ref name) = output_name {
                if !allowed.iter().any(|a| a == name) {
                    return false;
                }
            } else {
                // No name available, skip if filter is active
                return false;
            }
        }

        // Check per-monitor config for disabled outputs
        if let Some(ref name) = output_name {
            for monitor_cfg in &self.config.wallpaper.monitors {
                if monitor_cfg.output == *name && !monitor_cfg.enabled {
                    return false;
                }
            }
        }

        true
    }

    /// Get per-monitor overrides from config
    fn get_monitor_overrides(&self, output_name: &Option<String>) -> (Option<ColorScheme>, Option<usize>, Option<f32>, Option<String>) {
        if self.config.wallpaper.multi_monitor != MultiMonitorMode::Independent {
            return (None, None, None, None);
        }

        if let Some(ref name) = output_name {
            for monitor_cfg in &self.config.wallpaper.monitors {
                if monitor_cfg.output == *name {
                    let style_idx = monitor_cfg.style.as_ref().and_then(|s| {
                        VISUALIZER_STYLES.iter().position(|vs| vs.name().eq_ignore_ascii_case(s))
                    });
                    return (monitor_cfg.color_scheme, style_idx, monitor_cfg.opacity, monitor_cfg.audio_source.clone());
                }
            }
        }

        (None, None, None, None)
    }

    /// Create a layer surface for a specific output
    fn create_surface_for_output(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        let output_info = self.output_state.info(output);
        let output_name = output_info.as_ref().and_then(|i| i.name.clone());

        // Check if this output should be skipped
        if !self.should_create_surface(&output_name) {
            info!("Skipping output {:?} (filtered)", output_name);
            return;
        }

        // Get screen dimensions for this output
        let (screen_w, screen_h) = output_info
            .as_ref()
            .and_then(|i| i.logical_size)
            .map(|(w, h)| (w as u32, h as u32))
            .unwrap_or((1920, 1080));

        info!("Creating layer surface for output {:?} ({}x{})", output_name, screen_w, screen_h);

        let wl_surface = self.compositor_state.create_surface(qh);

        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            wl_surface,
            Layer::Background,
            Some("cavibe-wallpaper"),
            Some(output),
        );

        // Configure anchor based on wallpaper config
        let anchor = self.config.wallpaper.anchor.to_layer_shell_anchor();
        layer_surface.set_anchor(anchor);

        // Apply margins
        let (top, right, bottom, left) = self.config.wallpaper.effective_margins();
        layer_surface.set_margin(top, right, bottom, left);

        // Set explicit size if configured (not fullscreen)
        let explicit_size = if self.config.wallpaper.anchor != WallpaperAnchor::Fullscreen {
            if let Some((w, h)) = self.config.wallpaper.get_size(screen_w, screen_h) {
                layer_surface.set_size(w, h);
                info!("Explicit size set to: {}x{}", w, h);
                Some((w, h))
            } else {
                None
            }
        } else {
            None
        };

        layer_surface.set_exclusive_zone(-1); // Don't reserve space
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

        // Commit to get the configure event
        layer_surface.commit();

        // Get per-monitor overrides
        let (color_override, style_override, opacity_override, audio_source) = self.get_monitor_overrides(&output_name);

        let surface = OutputSurface {
            output_name,
            layer_surface,
            pool: None,
            width: 0,
            height: 0,
            configured: false,
            screen_width: screen_w,
            screen_height: screen_h,
            explicit_size,
            color_scheme_override: color_override,
            style_override,
            opacity_override,
            audio_source_key: audio_source,
            audio_data: Arc::new(AudioData::default()),
            spectrogram_history: Vec::new(),
        };

        self.surfaces.insert(output.id(), surface);
        info!("Layer surface created for output, waiting for configure event...");
    }

    /// Create surfaces for all currently known outputs
    fn create_surfaces_for_all_outputs(&mut self, qh: &QueueHandle<Self>) {
        let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        for output in outputs {
            self.create_surface_for_output(qh, &output);
        }
    }

    /// Check if any surface is configured
    fn any_configured(&self) -> bool {
        self.surfaces.values().any(|s| s.configured)
    }

    /// Draw a specific surface by wl_surface
    fn draw_surface(&mut self, surface_wl: &wl_surface::WlSurface) {
        // Find the OutputSurface matching this wl_surface
        let output_id = self.surfaces.iter()
            .find(|(_, s)| s.layer_surface.wl_surface() == surface_wl)
            .map(|(id, _)| id.clone());

        let output_id = match output_id {
            Some(id) => id,
            None => return,
        };

        let surface = match self.surfaces.get_mut(&output_id) {
            Some(s) => s,
            None => return,
        };

        if !surface.configured || surface.width == 0 || surface.height == 0 {
            return;
        }

        if !self.visible {
            // Render a fully transparent frame
            if surface.pool.is_none() {
                let pool = SlotPool::new(
                    (surface.width * surface.height * 4) as usize,
                    &self.shm,
                )
                .expect("Failed to create slot pool");
                surface.pool = Some(pool);
            }
            let pool = surface.pool.as_mut().unwrap();
            let (buffer, canvas) = pool
                .create_buffer(
                    surface.width as i32,
                    surface.height as i32,
                    (surface.width * 4) as i32,
                    wl_shm::Format::Argb8888,
                )
                .expect("Failed to create buffer");
            canvas.fill(0);
            let wl_surf = surface.layer_surface.wl_surface();
            buffer.attach_to(wl_surf).expect("Failed to attach buffer");
            wl_surf.damage_buffer(0, 0, surface.width as i32, surface.height as i32);
            wl_surf.commit();
            return;
        }

        // Ensure we have a pool
        if surface.pool.is_none() {
            let pool = SlotPool::new(
                (surface.width * surface.height * 4) as usize,
                &self.shm,
            )
            .expect("Failed to create slot pool");
            surface.pool = Some(pool);
        }

        let pool = surface.pool.as_mut().unwrap();

        // Get a buffer from the pool
        let (buffer, canvas) = pool
            .create_buffer(
                surface.width as i32,
                surface.height as i32,
                (surface.width * 4) as i32,
                wl_shm::Format::Argb8888,
            )
            .expect("Failed to create buffer");

        // Resolve per-surface overrides
        let color_scheme = surface.color_scheme_override.unwrap_or(self.color_scheme);
        let style = surface.style_override.unwrap_or(self.visualizer.current_style);
        let opacity = surface.opacity_override.unwrap_or(self.config.visualizer.opacity);

        // Render the visualizer to the canvas
        let width = surface.width as usize;
        let height = surface.height as usize;
        let frequencies = surface.audio_data.frequencies.clone();
        let waveform = surface.audio_data.waveform.clone();
        let intensity = surface.audio_data.intensity;
        let track_title = self.track_info.title.clone();
        let track_artist = self.track_info.artist.clone();
        let pixel_scale = 8;
        let bar_width = (self.config.visualizer.bar_width as usize) * pixel_scale;
        let bar_spacing = (self.config.visualizer.bar_spacing as usize) * pixel_scale;
        let time = self.time;

        // Update spectrogram history for this surface
        surface.spectrogram_history.push(frequencies.clone());
        if surface.spectrogram_history.len() > height {
            let excess = surface.spectrogram_history.len() - height;
            surface.spectrogram_history.drain(..excess);
        }

        let render_opts = RenderOptions {
            style,
            bar_width,
            bar_spacing,
            mirror: self.config.visualizer.mirror,
            reverse_mirror: self.config.visualizer.reverse_mirror,
            opacity,
            color_scheme: &color_scheme,
            waveform: &waveform,
            spectrogram_history: &surface.spectrogram_history,
            text_config: &self.config.text,
        };

        let mut cvs = Canvas { data: canvas, width, height };
        let frame_data = FrameData {
            frequencies: &frequencies,
            intensity,
            track_title: &track_title,
            track_artist: &track_artist,
            time,
        };
        render_to_buffer(&mut cvs, &frame_data, &render_opts);

        // Attach and commit
        let wl_surf = surface.layer_surface.wl_surface();
        buffer.attach_to(wl_surf).expect("Failed to attach buffer");
        wl_surf.damage_buffer(0, 0, surface.width as i32, surface.height as i32);
        wl_surf.commit();
    }

    /// Get a list of connected monitor names and their status
    pub fn list_monitors(&self) -> Vec<(String, bool)> {
        let mut result = Vec::new();
        for output in self.output_state.outputs() {
            let name = self.output_state.info(&output)
                .and_then(|i| i.name.clone())
                .unwrap_or_else(|| format!("unknown-{}", output.id()));
            let has_surface = self.surfaces.contains_key(&output.id());
            result.push((name, has_surface));
        }
        result
    }

    fn update(&mut self, dt: f32) {
        self.time += dt;
        self.visualizer.update(dt);
    }
}

/// Mutable pixel buffer with dimensions
struct Canvas<'a> {
    data: &'a mut [u8],
    width: usize,
    height: usize,
}

impl Canvas<'_> {
    /// Write a pixel with pre-multiplied alpha
    #[inline]
    fn put_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, opacity: f32) {
        let idx = (y * self.width + x) * 4;
        if idx + 3 < self.data.len() {
            self.data[idx] = (b as f32 * opacity) as u8;
            self.data[idx + 1] = (g as f32 * opacity) as u8;
            self.data[idx + 2] = (r as f32 * opacity) as u8;
            self.data[idx + 3] = (opacity * 255.0) as u8;
        }
    }
}

/// Per-frame data for text rendering
struct FrameData<'a> {
    frequencies: &'a [f32],
    intensity: f32,
    track_title: &'a Option<String>,
    track_artist: &'a Option<String>,
    time: f32,
}

/// Render visualizer to an ARGB8888 buffer
fn render_to_buffer(canvas: &mut Canvas, frame: &FrameData, opts: &RenderOptions) {
    // Clear to fully transparent (let wallpaper show through)
    canvas.data.fill(0);

    // Render bars
    render_bars(canvas, frame.frequencies, opts);

    // Render text
    render_text(canvas, frame, opts);
}

/// Compute bar layout shared by all styles: text area, slot dimensions, frequency mapping
struct BarLayout {
    bars_y_start: usize,
    bars_height: usize,
    start_x: usize,
    slot_width: usize,
    displayable: usize,
    render_frequencies: Vec<f32>,
}

fn compute_bar_layout(
    width: usize,
    height: usize,
    frequencies: &[f32],
    opts: &RenderOptions,
) -> Option<BarLayout> {
    if frequencies.is_empty() {
        return None;
    }

    let bar_count = frequencies.len().min(width);
    let text_height = if opts.text_config.show_title || opts.text_config.show_artist {
        60 + opts.text_config.margin_top as usize + opts.text_config.margin_bottom as usize
    } else {
        0
    };

    let (bars_y_start, bars_height) = match opts.text_config.position {
        TextPosition::Top => (text_height, height.saturating_sub(text_height)),
        TextPosition::Bottom => (0, height.saturating_sub(text_height)),
        TextPosition::Center | TextPosition::Coordinates { .. } => (0, height),
    };

    if bars_height == 0 {
        return None;
    }

    let slot_width = opts.bar_width + opts.bar_spacing;
    let max_bars = width / slot_width.max(1);
    let displayable = max_bars.min(bar_count);

    if displayable == 0 {
        return None;
    }

    let total_width = displayable * slot_width;
    let start_x = (width.saturating_sub(total_width)) / 2;

    let render_frequencies: Vec<f32> = match (opts.mirror, opts.reverse_mirror) {
        (true, true) => {
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            for i in 0..half {
                let freq_idx = ((half - 1 - i) * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = (i * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (true, false) => {
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            for i in 0..half {
                let freq_idx = (i * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            for i in 0..displayable - half {
                let freq_idx = ((displayable - half - 1 - i) * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (false, true) => {
            (0..displayable)
                .map(|i| {
                    let freq_idx = ((displayable - 1 - i) * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
        (false, false) => {
            (0..displayable)
                .map(|i| {
                    let freq_idx = (i * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
    };

    Some(BarLayout { bars_y_start, bars_height, start_x, slot_width, displayable, render_frequencies })
}

fn render_bars(
    canvas: &mut Canvas,
    frequencies: &[f32],
    opts: &RenderOptions,
) {
    let layout = match compute_bar_layout(canvas.width, canvas.height, frequencies, opts) {
        Some(l) => l,
        None => return,
    };

    match opts.style {
        1 => render_bars_mirrored(canvas, &layout, opts),
        2 => render_bars_wave(canvas, &layout, opts),
        3 => render_bars_dots(canvas, &layout, opts),
        4 => render_bars_blocks(canvas, &layout, opts),
        5 => render_bars_oscilloscope(canvas, &layout, opts),
        6 => render_bars_spectrogram(canvas, &layout, opts),
        7 => render_bars_radial(canvas, &layout, opts),
        _ => render_bars_classic(canvas, &layout, opts),
    }
}

/// Style 0: Classic vertical bars from bottom
fn render_bars_classic(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let bar_height = (magnitude * layout.bars_height as f32) as usize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for y_offset in 0..bar_height.min(layout.bars_height) {
            let y = layout.bars_y_start + layout.bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / layout.bars_height as f32;
            let (r, g, b) = opts.color_scheme.get_color(position, intensity);

            for bx in 0..opts.bar_width {
                let x = x_start + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, r, g, b, opts.opacity);
                }
            }
        }
    }
}

/// Style 1: Mirrored bars growing from center
fn render_bars_mirrored(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let center_y = layout.bars_y_start + layout.bars_height / 2;

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let half_height = (magnitude * layout.bars_height as f32 / 2.0) as usize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for y_offset in 0..half_height.min(layout.bars_height / 2) {
            let intensity = y_offset as f32 / (layout.bars_height as f32 / 2.0);
            let (r, g, b) = opts.color_scheme.get_color(position, intensity);

            // Upper half
            let y_up = center_y.saturating_sub(y_offset);
            if y_up >= layout.bars_y_start {
                for bx in 0..opts.bar_width {
                    let x = x_start + bx;
                    if x < canvas.width && y_up < canvas.height {
                        canvas.put_pixel(x, y_up, r, g, b, opts.opacity);
                    }
                }
            }

            // Lower half
            let y_down = center_y + y_offset;
            if y_down < layout.bars_y_start + layout.bars_height {
                for bx in 0..opts.bar_width {
                    let x = x_start + bx;
                    if x < canvas.width && y_down < canvas.height {
                        canvas.put_pixel(x, y_down, r, g, b, opts.opacity);
                    }
                }
            }
        }
    }
}

/// Style 2: Wave centered on middle row
fn render_bars_wave(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let center_y = layout.bars_y_start + layout.bars_height / 2;
    // Use a narrower bar for wave style
    let wave_width = (opts.bar_width / 3).max(1);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let wave_height = (magnitude * layout.bars_height as f32 / 2.0) as isize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for offset in -wave_height..=wave_height {
            let y = (center_y as isize + offset) as usize;
            if y >= layout.bars_y_start && y < layout.bars_y_start + layout.bars_height && y < canvas.height {
                let intensity = 1.0 - (offset.unsigned_abs() as f32 / wave_height.max(1) as f32);
                let (r, g, b) = opts.color_scheme.get_color(position, intensity);

                for bx in 0..wave_width {
                    let x = x_start + bx;
                    if x < canvas.width {
                        canvas.put_pixel(x, y, r, g, b, opts.opacity * intensity);
                    }
                }
            }
        }
    }
}

/// Style 3: Dots at peak with trailing dots below
fn render_bars_dots(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let dot_radius = (opts.bar_width / 3).max(2);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let peak_y = layout.bars_y_start + layout.bars_height - 1
            - (magnitude * (layout.bars_height - 1) as f32) as usize;
        let x_center = layout.start_x + i * layout.slot_width + opts.bar_width / 2;
        let position = i as f32 / layout.displayable as f32;
        let (r, g, b) = opts.color_scheme.get_color(position, magnitude);

        // Draw dot (filled circle)
        let r2 = (dot_radius * dot_radius) as isize;
        for dy in -(dot_radius as isize)..=(dot_radius as isize) {
            for dx in -(dot_radius as isize)..=(dot_radius as isize) {
                if dx * dx + dy * dy <= r2 {
                    let x = (x_center as isize + dx) as usize;
                    let y = (peak_y as isize + dy) as usize;
                    if x < canvas.width && y >= layout.bars_y_start && y < layout.bars_y_start + layout.bars_height && y < canvas.height {
                        canvas.put_pixel(x, y, r, g, b, opts.opacity);
                    }
                }
            }
        }

        // Draw trail below dot
        let trail_width = (opts.bar_width / 4).max(1);
        let trail_start = peak_y + dot_radius + 1;
        let trail_end = layout.bars_y_start + layout.bars_height;
        for y in trail_start..trail_end {
            let trail_intensity = 1.0 - ((y - trail_start) as f32 / (layout.bars_height as f32 / 2.0));
            if trail_intensity <= 0.0 {
                break;
            }
            let (tr, tg, tb) = opts.color_scheme.get_color(position, trail_intensity * magnitude);
            for bx in 0..trail_width {
                let x = x_center - trail_width / 2 + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, tr, tg, tb, opts.opacity * trail_intensity);
                }
            }
        }
    }
}

/// Style 4: Blocks with gradient fade at top edge
fn render_bars_blocks(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let fade_height = (opts.bar_width / 2).max(2);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let bar_height_f = magnitude * layout.bars_height as f32;
        let bar_height = bar_height_f as usize;
        let fractional = bar_height_f - bar_height as f32;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        // Draw solid portion
        for y_offset in 0..bar_height.min(layout.bars_height) {
            let y = layout.bars_y_start + layout.bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / layout.bars_height as f32;
            let (r, g, b) = opts.color_scheme.get_color(position, intensity);

            for bx in 0..opts.bar_width {
                let x = x_start + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, r, g, b, opts.opacity);
                }
            }
        }

        // Draw gradient fade at top edge
        if bar_height < layout.bars_height {
            let top_y = layout.bars_y_start + layout.bars_height - 1 - bar_height;
            let intensity = bar_height as f32 / layout.bars_height as f32;
            let (r, g, b) = opts.color_scheme.get_color(position, intensity);

            for fy in 0..fade_height.min(top_y.saturating_sub(layout.bars_y_start)) {
                let y = top_y - fy;
                let fade = fractional * (1.0 - fy as f32 / fade_height as f32);
                if y < canvas.height {
                    for bx in 0..opts.bar_width {
                        let x = x_start + bx;
                        if x < canvas.width {
                            canvas.put_pixel(x, y, r, g, b, opts.opacity * fade);
                        }
                    }
                }
            }
        }
    }
}

/// Style 5: Oscilloscope — raw waveform as a continuous line
fn render_bars_oscilloscope(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    if opts.waveform.is_empty() {
        return;
    }

    let num_samples = opts.waveform.len();
    let center_y = layout.bars_y_start + layout.bars_height / 2;
    let half_height = layout.bars_height as f32 / 2.0;
    // Line thickness in pixels
    let thickness = (opts.bar_width / 4).max(1);

    let mut prev_y: Option<usize> = None;

    for x in 0..canvas.width {
        // Map pixel x to sample index
        let sample_idx = (x * num_samples) / canvas.width;
        let sample = opts.waveform[sample_idx.min(num_samples - 1)];

        // Map sample (-1..1) to pixel y within bar area
        let y = ((center_y as f32 - sample * half_height) as usize)
            .max(layout.bars_y_start)
            .min(layout.bars_y_start + layout.bars_height - 1);

        let position = x as f32 / canvas.width as f32;
        let intensity = sample.abs().min(1.0);
        let (r, g, b) = opts.color_scheme.get_color(position, intensity.max(0.3));

        // Fill vertically between prev_y and y for smooth connections
        let y_min;
        let y_max;
        if let Some(py) = prev_y {
            y_min = py.min(y);
            y_max = py.max(y);
        } else {
            y_min = y;
            y_max = y;
        }

        for fill_y in y_min..=y_max {
            for t in 0..thickness {
                let px = x;
                let py = fill_y + t;
                if px < canvas.width && py >= layout.bars_y_start && py < layout.bars_y_start + layout.bars_height && py < canvas.height {
                    canvas.put_pixel(px, py, r, g, b, opts.opacity);
                }
            }
        }

        prev_y = Some(y);
    }
}

/// Style 6: Spectrogram — scrolling 2D heatmap (X=frequency, Y=time)
fn render_bars_spectrogram(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let history = opts.spectrogram_history;
    if history.is_empty() {
        return;
    }

    let num_rows = history.len().min(layout.bars_height);

    for (row_idx, slice) in history.iter().rev().take(num_rows).enumerate() {
        // row_idx 0 = newest (bottom), so draw from bottom up
        let y = layout.bars_y_start + layout.bars_height - 1 - row_idx;
        if y >= layout.bars_y_start + layout.bars_height {
            continue;
        }

        let num_freqs = slice.len();
        if num_freqs == 0 {
            continue;
        }

        for x in 0..canvas.width {
            let freq_idx = (x * num_freqs) / canvas.width;
            let magnitude = slice[freq_idx.min(num_freqs - 1)];
            let position = x as f32 / canvas.width as f32;
            let (r, g, b) = opts.color_scheme.get_color(position, magnitude);
            canvas.put_pixel(x, y, r, g, b, opts.opacity * magnitude.max(0.05));
        }
    }
}

/// Style 7: Radial — frequency bars radiating outward from a circle
fn render_bars_radial(canvas: &mut Canvas, layout: &BarLayout, opts: &RenderOptions) {
    let cx = canvas.width as f32 / 2.0;
    let cy = (layout.bars_y_start as f32) + layout.bars_height as f32 / 2.0;
    let half_dim = (canvas.width.min(layout.bars_height) as f32) / 2.0;
    let base_radius = half_dim * 0.35;
    let max_radius = half_dim * 0.95;
    let thickness = (opts.bar_width / 3).max(2);

    let bar_count = layout.render_frequencies.len();
    if bar_count == 0 {
        return;
    }

    // Draw base circle
    let circle_steps = (base_radius * std::f32::consts::TAU).ceil() as usize;
    for step in 0..circle_steps {
        let angle = (step as f32 / circle_steps as f32) * std::f32::consts::TAU;
        let px = (cx + angle.cos() * base_radius).round() as usize;
        let py = (cy + angle.sin() * base_radius).round() as usize;
        let position = (angle + std::f32::consts::FRAC_PI_2) / std::f32::consts::TAU;
        let position = position.rem_euclid(1.0);
        let (r, g, b) = opts.color_scheme.get_color(position, 0.3);
        for t in 0..thickness {
            let tx = px + t;
            if tx < canvas.width && py >= layout.bars_y_start && py < layout.bars_y_start + layout.bars_height && py < canvas.height {
                canvas.put_pixel(tx, py, r, g, b, opts.opacity * 0.5);
            }
        }
    }

    // Draw radial bars
    for i in 0..bar_count {
        let magnitude = layout.render_frequencies[i];
        if magnitude < 0.01 {
            continue;
        }
        // Angle: start at top (-PI/2), go clockwise
        let angle = -std::f32::consts::FRAC_PI_2
            + (i as f32 / bar_count as f32) * std::f32::consts::TAU;
        let bar_length = magnitude * (max_radius - base_radius);
        let position = i as f32 / bar_count as f32;

        // Draw line from base_radius to base_radius + bar_length
        let steps = (bar_length.ceil() as usize).max(1);
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        for s in 0..=steps {
            let r_dist = base_radius + (s as f32 / steps as f32) * bar_length;
            let px = (cx + cos_a * r_dist).round() as isize;
            let py_val = (cy + sin_a * r_dist).round() as isize;
            let intensity = (r_dist - base_radius) / (max_radius - base_radius);
            let (r, g, b) = opts.color_scheme.get_color(position, magnitude * 0.5 + intensity * 0.5);

            // Draw with thickness perpendicular to the radial direction
            for t in -(thickness as isize / 2)..=(thickness as isize / 2) {
                let tx = (px as f32 - sin_a * t as f32).round() as usize;
                let ty = (py_val as f32 + cos_a * t as f32).round() as usize;
                if tx < canvas.width && ty >= layout.bars_y_start && ty < layout.bars_y_start + layout.bars_height && ty < canvas.height {
                    canvas.put_pixel(tx, ty, r, g, b, opts.opacity);
                }
            }
        }
    }
}

fn render_text(canvas: &mut Canvas, frame: &FrameData, opts: &RenderOptions) {
    let text_config = opts.text_config;
    let width = canvas.width;
    let height = canvas.height;
    let track_title = frame.track_title;
    let track_artist = frame.track_artist;
    let intensity = frame.intensity;
    let time = frame.time;

    // Debug: log text config on first frame (time near 0)
    if time < 0.1 {
        info!(
            "Text config: position={:?}, alignment={:?}, font_style={:?}, animation={:?}, show_title={}, show_artist={}, margins=({},{},{})",
            text_config.position,
            text_config.alignment,
            text_config.font_style,
            text_config.animation_style,
            text_config.show_title,
            text_config.show_artist,
            text_config.margin_top,
            text_config.margin_bottom,
            text_config.margin_horizontal
        );
    }

    // Check if we should show text at all
    if !text_config.show_title && !text_config.show_artist {
        return;
    }

    // Build display text and track where title ends for color splitting
    let (text, title_len) = match (
        text_config.show_title,
        text_config.show_artist,
        track_title,
        track_artist,
    ) {
        (true, true, Some(title), Some(artist)) => {
            let combined = format!("{} - {}", title, artist);
            (combined, title.len())
        }
        (true, true, Some(title), None) => (title.clone(), title.len()),
        (true, true, None, Some(artist)) => (artist.clone(), 0),
        (true, false, Some(title), _) => (title.clone(), title.len()),
        (false, true, _, Some(artist)) => (artist.clone(), 0),
        _ => ("cavibe".to_string(), 6),
    };

    // Scale factor based on font style
    let scale = match text_config.font_style {
        FontStyle::Normal => 3,
        FontStyle::Bold => 4,
        FontStyle::Ascii => 2,
        FontStyle::Figlet => 5,
    };

    let char_width = 8 * scale;
    let char_height = 8 * scale;
    let char_spacing = match text_config.font_style {
        FontStyle::Bold => 2 * scale,
        _ => scale,
    };

    let text_area_height = char_height + 20;
    let margin_h = text_config.margin_horizontal as usize;

    // Calculate text Y position based on position setting
    let (base_text_y, coord_x_override) = match text_config.position {
        TextPosition::Top => (text_config.margin_top as usize, None),
        TextPosition::Bottom => (height.saturating_sub(text_area_height + text_config.margin_bottom as usize), None),
        TextPosition::Center => ((height.saturating_sub(char_height)) / 2, None),
        TextPosition::Coordinates { x, y } => (y.resolve(height), Some(x.resolve(width))),
    };

    let text_width = text.len() * (char_width + char_spacing);
    let available_width = width.saturating_sub(margin_h * 2);

    // Calculate base X position based on alignment (or coordinate override)
    let base_start_x = if let Some(cx) = coord_x_override {
        cx
    } else {
        match text_config.alignment {
            TextAlignment::Left => margin_h,
            TextAlignment::Center => margin_h + (available_width.saturating_sub(text_width)) / 2,
            TextAlignment::Right => margin_h + available_width.saturating_sub(text_width),
        }
    };

    // Apply scroll animation offset if text is wider than available space
    let scroll_offset = match text_config.animation_style {
        TextAnimation::Scroll if text_width > available_width => {
            let scroll_range = text_width - available_width + margin_h * 2;
            let scroll_speed = text_config.animation_speed * 30.0;
            let cycle_time = scroll_range as f32 / scroll_speed;
            let t = (time % (cycle_time * 2.0)) / cycle_time;
            let normalized = if t > 1.0 { 2.0 - t } else { t }; // Ping-pong
            (normalized * scroll_range as f32) as isize
        }
        _ => 0,
    };

    let y = base_text_y + (text_area_height.saturating_sub(char_height)) / 2;

    // Render background if configured
    if let Some(bg_color) = text_config.background_color {
        let bg_padding = 10;
        let bg_x_start = base_start_x.saturating_sub(bg_padding);
        let bg_x_end = (base_start_x + text_width + bg_padding).min(width);
        let bg_y_start = base_text_y.saturating_sub(bg_padding);
        let bg_y_end = (base_text_y + text_area_height + bg_padding).min(height);

        for py in bg_y_start..bg_y_end {
            for px in bg_x_start..bg_x_end {
                let idx = (py * width + px) * 4;
                if idx + 3 < canvas.data.len() {
                    canvas.data[idx] = (bg_color.b as f32 * opts.opacity) as u8;
                    canvas.data[idx + 1] = (bg_color.g as f32 * opts.opacity) as u8;
                    canvas.data[idx + 2] = (bg_color.r as f32 * opts.opacity) as u8;
                    canvas.data[idx + 3] = (opts.opacity * 255.0 * 0.8) as u8; // Slightly transparent
                }
            }
        }
    }

    // Get colors for text - support separate title/artist colors
    let colors: Vec<(u8, u8, u8)> = if text_config.use_color_scheme {
        // Use animated color scheme gradient
        opts.color_scheme.get_text_gradient(text.len(), intensity * text_config.pulse_intensity, time * text_config.animation_speed)
    } else {
        // Use custom colors with title/artist split
        let title_color = text_config.title_color.unwrap_or(crate::config::RgbColor { r: 255, g: 255, b: 255 });
        let artist_color = text_config.artist_color.unwrap_or(crate::config::RgbColor { r: 200, g: 200, b: 200 });

        text.chars().enumerate().map(|(i, _)| {
            // Title portion uses title_color, after " - " uses artist_color
            if title_len > 0 && i >= title_len + 3 {
                (artist_color.r, artist_color.g, artist_color.b)
            } else {
                (title_color.r, title_color.g, title_color.b)
            }
        }).collect()
    };

    for (i, ch) in text.chars().enumerate() {
        let base_x = (base_start_x as isize - scroll_offset + (i * (char_width + char_spacing)) as isize) as usize;

        // Apply animation effects per character
        let (char_x, char_y, char_opacity) = match text_config.animation_style {
            TextAnimation::Wave => {
                let wave_offset = ((time * text_config.animation_speed * 3.0 + i as f32 * 0.3).sin() * 8.0) as isize;
                (base_x, (y as isize + wave_offset).max(0) as usize, opts.opacity)
            }
            TextAnimation::Pulse => {
                let pulse = 0.7 + 0.3 * (intensity * text_config.pulse_intensity);
                (base_x, y, opts.opacity * pulse)
            }
            TextAnimation::Fade => {
                let fade = 0.5 + 0.5 * ((time * text_config.animation_speed).sin() * 0.5 + 0.5);
                (base_x, y, opts.opacity * fade)
            }
            TextAnimation::Scroll | TextAnimation::None => {
                (base_x, y, opts.opacity)
            }
        };

        // Skip if character is outside visible area
        if char_x >= width || char_x + char_width > width + char_width {
            continue;
        }

        let (r, g, b) = colors.get(i).copied().unwrap_or((255, 255, 255));

        // Render with font style variations
        match text_config.font_style {
            FontStyle::Bold => {
                // Render with slight offset for bold effect
                render_char(canvas, char_x, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, char_x + 1, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, char_x, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Figlet => {
                // Render with outline for figlet-like effect
                let outline_color = (r / 3, g / 3, b / 3);
                for ox in [0isize, 2].iter() {
                    for oy in [0isize, 2].iter() {
                        if *ox != 1 || *oy != 1 {
                            render_char(canvas,
                                (char_x as isize + ox) as usize,
                                (char_y as isize + oy) as usize,
                                ch, outline_color.0, outline_color.1, outline_color.2,
                                scale, char_opacity * 0.5);
                        }
                    }
                }
                render_char(canvas, char_x + 1, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Normal | FontStyle::Ascii => {
                render_char(canvas, char_x, char_y, ch, r, g, b, scale, char_opacity);
            }
        }
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

#[allow(clippy::too_many_arguments)]
fn render_char(canvas: &mut Canvas, x: usize, y: usize, ch: char, r: u8, g: u8, b: u8, scale: usize, opacity: f32) {
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
                        if px < canvas.width && py < canvas.height {
                            canvas.put_pixel(px, py, r, g, b, opacity);
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
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if !self.active {
            // Go idle — don't re-request frame callback, don't draw.
            // The main loop will kick-start us when audio resumes.
            return;
        }

        // Request next frame BEFORE drawing, because the compositor only
        // sends a frame callback for commits that have a listener attached.
        surface.frame(qh, surface.clone());

        // Draw current frame (this calls commit(), which the callback above will fire for)
        self.draw_surface(surface);
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
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        // Dynamically create a surface for the new output (hotplug)
        self.create_surface_for_output(qh, &output);
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
        output: wl_output::WlOutput,
    ) {
        // Remove the surface for the destroyed output (hotplug unplug)
        if let Some(surface) = self.surfaces.remove(&output.id()) {
            info!("Output {:?} destroyed, removing surface", surface.output_name);
            // LayerSurface is dropped here, cleaning up Wayland resources
        }
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
        let (suggested_width, suggested_height) = configure.new_size;

        // Find the OutputSurface matching this layer surface
        let output_id = self.surfaces.iter()
            .find(|(_, s)| &s.layer_surface == layer)
            .map(|(id, _)| id.clone());

        let output_id = match output_id {
            Some(id) => id,
            None => return,
        };

        let surface = match self.surfaces.get_mut(&output_id) {
            Some(s) => s,
            None => return,
        };

        // Use explicit size if configured, otherwise use compositor suggestion
        let (width, height) = if let Some((w, h)) = surface.explicit_size {
            if suggested_width > 0 && suggested_height > 0 {
                (suggested_width.min(w), suggested_height.min(h))
            } else {
                (w, h)
            }
        } else {
            let w = if suggested_width > 0 { suggested_width } else { surface.screen_width.max(1920) };
            let h = if suggested_height > 0 { suggested_height } else { surface.screen_height.max(1080) };
            (w, h)
        };

        surface.width = width;
        surface.height = height;

        info!("Layer surface configured for {:?}: {}x{} (suggested: {}x{}, explicit: {:?})",
              surface.output_name, surface.width, surface.height, suggested_width, suggested_height, surface.explicit_size);

        // Recreate the pool for new size
        surface.pool = None;
        surface.configured = true;

        // Request frame callback then do initial draw (callback must be
        // attached before commit so the compositor knows to send the next one)
        let wl_surface = layer.wl_surface();
        wl_surface.frame(qh, wl_surface.clone());

        // Initial draw
        let wl_surface_clone = wl_surface.clone();
        self.draw_surface(&wl_surface_clone);
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
pub async fn run(config: Config, ipc_rx: mpsc::Receiver<IpcCommand>) -> Result<()> {
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
        ipc_rx,
    );

    // Do an initial roundtrip to discover outputs
    event_queue
        .roundtrip(&mut state)
        .context("Failed initial Wayland roundtrip")?;

    // Create layer surfaces for all known outputs
    state.create_surfaces_for_all_outputs(&qh);

    // Do another roundtrip to get configure events
    event_queue
        .roundtrip(&mut state)
        .context("Failed Wayland roundtrip after surface creation")?;

    // Wait for at least one surface to be configured
    let start = Instant::now();
    let timeout = Duration::from_secs(30);
    while !state.any_configured() && start.elapsed() < timeout {
        event_queue
            .roundtrip(&mut state)
            .context("Wayland roundtrip failed while waiting for configure")?;
        if !state.any_configured() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if !state.any_configured() {
        anyhow::bail!("No layer surface was configured within 30 seconds - compositor may not support wlr-layer-shell or no outputs available");
    }

    let configured_count = state.surfaces.values().filter(|s| s.configured).count();
    let total_count = state.surfaces.len();
    info!("Layer surfaces configured: {}/{} outputs", configured_count, total_count);

    // Collect unique audio sources across all surfaces
    // Always include None (default) for surfaces without an override
    let mut audio_sources: Vec<Option<String>> = vec![None];
    for surface in state.surfaces.values() {
        if let Some(ref source) = surface.audio_source_key {
            if !audio_sources.iter().any(|s| s.as_deref() == Some(source.as_str())) {
                audio_sources.push(Some(source.clone()));
            }
        }
    }

    // Create one audio pipeline per unique source
    for source in &audio_sources {
        // For the default pipeline, use config.audio.device; for overrides, use the sink name
        let device = source.clone().or_else(|| config.audio.device.clone());
        let (capture, rx) = audio::create_audio_pipeline(
            config.visualizer.bars,
            config.audio.smoothing,
            config.audio.sensitivity,
            device,
        )?;
        state.audio_pipelines.insert(source.clone(), AudioPipeline {
            _capture: capture,
            rx,
        });
        info!("Audio pipeline created for source: {:?}", source);
    }

    // Start metadata watcher
    let metadata_rx = metadata::start_watcher();

    info!("Wayland wallpaper mode running. Press Ctrl+C to stop.");



    // Style rotation timer
    let mut style_timer = Instant::now();
    let rotation_interval = Duration::from_secs(config.display.rotation_interval_secs);
    let color_schemes = [
        ColorScheme::Spectrum,
        ColorScheme::Rainbow,
        ColorScheme::Fire,
        ColorScheme::Ocean,
        ColorScheme::Forest,
        ColorScheme::Purple,
        ColorScheme::Monochrome,
    ];
    let mut color_scheme_idx = color_schemes
        .iter()
        .position(|&c| c == config.visualizer.color_scheme)
        .unwrap_or(0);

    // Main loop
    while state.running {
        let frame_start = Instant::now();

        // Collect latest audio data from all pipelines
        let mut latest_audio: HashMap<Option<String>, Arc<AudioData>> = HashMap::new();
        for (source_key, pipeline) in &state.audio_pipelines {
            latest_audio.insert(source_key.clone(), pipeline.rx.borrow().clone());
        }

        // Update each surface's audio data based on its source key
        for surface in state.surfaces.values_mut() {
            let key = &surface.audio_source_key;
            if let Some(data) = latest_audio.get(key) {
                surface.audio_data = data.clone();
            } else if let Some(data) = latest_audio.get(&None) {
                // Fall back to default pipeline
                surface.audio_data = data.clone();
            }
        }

        // Detect whether any audio is playing
        let has_audio = state.surfaces.values()
            .any(|s| s.audio_data.intensity > 0.001);

        if has_audio && !state.active {
            // Audio just started — kick-start frame callbacks on all surfaces
            state.active = true;
            for surface in state.surfaces.values() {
                if surface.configured {
                    let wl_surface = surface.layer_surface.wl_surface();
                    wl_surface.frame(&qh, wl_surface.clone());
                    wl_surface.commit();
                }
            }
        } else if !has_audio && state.active {
            // Audio stopped — let frame callbacks expire (they'll check active flag)
            state.active = false;
        }

        // Update metadata
        state.track_info = metadata_rx.borrow().clone();

        // Calculate delta time
        let dt = state.last_frame.elapsed().as_secs_f32();
        state.last_frame = Instant::now();
        state.update(dt);

        // Process IPC commands (non-blocking)
        while let Ok(cmd) = state.ipc_rx.try_recv() {
            // Intercept audio commands before generic handler
            match cmd {
                IpcCommand::ListSources { reply } => {
                    let response = match audio::list_sources() {
                        Ok(sources) => {
                            let list: Vec<String> = sources
                                .iter()
                                .map(|(name, s)| format!("{} ({})", name, s))
                                .collect();
                            format!("ok: {}", list.join(", "))
                        }
                        Err(e) => format!("err: {}", e),
                    };
                    let _ = reply.send(response);
                }
                IpcCommand::SetSource { name, reply } => {
                    let result = if name == "default" {
                        audio::create_audio_pipeline(
                            config.visualizer.bars,
                            config.audio.smoothing,
                            config.audio.sensitivity,
                            config.audio.device.clone(),
                        )
                    } else {
                        audio::create_audio_pipeline_with_source(
                            config.visualizer.bars,
                            config.audio.smoothing,
                            config.audio.sensitivity,
                            name.clone(),
                        )
                    };
                    match result {
                        Ok((capture, rx)) => {
                            state.audio_pipelines.remove(&None);
                            state.audio_pipelines.insert(None, AudioPipeline {
                                _capture: capture,
                                rx,
                            });
                            let _ = reply.send(format!("ok: {}", name));
                        }
                        Err(e) => {
                            let _ = reply.send(format!("err: {}", e));
                        }
                    }
                }
                cmd => {
                    let mut opacity = state.config.visualizer.opacity;
                    let monitors = state.list_monitors();
                    crate::ipc::process_ipc_command(
                        cmd,
                        &mut state.visualizer,
                        &mut state.color_scheme,
                        &mut state.visible,
                        &mut opacity,
                        &mut state.config,
                        &monitors,
                    );
                    state.config.visualizer.opacity = opacity;
                }
            }
        }

        // Auto-rotate color schemes if enabled
        if config.display.rotate_styles && style_timer.elapsed() >= rotation_interval {
            color_scheme_idx = (color_scheme_idx + 1) % color_schemes.len();
            state.color_scheme = color_schemes[color_scheme_idx];
            info!("Rotated to color scheme: {:?}", state.color_scheme);
            style_timer = Instant::now();
        }

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

        // Sleep — use short interval when active (audio playing) for responsive
        // state updates, longer when idle to minimize CPU usage
        let elapsed = frame_start.elapsed();
        let poll_interval = if state.active {
            Duration::from_millis(4)
        } else {
            Duration::from_millis(50)
        };
        if elapsed < poll_interval {
            std::thread::sleep(poll_interval - elapsed);
        }
    }

    info!("Wayland wallpaper mode stopped");
    Ok(())
}
