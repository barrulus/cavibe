use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::config::VisualizerConfig;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

/// Calculate evenly distributed x position using integer math to avoid truncation artifacts.
/// Returns the x coordinate for bar at index `i` out of `count` bars across `width` pixels.
#[inline]
fn bar_x_position(i: usize, count: usize, width: u16) -> u16 {
    if count == 0 {
        return 0;
    }
    // Use integer math: (i * width) / count
    // This distributes positions evenly without floating-point truncation issues
    ((i * width as usize) / count) as u16
}

/// Calculate the bar width based on config proportions.
/// Returns how many pixels to fill for each bar, accounting for the bar_width:bar_spacing ratio.
#[inline]
fn calculate_bar_dimensions(slot_width: u16, config: &VisualizerConfig) -> u16 {
    if slot_width == 0 {
        return 0;
    }
    // Calculate ratio of bar to total (bar + spacing)
    let bar_ratio = config.bar_width as f32 / (config.bar_width + config.bar_spacing) as f32;
    // Apply ratio to get draw width, minimum 1
    ((slot_width as f32 * bar_ratio).round() as u16).max(1).min(slot_width)
}

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
        config: &VisualizerConfig,
    ) {
        let block = Block::default().borders(Borders::NONE);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let bar_count = audio.frequencies.len().min(inner.width as usize);

        for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
            let bar_height = (magnitude * inner.height as f32) as u16;
            let bar_height = bar_height.min(inner.height);

            // Calculate bar position and width using integer math for even spacing
            let x_start = bar_x_position(i, bar_count, inner.width);
            let x_end = bar_x_position(i + 1, bar_count, inner.width);
            let slot_width = (x_end - x_start).max(1);

            let x = inner.x + x_start;
            let position = i as f32 / bar_count as f32;

            // Use config ratio for bar vs spacing
            let draw_width = calculate_bar_dimensions(slot_width, config);

            // Draw bar from bottom up
            for y_offset in 0..bar_height {
                let y = if config.mirror {
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
                    for bx in 0..draw_width {
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
            if config.mirror {
                for y_offset in 0..bar_height / 2 {
                    let y = inner.y + inner.height / 2 + y_offset;
                    if y < inner.y + inner.height {
                        let intensity = y_offset as f32 / inner.height as f32;
                        let (r, g, b) = color_scheme.get_color(position, intensity);

                        for bx in 0..draw_width {
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
