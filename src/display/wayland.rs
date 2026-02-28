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
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    seat::pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
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
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Proxy, QueueHandle,
};

use crate::audio::{self, AudioCapture, AudioData};
use crate::color::ColorScheme;
use crate::config::{Config, MultiMonitorMode, WallpaperAnchor, WallpaperLayer};
use crate::ipc::{IpcCommand, PendingChanges};
use crate::metadata::{self, TrackInfo};
use crate::renderer;
use crate::visualizer::VisualizerState;
use tokio::sync::{mpsc, watch};

impl WallpaperLayer {
    /// Convert to layer-shell Layer type
    pub fn to_layer_shell_layer(self) -> Layer {
        match self {
            WallpaperLayer::Background => Layer::Background,
            WallpaperLayer::Bottom => Layer::Bottom,
            WallpaperLayer::Top => Layer::Top,
            WallpaperLayer::Overlay => Layer::Overlay,
        }
    }
}

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

/// An audio capture pipeline with its receiver
struct AudioPipeline {
    _capture: AudioCapture,
    rx: watch::Receiver<Arc<AudioData>>,
}

/// State for drag-to-move interaction
#[derive(Default)]
struct DragState {
    is_dragging: bool,
    last_x: f64,
    last_y: f64,
    /// Accumulated drag delta to apply in the main loop
    pending_dx: f64,
    pending_dy: f64,
    /// Whether to save margins (set on release)
    save_pending: bool,
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
    // Reusable pixel canvas (RGBA)
    canvas: renderer::Canvas,
}

/// Wayland layer-shell wallpaper renderer with multi-monitor support
struct WallpaperState {
    // Wayland state
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,
    seat_state: Option<SeatState>,
    pointer: Option<wl_pointer::WlPointer>,
    drag: DragState,

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
            seat_state: None,
            pointer: None,
            drag: DragState::default(),
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
                        renderer::styles::STYLE_NAMES.iter().position(|&name| name.eq_ignore_ascii_case(s))
                    });
                    return (monitor_cfg.color_scheme, style_idx, monitor_cfg.opacity, monitor_cfg.audio_source.clone());
                }
            }
        }

        (None, None, None, None)
    }

    /// Create a layer surface for a specific output
    fn create_surface_for_output(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        // Skip if we already have a surface for this output
        if self.surfaces.contains_key(&output.id()) {
            return;
        }

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
            self.config.wallpaper.layer.to_layer_shell_layer(),
            Some("cavibe-wallpaper"),
            Some(output),
        );

        // Configure anchor based on wallpaper config
        let anchor = self.config.wallpaper.anchor.to_layer_shell_anchor();
        layer_surface.set_anchor(anchor);

        // Apply margins
        let (top, right, bottom, left) = self.config.wallpaper.effective_margins();
        layer_surface.set_margin(top, right, bottom, left);

        // Set size for non-fullscreen anchors.
        // Layer-shell requires explicit width when LEFT+RIGHT aren't both set,
        // and explicit height when TOP+BOTTOM aren't both set.
        let needs_width = !anchor.contains(Anchor::LEFT | Anchor::RIGHT);
        let needs_height = !anchor.contains(Anchor::TOP | Anchor::BOTTOM);
        let explicit_size = if needs_width || needs_height {
            let configured = self.config.wallpaper.get_size(screen_w, screen_h);
            let (w, h) = configured.unwrap_or((screen_w / 2, screen_h / 2));
            layer_surface.set_size(w, h);
            info!("Explicit size set to: {}x{}", w, h);
            Some((w, h))
        } else {
            None
        };

        layer_surface.set_exclusive_zone(-1); // Don't reserve space
        let interactivity = if self.config.wallpaper.draggable {
            KeyboardInteractivity::OnDemand
        } else {
            KeyboardInteractivity::None
        };
        layer_surface.set_keyboard_interactivity(interactivity);

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
            canvas: renderer::Canvas::new(0, 0),
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

        // Resize the per-surface canvas
        surface.canvas.resize(width, height);

        let render_params = renderer::RenderParams {
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

        let frame_data = renderer::FrameData {
            frequencies: &frequencies,
            intensity,
            track_title: &track_title,
            track_artist: &track_artist,
            time,
        };
        renderer::render_frame(&mut surface.canvas, &frame_data, &render_params);

        // Convert RGBA to ARGB8888 for Wayland
        surface.canvas.write_argb8888(canvas);

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

    /// Convert current anchor to top-left for drag positioning.
    /// Calculates equivalent margin_left/margin_top to preserve the surface position.
    fn convert_to_topleft_anchor(&mut self) {
        if self.config.wallpaper.anchor == WallpaperAnchor::TopLeft {
            return;
        }

        // Get the first surface's dimensions to calculate position
        let Some(surface) = self.surfaces.values().next() else { return };
        let sw = surface.screen_width as i32;
        let sh = surface.screen_height as i32;
        let w = surface.width as i32;
        let h = surface.height as i32;
        let (mt, mr, mb, ml) = self.config.wallpaper.effective_margins();

        // Calculate current top-left position based on anchor
        let (x, y) = match self.config.wallpaper.anchor {
            WallpaperAnchor::TopLeft => (ml, mt),
            WallpaperAnchor::Top => ((sw - w) / 2, mt),
            WallpaperAnchor::TopRight => (sw - w - mr, mt),
            WallpaperAnchor::Left => (ml, (sh - h) / 2),
            WallpaperAnchor::Center => ((sw - w) / 2, (sh - h) / 2),
            WallpaperAnchor::Right => (sw - w - mr, (sh - h) / 2),
            WallpaperAnchor::BottomLeft => (ml, sh - h - mb),
            WallpaperAnchor::Bottom => ((sw - w) / 2, sh - h - mb),
            WallpaperAnchor::BottomRight => (sw - w - mr, sh - h - mb),
            WallpaperAnchor::Fullscreen => (0, 0),
        };

        self.config.wallpaper.anchor = WallpaperAnchor::TopLeft;
        self.config.wallpaper.margin = 0;
        self.config.wallpaper.margin_top = y;
        self.config.wallpaper.margin_left = x;
        self.config.wallpaper.margin_right = 0;
        self.config.wallpaper.margin_bottom = 0;

        // Apply the new anchor + margins to all surfaces.
        // Top-left anchor requires explicit size — use current surface dimensions.
        let anchor = Anchor::TOP | Anchor::LEFT;
        for surface in self.surfaces.values_mut() {
            surface.layer_surface.set_anchor(anchor);
            surface.layer_surface.set_margin(y, 0, 0, x);
            let (sw, sh) = surface.explicit_size.unwrap_or((surface.width, surface.height));
            surface.layer_surface.set_size(sw, sh);
            surface.explicit_size = Some((sw, sh));
            surface.layer_surface.commit();
        }

        // Also store size in config so it persists
        if self.config.wallpaper.width.is_none() || self.config.wallpaper.height.is_none() {
            if let Some(surface) = self.surfaces.values().next() {
                if let Some((w, h)) = surface.explicit_size {
                    self.config.wallpaper.width = Some(w.to_string());
                    self.config.wallpaper.height = Some(h.to_string());
                }
            }
        }

        info!("Converted to top-left anchor for drag: position ({}, {})", x, y);
    }

    /// Apply a drag delta (in surface-local pixels) to the margins.
    /// Requires top-left anchor (call convert_to_topleft_anchor first).
    fn apply_drag_delta(&mut self, dx: f64, dy: f64) {
        self.config.wallpaper.margin_left += dx as i32;
        self.config.wallpaper.margin_top += dy as i32;

        let mt = self.config.wallpaper.margin_top;
        let ml = self.config.wallpaper.margin_left;
        for surface in self.surfaces.values() {
            surface.layer_surface.set_margin(mt, 0, 0, ml);
            surface.layer_surface.commit();
        }
    }

    /// Save current state to the config file (style, color, layer, position, etc.)
    /// Creates the config file from the default template if it doesn't exist.
    fn save_state_to_config(&self) {
        let Some(path) = Config::default_path() else {
            return;
        };

        // Create config from template if it doesn't exist
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let template = Config::generate_config_template();
            if std::fs::write(&path, &template).is_err() {
                tracing::warn!("Failed to create config file at {}", path.display());
                return;
            }
            info!("Created config file at {}", path.display());
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                match content.parse::<toml_edit::DocumentMut>() {
                    Ok(mut doc) => {
                        // Ensure [visualizer] section exists
                        if !doc.contains_key("visualizer") {
                            doc["visualizer"] = toml_edit::table();
                        }
                        doc["visualizer"]["style"] = toml_edit::value(self.visualizer.current_style_name().to_lowercase());
                        doc["visualizer"]["color_scheme"] = toml_edit::value(self.color_scheme.name().to_lowercase());
                        doc["visualizer"]["opacity"] = toml_edit::value(self.config.visualizer.opacity as f64);

                        // Ensure [text] section exists
                        if !doc.contains_key("text") {
                            doc["text"] = toml_edit::table();
                        }
                        doc["text"]["show_title"] = toml_edit::value(self.config.text.show_title);
                        doc["text"]["show_artist"] = toml_edit::value(self.config.text.show_artist);
                        doc["text"]["position"] = toml_edit::value(self.config.text.position.to_string());
                        doc["text"]["font_style"] = toml_edit::value(format!("{:?}", self.config.text.font_style).to_lowercase());
                        doc["text"]["animation_style"] = toml_edit::value(format!("{:?}", self.config.text.animation_style).to_lowercase());

                        // Ensure [wallpaper] section exists
                        if !doc.contains_key("wallpaper") {
                            doc["wallpaper"] = toml_edit::table();
                        }
                        doc["wallpaper"]["layer"] = toml_edit::value(self.config.wallpaper.layer.name());
                        doc["wallpaper"]["anchor"] = toml_edit::value(self.config.wallpaper.anchor.name());
                        doc["wallpaper"]["draggable"] = toml_edit::value(self.config.wallpaper.draggable);
                        doc["wallpaper"]["margin"] = toml_edit::value(self.config.wallpaper.margin as i64);
                        doc["wallpaper"]["margin_top"] = toml_edit::value(self.config.wallpaper.margin_top as i64);
                        doc["wallpaper"]["margin_right"] = toml_edit::value(self.config.wallpaper.margin_right as i64);
                        doc["wallpaper"]["margin_bottom"] = toml_edit::value(self.config.wallpaper.margin_bottom as i64);
                        doc["wallpaper"]["margin_left"] = toml_edit::value(self.config.wallpaper.margin_left as i64);
                        if let Some(ref w) = self.config.wallpaper.width {
                            doc["wallpaper"]["width"] = toml_edit::value(w.as_str());
                        }
                        if let Some(ref h) = self.config.wallpaper.height {
                            doc["wallpaper"]["height"] = toml_edit::value(h.as_str());
                        }

                        let _ = std::fs::write(&path, doc.to_string());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse config for save: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read config for save: {}", e);
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

        // Use explicit size if configured, otherwise use compositor suggestion.
        // Don't accept compositor shrinking — it reduces suggested size when
        // margins push the surface near screen edges.
        let (width, height) = if let Some((w, h)) = surface.explicit_size {
            (w, h)
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

impl SeatHandler for WallpaperState {
    fn seat_state(&mut self) -> &mut SeatState {
        self.seat_state.as_mut().expect("SeatState not initialized")
    }

    fn new_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Some(ref mut seat_state) = self.seat_state {
                if let Ok(pointer) = seat_state.get_pointer(qh, &seat) {
                    self.pointer = Some(pointer);
                    info!("Pointer capability acquired (drag-to-move available)");
                }
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
                self.drag = DragState::default();
                info!("Pointer capability removed");
            }
        }
    }

    fn remove_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }
}

impl PointerHandler for WallpaperState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        if !self.config.wallpaper.draggable {
            return;
        }

        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.drag.last_x = event.position.0;
                    self.drag.last_y = event.position.1;
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.drag.is_dragging = true;
                    self.drag.last_x = event.position.0;
                    self.drag.last_y = event.position.1;
                    self.drag.pending_dx = 0.0;
                    self.drag.pending_dy = 0.0;
                }
                PointerEventKind::Motion { .. } if self.drag.is_dragging => {
                    let dx = event.position.0 - self.drag.last_x;
                    let dy = event.position.1 - self.drag.last_y;
                    self.drag.last_x = event.position.0;
                    self.drag.last_y = event.position.1;
                    self.drag.pending_dx += dx;
                    self.drag.pending_dy += dy;
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    if self.drag.is_dragging {
                        self.drag.is_dragging = false;
                        self.drag.save_pending = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.drag.is_dragging {
                        self.drag.is_dragging = false;
                        self.drag.save_pending = true;
                    }
                }
                _ => {}
            }
        }
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
delegate_seat!(WallpaperState);
delegate_pointer!(WallpaperState);
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

    // Create state (without seat — initialized after surfaces are configured)
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

    // Initialize seat/pointer for drag-to-move (after surfaces are configured
    // to avoid interfering with layer surface setup)
    state.seat_state = Some(SeatState::new(&globals, &qh));
    event_queue
        .roundtrip(&mut state)
        .context("Failed Wayland roundtrip after seat initialization")?;

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
        let mut pending = PendingChanges::default();
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
                IpcCommand::ResizeRelative { delta, is_percent, reply } => {
                    if state.config.wallpaper.anchor == WallpaperAnchor::Fullscreen {
                        let _ = reply.send("err: cannot resize in fullscreen anchor mode".to_string());
                    } else if let Some(first_surface) = state.surfaces.values().next() {
                        let cur_w = first_surface.width as i32;
                        let cur_h = first_surface.height as i32;
                        let (new_w, new_h) = if is_percent {
                            let factor = 1.0 + delta as f64 / 100.0;
                            ((cur_w as f64 * factor) as i32, (cur_h as f64 * factor) as i32)
                        } else {
                            (cur_w + delta, cur_h + delta)
                        };
                        let new_w = new_w.max(1) as u32;
                        let new_h = new_h.max(1) as u32;
                        // Adjust margins to keep the visual center stable
                        let dw = new_w as i32 - cur_w;
                        let dh = new_h as i32 - cur_h;
                        state.config.wallpaper.margin_left -= dw / 2;
                        state.config.wallpaper.margin_top -= dh / 2;
                        state.config.wallpaper.width = Some(new_w.to_string());
                        state.config.wallpaper.height = Some(new_h.to_string());
                        pending.surface_update = true;
                        pending.save_config = true;
                        let _ = reply.send(format!("ok: {}x{}", new_w, new_h));
                    } else {
                        let _ = reply.send("err: no surfaces configured".to_string());
                    }
                }
                IpcCommand::Resize { width, height, reply } => {
                    if let Some(first_surface) = state.surfaces.values().next() {
                        let cur_w = first_surface.width as i32;
                        let cur_h = first_surface.height as i32;
                        // Resolve the new size to get pixel values for margin adjustment
                        let screen_w = first_surface.screen_width;
                        let screen_h = first_surface.screen_height;
                        state.config.wallpaper.width = Some(width.clone());
                        state.config.wallpaper.height = Some(height.clone());
                        if let Some((nw, nh)) = state.config.wallpaper.get_size(screen_w, screen_h) {
                            let dw = nw as i32 - cur_w;
                            let dh = nh as i32 - cur_h;
                            state.config.wallpaper.margin_left -= dw / 2;
                            state.config.wallpaper.margin_top -= dh / 2;
                        }
                        pending.surface_update = true;
                        pending.save_config = true;
                        let _ = reply.send(format!("ok: {}x{}", width, height));
                    } else {
                        let _ = reply.send("err: no surfaces configured".to_string());
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
                        &mut pending,
                    );
                    state.config.visualizer.opacity = opacity;
                }
            }
        }

        // Handle pending layer change (requires surface recreation)
        if pending.layer_change {
            info!("Layer changed to {}, recreating surfaces", state.config.wallpaper.layer.name());
            // Destroy all existing surfaces
            state.surfaces.clear();
            // Recreate surfaces with the new layer
            state.create_surfaces_for_all_outputs(&qh);
            // Roundtrip to get configure events for new surfaces
            let _ = event_queue.roundtrip(&mut state);
        }

        // Handle pending surface property updates (anchor/margin/size — dynamic)
        if pending.surface_update && !pending.layer_change {
            let anchor = state.config.wallpaper.anchor.to_layer_shell_anchor();
            let (mt, mr, mb, ml) = state.config.wallpaper.effective_margins();

            for surface in state.surfaces.values_mut() {
                surface.layer_surface.set_anchor(anchor);
                surface.layer_surface.set_margin(mt, mr, mb, ml);

                // Update size — use same fallback logic as create_surface_for_output
                let needs_width = !anchor.contains(Anchor::LEFT | Anchor::RIGHT);
                let needs_height = !anchor.contains(Anchor::TOP | Anchor::BOTTOM);
                if needs_width || needs_height {
                    let configured = state.config.wallpaper.get_size(surface.screen_width, surface.screen_height);
                    let (w, h) = configured.unwrap_or_else(|| {
                        // Preserve current size if available, otherwise half screen
                        surface.explicit_size.unwrap_or((surface.screen_width / 2, surface.screen_height / 2))
                    });
                    surface.layer_surface.set_size(w, h);
                    surface.explicit_size = Some((w, h));
                } else {
                    // Fullscreen — let compositor decide
                    surface.layer_surface.set_size(0, 0);
                    surface.explicit_size = None;
                }

                surface.layer_surface.commit();
            }
        }

        // Apply accumulated drag delta
        if state.drag.pending_dx != 0.0 || state.drag.pending_dy != 0.0 {
            let dx = state.drag.pending_dx;
            let dy = state.drag.pending_dy;
            state.drag.pending_dx = 0.0;
            state.drag.pending_dy = 0.0;
            state.apply_drag_delta(dx, dy);
        }

        // Save margins after drag release
        if state.drag.save_pending {
            state.drag.save_pending = false;
            state.save_state_to_config();
        }

        // Handle drag mode change — update keyboard interactivity and anchor
        if pending.drag_changed {
            let interactivity = if state.config.wallpaper.draggable {
                KeyboardInteractivity::OnDemand
            } else {
                KeyboardInteractivity::None
            };
            for surface in state.surfaces.values() {
                surface.layer_surface.set_keyboard_interactivity(interactivity);
                surface.layer_surface.commit();
            }
            // Convert to top-left anchor for reliable margin-based positioning
            if state.config.wallpaper.draggable {
                state.convert_to_topleft_anchor();
            }
        }

        // Save config if state changed via IPC
        if pending.save_config {
            state.save_state_to_config();
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
