use std::sync::Mutex;

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

        let mut canvas = super::braille::BrailleCanvas::new(area.width as usize, area.height as usize);
        let num_samples = audio.waveform.len();
        let center_y = canvas.grid_h as f32 / 2.0;

        // Map waveform samples to grid positions and connect with lines
        let mut prev_gx: Option<usize> = None;
        let mut prev_gy: Option<usize> = None;

        for gx in 0..canvas.grid_w {
            let sample_idx = (gx * num_samples) / canvas.grid_w;
            let sample = audio.waveform[sample_idx.min(num_samples - 1)];
            let gy = ((center_y - sample * center_y) as usize).min(canvas.grid_h - 1);

            if let (Some(px), Some(py)) = (prev_gx, prev_gy) {
                canvas.line(px, py, gx, gy);
            } else {
                canvas.set(gx, gy);
            }

            prev_gx = Some(gx);
            prev_gy = Some(gy);
        }

        let waveform = &audio.waveform;
        let grid_w = canvas.grid_w;
        canvas.render(frame, area, |cx, _cy| {
            let position = cx as f32 / area.width as f32;
            let sample_idx = (cx * 2 * num_samples) / grid_w;
            let intensity = waveform[sample_idx.min(num_samples - 1)].abs();
            Some(color_scheme.get_color(position, intensity.min(1.0)))
        });
    }
}

/// Spectrogram/waterfall style - scrolling 2D heatmap where X=frequency, Y=time
pub struct SpectrogramStyle {
    history: Mutex<Vec<Vec<f32>>>,
}

impl SpectrogramStyle {
    const fn new() -> Self {
        Self {
            history: Mutex::new(Vec::new()),
        }
    }
}

impl Visualizer for SpectrogramStyle {
    fn name(&self) -> &'static str {
        "Spectrogram"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
        _config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
            return;
        }

        let max_rows = area.height as usize;
        let mut history = self.history.lock().unwrap();

        // Push current frame and trim to visible height
        history.push(audio.frequencies.clone());
        if history.len() > max_rows {
            let excess = history.len() - max_rows;
            history.drain(..excess);
        }

        let num_cols = area.width as usize;

        // Render: index 0 = oldest (top), last = newest (bottom)
        for (row_idx, slice) in history.iter().enumerate() {
            let y = area.y + row_idx as u16;
            if y >= area.y + area.height {
                break;
            }

            for col in 0..num_cols {
                let freq_idx = (col * slice.len()) / num_cols;
                let magnitude = slice[freq_idx.min(slice.len() - 1)];
                let position = col as f32 / num_cols as f32;
                let (r, g, b) = color_scheme.get_color(position, magnitude);

                let cell = frame.buffer_mut().cell_mut((area.x + col as u16, y));
                if let Some(cell) = cell {
                    cell.set_char('█');
                    cell.set_fg(Color::Rgb(r, g, b));
                }
            }
        }
    }
}

static SPECTROGRAM_STYLE: SpectrogramStyle = SpectrogramStyle::new();

/// Radial bars style — frequency bars radiate outward from a circle
pub struct RadialBarsStyle;

impl Visualizer for RadialBarsStyle {
    fn name(&self) -> &'static str {
        "Radial"
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &AudioData,
        color_scheme: &ColorScheme,
        _config: &VisualizerConfig,
    ) {
        if area.width == 0 || area.height == 0 || audio.frequencies.is_empty() {
            return;
        }

        let char_w = area.width as usize;
        let char_h = area.height as usize;
        let mut canvas = super::braille::BrailleCanvas::new(char_w, char_h);

        // Fit a circle in braille grid space. Terminal aspect ratio ~0.5 (chars are ~2x tall
        // as they are wide), but in braille grid coords each char is 2 wide × 4 tall,
        // so effective pixel aspect is (2/4) * (char_w/char_h) → the braille grid already
        // halves the distortion. We use aspect=0.5 so fit_circle compensates correctly.
        let (cx, cy, max_radius) =
            super::radial::fit_circle(canvas.grid_w, canvas.grid_h, 0.5);

        let base_radius = max_radius * 0.4;
        let bar_count = audio.frequencies.len();

        // Draw base circle outline
        let circle_steps = (base_radius * std::f32::consts::TAU).ceil() as usize;
        if circle_steps > 0 {
            let mut prev: Option<(usize, usize)> = None;
            for step in 0..=circle_steps {
                let angle = (step as f32 / circle_steps as f32) * std::f32::consts::TAU;
                let (gx, gy) = super::radial::polar_to_grid(cx, cy, angle, base_radius);
                let gx = gx.round() as usize;
                let gy = gy.round() as usize;
                if let Some((px, py)) = prev {
                    canvas.line(px, py, gx, gy);
                } else {
                    canvas.set(gx, gy);
                }
                prev = Some((gx, gy));
            }
        }

        // Draw radial bars for each frequency bin
        // Angle 0 = top (12 o'clock), proceeding clockwise
        for i in 0..bar_count {
            let magnitude = audio.frequencies[i];
            if magnitude < 0.01 {
                continue;
            }
            // Map to angle: start at -PI/2 (top), go clockwise (positive angle)
            let angle =
                -std::f32::consts::FRAC_PI_2 + (i as f32 / bar_count as f32) * std::f32::consts::TAU;
            let bar_length = magnitude * (max_radius - base_radius);

            let (x0, y0) = super::radial::polar_to_grid(cx, cy, angle, base_radius);
            let (x1, y1) = super::radial::polar_to_grid(cx, cy, angle, base_radius + bar_length);

            canvas.line(
                x0.round() as usize,
                y0.round() as usize,
                x1.round() as usize,
                y1.round() as usize,
            );
        }

        // Render with color based on angle position and magnitude
        let grid_w = canvas.grid_w;
        let grid_h = canvas.grid_h;
        let cx_f = cx;
        let cy_f = cy;
        canvas.render(frame, area, |char_cx, char_cy| {
            // Map character cell center back to angle to determine color
            let gx = (char_cx * 2) as f32 + 1.0;
            let gy = (char_cy * 4) as f32 + 2.0;
            if gx >= grid_w as f32 || gy >= grid_h as f32 {
                return None;
            }
            let dx = gx - cx_f;
            let dy = gy - cy_f;
            let angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
            let position = (angle / std::f32::consts::TAU).rem_euclid(1.0);
            let dist = (dx * dx + dy * dy).sqrt();
            let intensity = ((dist - base_radius) / (max_radius - base_radius)).clamp(0.0, 1.0);
            Some(color_scheme.get_color(position, intensity))
        });
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
    &SPECTROGRAM_STYLE,
    &RadialBarsStyle,
];
