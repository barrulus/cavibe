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
use crate::config::{Config, FontStyle, TextAlignment, TextAnimation, TextConfig, TextPosition, WallpaperAnchor};
use crate::ipc::IpcCommand;
use crate::metadata::{self, TrackInfo};
use crate::visualizer::VisualizerState;
use tokio::sync::mpsc;

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
    bar_width: usize,
    bar_spacing: usize,
    mirror: bool,
    reverse_mirror: bool,
    opacity: f32,
    color_scheme: &'a ColorScheme,

    // Text settings
    text_config: &'a TextConfig,
}

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

    // Screen dimensions (from output)
    screen_width: u32,
    screen_height: u32,
    // Explicit size from config (if any)
    explicit_size: Option<(u32, u32)>,

    // Visualizer state
    visualizer: VisualizerState,
    color_scheme: ColorScheme,
    audio_data: Arc<AudioData>,
    track_info: Arc<TrackInfo>,
    last_frame: Instant,
    time: f32,

    // Control
    running: bool,
    visible: bool,
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
            layer_surface: None,
            pool: None,
            width: 0,
            height: 0,
            configured: false,
            screen_width: 0,
            screen_height: 0,
            explicit_size: None,
            visualizer,
            color_scheme,
            audio_data: Arc::new(AudioData::default()),
            track_info: Arc::new(TrackInfo::default()),
            last_frame: Instant::now(),
            time: 0.0,
            running: true,
            visible: true,
            config,
            ipc_rx,
        }
    }

    fn create_layer_surface(&mut self, qh: &QueueHandle<Self>) {
        info!("Creating layer surface...");

        // Get screen dimensions from output state before creating surface
        let (screen_w, screen_h) = self.get_screen_dimensions();
        self.screen_width = screen_w;
        self.screen_height = screen_h;
        info!("Screen dimensions: {}x{}", screen_w, screen_h);

        let surface = self.compositor_state.create_surface(qh);

        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Background,
            Some("cavibe-wallpaper"),
            None, // Use default output
        );

        // Configure anchor based on wallpaper config
        let anchor = self.config.wallpaper.anchor.to_layer_shell_anchor();
        layer_surface.set_anchor(anchor);
        info!("Anchor set to: {:?} -> {:?}", self.config.wallpaper.anchor, anchor);

        // Apply margins
        let (top, right, bottom, left) = self.config.wallpaper.effective_margins();
        layer_surface.set_margin(top, right, bottom, left);
        info!("Margins set to: top={}, right={}, bottom={}, left={}", top, right, bottom, left);

        // Set explicit size if configured (not fullscreen)
        if self.config.wallpaper.anchor != WallpaperAnchor::Fullscreen {
            if let Some((w, h)) = self.config.wallpaper.get_size(screen_w, screen_h) {
                layer_surface.set_size(w, h);
                self.explicit_size = Some((w, h));
                info!("Explicit size set to: {}x{}", w, h);
            }
        }

        layer_surface.set_exclusive_zone(-1); // Don't reserve space
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

        // Commit to get the configure event
        layer_surface.commit();

        self.layer_surface = Some(layer_surface);
        info!("Layer surface created, waiting for configure event...");
    }

    /// Get screen dimensions from the first available output
    fn get_screen_dimensions(&self) -> (u32, u32) {
        for output in self.output_state.outputs() {
            if let Some(info) = self.output_state.info(&output) {
                if let Some((w, h)) = info.logical_size {
                    return (w as u32, h as u32);
                }
            }
        }
        // Fallback if no output info available
        (1920, 1080)
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }

        if self.layer_surface.is_none() {
            return;
        }

        if !self.visible {
            // Render a fully transparent frame
            if self.pool.is_none() {
                let pool = SlotPool::new(
                    (self.width * self.height * 4) as usize,
                    &self.shm,
                )
                .expect("Failed to create slot pool");
                self.pool = Some(pool);
            }
            let pool = self.pool.as_mut().unwrap();
            let (buffer, canvas) = pool
                .create_buffer(
                    self.width as i32,
                    self.height as i32,
                    (self.width * 4) as i32,
                    wl_shm::Format::Argb8888,
                )
                .expect("Failed to create buffer");
            for pixel in canvas.chunks_exact_mut(4) {
                pixel[0] = 0;
                pixel[1] = 0;
                pixel[2] = 0;
                pixel[3] = 0;
            }
            let layer_surface = self.layer_surface.as_ref().unwrap();
            let surface = layer_surface.wl_surface();
            buffer.attach_to(surface).expect("Failed to attach buffer");
            surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
            surface.commit();
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
        // Scale bar dimensions for pixel rendering (config values are for terminal chars ~8-10px)
        let pixel_scale = 8;
        let bar_width = (self.config.visualizer.bar_width as usize) * pixel_scale;
        let bar_spacing = (self.config.visualizer.bar_spacing as usize) * pixel_scale;
        let time = self.time;

        let render_opts = RenderOptions {
            bar_width,
            bar_spacing,
            mirror: self.config.visualizer.mirror,
            reverse_mirror: self.config.visualizer.reverse_mirror,
            opacity: self.config.visualizer.opacity,
            color_scheme: &color_scheme,
            text_config: &self.config.text,
        };

        render_to_buffer(
            canvas,
            width,
            height,
            &frequencies,
            intensity,
            &track_title,
            &track_artist,
            time,
            &render_opts,
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
    time: f32,
    opts: &RenderOptions,
) {
    // Clear to fully transparent (let wallpaper show through)
    for pixel in canvas.chunks_exact_mut(4) {
        pixel[0] = 0; // B
        pixel[1] = 0; // G
        pixel[2] = 0; // R
        pixel[3] = 0; // A - fully transparent background
    }

    // Render bars
    render_bars(canvas, width, height, frequencies, opts);

    // Render text
    render_text(canvas, width, height, track_title, track_artist, intensity, time, opts);
}

fn render_bars(
    canvas: &mut [u8],
    width: usize,
    height: usize,
    frequencies: &[f32],
    opts: &RenderOptions,
) {
    if frequencies.is_empty() {
        return;
    }

    let bar_count = frequencies.len().min(width);
    // Reserve space for text based on position
    let text_height = if opts.text_config.show_title || opts.text_config.show_artist {
        60 + opts.text_config.margin_top as usize + opts.text_config.margin_bottom as usize
    } else {
        0
    };

    let (bars_y_start, bars_height) = match opts.text_config.position {
        TextPosition::Top => (text_height, height.saturating_sub(text_height)),
        TextPosition::Bottom => (0, height.saturating_sub(text_height)),
        TextPosition::Center => (0, height), // Text overlays bars
    };

    if bars_height == 0 {
        return;
    }

    let slot_width = opts.bar_width + opts.bar_spacing;

    // Calculate how many bars fit
    let max_bars = width / slot_width.max(1);
    let displayable = max_bars.min(bar_count);

    if displayable == 0 {
        return;
    }

    // Center the bars
    let total_width = displayable * slot_width;
    let start_x = (width.saturating_sub(total_width)) / 2;

    // Prepare frequency data based on mirror and reverse settings
    let render_frequencies: Vec<f32> = match (opts.mirror, opts.reverse_mirror) {
        (true, true) => {
            // Mirror + Reverse: lows meet in middle, highs on outside
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            // Left side: high to low (highs on outside)
            for i in 0..half {
                let freq_idx = ((half - 1 - i) * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            // Right side: low to high (mirrored, highs on outside)
            for i in 0..displayable - half {
                let freq_idx = (i * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (true, false) => {
            // Mirror only: highs meet in middle, lows on outside
            let half = displayable / 2;
            let mut result = Vec::with_capacity(displayable);
            // Left side: low to high (lows on outside)
            for i in 0..half {
                let freq_idx = (i * frequencies.len()) / half.max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            // Right side: high to low (mirrored, lows on outside)
            for i in 0..displayable - half {
                let freq_idx = ((displayable - half - 1 - i) * frequencies.len()) / (displayable - half).max(1);
                result.push(frequencies[freq_idx.min(frequencies.len() - 1)]);
            }
            result
        }
        (false, true) => {
            // Reverse only: high frequencies on left, low on right
            (0..displayable)
                .map(|i| {
                    let freq_idx = ((displayable - 1 - i) * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
        (false, false) => {
            // Normal: low frequencies on left, high on right
            (0..displayable)
                .map(|i| {
                    let freq_idx = (i * frequencies.len()) / displayable.max(1);
                    frequencies[freq_idx.min(frequencies.len() - 1)]
                })
                .collect()
        }
    };

    for i in 0..displayable {
        let magnitude = render_frequencies[i];

        let bar_height = (magnitude * bars_height as f32) as usize;
        let x_start = start_x + i * slot_width;
        let position = i as f32 / displayable as f32;

        // Draw bar from bottom up
        for y_offset in 0..bar_height.min(bars_height) {
            let y = bars_y_start + bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / bars_height as f32;
            let (r, g, b) = opts.color_scheme.get_color(position, intensity);

            for bx in 0..opts.bar_width {
                let x = x_start + bx;
                if x < width && y < height {
                    let idx = (y * width + x) * 4;
                    if idx + 3 < canvas.len() {
                        // Pre-multiplied alpha: RGB values must be multiplied by alpha
                        canvas[idx] = (b as f32 * opts.opacity) as u8;     // B
                        canvas[idx + 1] = (g as f32 * opts.opacity) as u8; // G
                        canvas[idx + 2] = (r as f32 * opts.opacity) as u8; // R
                        canvas[idx + 3] = (opts.opacity * 255.0) as u8;    // A
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
    intensity: f32,
    time: f32,
    opts: &RenderOptions,
) {
    let text_config = opts.text_config;

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
        FontStyle::Figlet => 1 * scale,
        _ => 1 * scale,
    };

    let text_area_height = char_height + 20;
    let margin_h = text_config.margin_horizontal as usize;

    // Calculate text Y position based on position setting
    let base_text_y = match text_config.position {
        TextPosition::Top => text_config.margin_top as usize,
        TextPosition::Bottom => height.saturating_sub(text_area_height + text_config.margin_bottom as usize),
        TextPosition::Center => (height.saturating_sub(char_height)) / 2,
    };

    let text_width = text.len() * (char_width + char_spacing);
    let available_width = width.saturating_sub(margin_h * 2);

    // Calculate base X position based on alignment
    let base_start_x = match text_config.alignment {
        TextAlignment::Left => margin_h,
        TextAlignment::Center => margin_h + (available_width.saturating_sub(text_width)) / 2,
        TextAlignment::Right => margin_h + available_width.saturating_sub(text_width),
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
                if idx + 3 < canvas.len() {
                    canvas[idx] = (bg_color.b as f32 * opts.opacity) as u8;
                    canvas[idx + 1] = (bg_color.g as f32 * opts.opacity) as u8;
                    canvas[idx + 2] = (bg_color.r as f32 * opts.opacity) as u8;
                    canvas[idx + 3] = (opts.opacity * 255.0 * 0.8) as u8; // Slightly transparent
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
                render_char(canvas, width, height, char_x, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, width, height, char_x + 1, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, width, height, char_x, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Figlet => {
                // Render with outline for figlet-like effect
                let outline_color = (r / 3, g / 3, b / 3);
                for ox in [0isize, 2].iter() {
                    for oy in [0isize, 2].iter() {
                        if *ox != 1 || *oy != 1 {
                            render_char(canvas, width, height,
                                (char_x as isize + ox) as usize,
                                (char_y as isize + oy) as usize,
                                ch, outline_color.0, outline_color.1, outline_color.2,
                                scale, char_opacity * 0.5);
                        }
                    }
                }
                render_char(canvas, width, height, char_x + 1, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Normal | FontStyle::Ascii => {
                render_char(canvas, width, height, char_x, char_y, ch, r, g, b, scale, char_opacity);
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

fn render_char(canvas: &mut [u8], width: usize, height: usize, x: usize, y: usize, ch: char, r: u8, g: u8, b: u8, scale: usize, opacity: f32) {
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
                                // Pre-multiplied alpha: RGB values must be multiplied by alpha
                                canvas[idx] = (b as f32 * opacity) as u8;
                                canvas[idx + 1] = (g as f32 * opacity) as u8;
                                canvas[idx + 2] = (r as f32 * opacity) as u8;
                                canvas[idx + 3] = (opacity * 255.0) as u8;
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
        let (suggested_width, suggested_height) = configure.new_size;

        // Use explicit size if configured, otherwise use compositor suggestion
        let (width, height) = if let Some((w, h)) = self.explicit_size {
            // Use our explicit size, but respect compositor's suggestion if it's larger
            // (compositor may have constraints)
            if suggested_width > 0 && suggested_height > 0 {
                (suggested_width.min(w), suggested_height.min(h))
            } else {
                (w, h)
            }
        } else {
            // No explicit size - use compositor suggestion or fallback
            let w = if suggested_width > 0 { suggested_width } else { self.screen_width.max(1920) };
            let h = if suggested_height > 0 { suggested_height } else { self.screen_height.max(1080) };
            (w, h)
        };

        self.width = width;
        self.height = height;

        info!("Layer surface configured: {}x{} (suggested: {}x{}, explicit: {:?})",
              self.width, self.height, suggested_width, suggested_height, self.explicit_size);

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

    // Create the layer surface
    state.create_layer_surface(&qh);

    // Do a roundtrip to ensure the layer surface gets configured
    event_queue
        .roundtrip(&mut state)
        .context("Failed initial Wayland roundtrip")?;

    // Wait for layer surface to be configured (handles race at compositor startup)
    let start = Instant::now();
    let timeout = Duration::from_secs(30);
    while !state.configured && start.elapsed() < timeout {
        event_queue
            .roundtrip(&mut state)
            .context("Wayland roundtrip failed while waiting for configure")?;
        if !state.configured {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if !state.configured {
        anyhow::bail!("Layer surface was not configured within 30 seconds - compositor may not support wlr-layer-shell or no outputs available");
    }

    info!("Layer surface configured successfully");

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

        // Update audio and metadata
        state.audio_data = audio_rx.borrow().clone();
        state.track_info = metadata_rx.borrow().clone();

        // Calculate delta time
        let dt = state.last_frame.elapsed().as_secs_f32();
        state.last_frame = Instant::now();
        state.update(dt);

        // Process IPC commands (non-blocking)
        while let Ok(cmd) = state.ipc_rx.try_recv() {
            let mut opacity = state.config.visualizer.opacity;
            crate::ipc::process_ipc_command(
                cmd,
                &mut state.visualizer,
                &mut state.color_scheme,
                &mut state.visible,
                &mut opacity,
                &mut state.config,
            );
            state.config.visualizer.opacity = opacity;
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
