//! Visualizer state management.
//!
//! Tracks the current style index, time, and style cycling.
//! The actual rendering is handled by `crate::renderer`.

use crate::config::{TextConfig, VisualizerConfig};
use crate::renderer::styles;

/// Combined visualizer state
pub struct VisualizerState {
    pub current_style: usize,
    pub time: f32,
}

impl VisualizerState {
    pub fn new(visualizer_config: VisualizerConfig, _text_config: TextConfig) -> Self {
        let initial_style = visualizer_config
            .style
            .as_deref()
            .and_then(|name| {
                styles::STYLE_NAMES
                    .iter()
                    .position(|&s| s.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        Self {
            current_style: initial_style,
            time: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn next_style(&mut self) {
        self.current_style = (self.current_style + 1) % styles::style_count();
    }

    pub fn prev_style(&mut self) {
        if self.current_style == 0 {
            self.current_style = styles::style_count() - 1;
        } else {
            self.current_style -= 1;
        }
    }

    pub fn current_style_name(&self) -> &'static str {
        styles::STYLE_NAMES[self.current_style]
    }
}
