use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum TextPosition {
    Top,
    #[default]
    Bottom,
    Center,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Bold,
    Ascii,
    Figlet,
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

    pub fn default_with_args(args: &crate::Args) -> Self {
        let mut config = Self::default();
        config.display.mode = args.mode;
        config.visualizer.bars = args.bars;
        config.display.rotate_styles = args.rotate;
        config.display.rotation_interval_secs = args.rotate_interval;
        config.visualizer.color_scheme = args.colors.parse().unwrap_or_default();
        config
    }

    pub fn config_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|p| p.join("cavibe").join("config.toml"))
    }
}
