//! All 8 visualization style render functions.
//!
//! Each function takes a `Canvas`, a `BarLayout`, and `RenderParams` and
//! writes pixels directly into the canvas buffer.

use super::layout::{compute_bar_layout, BarLayout};
use super::{Canvas, RenderParams};

/// Human-readable names for each style, indexed by style number.
pub const STYLE_NAMES: &[&str] = &[
    "Classic Bars",
    "Mirrored",
    "Wave",
    "Dots",
    "Blocks",
    "Oscilloscope",
    "Spectrogram",
    "Radial",
];

/// Total number of styles.
pub fn style_count() -> usize {
    STYLE_NAMES.len()
}

/// Dispatch to the correct style renderer.
pub fn render_bars(canvas: &mut Canvas, frequencies: &[f32], params: &RenderParams) {
    let layout = match compute_bar_layout(canvas.width, canvas.height, frequencies, params) {
        Some(l) => l,
        None => return,
    };

    match params.style {
        1 => render_bars_mirrored(canvas, &layout, params),
        2 => render_bars_wave(canvas, &layout, params),
        3 => render_bars_dots(canvas, &layout, params),
        4 => render_bars_blocks(canvas, &layout, params),
        5 => render_bars_oscilloscope(canvas, &layout, params),
        6 => render_bars_spectrogram(canvas, &layout, params),
        7 => render_bars_radial(canvas, &layout, params),
        _ => render_bars_classic(canvas, &layout, params),
    }
}

/// Style 0: Classic vertical bars from bottom
fn render_bars_classic(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let bar_height = (magnitude * layout.bars_height as f32) as usize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for y_offset in 0..bar_height.min(layout.bars_height) {
            let y = layout.bars_y_start + layout.bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / layout.bars_height as f32;
            let (r, g, b) = params.color_scheme.get_color(position, intensity);

            for bx in 0..params.bar_width {
                let x = x_start + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, r, g, b, params.opacity);
                }
            }
        }
    }
}

/// Style 1: Mirrored bars growing from center
fn render_bars_mirrored(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let center_y = layout.bars_y_start + layout.bars_height / 2;

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let half_height = (magnitude * layout.bars_height as f32 / 2.0) as usize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for y_offset in 0..half_height.min(layout.bars_height / 2) {
            let intensity = y_offset as f32 / (layout.bars_height as f32 / 2.0);
            let (r, g, b) = params.color_scheme.get_color(position, intensity);

            // Upper half
            let y_up = center_y.saturating_sub(y_offset);
            if y_up >= layout.bars_y_start {
                for bx in 0..params.bar_width {
                    let x = x_start + bx;
                    if x < canvas.width && y_up < canvas.height {
                        canvas.put_pixel(x, y_up, r, g, b, params.opacity);
                    }
                }
            }

            // Lower half
            let y_down = center_y + y_offset;
            if y_down < layout.bars_y_start + layout.bars_height {
                for bx in 0..params.bar_width {
                    let x = x_start + bx;
                    if x < canvas.width && y_down < canvas.height {
                        canvas.put_pixel(x, y_down, r, g, b, params.opacity);
                    }
                }
            }
        }
    }
}

/// Style 2: Wave centered on middle row
fn render_bars_wave(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let center_y = layout.bars_y_start + layout.bars_height / 2;
    let wave_width = (params.bar_width / 3).max(1);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let wave_height = (magnitude * layout.bars_height as f32 / 2.0) as isize;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        for offset in -wave_height..=wave_height {
            let y = (center_y as isize + offset) as usize;
            if y >= layout.bars_y_start && y < layout.bars_y_start + layout.bars_height && y < canvas.height {
                let intensity = 1.0 - (offset.unsigned_abs() as f32 / wave_height.max(1) as f32);
                let (r, g, b) = params.color_scheme.get_color(position, intensity);

                for bx in 0..wave_width {
                    let x = x_start + bx;
                    if x < canvas.width {
                        canvas.put_pixel(x, y, r, g, b, params.opacity * intensity);
                    }
                }
            }
        }
    }
}

/// Style 3: Dots at peak with trailing dots below
fn render_bars_dots(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let dot_radius = (params.bar_width / 3).max(2);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let peak_y = layout.bars_y_start + layout.bars_height - 1
            - (magnitude * (layout.bars_height - 1) as f32) as usize;
        let x_center = layout.start_x + i * layout.slot_width + params.bar_width / 2;
        let position = i as f32 / layout.displayable as f32;
        let (r, g, b) = params.color_scheme.get_color(position, magnitude);

        // Draw dot (filled circle)
        let r2 = (dot_radius * dot_radius) as isize;
        for dy in -(dot_radius as isize)..=(dot_radius as isize) {
            for dx in -(dot_radius as isize)..=(dot_radius as isize) {
                if dx * dx + dy * dy <= r2 {
                    let x = (x_center as isize + dx) as usize;
                    let y = (peak_y as isize + dy) as usize;
                    if x < canvas.width && y >= layout.bars_y_start && y < layout.bars_y_start + layout.bars_height && y < canvas.height {
                        canvas.put_pixel(x, y, r, g, b, params.opacity);
                    }
                }
            }
        }

        // Draw trail below dot
        let trail_width = (params.bar_width / 4).max(1);
        let trail_start = peak_y + dot_radius + 1;
        let trail_end = layout.bars_y_start + layout.bars_height;
        for y in trail_start..trail_end {
            let trail_intensity = 1.0 - ((y - trail_start) as f32 / (layout.bars_height as f32 / 2.0));
            if trail_intensity <= 0.0 {
                break;
            }
            let (tr, tg, tb) = params.color_scheme.get_color(position, trail_intensity * magnitude);
            for bx in 0..trail_width {
                let x = x_center - trail_width / 2 + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, tr, tg, tb, params.opacity * trail_intensity);
                }
            }
        }
    }
}

/// Style 4: Blocks with gradient fade at top edge
fn render_bars_blocks(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let fade_height = (params.bar_width / 2).max(2);

    for i in 0..layout.displayable {
        let magnitude = layout.render_frequencies[i];
        let bar_height_f = magnitude * layout.bars_height as f32;
        let bar_height = bar_height_f as usize;
        let fractional = bar_height_f - bar_height as f32;
        let x_start = layout.start_x + i * layout.slot_width;
        let position = i as f32 / layout.displayable as f32;

        // Draw solid portion
        for y_offset in 0..bar_height.min(layout.bars_height) {
            let y = layout.bars_y_start + layout.bars_height - 1 - y_offset;
            let intensity = y_offset as f32 / layout.bars_height as f32;
            let (r, g, b) = params.color_scheme.get_color(position, intensity);

            for bx in 0..params.bar_width {
                let x = x_start + bx;
                if x < canvas.width && y < canvas.height {
                    canvas.put_pixel(x, y, r, g, b, params.opacity);
                }
            }
        }

        // Draw gradient fade at top edge
        if bar_height < layout.bars_height {
            let top_y = layout.bars_y_start + layout.bars_height - 1 - bar_height;
            let intensity = bar_height as f32 / layout.bars_height as f32;
            let (r, g, b) = params.color_scheme.get_color(position, intensity);

            for fy in 0..fade_height.min(top_y.saturating_sub(layout.bars_y_start)) {
                let y = top_y - fy;
                let fade = fractional * (1.0 - fy as f32 / fade_height as f32);
                if y < canvas.height {
                    for bx in 0..params.bar_width {
                        let x = x_start + bx;
                        if x < canvas.width {
                            canvas.put_pixel(x, y, r, g, b, params.opacity * fade);
                        }
                    }
                }
            }
        }
    }
}

/// Style 5: Oscilloscope — raw waveform as a continuous line
fn render_bars_oscilloscope(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    if params.waveform.is_empty() {
        return;
    }

    let num_samples = params.waveform.len();
    let center_y = layout.bars_y_start + layout.bars_height / 2;
    let half_height = layout.bars_height as f32 / 2.0;
    let thickness = (params.bar_width / 4).max(1);

    let mut prev_y: Option<usize> = None;

    for x in 0..canvas.width {
        let sample_idx = (x * num_samples) / canvas.width;
        let sample = params.waveform[sample_idx.min(num_samples - 1)];

        let y = ((center_y as f32 - sample * half_height) as usize)
            .max(layout.bars_y_start)
            .min(layout.bars_y_start + layout.bars_height - 1);

        let position = x as f32 / canvas.width as f32;
        let intensity = sample.abs().min(1.0);
        let (r, g, b) = params.color_scheme.get_color(position, intensity.max(0.3));

        let y_min;
        let y_max;
        if let Some(py) = prev_y {
            y_min = py.min(y);
            y_max = py.max(y);
        } else {
            y_min = y;
            y_max = y;
        }

        for fill_y in y_min..=y_max {
            for t in 0..thickness {
                let py = fill_y + t;
                if x < canvas.width && py >= layout.bars_y_start && py < layout.bars_y_start + layout.bars_height && py < canvas.height {
                    canvas.put_pixel(x, py, r, g, b, params.opacity);
                }
            }
        }

        prev_y = Some(y);
    }
}

/// Style 6: Spectrogram — scrolling 2D heatmap (X=frequency, Y=time)
fn render_bars_spectrogram(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let history = params.spectrogram_history;
    if history.is_empty() {
        return;
    }

    let num_rows = history.len().min(layout.bars_height);

    for (row_idx, slice) in history.iter().rev().take(num_rows).enumerate() {
        let y = layout.bars_y_start + layout.bars_height - 1 - row_idx;
        if y >= layout.bars_y_start + layout.bars_height {
            continue;
        }

        let num_freqs = slice.len();
        if num_freqs == 0 {
            continue;
        }

        for x in 0..canvas.width {
            let freq_idx = (x * num_freqs) / canvas.width;
            let magnitude = slice[freq_idx.min(num_freqs - 1)];
            let position = x as f32 / canvas.width as f32;
            let (r, g, b) = params.color_scheme.get_color(position, magnitude);
            canvas.put_pixel(x, y, r, g, b, params.opacity * magnitude.max(0.05));
        }
    }
}

/// Style 7: Radial — frequency bars radiating outward from a circle
fn render_bars_radial(canvas: &mut Canvas, layout: &BarLayout, params: &RenderParams) {
    let cx = canvas.width as f32 / 2.0;
    let cy = (layout.bars_y_start as f32) + layout.bars_height as f32 / 2.0;
    let half_dim = (canvas.width.min(layout.bars_height) as f32) / 2.0;
    let base_radius = half_dim * 0.35;
    let max_radius = half_dim * 0.95;
    let thickness = (params.bar_width / 3).max(2);

    let bar_count = layout.render_frequencies.len();
    if bar_count == 0 {
        return;
    }

    // Draw base circle
    let circle_steps = (base_radius * std::f32::consts::TAU).ceil() as usize;
    for step in 0..circle_steps {
        let angle = (step as f32 / circle_steps as f32) * std::f32::consts::TAU;
        let px = (cx + angle.cos() * base_radius).round() as usize;
        let py = (cy + angle.sin() * base_radius).round() as usize;
        let position = (angle + std::f32::consts::FRAC_PI_2) / std::f32::consts::TAU;
        let position = position.rem_euclid(1.0);
        let (r, g, b) = params.color_scheme.get_color(position, 0.3);
        for t in 0..thickness {
            let tx = px + t;
            if tx < canvas.width && py >= layout.bars_y_start && py < layout.bars_y_start + layout.bars_height && py < canvas.height {
                canvas.put_pixel(tx, py, r, g, b, params.opacity * 0.5);
            }
        }
    }

    // Draw radial bars
    for i in 0..bar_count {
        let magnitude = layout.render_frequencies[i];
        if magnitude < 0.01 {
            continue;
        }
        let angle = -std::f32::consts::FRAC_PI_2
            + (i as f32 / bar_count as f32) * std::f32::consts::TAU;
        let bar_length = magnitude * (max_radius - base_radius);
        let position = i as f32 / bar_count as f32;

        let steps = (bar_length.ceil() as usize).max(1);
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        for s in 0..=steps {
            let r_dist = base_radius + (s as f32 / steps as f32) * bar_length;
            let px = (cx + cos_a * r_dist).round() as isize;
            let py_val = (cy + sin_a * r_dist).round() as isize;
            let intensity = (r_dist - base_radius) / (max_radius - base_radius);
            let (r, g, b) = params.color_scheme.get_color(position, magnitude * 0.5 + intensity * 0.5);

            for t in -(thickness as isize / 2)..=(thickness as isize / 2) {
                let tx = (px as f32 - sin_a * t as f32).round() as usize;
                let ty = (py_val as f32 + cos_a * t as f32).round() as usize;
                if tx < canvas.width && ty >= layout.bars_y_start && ty < layout.bars_y_start + layout.bars_height && ty < canvas.height {
                    canvas.put_pixel(tx, ty, r, g, b, params.opacity);
                }
            }
        }
    }
}
