//! Unified pixel-based renderer
//!
//! All visualization styles render to an owned RGBA pixel buffer (`Canvas`).
//! Output backends (Wayland layer-shell, terminal half-block) convert the
//! canvas to their native format at submission time.

pub mod layout;
pub mod styles;
pub mod text;

use crate::color::ColorScheme;
use crate::config::TextConfig;

/// Owned RGBA pixel buffer.
///
/// Internal format is 4 bytes per pixel in **RGBA** order.
/// Call [`Canvas::write_argb8888`] to convert to the pre-multiplied ARGB8888
/// format required by Wayland `wl_shm`.
pub struct Canvas {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            data: vec![0u8; width * height * 4],
            width,
            height,
        }
    }

    /// Resize the canvas, reallocating only when the buffer is too small.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        let needed = width * height * 4;
        if self.data.len() < needed {
            self.data.resize(needed, 0);
        }
    }

    /// Clear the canvas to fully transparent black.
    #[inline]
    pub fn clear(&mut self) {
        let len = self.width * self.height * 4;
        self.data[..len].fill(0);
    }

    /// Write a pixel at (x, y) with the given color and opacity.
    /// Stored as RGBA internally.
    #[inline]
    pub fn put_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, opacity: f32) {
        let idx = (y * self.width + x) * 4;
        if idx + 3 < self.data.len() {
            let a = (opacity * 255.0) as u8;
            self.data[idx] = (r as f32 * opacity) as u8;
            self.data[idx + 1] = (g as f32 * opacity) as u8;
            self.data[idx + 2] = (b as f32 * opacity) as u8;
            self.data[idx + 3] = a;
        }
    }

    /// Read the RGBA values at (x, y). Returns (r, g, b, a) — pre-multiplied.
    #[inline]
    pub fn get_pixel(&self, x: usize, y: usize) -> (u8, u8, u8, u8) {
        let idx = (y * self.width + x) * 4;
        if idx + 3 < self.data.len() {
            (self.data[idx], self.data[idx + 1], self.data[idx + 2], self.data[idx + 3])
        } else {
            (0, 0, 0, 0)
        }
    }

    /// Convert the RGBA canvas to pre-multiplied ARGB8888 and write into `dest`.
    /// `dest` must be at least `width * height * 4` bytes.
    /// Wayland wl_shm expects ARGB8888 in native byte order: [B, G, R, A] on little-endian.
    pub fn write_argb8888(&self, dest: &mut [u8]) {
        let pixel_count = self.width * self.height;
        for i in 0..pixel_count {
            let si = i * 4;
            let r = self.data[si];
            let g = self.data[si + 1];
            let b = self.data[si + 2];
            let a = self.data[si + 3];
            // ARGB8888 in little-endian memory: B G R A
            dest[si] = b;
            dest[si + 1] = g;
            dest[si + 2] = r;
            dest[si + 3] = a;
        }
    }
}

/// Per-frame data passed to the renderer.
pub struct FrameData<'a> {
    pub frequencies: &'a [f32],
    pub intensity: f32,
    pub track_title: &'a Option<String>,
    pub track_artist: &'a Option<String>,
    pub time: f32,
}

/// Parameters controlling how a frame is rendered.
pub struct RenderParams<'a> {
    pub style: usize,
    pub bar_width: usize,
    pub bar_spacing: usize,
    pub mirror: bool,
    pub reverse_mirror: bool,
    pub opacity: f32,
    pub color_scheme: &'a ColorScheme,
    pub waveform: &'a [f32],
    pub spectrogram_history: &'a [Vec<f32>],
    pub text_config: &'a TextConfig,
}

/// Main entry point: render a complete frame to the canvas.
pub fn render_frame(canvas: &mut Canvas, frame: &FrameData, params: &RenderParams) {
    canvas.clear();
    styles::render_bars(canvas, frame.frequencies, params);
    text::render_text(canvas, frame, params);
}
