pub mod terminal;
pub mod wallpaper;

#[cfg(feature = "wayland")]
pub mod wayland;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    #[default]
    Terminal,
    Wallpaper,
}
