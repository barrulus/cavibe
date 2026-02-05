use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::color::ColorScheme;
use crate::display::DisplayMode;

/// Multi-monitor display mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MultiMonitorMode {
    #[default]
    Clone,       // Same visualization on all monitors
    Independent, // Per-monitor overrides allowed
}

/// Per-monitor configuration overrides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub output: String, // Output name, e.g. "DP-1"
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub color_scheme: Option<ColorScheme>,
    pub style: Option<String>, // Style name
    pub opacity: Option<f32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub display: DisplayConfig,
    pub audio: AudioConfig,
    pub visualizer: VisualizerConfig,
    pub text: TextConfig,
    #[serde(default)]
    pub wallpaper: WallpaperConfig,
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
    /// When true with mirror, reverses the pattern: lows meet in middle, highs on outside
    #[serde(default)]
    pub reverse_mirror: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

fn default_opacity() -> f32 {
    1.0
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

/// A coordinate value that can be pixels or a percentage of the total dimension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoordValue {
    Pixels(i32),
    Percent(f32),
}

impl CoordValue {
    /// Resolve this coordinate value to an absolute pixel/cell position.
    pub fn resolve(&self, total: usize) -> usize {
        match self {
            CoordValue::Pixels(px) => (*px).max(0) as usize,
            CoordValue::Percent(pct) => (total as f32 * pct / 100.0) as usize,
        }
    }
}

impl fmt::Display for CoordValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoordValue::Pixels(px) => write!(f, "{}", px),
            CoordValue::Percent(pct) => write!(f, "{}%", pct),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextPosition {
    Top,
    Bottom,
    Center,
    Coordinates { x: CoordValue, y: CoordValue },
}

impl Default for TextPosition {
    fn default() -> Self {
        TextPosition::Bottom
    }
}

impl fmt::Display for TextPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextPosition::Top => write!(f, "top"),
            TextPosition::Bottom => write!(f, "bottom"),
            TextPosition::Center => write!(f, "center"),
            TextPosition::Coordinates { x, y } => write!(f, "{},{}", x, y),
        }
    }
}

impl FromStr for TextPosition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "top" => Ok(TextPosition::Top),
            "bottom" => Ok(TextPosition::Bottom),
            "center" => Ok(TextPosition::Center),
            other => {
                // Try parsing as "X,Y" coordinates
                let parts: Vec<&str> = other.split(',').collect();
                if parts.len() != 2 {
                    return Err(format!(
                        "Invalid text position '{}': expected top, bottom, center, or X,Y coordinates",
                        s
                    ));
                }
                let x = parse_coord_value(parts[0].trim())
                    .map_err(|e| format!("Invalid X coordinate '{}': {}", parts[0].trim(), e))?;
                let y = parse_coord_value(parts[1].trim())
                    .map_err(|e| format!("Invalid Y coordinate '{}': {}", parts[1].trim(), e))?;
                Ok(TextPosition::Coordinates { x, y })
            }
        }
    }
}

fn parse_coord_value(s: &str) -> Result<CoordValue, String> {
    if s.ends_with('%') {
        let num = s.trim_end_matches('%');
        let pct: f32 = num.parse().map_err(|_| format!("not a valid number: {}", num))?;
        Ok(CoordValue::Percent(pct))
    } else {
        let px: i32 = s.parse().map_err(|_| format!("not a valid number: {}", s))?;
        Ok(CoordValue::Pixels(px))
    }
}

impl Serialize for TextPosition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TextPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TextPosition::from_str(&s).map_err(serde::de::Error::custom)
    }
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
}

/// Anchor position for wallpaper (9-point grid + fullscreen)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, ValueEnum, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    #[default]
    Fullscreen, // All edges anchored (current default behavior)
}

/// Size specification for wallpaper
#[derive(Debug, Clone, PartialEq)]
pub struct WallpaperSize {
    pub width: WallpaperDimension,
    pub height: WallpaperDimension,
}

/// Single dimension specification (pixels or percentage)
#[derive(Debug, Clone, PartialEq)]
pub enum WallpaperDimension {
    Pixels(u32),
    Percentage(f32),
}

impl WallpaperSize {
    /// Parse size string like "400x300", "50%x50%", or "400x50%"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('x').collect();
        if parts.len() != 2 {
            return None;
        }

        let width = Self::parse_dimension(parts[0])?;
        let height = Self::parse_dimension(parts[1])?;

        Some(WallpaperSize { width, height })
    }

    fn parse_dimension(s: &str) -> Option<WallpaperDimension> {
        let s = s.trim();
        if s.ends_with('%') {
            let pct: f32 = s.trim_end_matches('%').parse().ok()?;
            if pct > 0.0 && pct <= 100.0 {
                Some(WallpaperDimension::Percentage(pct))
            } else {
                None
            }
        } else {
            let px: u32 = s.parse().ok()?;
            if px > 0 {
                Some(WallpaperDimension::Pixels(px))
            } else {
                None
            }
        }
    }

    /// Resolve size to actual pixels given screen dimensions
    pub fn resolve(&self, screen_w: u32, screen_h: u32) -> (u32, u32) {
        let w = match self.width {
            WallpaperDimension::Pixels(px) => px,
            WallpaperDimension::Percentage(pct) => {
                ((screen_w as f32 * pct / 100.0) as u32).max(1)
            }
        };
        let h = match self.height {
            WallpaperDimension::Pixels(px) => px,
            WallpaperDimension::Percentage(pct) => {
                ((screen_h as f32 * pct / 100.0) as u32).max(1)
            }
        };
        (w, h)
    }
}

/// Wallpaper positioning and sizing config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WallpaperConfig {
    pub anchor: WallpaperAnchor,
    pub width: Option<String>,  // "400" or "50%"
    pub height: Option<String>, // "300" or "50%"
    pub margin: i32,            // Uniform margin (shorthand)
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    #[serde(default)]
    pub multi_monitor: MultiMonitorMode,
    #[serde(default)]
    pub outputs: Option<Vec<String>>,   // CLI filter: only these outputs
    #[serde(default)]
    pub monitors: Vec<MonitorConfig>,   // Per-monitor overrides
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            anchor: WallpaperAnchor::Fullscreen,
            width: None,
            height: None,
            margin: 0,
            margin_top: 0,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
            multi_monitor: MultiMonitorMode::default(),
            outputs: None,
            monitors: Vec::new(),
        }
    }
}

impl WallpaperConfig {
    /// Get the effective margins, applying the uniform margin as a base
    pub fn effective_margins(&self) -> (i32, i32, i32, i32) {
        let top = if self.margin_top != 0 { self.margin_top } else { self.margin };
        let right = if self.margin_right != 0 { self.margin_right } else { self.margin };
        let bottom = if self.margin_bottom != 0 { self.margin_bottom } else { self.margin };
        let left = if self.margin_left != 0 { self.margin_left } else { self.margin };
        (top, right, bottom, left)
    }

    /// Parse and resolve the configured size, if any
    pub fn get_size(&self, screen_w: u32, screen_h: u32) -> Option<(u32, u32)> {
        match (&self.width, &self.height) {
            (Some(w), Some(h)) => {
                let size_str = format!("{}x{}", w, h);
                WallpaperSize::parse(&size_str).map(|s| s.resolve(screen_w, screen_h))
            }
            (Some(w), None) => {
                // Width only - use screen height
                let w_dim = WallpaperSize::parse_dimension(w)?;
                let width = match w_dim {
                    WallpaperDimension::Pixels(px) => px,
                    WallpaperDimension::Percentage(pct) => ((screen_w as f32 * pct / 100.0) as u32).max(1),
                };
                Some((width, screen_h))
            }
            (None, Some(h)) => {
                // Height only - use screen width
                let h_dim = WallpaperSize::parse_dimension(h)?;
                let height = match h_dim {
                    WallpaperDimension::Pixels(px) => px,
                    WallpaperDimension::Percentage(pct) => ((screen_h as f32 * pct / 100.0) as u32).max(1),
                };
                Some((screen_w, height))
            }
            (None, None) => None,
        }
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
                reverse_mirror: false,
                opacity: 1.0,
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
            wallpaper: WallpaperConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
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
# Reverse mirror: lows meet in middle, highs on outside (requires mirror = true)
reverse_mirror = false
# Opacity level (0.0-1.0, where 1.0 is fully opaque, wallpaper mode only)
opacity = 1.0

[text]
# Show track title
show_title = true
# Show artist name
show_artist = true
# Animation speed multiplier
animation_speed = 1.0
# Pulse intensity on beat (0.0-1.0)
pulse_intensity = 0.8
# Text position: top, bottom, center, or "X,Y" coordinates (pixels or percentages)
# Examples: "bottom", "200,600", "25%,75%", "50%,90%"
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
# Custom colors (null = use color scheme for title/artist, no background)
# title_color = { r = 255, g = 255, b = 255 }
# artist_color = { r = 200, g = 200, b = 200 }
# background_color = { r = 0, g = 0, b = 0 }  # semi-transparent text background (wallpaper only)
# Use visualizer color scheme for text gradient
use_color_scheme = true

[wallpaper]
# Anchor position: fullscreen, center, top, bottom, left, right,
# top-left, top-right, bottom-left, bottom-right
anchor = "fullscreen"
# Size (omit for fullscreen): pixels "400" or percentage "50%"
# width = "50%"
# height = "300"
# Margins from screen edges (pixels) - uniform margin for all edges
margin = 0
# Individual margins (override uniform margin if non-zero)
# margin_top = 0
# margin_right = 0
# margin_bottom = 0
# margin_left = 0
# Multi-monitor mode: "clone" (same on all) or "independent" (per-monitor overrides)
# multi_monitor = "clone"
# Only show on specific outputs (by name, e.g. "DP-1"):
# outputs = ["DP-1", "HDMI-A-1"]

# Per-monitor overrides (only used in independent mode):
# [[wallpaper.monitors]]
# output = "DP-1"
# enabled = true
# color_scheme = "rainbow"
# # style = "wave"
# # opacity = 0.8
#
# [[wallpaper.monitors]]
# output = "HDMI-A-1"
# enabled = false
"#
        .to_string()
    }

    /// Merge CLI arguments into config (CLI takes priority)
    pub fn merge_args(&mut self, args: &crate::Args) {
        // Display settings - only override if explicitly provided via CLI
        if let Some(mode) = args.mode {
            self.display.mode = mode;
        }
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
        if args.reverse_mirror {
            self.visualizer.reverse_mirror = true;
        }
        if let Some(opacity) = args.opacity {
            self.visualizer.opacity = opacity.clamp(0.0, 1.0);
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

        // Wallpaper settings
        if let Some(ref size) = args.wallpaper_size {
            // Parse "WIDTHxHEIGHT" format
            if let Some(parsed) = WallpaperSize::parse(size) {
                self.wallpaper.width = Some(match parsed.width {
                    WallpaperDimension::Pixels(px) => px.to_string(),
                    WallpaperDimension::Percentage(pct) => format!("{}%", pct),
                });
                self.wallpaper.height = Some(match parsed.height {
                    WallpaperDimension::Pixels(px) => px.to_string(),
                    WallpaperDimension::Percentage(pct) => format!("{}%", pct),
                });
            }
        }
        if let Some(anchor) = args.wallpaper_anchor {
            self.wallpaper.anchor = anchor;
        }
        if let Some(margin) = args.wallpaper_margin {
            self.wallpaper.margin = margin;
            self.wallpaper.margin_top = margin;
            self.wallpaper.margin_right = margin;
            self.wallpaper.margin_bottom = margin;
            self.wallpaper.margin_left = margin;
        }

        // Multi-monitor settings
        if let Some(mode) = args.multi_monitor {
            self.wallpaper.multi_monitor = mode;
        }
        if let Some(ref output) = args.output {
            self.wallpaper.outputs = Some(
                output.split(',').map(|s| s.trim().to_string()).collect(),
            );
        }
    }
}
