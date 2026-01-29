use crate::audio::AudioData;
use crate::color::ColorScheme;
use crate::config::{FontStyle, TextAlignment, TextAnimation, TextConfig, TextPosition};
use crate::metadata::TrackInfo;
use ratatui::prelude::*;
use std::sync::Arc;

use super::ascii_font;

pub struct TextAnimator {
    config: TextConfig,
    scroll_offset: f32,
    pulse_phase: f32,
    fade_phase: f32,
    wave_phase: f32,
}

impl TextAnimator {
    pub fn new(config: TextConfig) -> Self {
        Self {
            config,
            scroll_offset: 0.0,
            pulse_phase: 0.0,
            fade_phase: 0.0,
            wave_phase: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32) {
        let speed = self.config.animation_speed;
        self.scroll_offset += dt * 20.0 * speed;
        self.pulse_phase += dt * 3.0 * speed;
        self.fade_phase += dt * 1.5 * speed;
        self.wave_phase += dt * 2.0 * speed;
    }

    /// Get the text position enum for layout purposes
    pub fn position(&self) -> TextPosition {
        self.config.position
    }

    /// Get the font style for layout purposes
    pub fn font_style(&self) -> FontStyle {
        self.config.font_style
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

        // Apply margins
        let area = self.apply_margins(area);
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Render background if configured
        if let Some(bg) = &self.config.background_color {
            self.render_background(frame, area, bg.r, bg.g, bg.b);
        }

        // Build display text based on config
        let text = self.build_display_text(track);
        if text.is_empty() {
            self.render_placeholder(frame, area, audio, color_scheme, time);
            return;
        }

        // Render the text with animation
        self.render_animated_text(frame, area, &text, audio, color_scheme, time, false);
    }

    fn apply_margins(&self, area: Rect) -> Rect {
        let h_margin = self.config.margin_horizontal;
        let t_margin = self.config.margin_top;
        let b_margin = self.config.margin_bottom;

        let x = area.x + h_margin.min(area.width / 2);
        let width = area.width.saturating_sub(h_margin * 2);
        let y = area.y + t_margin.min(area.height / 2);
        let height = area.height.saturating_sub(t_margin + b_margin);

        Rect::new(x, y, width, height)
    }

    fn build_display_text(&self, track: &Arc<TrackInfo>) -> String {
        let mut parts = Vec::new();

        if self.config.show_title {
            if let Some(ref title) = track.title {
                parts.push(title.clone());
            }
        }

        if self.config.show_artist {
            if let Some(ref artist) = track.artist {
                parts.push(artist.clone());
            }
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join(" - ")
        }
    }

    fn render_animated_text(
        &self,
        frame: &mut Frame,
        area: Rect,
        text: &str,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
        is_placeholder: bool,
    ) {
        // Handle ASCII art font styles
        match self.config.font_style {
            FontStyle::Ascii => {
                self.render_ascii_text(frame, area, text, audio, color_scheme, time, is_placeholder);
                return;
            }
            FontStyle::Figlet => {
                self.render_figlet_text(frame, area, text, audio, color_scheme, time, is_placeholder);
                return;
            }
            _ => {}
        }

        // Normal/Bold rendering
        let text_chars: Vec<char> = text.chars().collect();
        let text_len = text_chars.len();

        // Get base colors
        let colors = if self.config.use_color_scheme || is_placeholder {
            color_scheme.get_text_gradient(text_len, audio.intensity, time)
        } else {
            self.get_custom_colors(text_len)
        };

        // Calculate X position based on alignment and animation
        let (positions, visible_range) = self.calculate_positions(area, text_len);

        let y = area.y + area.height / 2;

        // Render each visible character
        for i in visible_range {
            if i >= text_chars.len() {
                continue;
            }

            let x = positions[i];
            if x < area.x || x >= area.x + area.width {
                continue;
            }

            if y >= area.y + area.height {
                continue;
            }

            let ch = text_chars[i];
            let (r, g, b) = self.apply_animation_effect(colors[i], i, audio);

            if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                cell.set_char(ch);
                cell.set_fg(Color::Rgb(r, g, b));

                // Apply font style
                match self.config.font_style {
                    FontStyle::Bold => {
                        cell.set_style(Style::default().bold());
                    }
                    FontStyle::Normal => {
                        if audio.bass > 0.5 {
                            cell.set_style(Style::default().bold());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_ascii_text(
        &self,
        frame: &mut Frame,
        area: Rect,
        text: &str,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
        is_placeholder: bool,
    ) {
        let rows = ascii_font::render_ascii(text);
        let font_height = ascii_font::ASCII_HEIGHT as usize;
        let text_width = ascii_font::ascii_width(text);

        self.render_multiline_text(
            frame, area, &rows, text_width, font_height,
            audio, color_scheme, time, is_placeholder,
        );
    }

    fn render_figlet_text(
        &self,
        frame: &mut Frame,
        area: Rect,
        text: &str,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
        is_placeholder: bool,
    ) {
        let rows = ascii_font::render_figlet(text);
        let font_height = ascii_font::FIGLET_HEIGHT as usize;
        let text_width = ascii_font::figlet_width(text);

        self.render_multiline_text(
            frame, area, &rows, text_width, font_height,
            audio, color_scheme, time, is_placeholder,
        );
    }

    fn render_multiline_text(
        &self,
        frame: &mut Frame,
        area: Rect,
        rows: &[String],
        text_width: usize,
        font_height: usize,
        audio: &Arc<AudioData>,
        color_scheme: &ColorScheme,
        time: f32,
        is_placeholder: bool,
    ) {
        if area.height < font_height as u16 {
            return;
        }

        // Get base colors (one per column of the rendered text)
        let colors = if self.config.use_color_scheme || is_placeholder {
            color_scheme.get_text_gradient(text_width, audio.intensity, time)
        } else {
            self.get_custom_colors(text_width)
        };

        // Check if we're in scrolling mode
        let is_scrolling = matches!(self.config.animation_style, TextAnimation::Scroll)
            && text_width > area.width as usize;

        // Calculate vertical center
        let start_y = area.y + (area.height.saturating_sub(font_height as u16)) / 2;

        // Render each row of the ASCII art
        for (row_idx, row) in rows.iter().enumerate() {
            let y = start_y + row_idx as u16;
            if y >= area.y + area.height {
                break;
            }

            let row_chars: Vec<char> = row.chars().collect();
            let row_len = row_chars.len();

            // Calculate start X for this row based on its actual length
            let row_start_x = if is_scrolling {
                // Scrolling mode - will be calculated per-character
                area.x
            } else {
                // Static positioning based on alignment using actual row length
                match self.config.alignment {
                    TextAlignment::Left => area.x,
                    TextAlignment::Center => {
                        area.x + (area.width.saturating_sub(row_len as u16)) / 2
                    }
                    TextAlignment::Right => {
                        area.x + area.width.saturating_sub(row_len as u16)
                    }
                }
            };

            for (col_idx, ch) in row_chars.iter().enumerate() {
                let x = if is_scrolling {
                    // Scrolling animation
                    let total_scroll = text_width + area.width as usize;
                    let scroll_pos = (self.scroll_offset as usize) % total_scroll;
                    let display_col = col_idx as i32 + area.width as i32 - scroll_pos as i32;
                    let x = area.x as i32 + display_col;
                    if x < area.x as i32 || x >= (area.x + area.width) as i32 {
                        continue;
                    }
                    x as u16
                } else {
                    // Static positioning
                    let x = row_start_x + col_idx as u16;
                    if x >= area.x + area.width {
                        continue;
                    }
                    x
                };

                // Get color for this column
                let color_idx = col_idx.min(colors.len().saturating_sub(1));
                let (r, g, b) = self.apply_animation_effect(colors[color_idx], col_idx, audio);

                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_char(*ch);
                    cell.set_fg(Color::Rgb(r, g, b));

                    // Bold on bass hit
                    if audio.bass > 0.5 {
                        cell.set_style(Style::default().bold());
                    }
                }
            }
        }
    }

    fn calculate_positions(&self, area: Rect, text_len: usize) -> (Vec<u16>, std::ops::Range<usize>) {
        let width = area.width as usize;

        match self.config.animation_style {
            TextAnimation::Scroll if text_len > width => {
                // Scrolling: text moves left continuously
                let total_scroll = text_len + width;
                let scroll_pos = (self.scroll_offset as usize) % total_scroll;

                let positions: Vec<u16> = (0..text_len)
                    .map(|i| {
                        let pos = i as i32 + area.x as i32 + width as i32 - scroll_pos as i32;
                        pos.max(0) as u16
                    })
                    .collect();

                (positions, 0..text_len)
            }
            _ => {
                // Static positioning based on alignment
                let start_x = match self.config.alignment {
                    TextAlignment::Left => area.x,
                    TextAlignment::Center => {
                        area.x + (area.width.saturating_sub(text_len as u16)) / 2
                    }
                    TextAlignment::Right => {
                        area.x + area.width.saturating_sub(text_len as u16)
                    }
                };

                let positions: Vec<u16> = (0..text_len)
                    .map(|i| start_x + i as u16)
                    .collect();

                (positions, 0..text_len.min(width))
            }
        }
    }

    fn apply_animation_effect(&self, base_color: (u8, u8, u8), char_index: usize, audio: &Arc<AudioData>) -> (u8, u8, u8) {
        let (r, g, b) = base_color;
        let intensity = self.config.pulse_intensity;

        match self.config.animation_style {
            TextAnimation::None => (r, g, b),

            TextAnimation::Scroll => {
                // Subtle pulse on bass
                let pulse = 1.0 + audio.bass * 0.3 * intensity * (self.pulse_phase + char_index as f32 * 0.1).sin();
                (
                    ((r as f32 * pulse).min(255.0)) as u8,
                    ((g as f32 * pulse).min(255.0)) as u8,
                    ((b as f32 * pulse).min(255.0)) as u8,
                )
            }

            TextAnimation::Pulse => {
                // Strong pulse synchronized with bass
                let pulse = 0.5 + audio.bass * 0.5 * intensity + 0.3 * (self.pulse_phase * 2.0).sin().abs();
                (
                    ((r as f32 * pulse).min(255.0)) as u8,
                    ((g as f32 * pulse).min(255.0)) as u8,
                    ((b as f32 * pulse).min(255.0)) as u8,
                )
            }

            TextAnimation::Fade => {
                // Smooth fade in/out
                let fade = 0.3 + 0.7 * ((self.fade_phase).sin() * 0.5 + 0.5);
                (
                    ((r as f32 * fade).min(255.0)) as u8,
                    ((g as f32 * fade).min(255.0)) as u8,
                    ((b as f32 * fade).min(255.0)) as u8,
                )
            }

            TextAnimation::Wave => {
                // Wave effect across characters
                let wave_offset = char_index as f32 * 0.3;
                let wave = 0.5 + 0.5 * (self.wave_phase + wave_offset).sin();
                let brightness = 0.4 + 0.6 * wave * (1.0 + audio.intensity * intensity);
                (
                    ((r as f32 * brightness).min(255.0)) as u8,
                    ((g as f32 * brightness).min(255.0)) as u8,
                    ((b as f32 * brightness).min(255.0)) as u8,
                )
            }
        }
    }

    fn get_custom_colors(&self, len: usize) -> Vec<(u8, u8, u8)> {
        let title_color = self.config.title_color
            .map(|c| (c.r, c.g, c.b))
            .unwrap_or((255, 255, 255));

        vec![title_color; len]
    }

    fn render_background(&self, frame: &mut Frame, area: Rect, r: u8, g: u8, b: u8) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_bg(Color::Rgb(r, g, b));
                }
            }
        }
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
        self.render_animated_text(frame, area, text, audio, color_scheme, time, true);
    }

}

impl Default for TextAnimator {
    fn default() -> Self {
        Self::new(TextConfig::default())
    }
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            show_title: true,
            show_artist: true,
            animation_speed: 1.0,
            pulse_intensity: 0.8,
            position: TextPosition::Bottom,
            font_style: crate::config::FontStyle::Normal,
            alignment: TextAlignment::Center,
            animation_style: TextAnimation::Scroll,
            margin_top: 0,
            margin_bottom: 0,
            margin_horizontal: 2,
            title_color: None,
            artist_color: None,
            background_color: None,
            use_color_scheme: true,
        }
    }
}
