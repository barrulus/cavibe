use crate::audio::AudioData;
use crate::color::ColorScheme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

pub struct BarVisualizer {
    num_bars: usize,
}

impl BarVisualizer {
    pub fn new(num_bars: usize) -> Self {
        Self { num_bars }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
        mirror: bool,
    ) {
        let block = Block::default().borders(Borders::NONE);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let bar_count = audio.frequencies.len().min(inner.width as usize);
        let bar_width = inner.width / bar_count as u16;

        for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
            let bar_height = (magnitude * inner.height as f32) as u16;
            let bar_height = bar_height.min(inner.height);

            let x = inner.x + (i as u16 * bar_width);
            let position = i as f32 / bar_count as f32;

            // Draw bar from bottom up
            for y_offset in 0..bar_height {
                let y = if mirror {
                    // Mirror mode: bars grow from center
                    inner.y + inner.height / 2 - y_offset / 2
                } else {
                    // Normal mode: bars grow from bottom
                    inner.y + inner.height - 1 - y_offset
                };

                if y >= inner.y && y < inner.y + inner.height {
                    let intensity = y_offset as f32 / inner.height as f32;
                    let (r, g, b) = color_scheme.get_color(position, intensity);

                    // Draw bar segment
                    for bx in 0..bar_width.saturating_sub(1) {
                        let cell_x = x + bx;
                        if cell_x < inner.x + inner.width {
                            let cell = frame.buffer_mut().cell_mut((cell_x, y));
                            if let Some(cell) = cell {
                                cell.set_char('█');
                                cell.set_fg(Color::Rgb(r, g, b));
                            }
                        }
                    }
                }
            }

            // Mirror: draw lower half too
            if mirror {
                for y_offset in 0..bar_height / 2 {
                    let y = inner.y + inner.height / 2 + y_offset;
                    if y < inner.y + inner.height {
                        let intensity = y_offset as f32 / inner.height as f32;
                        let (r, g, b) = color_scheme.get_color(position, intensity);

                        for bx in 0..bar_width.saturating_sub(1) {
                            let cell_x = x + bx;
                            if cell_x < inner.x + inner.width {
                                let cell = frame.buffer_mut().cell_mut((cell_x, y));
                                if let Some(cell) = cell {
                                    cell.set_char('█');
                                    cell.set_fg(Color::Rgb(r, g, b));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
