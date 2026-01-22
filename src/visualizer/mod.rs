mod bars;
mod styles;
mod text;

pub use bars::BarVisualizer;
pub use styles::{VisualizerStyle, VISUALIZER_STYLES};
pub use text::TextAnimator;

use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::metadata::TrackInfo;
use ratatui::prelude::*;
use std::sync::Arc;

/// Trait for different visualizer rendering styles
pub trait Visualizer {
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    );

    fn name(&self) -> &'static str;
}

/// Combined visualizer state
pub struct VisualizerState {
    pub bar_visualizer: BarVisualizer,
    pub text_animator: TextAnimator,
    pub current_style: usize,
    pub time: f32,
}

impl VisualizerState {
    pub fn new(num_bars: usize) -> Self {
        Self {
            bar_visualizer: BarVisualizer::new(num_bars),
            text_animator: TextAnimator::new(),
            current_style: 0,
            time: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.text_animator.update(dt);
    }

    pub fn next_style(&mut self) {
        self.current_style = (self.current_style + 1) % VISUALIZER_STYLES.len();
    }

    pub fn current_style_name(&self) -> &'static str {
        VISUALIZER_STYLES[self.current_style].name()
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &Arc<AudioData>,
        track: &Arc<TrackInfo>,
        color_scheme: &ColorScheme,
    ) {
        // Split area for visualizer and text
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        // Render bars
        VISUALIZER_STYLES[self.current_style].render(frame, chunks[0], audio, color_scheme);

        // Render animated text
        self.text_animator
            .render(frame, chunks[1], track, audio, color_scheme, self.time);
    }
}
