use crate::audio::AudioData;
use crate::color::ColorScheme;
use ratatui::prelude::*;

use super::Visualizer;

/// Classic vertical bars style
pub struct ClassicBars;

impl Visualizer for ClassicBars {
    fn name(&self) -> &'static str {
        "Classic Bars"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    ) {
        render_bars(frame, area, audio, color_scheme, false);
    }
}

/// Mirrored bars (grow from center)
pub struct MirroredBars;

impl Visualizer for MirroredBars {
    fn name(&self) -> &'static str {
        "Mirrored"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    ) {
        render_bars(frame, area, audio, color_scheme, true);
    }
}

/// Wave style visualization
pub struct WaveStyle;

impl Visualizer for WaveStyle {
    fn name(&self) -> &'static str {
        "Wave"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let center_y = area.y + area.height / 2;

        for (i, &magnitude) in audio.frequencies.iter().enumerate() {
            let x = area.x + (i as u16 * area.width / audio.frequencies.len() as u16);
            if x >= area.x + area.width {
                break;
            }

            let wave_height = (magnitude * (area.height as f32 / 2.0)) as i16;
            let position = i as f32 / audio.frequencies.len() as f32;

            // Draw wave line
            for offset in -wave_height..=wave_height {
                let y = (center_y as i16 + offset) as u16;
                if y >= area.y && y < area.y + area.height {
                    let intensity = 1.0 - (offset.abs() as f32 / wave_height.max(1) as f32);
                    let (r, g, b) = color_scheme.get_color(position, intensity);

                    let cell = frame.buffer_mut().cell_mut((x, y));
                    if let Some(cell) = cell {
                        cell.set_char('│');
                        cell.set_fg(Color::Rgb(r, g, b));
                    }
                }
            }
        }
    }
}

/// Dots/scatter style
pub struct DotsStyle;

impl Visualizer for DotsStyle {
    fn name(&self) -> &'static str {
        "Dots"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        for (i, &magnitude) in audio.frequencies.iter().enumerate() {
            let x = area.x + (i as u16 * area.width / audio.frequencies.len() as u16);
            if x >= area.x + area.width {
                break;
            }

            let dot_y = area.y + area.height - 1 - (magnitude * (area.height - 1) as f32) as u16;
            let position = i as f32 / audio.frequencies.len() as f32;
            let (r, g, b) = color_scheme.get_color(position, magnitude);

            // Draw dot
            if dot_y >= area.y && dot_y < area.y + area.height {
                let cell = frame.buffer_mut().cell_mut((x, dot_y));
                if let Some(cell) = cell {
                    cell.set_char('●');
                    cell.set_fg(Color::Rgb(r, g, b));
                }
            }

            // Draw trail below dot
            for y in (dot_y + 1)..(area.y + area.height) {
                let trail_intensity = 1.0 - ((y - dot_y) as f32 / (area.height / 2) as f32);
                if trail_intensity > 0.0 {
                    let (r, g, b) = color_scheme.get_color(position, trail_intensity * magnitude);
                    let cell = frame.buffer_mut().cell_mut((x, y));
                    if let Some(cell) = cell {
                        cell.set_char('·');
                        cell.set_fg(Color::Rgb(r, g, b));
                    }
                }
            }
        }
    }
}

/// Blocks/levels style
pub struct BlocksStyle;

impl Visualizer for BlocksStyle {
    fn name(&self) -> &'static str {
        "Blocks"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let block_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let bar_width = (area.width / audio.frequencies.len() as u16).max(1);

        for (i, &magnitude) in audio.frequencies.iter().enumerate() {
            let x = area.x + (i as u16 * bar_width);
            if x >= area.x + area.width {
                break;
            }

            let position = i as f32 / audio.frequencies.len() as f32;
            let full_blocks = (magnitude * area.height as f32) as u16;
            let partial = ((magnitude * area.height as f32) % 1.0 * 8.0) as usize;

            // Draw full blocks
            for b in 0..full_blocks.min(area.height) {
                let y = area.y + area.height - 1 - b;
                let intensity = b as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..bar_width.saturating_sub(1).max(1) {
                    let cell = frame.buffer_mut().cell_mut((x + bx, y));
                    if let Some(cell) = cell {
                        cell.set_char('█');
                        cell.set_fg(Color::Rgb(r, g, b));
                    }
                }
            }

            // Draw partial block on top
            if full_blocks < area.height && partial > 0 {
                let y = area.y + area.height - 1 - full_blocks;
                let intensity = full_blocks as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..bar_width.saturating_sub(1).max(1) {
                    let cell = frame.buffer_mut().cell_mut((x + bx, y));
                    if let Some(cell) = cell {
                        cell.set_char(block_chars[partial.min(7)]);
                        cell.set_fg(Color::Rgb(r, g, b));
                    }
                }
            }
        }
    }
}

fn render_bars(
    frame: &mut Frame,
    area: Rect,
    audio: &AudioData,
    color_scheme: &ColorScheme,
    mirror: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar_count = audio.frequencies.len().min(area.width as usize);
    let bar_width = (area.width / bar_count as u16).max(1);

    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let bar_height = (magnitude * area.height as f32) as u16;
        let bar_height = bar_height.min(area.height);

        let x = area.x + (i as u16 * bar_width);
        let position = i as f32 / bar_count as f32;

        if mirror {
            let half_height = bar_height / 2;
            let center = area.y + area.height / 2;

            for y_offset in 0..half_height {
                let intensity = y_offset as f32 / (area.height / 2) as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                // Upper half
                let y_up = center.saturating_sub(y_offset);
                if y_up >= area.y {
                    for bx in 0..bar_width.saturating_sub(1).max(1) {
                        let cell = frame.buffer_mut().cell_mut((x + bx, y_up));
                        if let Some(cell) = cell {
                            cell.set_char('█');
                            cell.set_fg(Color::Rgb(r, g, b));
                        }
                    }
                }

                // Lower half
                let y_down = center + y_offset;
                if y_down < area.y + area.height {
                    for bx in 0..bar_width.saturating_sub(1).max(1) {
                        let cell = frame.buffer_mut().cell_mut((x + bx, y_down));
                        if let Some(cell) = cell {
                            cell.set_char('█');
                            cell.set_fg(Color::Rgb(r, g, b));
                        }
                    }
                }
            }
        } else {
            for y_offset in 0..bar_height {
                let y = area.y + area.height - 1 - y_offset;
                let intensity = y_offset as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..bar_width.saturating_sub(1).max(1) {
                    if x + bx < area.x + area.width {
                        let cell = frame.buffer_mut().cell_mut((x + bx, y));
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

/// All available visualizer styles
pub static VISUALIZER_STYLES: &[&dyn Visualizer] = &[
    &ClassicBars,
    &MirroredBars,
    &WaveStyle,
    &DotsStyle,
    &BlocksStyle,
];
