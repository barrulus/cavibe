use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::color::ColorScheme;
use crate::display::DisplayMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub display: DisplayConfig,
    pub audio: AudioConfig,
    pub visualizer: VisualizerConfig,
    pub text: TextConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub mode: DisplayMode,
    pub rotate_styles: bool,
    pub rotation_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub device: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: usize,
    pub smoothing: f32,
    pub sensitivity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizerConfig {
    pub bars: usize,
    pub color_scheme: ColorScheme,
    pub bar_width: u16,
    pub bar_spacing: u16,
    pub mirror: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextConfig {
    pub show_title: bool,
    pub show_artist: bool,
    pub animation_speed: f32,
    pub pulse_intensity: f32,
    pub position: TextPosition,
    pub font_style: FontStyle,
    // New fields for Issue #3
    pub alignment: TextAlignment,
    pub animation_style: TextAnimation,
    pub margin_top: u16,
    pub margin_bottom: u16,
    pub margin_horizontal: u16,
    pub title_color: Option<RgbColor>,
    pub artist_color: Option<RgbColor>,
    pub background_color: Option<RgbColor>,
    pub use_color_scheme: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TextPosition {
    Top,
    #[default]
    Bottom,
    Center,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    #[default]
    Normal,
    Bold,
    Ascii,
    Figlet,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TextAnimation {
    #[default]
    Scroll,
    Pulse,
    Fade,
    Wave,
    #[serde(rename = "none")]
    None,
}

/// RGB color representation for configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse from hex string like "#FF0000" or "FF0000"
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }

    pub fn to_tuple(&self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            display: DisplayConfig {
                mode: DisplayMode::Terminal,
                rotate_styles: false,
                rotation_interval_secs: 30,
            },
            audio: AudioConfig {
                device: None,
                sample_rate: 44100,
                buffer_size: 1024,
                smoothing: 0.7,
                sensitivity: 1.0,
            },
            visualizer: VisualizerConfig {
                bars: 64,
                color_scheme: ColorScheme::Spectrum,
                bar_width: 2,
                bar_spacing: 1,
                mirror: false,
            },
            text: TextConfig {
                show_title: true,
                show_artist: true,
                animation_speed: 1.0,
                pulse_intensity: 0.8,
                position: TextPosition::Bottom,
                font_style: FontStyle::Normal,
                alignment: TextAlignment::Center,
                animation_style: TextAnimation::Scroll,
                margin_top: 0,
                margin_bottom: 0,
                margin_horizontal: 2,
                title_color: None,
                artist_color: None,
                background_color: None,
                use_color_scheme: true,
            },
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the default XDG config path (~/.config/cavibe/config.toml)
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("cavibe").join("config.toml"))
    }

    /// Load config from the default XDG path if it exists
    /// Returns None if file doesn't exist, logs warning on parse errors
    pub fn load_from_default_path() -> Option<Self> {
        let path = Self::default_path()?;
        if path.exists() {
            match Self::load(&path) {
                Ok(config) => Some(config),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to parse config at {}: {}\nUsing defaults.",
                        path.display(),
                        e
                    );
                    None
                }
            }
        } else {
            None
        }
    }

    /// Initialize default config file at XDG path, returns the path
    pub fn init_default_config() -> Result<PathBuf> {
        let path = Self::default_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write the config template
        let template = Self::generate_config_template();
        std::fs::write(&path, template)?;

        Ok(path)
    }

    /// Generate a commented TOML config template
    pub fn generate_config_template() -> String {
        r#"# Cavibe Configuration
# This file is auto-generated. Edit as needed.

[display]
# Display mode: "terminal" or "wallpaper"
mode = "terminal"
# Automatically rotate visualizer styles
rotate_styles = false
# Rotation interval in seconds
rotation_interval_secs = 30

[audio]
# Audio device (null = default)
# device = "pulse"
# Sample rate in Hz
sample_rate = 44100
# Buffer size for audio capture
buffer_size = 1024
# Smoothing factor (0.0-1.0, higher = smoother)
smoothing = 0.7
# Audio sensitivity multiplier (0.1-10.0)
sensitivity = 1.0

[visualizer]
# Number of frequency bars
bars = 64
# Color scheme: "spectrum", "rainbow", "fire", "ocean", "monochrome"
color_scheme = "spectrum"
# Width of each bar in characters
bar_width = 2
# Spacing between bars in characters
bar_spacing = 1
# Mirror visualization horizontally
mirror = false

[text]
# Show track title
show_title = true
# Show artist name
show_artist = true
# Animation speed multiplier
animation_speed = 1.0
# Pulse intensity on beat (0.0-1.0)
pulse_intensity = 0.8
# Text position: top, bottom, center
position = "bottom"
# Font style: normal, bold, ascii, figlet
font_style = "normal"
# Text alignment: left, center, right
alignment = "center"
# Animation style: scroll, pulse, fade, wave, none
animation_style = "scroll"
# Margins
margin_top = 0
margin_bottom = 0
margin_horizontal = 2
# Custom colors (hex format, null = use color scheme)
# title_color = { r = 255, g = 255, b = 255 }
# artist_color = { r = 200, g = 200, b = 200 }
# background_color = { r = 0, g = 0, b = 0 }
# Use visualizer color scheme for text
use_color_scheme = true
"#
        .to_string()
    }

    /// Merge CLI arguments into config (CLI takes priority)
    pub fn merge_args(&mut self, args: &crate::Args) {
        // Display settings
        self.display.mode = args.mode;
        if args.rotate {
            self.display.rotate_styles = true;
        }
        self.display.rotation_interval_secs = args.rotate_interval;

        // Audio settings
        if let Some(ref device) = args.audio_device {
            self.audio.device = Some(device.clone());
        }
        if let Some(rate) = args.sample_rate {
            self.audio.sample_rate = rate;
        }
        if let Some(size) = args.buffer_size {
            self.audio.buffer_size = size;
        }
        if let Some(smoothing) = args.smoothing {
            self.audio.smoothing = smoothing;
        }
        self.audio.sensitivity = args.sensitivity;

        // Visualizer settings
        self.visualizer.bars = args.bars;
        self.visualizer.color_scheme = args.colors.parse().unwrap_or(self.visualizer.color_scheme);
        if let Some(width) = args.bar_width {
            self.visualizer.bar_width = width;
        }
        if let Some(spacing) = args.bar_spacing {
            self.visualizer.bar_spacing = spacing;
        }
        if args.mirror {
            self.visualizer.mirror = true;
        }

        // Text settings
        if let Some(show) = args.show_title {
            self.text.show_title = show;
        }
        if let Some(show) = args.show_artist {
            self.text.show_artist = show;
        }
        if let Some(speed) = args.animation_speed {
            self.text.animation_speed = speed;
        }
        if let Some(intensity) = args.pulse_intensity {
            self.text.pulse_intensity = intensity;
        }
        if let Some(pos) = args.text_position {
            self.text.position = pos;
        }
        if let Some(style) = args.font_style {
            self.text.font_style = style;
        }
        if let Some(align) = args.text_alignment {
            self.text.alignment = align;
        }
        if let Some(anim) = args.text_animation {
            self.text.animation_style = anim;
        }
        if let Some(m) = args.margin_top {
            self.text.margin_top = m;
        }
        if let Some(m) = args.margin_bottom {
            self.text.margin_bottom = m;
        }
        if let Some(m) = args.margin_horizontal {
            self.text.margin_horizontal = m;
        }
        if let Some(ref color) = args.title_color {
            self.text.title_color = RgbColor::from_hex(color);
        }
        if let Some(ref color) = args.artist_color {
            self.text.artist_color = RgbColor::from_hex(color);
        }
    }
}
