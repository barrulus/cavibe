use anyhow::Result;
use clap::Parser;
use tracing::info;

mod audio;
mod color;
mod config;
mod display;
mod metadata;
mod visualizer;

use config::Config;
use display::DisplayMode;

#[derive(Parser, Debug)]
#[command(name = "cavibe")]
#[command(author, version, about = "Audio visualizer with animated song display")]
struct Args {
    /// Display mode: terminal or wallpaper
    #[arg(short, long, default_value = "terminal")]
    mode: DisplayMode,

    /// Config file path
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Number of frequency bars
    #[arg(short, long, default_value = "64")]
    bars: usize,

    /// Color scheme: spectrum, rainbow, fire, ocean, custom
    #[arg(long, default_value = "spectrum")]
    colors: String,

    /// Rotate display styles automatically
    #[arg(long)]
    rotate: bool,

    /// Rotation interval in seconds
    #[arg(long, default_value = "30")]
    rotate_interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("cavibe=info".parse()?),
        )
        .init();

    let args = Args::parse();

    info!("Starting Cavibe in {:?} mode", args.mode);

    // Load or create config
    let config = match &args.config {
        Some(path) => Config::load(path)?,
        None => Config::default_with_args(&args),
    };

    // Run the visualizer
    match args.mode {
        DisplayMode::Terminal => {
            display::terminal::run(config).await?;
        }
        DisplayMode::Wallpaper => {
            display::wallpaper::run(config).await?;
        }
    }

    Ok(())
}
