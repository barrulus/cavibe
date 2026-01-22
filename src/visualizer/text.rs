use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::metadata::TrackInfo;
use ratatui::prelude::*;
use std::sync::Arc;

pub struct TextAnimator {
    scroll_offset: f32,
    pulse_phase: f32,
}

impl TextAnimator {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0.0,
            pulse_phase: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.scroll_offset += dt * 20.0; // Scroll speed
        self.pulse_phase += dt * 3.0;
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        track: &Arc<TrackInfo>,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let text = track.display_text();
        if text.is_empty() {
            // Show placeholder when no track info
            self.render_placeholder(frame, area, audio, color_scheme, time);
            return;
        }

        // Calculate text positioning
        let text_chars: Vec<char> = text.chars().collect();
        let text_len = text_chars.len();

        // Get colors for each character with gradient and pulse effect
        let colors = color_scheme.get_text_gradient(text_len, audio.intensity, time);

        // Center text or scroll if too long
        let start_x = if text_len <= area.width as usize {
            area.x + (area.width - text_len as u16) / 2
        } else {
            // Scrolling text
            let scroll = (self.scroll_offset as usize) % (text_len + area.width as usize);
            area.x.saturating_sub(scroll as u16)
        };

        let y = area.y + area.height / 2;

        // Render each character with its color
        for (i, ch) in text_chars.iter().enumerate() {
            let x = start_x.saturating_add(i as u16);

            if x >= area.x && x < area.x + area.width && y < area.y + area.height {
                let (r, g, b) = colors.get(i).copied().unwrap_or((255, 255, 255));

                // Add bass pulse effect to brightness
                let pulse = 1.0 + audio.bass * 0.3 * (self.pulse_phase + i as f32 * 0.1).sin();
                let r = ((r as f32 * pulse).min(255.0)) as u8;
                let g = ((g as f32 * pulse).min(255.0)) as u8;
                let b = ((b as f32 * pulse).min(255.0)) as u8;

                let cell = frame.buffer_mut().cell_mut((x, y));
                if let Some(cell) = cell {
                    cell.set_char(*ch);
                    cell.set_fg(Color::Rgb(r, g, b));
                    // Bold on beat
                    if audio.bass > 0.5 {
                        cell.set_style(Style::default().bold());
                    }
                }
            }
        }

        // Draw intensity bar below text
        self.render_intensity_bar(frame, area, audio, color_scheme);
    }

    fn render_placeholder(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
    ) {
        let text = "♪ cavibe ♪";
        let text_chars: Vec<char> = text.chars().collect();
        let text_len = text_chars.len();

        let colors = color_scheme.get_text_gradient(text_len, audio.intensity, time);

        let start_x = area.x + (area.width.saturating_sub(text_len as u16)) / 2;
        let y = area.y + area.height / 2;

        for (i, ch) in text_chars.iter().enumerate() {
            let x = start_x + i as u16;
            if x < area.x + area.width {
                let (r, g, b) = colors.get(i).copied().unwrap_or((128, 128, 128));

                let cell = frame.buffer_mut().cell_mut((x, y));
                if let Some(cell) = cell {
                    cell.set_char(*ch);
                    cell.set_fg(Color::Rgb(r, g, b));
                }
            }
        }
    }

    fn render_intensity_bar(
        &self,
        frame: &mut Frame,
        area: Rect,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
    ) {
        if area.height < 2 {
            return;
        }

        let y = area.y + area.height - 1;
        let bar_width = (audio.intensity * area.width as f32) as u16;

        for x in 0..bar_width {
            let pos = x as f32 / area.width as f32;
            let (r, g, b) = color_scheme.get_color(pos, audio.intensity);

            let cell = frame.buffer_mut().cell_mut((area.x + x, y));
            if let Some(cell) = cell {
                cell.set_char('▀');
                cell.set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}

impl Default for TextAnimator {
    fn default() -> Self {
        Self::new()
    }
}
