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
    /// Reusable scratch buffer for per-frame bar frequency layout.
    /// Owned here so every render reuses the same allocation.
    pub layout_scratch: Vec<f32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            data: vec![0u8; width * height * 4],
            width,
            height,
            layout_scratch: Vec::new(),
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
            if opacity >= 1.0 {
                self.data[idx] = r;
                self.data[idx + 1] = g;
                self.data[idx + 2] = b;
                self.data[idx + 3] = 255;
            } else {
                let a = (opacity * 255.0) as u8;
                self.data[idx] = (r as f32 * opacity) as u8;
                self.data[idx + 1] = (g as f32 * opacity) as u8;
                self.data[idx + 2] = (b as f32 * opacity) as u8;
                self.data[idx + 3] = a;
            }
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
        let byte_count = self.width * self.height * 4;
        let src = &self.data[..byte_count];
        let dst = &mut dest[..byte_count];
        // Swap R and B channels: RGBA [R,G,B,A] -> ARGB8888 LE [B,G,R,A]
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2]; // B
            d[1] = s[1]; // G
            d[2] = s[0]; // R
            d[3] = s[3]; // A
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
