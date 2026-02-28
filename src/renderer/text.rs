//! Bitmap font text rendering for the pixel canvas.
//!
//! Renders track info using an 8×8 bitmap font, with support for font styles
//! (Normal, Bold, Ascii, Figlet), text animations (Scroll, Pulse, Fade, Wave),
//! and alignment/positioning.

use tracing::info;

use crate::config::{FontStyle, TextAlignment, TextAnimation, TextPosition};
use super::{Canvas, FrameData, RenderParams};

pub fn render_text(canvas: &mut Canvas, frame: &FrameData, params: &RenderParams) {
    let text_config = params.text_config;
    let width = canvas.width;
    let height = canvas.height;
    let track_title = frame.track_title;
    let track_artist = frame.track_artist;
    let intensity = frame.intensity;
    let time = frame.time;

    if time < 0.1 {
        info!(
            "Text config: position={:?}, alignment={:?}, font_style={:?}, animation={:?}, show_title={}, show_artist={}, margins=({},{},{})",
            text_config.position,
            text_config.alignment,
            text_config.font_style,
            text_config.animation_style,
            text_config.show_title,
            text_config.show_artist,
            text_config.margin_top,
            text_config.margin_bottom,
            text_config.margin_horizontal
        );
    }

    if !text_config.show_title && !text_config.show_artist {
        return;
    }

    // Build display text and track where title ends for color splitting
    let (text, title_len) = match (
        text_config.show_title,
        text_config.show_artist,
        track_title,
        track_artist,
    ) {
        (true, true, Some(title), Some(artist)) => {
            let combined = format!("{} - {}", title, artist);
            (combined, title.len())
        }
        (true, true, Some(title), None) => (title.clone(), title.len()),
        (true, true, None, Some(artist)) => (artist.clone(), 0),
        (true, false, Some(title), _) => (title.clone(), title.len()),
        (false, true, _, Some(artist)) => (artist.clone(), 0),
        _ => ("cavibe".to_string(), 6),
    };

    // Scale factor based on font style, proportional to canvas size.
    // Base scales are tuned for ~800px height; scale proportionally for other sizes.
    let base_scale = match text_config.font_style {
        FontStyle::Normal => 3.0,
        FontStyle::Bold => 4.0,
        FontStyle::Ascii => 2.0,
        FontStyle::Figlet => 5.0,
    };
    let size_factor = height as f32 / 800.0;
    let scale = (base_scale * size_factor).round().max(1.0) as usize;

    let char_width = 8 * scale;
    let char_height = 8 * scale;
    let char_spacing = match text_config.font_style {
        FontStyle::Bold => 2 * scale,
        _ => scale,
    };

    let text_area_height = char_height + 20;
    let margin_h = text_config.margin_horizontal as usize;

    // Calculate text Y position based on position setting
    let (base_text_y, coord_x_override) = match text_config.position {
        TextPosition::Top => (text_config.margin_top as usize, None),
        TextPosition::Bottom => (height.saturating_sub(text_area_height + text_config.margin_bottom as usize), None),
        TextPosition::Center => ((height.saturating_sub(char_height)) / 2, None),
        TextPosition::Coordinates { x, y } => (y.resolve(height), Some(x.resolve(width))),
    };

    let text_width = text.len() * (char_width + char_spacing);
    let available_width = width.saturating_sub(margin_h * 2);

    // Calculate base X position based on alignment (or coordinate override)
    let base_start_x = if let Some(cx) = coord_x_override {
        cx
    } else {
        match text_config.alignment {
            TextAlignment::Left => margin_h,
            TextAlignment::Center => margin_h + (available_width.saturating_sub(text_width)) / 2,
            TextAlignment::Right => margin_h + available_width.saturating_sub(text_width),
        }
    };

    // Apply scroll animation offset if text is wider than available space
    let scroll_offset = match text_config.animation_style {
        TextAnimation::Scroll if text_width > available_width => {
            let scroll_range = text_width - available_width + margin_h * 2;
            let scroll_speed = text_config.animation_speed * 30.0;
            let cycle_time = scroll_range as f32 / scroll_speed;
            let t = (time % (cycle_time * 2.0)) / cycle_time;
            let normalized = if t > 1.0 { 2.0 - t } else { t };
            (normalized * scroll_range as f32) as isize
        }
        _ => 0,
    };

    let y = base_text_y + (text_area_height.saturating_sub(char_height)) / 2;

    // Render background if configured
    if let Some(bg_color) = text_config.background_color {
        let bg_padding = 10;
        let bg_x_start = base_start_x.saturating_sub(bg_padding);
        let bg_x_end = (base_start_x + text_width + bg_padding).min(width);
        let bg_y_start = base_text_y.saturating_sub(bg_padding);
        let bg_y_end = (base_text_y + text_area_height + bg_padding).min(height);

        for py in bg_y_start..bg_y_end {
            for px in bg_x_start..bg_x_end {
                let idx = (py * width + px) * 4;
                if idx + 3 < canvas.data.len() {
                    canvas.data[idx] = (bg_color.r as f32 * params.opacity) as u8;
                    canvas.data[idx + 1] = (bg_color.g as f32 * params.opacity) as u8;
                    canvas.data[idx + 2] = (bg_color.b as f32 * params.opacity) as u8;
                    canvas.data[idx + 3] = (params.opacity * 255.0 * 0.8) as u8;
                }
            }
        }
    }

    // Get colors for text
    let colors: Vec<(u8, u8, u8)> = if text_config.use_color_scheme {
        params.color_scheme.get_text_gradient(text.len(), intensity * text_config.pulse_intensity, time * text_config.animation_speed)
    } else {
        let title_color = text_config.title_color.unwrap_or(crate::config::RgbColor { r: 255, g: 255, b: 255 });
        let artist_color = text_config.artist_color.unwrap_or(crate::config::RgbColor { r: 200, g: 200, b: 200 });

        text.chars().enumerate().map(|(i, _)| {
            if title_len > 0 && i >= title_len + 3 {
                (artist_color.r, artist_color.g, artist_color.b)
            } else {
                (title_color.r, title_color.g, title_color.b)
            }
        }).collect()
    };

    for (i, ch) in text.chars().enumerate() {
        let base_x = (base_start_x as isize - scroll_offset + (i * (char_width + char_spacing)) as isize) as usize;

        // Apply animation effects per character
        let (char_x, char_y, char_opacity) = match text_config.animation_style {
            TextAnimation::Wave => {
                let wave_offset = ((time * text_config.animation_speed * 3.0 + i as f32 * 0.3).sin() * 8.0) as isize;
                (base_x, (y as isize + wave_offset).max(0) as usize, params.opacity)
            }
            TextAnimation::Pulse => {
                let pulse = 0.7 + 0.3 * (intensity * text_config.pulse_intensity);
                (base_x, y, params.opacity * pulse)
            }
            TextAnimation::Fade => {
                let fade = 0.5 + 0.5 * ((time * text_config.animation_speed).sin() * 0.5 + 0.5);
                (base_x, y, params.opacity * fade)
            }
            TextAnimation::Scroll | TextAnimation::None => {
                (base_x, y, params.opacity)
            }
        };

        // Skip if character is outside visible area
        if char_x >= width || char_x + char_width > width + char_width {
            continue;
        }

        let (r, g, b) = colors.get(i).copied().unwrap_or((255, 255, 255));

        // Render with font style variations
        match text_config.font_style {
            FontStyle::Bold => {
                render_char(canvas, char_x, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, char_x + 1, char_y, ch, r, g, b, scale, char_opacity);
                render_char(canvas, char_x, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Figlet => {
                let outline_color = (r / 3, g / 3, b / 3);
                for ox in [0isize, 2].iter() {
                    for oy in [0isize, 2].iter() {
                        if *ox != 1 || *oy != 1 {
                            render_char(canvas,
                                (char_x as isize + ox) as usize,
                                (char_y as isize + oy) as usize,
                                ch, outline_color.0, outline_color.1, outline_color.2,
                                scale, char_opacity * 0.5);
                        }
                    }
                }
                render_char(canvas, char_x + 1, char_y + 1, ch, r, g, b, scale, char_opacity);
            }
            FontStyle::Normal | FontStyle::Ascii => {
                render_char(canvas, char_x, char_y, ch, r, g, b, scale, char_opacity);
            }
        }
    }
}

/// Simple 8x8 bitmap font for basic text rendering.
/// Each character is represented as 8 bytes, one per row.
fn get_char_bitmap(ch: char) -> Option<[u8; 8]> {
    let ch = ch.to_ascii_uppercase();
    Some(match ch {
        'A' => [0x18, 0x24, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x00],
        'B' => [0x7C, 0x42, 0x7C, 0x42, 0x42, 0x42, 0x7C, 0x00],
        'C' => [0x3C, 0x42, 0x40, 0x40, 0x40, 0x42, 0x3C, 0x00],
        'D' => [0x78, 0x44, 0x42, 0x42, 0x42, 0x44, 0x78, 0x00],
        'E' => [0x7E, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x7E, 0x00],
        'F' => [0x7E, 0x40, 0x7C, 0x40, 0x40, 0x40, 0x40, 0x00],
        'G' => [0x3C, 0x42, 0x40, 0x4E, 0x42, 0x42, 0x3C, 0x00],
        'H' => [0x42, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x00],
        'I' => [0x3E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x3E, 0x00],
        'J' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x44, 0x38, 0x00],
        'K' => [0x42, 0x44, 0x78, 0x48, 0x44, 0x42, 0x42, 0x00],
        'L' => [0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7E, 0x00],
        'M' => [0x42, 0x66, 0x5A, 0x42, 0x42, 0x42, 0x42, 0x00],
        'N' => [0x42, 0x62, 0x52, 0x4A, 0x46, 0x42, 0x42, 0x00],
        'O' => [0x3C, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'P' => [0x7C, 0x42, 0x42, 0x7C, 0x40, 0x40, 0x40, 0x00],
        'Q' => [0x3C, 0x42, 0x42, 0x42, 0x4A, 0x44, 0x3A, 0x00],
        'R' => [0x7C, 0x42, 0x42, 0x7C, 0x48, 0x44, 0x42, 0x00],
        'S' => [0x3C, 0x42, 0x30, 0x0C, 0x02, 0x42, 0x3C, 0x00],
        'T' => [0x7F, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00],
        'U' => [0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'V' => [0x42, 0x42, 0x42, 0x42, 0x24, 0x24, 0x18, 0x00],
        'W' => [0x42, 0x42, 0x42, 0x5A, 0x5A, 0x66, 0x42, 0x00],
        'X' => [0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x42, 0x00],
        'Y' => [0x41, 0x22, 0x14, 0x08, 0x08, 0x08, 0x08, 0x00],
        'Z' => [0x7E, 0x04, 0x08, 0x10, 0x20, 0x40, 0x7E, 0x00],
        '0' => [0x3C, 0x42, 0x46, 0x5A, 0x62, 0x42, 0x3C, 0x00],
        '1' => [0x08, 0x18, 0x28, 0x08, 0x08, 0x08, 0x3E, 0x00],
        '2' => [0x3C, 0x42, 0x02, 0x0C, 0x30, 0x40, 0x7E, 0x00],
        '3' => [0x3C, 0x42, 0x02, 0x1C, 0x02, 0x42, 0x3C, 0x00],
        '4' => [0x04, 0x0C, 0x14, 0x24, 0x7E, 0x04, 0x04, 0x00],
        '5' => [0x7E, 0x40, 0x7C, 0x02, 0x02, 0x42, 0x3C, 0x00],
        '6' => [0x1C, 0x20, 0x40, 0x7C, 0x42, 0x42, 0x3C, 0x00],
        '7' => [0x7E, 0x02, 0x04, 0x08, 0x10, 0x10, 0x10, 0x00],
        '8' => [0x3C, 0x42, 0x42, 0x3C, 0x42, 0x42, 0x3C, 0x00],
        '9' => [0x3C, 0x42, 0x42, 0x3E, 0x02, 0x04, 0x38, 0x00],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x08, 0x10],
        '!' => [0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x08, 0x00],
        '?' => [0x3C, 0x42, 0x02, 0x0C, 0x10, 0x00, 0x10, 0x00],
        ':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        '\'' => [0x08, 0x08, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00],
        '"' => [0x24, 0x24, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00],
        '(' => [0x04, 0x08, 0x10, 0x10, 0x10, 0x08, 0x04, 0x00],
        ')' => [0x20, 0x10, 0x08, 0x08, 0x08, 0x10, 0x20, 0x00],
        '&' => [0x30, 0x48, 0x30, 0x50, 0x4A, 0x44, 0x3A, 0x00],
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn render_char(canvas: &mut Canvas, x: usize, y: usize, ch: char, r: u8, g: u8, b: u8, scale: usize, opacity: f32) {
    let bitmap = match get_char_bitmap(ch) {
        Some(b) => b,
        None => return,
    };

    for (row_idx, &row) in bitmap.iter().enumerate() {
        for col in 0..8 {
            if (row >> (7 - col)) & 1 == 1 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row_idx * scale + sy;
                        if px < canvas.width && py < canvas.height {
                            canvas.put_pixel(px, py, r, g, b, opacity);
                        }
                    }
                }
            }
        }
    }
}
