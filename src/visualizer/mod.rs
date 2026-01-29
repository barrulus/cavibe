mod ascii_font;
mod bars;
mod styles;
mod text;

pub use bars::BarVisualizer;
pub use styles::VISUALIZER_STYLES;
pub use text::TextAnimator;

use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::config::{FontStyle, TextConfig, TextPosition, VisualizerConfig};
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
        config: &VisualizerConfig,
    );

    fn name(&self) -> &'static str;
}

/// Combined visualizer state
pub struct VisualizerState {
    pub bar_visualizer: BarVisualizer,
    pub text_animator: TextAnimator,
    pub visualizer_config: VisualizerConfig,
    pub current_style: usize,
    pub time: f32,
}

impl VisualizerState {
    pub fn new(visualizer_config: VisualizerConfig, text_config: TextConfig) -> Self {
        Self {
            bar_visualizer: BarVisualizer::new(visualizer_config.bars),
            text_animator: TextAnimator::new(text_config),
            visualizer_config,
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
        // Layout based on text position
        let (bars_area, text_area) = self.calculate_layout(area);

        // Render bars
        VISUALIZER_STYLES[self.current_style].render(
            frame,
            bars_area,
            audio,
            color_scheme,
            &self.visualizer_config,
        );

        // Render animated text
        self.text_animator
            .render(frame, text_area, track, audio, color_scheme, self.time);
    }

    fn calculate_layout(&self, area: Rect) -> (Rect, Rect) {
        // Calculate text height based on font style
        let text_height = self.text_height();

        match self.text_animator.position() {
            TextPosition::Top => {
                // Text at top, bars below
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(text_height), Constraint::Min(3)])
                    .split(area);
                (chunks[1], chunks[0])
            }
            TextPosition::Bottom => {
                // Bars at top, text below (default)
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(text_height)])
                    .split(area);
                (chunks[0], chunks[1])
            }
            TextPosition::Center => {
                // Text overlays center of bars
                // Bars take full area, text gets center portion
                let text_y = area.y + (area.height.saturating_sub(text_height)) / 2;
                let text_area = Rect::new(area.x, text_y, area.width, text_height);
                (area, text_area)
            }
        }
    }

    /// Get the text height based on font style
    fn text_height(&self) -> u16 {
        match self.text_animator.font_style() {
            FontStyle::Figlet => ascii_font::FIGLET_HEIGHT,
            FontStyle::Ascii => ascii_font::ASCII_HEIGHT,
            _ => 2, // Normal/Bold single-line text
        }
    }
}
