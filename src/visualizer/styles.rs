use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::config::VisualizerConfig;
use ratatui::prelude::*;

use super::Visualizer;

/// Calculate bar layout parameters.
/// Returns (bar_draw_width, slot_width, displayable_bar_count) where:
/// - bar_draw_width: actual width of each bar in characters
/// - slot_width: total width per bar including spacing
/// - displayable_bar_count: how many bars can fit in the available width
#[inline]
fn calculate_bar_layout(area_width: u16, bar_count: usize, config: &VisualizerConfig) -> (u16, u16, usize) {
    let bar_width = config.bar_width.max(1);
    let bar_spacing = config.bar_spacing;
    let slot_width = bar_width + bar_spacing;

    // Calculate how many bars can fit
    let max_bars = (area_width as usize) / (slot_width as usize);
    let displayable = max_bars.min(bar_count);

    (bar_width, slot_width, displayable)
}

/// Calculate x position for bar at index i with given slot width, centered in area
#[inline]
fn bar_x_position(i: usize, slot_width: u16, area_x: u16, area_width: u16, displayable_count: usize) -> u16 {
    // Center the bars in the available area
    let total_bars_width = displayable_count as u16 * slot_width;
    let start_offset = (area_width.saturating_sub(total_bars_width)) / 2;
    area_x + start_offset + (i as u16 * slot_width)
}

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
        config: &VisualizerConfig,
    ) {
        render_bars(frame, area, audio, color_scheme, config);
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
        config: &VisualizerConfig,
    ) {
        // Force mirror mode for this style
        let mut mirrored_config = config.clone();
        mirrored_config.mirror = true;
        render_bars(frame, area, audio, color_scheme, &mirrored_config);
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
        config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
            return;
        }

        let center_y = area.y + area.height / 2;
        let bar_count = audio.frequencies.len();
        let (_, slot_width, displayable) = calculate_bar_layout(area.width, bar_count, config);

        if displayable == 0 {
            return;
        }

        for i in 0..displayable {
            let freq_idx = (i * bar_count) / displayable;
            let magnitude = audio.frequencies[freq_idx];

            let x = bar_x_position(i, slot_width, area.x, area.width, displayable);
            if x >= area.x + area.width {
                break;
            }

            let wave_height = (magnitude * (area.height as f32 / 2.0)) as i16;
            let position = i as f32 / displayable as f32;

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
        config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
            return;
        }

        let bar_count = audio.frequencies.len();
        let (_, slot_width, displayable) = calculate_bar_layout(area.width, bar_count, config);

        if displayable == 0 {
            return;
        }

        for i in 0..displayable {
            let freq_idx = (i * bar_count) / displayable;
            let magnitude = audio.frequencies[freq_idx];

            let x = bar_x_position(i, slot_width, area.x, area.width, displayable);
            if x >= area.x + area.width {
                break;
            }

            let dot_y = area.y + area.height - 1 - (magnitude * (area.height - 1) as f32) as u16;
            let position = i as f32 / displayable as f32;
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
        config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
            return;
        }

        let block_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let bar_count = audio.frequencies.len();
        let (draw_width, slot_width, displayable) = calculate_bar_layout(area.width, bar_count, config);

        if displayable == 0 {
            return;
        }

        for i in 0..displayable {
            let freq_idx = (i * bar_count) / displayable;
            let magnitude = audio.frequencies[freq_idx];

            let x = bar_x_position(i, slot_width, area.x, area.width, displayable);
            if x >= area.x + area.width {
                break;
            }

            let position = i as f32 / displayable as f32;
            let full_blocks = (magnitude * area.height as f32) as u16;
            let partial = ((magnitude * area.height as f32) % 1.0 * 8.0) as usize;

            // Draw full blocks
            for b in 0..full_blocks.min(area.height) {
                let y = area.y + area.height - 1 - b;
                let intensity = b as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..draw_width {
                    if x + bx < area.x + area.width {
                        let cell = frame.buffer_mut().cell_mut((x + bx, y));
                        if let Some(cell) = cell {
                            cell.set_char('█');
                            cell.set_fg(Color::Rgb(r, g, b));
                        }
                    }
                }
            }

            // Draw partial block on top
            if full_blocks < area.height && partial > 0 {
                let y = area.y + area.height - 1 - full_blocks;
                let intensity = full_blocks as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..draw_width {
                    if x + bx < area.x + area.width {
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
}

fn render_bars(
    frame: &mut Frame,
    area: Rect,
    audio: &AudioData,
    color_scheme: &ColorScheme,
    config: &VisualizerConfig,
) {
    if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
        return;
    }

    let bar_count = audio.frequencies.len();
    let (draw_width, slot_width, displayable) = calculate_bar_layout(area.width, bar_count, config);

    if displayable == 0 {
        return;
    }

    // Sample frequencies if we have more data than displayable bars
    for i in 0..displayable {
        // Map displayable index to frequency index
        let freq_idx = (i * bar_count) / displayable;
        let magnitude = audio.frequencies[freq_idx];

        let bar_height = (magnitude * area.height as f32) as u16;
        let bar_height = bar_height.min(area.height);

        let x = bar_x_position(i, slot_width, area.x, area.width, displayable);
        let position = i as f32 / displayable as f32;

        if config.mirror {
            let half_height = bar_height / 2;
            let center = area.y + area.height / 2;

            for y_offset in 0..half_height {
                let intensity = y_offset as f32 / (area.height / 2) as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                // Upper half
                let y_up = center.saturating_sub(y_offset);
                if y_up >= area.y {
                    for bx in 0..draw_width {
                        if x + bx < area.x + area.width {
                            let cell = frame.buffer_mut().cell_mut((x + bx, y_up));
                            if let Some(cell) = cell {
                                cell.set_char('█');
                                cell.set_fg(Color::Rgb(r, g, b));
                            }
                        }
                    }
                }

                // Lower half
                let y_down = center + y_offset;
                if y_down < area.y + area.height {
                    for bx in 0..draw_width {
                        if x + bx < area.x + area.width {
                            let cell = frame.buffer_mut().cell_mut((x + bx, y_down));
                            if let Some(cell) = cell {
                                cell.set_char('█');
                                cell.set_fg(Color::Rgb(r, g, b));
                            }
                        }
                    }
                }
            }
        } else {
            for y_offset in 0..bar_height {
                let y = area.y + area.height - 1 - y_offset;
                let intensity = y_offset as f32 / area.height as f32;
                let (r, g, b) = color_scheme.get_color(position, intensity);

                for bx in 0..draw_width {
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

/// Oscilloscope style - raw waveform display using braille characters
pub struct OscilloscopeStyle;

impl Visualizer for OscilloscopeStyle {
    fn name(&self) -> &'static str {
        "Oscilloscope"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
        _config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.waveform.is_empty() {
            return;
        }

        // Braille grid: each character cell is 2 dots wide x 4 dots tall
        let grid_w = area.width as usize * 2;
        let grid_h = area.height as usize * 4;

        // Allocate dot grid
        let mut grid = vec![false; grid_w * grid_h];

        let num_samples = audio.waveform.len();
        let center_y = grid_h as f32 / 2.0;

        // Map waveform samples to grid positions and connect with lines
        let mut prev_gx: Option<usize> = None;
        let mut prev_gy: Option<usize> = None;

        for gx in 0..grid_w {
            // Map grid x to sample index
            let sample_idx = (gx * num_samples) / grid_w;
            let sample = audio.waveform[sample_idx.min(num_samples - 1)];

            // Map sample (-1..1) to grid y
            let gy = ((center_y - sample * center_y) as usize).min(grid_h - 1);

            if let (Some(px), Some(py)) = (prev_gx, prev_gy) {
                // Bresenham line from (px, py) to (gx, gy)
                bresenham_line(&mut grid, grid_w, grid_h, px, py, gx, gy);
            } else {
                // First point
                grid[gy * grid_w + gx] = true;
            }

            prev_gx = Some(gx);
            prev_gy = Some(gy);
        }

        // Convert dot grid to braille characters
        // Braille dot positions within a 2x4 cell:
        // (0,0)=0x01 (1,0)=0x08
        // (0,1)=0x02 (1,1)=0x10
        // (0,2)=0x04 (1,2)=0x20
        // (0,3)=0x40 (1,3)=0x80
        let dot_map: [[u8; 4]; 2] = [
            [0x01, 0x02, 0x04, 0x40],
            [0x08, 0x10, 0x20, 0x80],
        ];

        for cy in 0..area.height as usize {
            for cx in 0..area.width as usize {
                let mut braille: u8 = 0;
                let mut has_dots = false;

                for (dx, col) in dot_map.iter().enumerate() {
                    for (dy, &bit) in col.iter().enumerate() {
                        let gx = cx * 2 + dx;
                        let gy = cy * 4 + dy;
                        if gx < grid_w && gy < grid_h && grid[gy * grid_w + gx] {
                            braille |= bit;
                            has_dots = true;
                        }
                    }
                }

                if has_dots {
                    let position = cx as f32 / area.width as f32;
                    // Use the average waveform intensity for color
                    let sample_idx = (cx * 2 * num_samples) / grid_w;
                    let intensity = audio.waveform[sample_idx.min(num_samples - 1)].abs();
                    let (r, g, b) = color_scheme.get_color(position, intensity.min(1.0));

                    let ch = char::from_u32(0x2800 + braille as u32).unwrap_or(' ');
                    let cell = frame.buffer_mut().cell_mut(
                        (area.x + cx as u16, area.y + cy as u16),
                    );
                    if let Some(cell) = cell {
                        cell.set_char(ch);
                        cell.set_fg(Color::Rgb(r, g, b));
                    }
                }
            }
        }
    }
}

/// Draw a line on the dot grid using Bresenham's algorithm
fn bresenham_line(grid: &mut [bool], grid_w: usize, grid_h: usize, x0: usize, y0: usize, x1: usize, y1: usize) {
    let mut x0 = x0 as isize;
    let mut y0 = y0 as isize;
    let x1 = x1 as isize;
    let y1 = y1 as isize;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: isize = if x0 < x1 { 1 } else { -1 };
    let sy: isize = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 >= 0 && x0 < grid_w as isize && y0 >= 0 && y0 < grid_h as isize {
            grid[y0 as usize * grid_w + x0 as usize] = true;
        }

        if x0 == x1 && y0 == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// All available visualizer styles
pub static VISUALIZER_STYLES: &[&(dyn Visualizer + Sync)] = &[
    &ClassicBars,
    &MirroredBars,
    &WaveStyle,
    &DotsStyle,
    &BlocksStyle,
    &OscilloscopeStyle,
];
