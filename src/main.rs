use anyhow::Result;
use clap::{Parser, Subcommand};

mod audio;
mod color;
mod config;
mod display;
mod ipc;
mod metadata;
mod visualizer;

use config::{Config, FontStyle, MultiMonitorMode, TextAlignment, TextAnimation, TextPosition, WallpaperAnchor};
use display::DisplayMode;

#[derive(Parser, Debug)]
#[command(name = "cavibe")]
#[command(author, version, about = "Audio visualizer with animated song display")]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Display mode: terminal or wallpaper
    #[arg(short, long)]
    pub mode: Option<DisplayMode>,

    /// Config file path
    #[arg(short, long)]
    pub config: Option<std::path::PathBuf>,

    /// Initialize default config file at ~/.config/cavibe/config.toml
    #[arg(long)]
    pub init_config: bool,

    /// Skip loading config file from default location
    #[arg(long)]
    pub no_config: bool,

    // === Visualizer settings ===
    /// Number of frequency bars
    #[arg(short, long, default_value = "64")]
    pub bars: usize,

    /// Color scheme: spectrum, rainbow, fire, ocean, monochrome
    #[arg(long, default_value = "spectrum")]
    pub colors: String,

    /// Rotate display styles automatically
    #[arg(long)]
    pub rotate: bool,

    /// Rotation interval in seconds
    #[arg(long, default_value = "30")]
    pub rotate_interval: u64,

    /// Width of each bar in characters
    #[arg(long)]
    pub bar_width: Option<u16>,

    /// Spacing between bars in characters
    #[arg(long)]
    pub bar_spacing: Option<u16>,

    /// Mirror visualization horizontally
    #[arg(long)]
    pub mirror: bool,

    /// Reverse mirror: lows meet in middle, highs on outside (requires --mirror)
    #[arg(long)]
    pub reverse_mirror: bool,

    /// Opacity level (0.0-1.0, wallpaper mode only)
    #[arg(long)]
    pub opacity: Option<f32>,

    // === Audio settings ===
    /// Audio device name (e.g., "pulse")
    #[arg(long)]
    pub audio_device: Option<String>,

    /// Sample rate in Hz
    #[arg(long)]
    pub sample_rate: Option<u32>,

    /// Buffer size for audio capture
    #[arg(long)]
    pub buffer_size: Option<usize>,

    /// Smoothing factor (0.0-1.0, higher = smoother)
    #[arg(long)]
    pub smoothing: Option<f32>,

    /// Audio sensitivity (0.1-10.0, default 1.0)
    #[arg(short, long, default_value = "1.0")]
    pub sensitivity: f32,

    // === Text settings ===
    /// Show track title
    #[arg(long)]
    pub show_title: Option<bool>,

    /// Show artist name
    #[arg(long)]
    pub show_artist: Option<bool>,

    /// Text animation speed multiplier
    #[arg(long)]
    pub animation_speed: Option<f32>,

    /// Pulse intensity on beat (0.0-1.0)
    #[arg(long)]
    pub pulse_intensity: Option<f32>,

    /// Text position: top, bottom, center, or X,Y coordinates (e.g. "50%,90%")
    #[arg(long)]
    pub text_position: Option<TextPosition>,

    /// Font style: normal, bold, ascii, figlet
    #[arg(long)]
    pub font_style: Option<FontStyle>,

    /// Text alignment: left, center, right
    #[arg(long)]
    pub text_alignment: Option<TextAlignment>,

    /// Text animation: scroll, pulse, fade, wave, none
    #[arg(long)]
    pub text_animation: Option<TextAnimation>,

    /// Top margin for text area
    #[arg(long)]
    pub margin_top: Option<u16>,

    /// Bottom margin for text area
    #[arg(long)]
    pub margin_bottom: Option<u16>,

    /// Horizontal margin for text area
    #[arg(long)]
    pub margin_horizontal: Option<u16>,

    /// Title text color (hex, e.g., "#FF0000")
    #[arg(long)]
    pub title_color: Option<String>,

    /// Artist text color (hex, e.g., "#00FF00")
    #[arg(long)]
    pub artist_color: Option<String>,

    // === Wallpaper settings ===
    /// Wallpaper size: WIDTHxHEIGHT (pixels or %, e.g., "400x300" or "50%x50%")
    #[arg(long)]
    pub wallpaper_size: Option<String>,

    /// Wallpaper anchor position
    #[arg(long, value_enum)]
    pub wallpaper_anchor: Option<WallpaperAnchor>,

    /// Wallpaper margin from all edges (pixels)
    #[arg(long)]
    pub wallpaper_margin: Option<i32>,

    /// Only show on specific outputs (comma-separated, e.g. "DP-1,HDMI-A-1")
    #[arg(long)]
    pub output: Option<String>,

    /// Multi-monitor mode: clone or independent
    #[arg(long, value_enum)]
    pub multi_monitor: Option<MultiMonitorMode>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Control a running cavibe instance
    Ctl {
        #[command(subcommand)]
        action: CtlAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum CtlAction {
    /// Change visualizer style
    Style {
        /// Direction: next, prev
        direction: String,
    },
    /// Change color scheme
    Color {
        /// Direction: next, prev
        direction: String,
    },
    /// Toggle visibility
    Toggle,
    /// Set opacity (0.0-1.0)
    Opacity {
        /// Opacity value
        value: f32,
    },
    /// Reload config file
    Reload,
    /// Show current status
    Status,
    /// List available options
    List {
        /// What to list: styles, colors, monitors
        what: String,
    },
    /// Check if daemon is running
    Ping,
    /// Change text settings
    Text {
        #[command(subcommand)]
        action: TextAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum TextAction {
    /// Set text position: top, bottom, center
    Position {
        value: String,
    },
    /// Set font style: normal, bold, ascii, figlet
    Font {
        value: String,
    },
    /// Set text animation: scroll, pulse, fade, wave, none
    Animation {
        value: String,
    },
    /// Toggle text visibility
    Toggle,
}

impl CtlAction {
    /// Convert to the wire protocol line
    fn to_protocol_line(&self) -> String {
        match self {
            CtlAction::Style { direction } => format!("style {}", direction),
            CtlAction::Color { direction } => format!("color {}", direction),
            CtlAction::Toggle => "toggle".to_string(),
            CtlAction::Opacity { value } => format!("opacity {}", value),
            CtlAction::Reload => "reload".to_string(),
            CtlAction::Status => "status".to_string(),
            CtlAction::List { what } => format!("list {}", what),
            CtlAction::Ping => "ping".to_string(),
            CtlAction::Text { action } => match action {
                TextAction::Position { value } => format!("text position {}", value),
                TextAction::Font { value } => format!("text font {}", value),
                TextAction::Animation { value } => format!("text animation {}", value),
                TextAction::Toggle => "text toggle".to_string(),
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle ctl subcommand - client mode, no daemon startup
    if let Some(Command::Ctl { action }) = &args.command {
        let response = ipc::send_command(&action.to_protocol_line()).await?;
        println!("{}", response);
        return Ok(());
    }

    // Handle --init-config flag (before logging init)
    if args.init_config {
        match Config::init_default_config() {
            Ok(path) => {
                println!("Created config file at: {}", path.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("Failed to create config file: {}", e);
                return Err(e);
            }
        }
    }

    // Load config with priority: explicit -c path > XDG config > defaults
    // Then merge CLI args on top
    let mut config = if let Some(ref path) = args.config {
        // Explicit config path specified
        Config::load(path)?
    } else if !args.no_config {
        // Try loading from XDG default path
        Config::load_from_default_path().unwrap_or_default()
    } else {
        // --no-config flag: use defaults
        Config::default()
    };

    // Merge CLI arguments (CLI takes priority over config file)
    config.merge_args(&args);

    // Initialize logging - only enable info level for wallpaper mode
    // Terminal mode uses a TUI that would be corrupted by log output
    let log_level = if config.display.mode == DisplayMode::Wallpaper {
        "cavibe=info"
    } else {
        "cavibe=error"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(log_level.parse()?),
        )
        .init();

    // Run the visualizer
    match config.display.mode {
        DisplayMode::Terminal => {
            display::terminal::run(config).await?;
        }
        DisplayMode::Wallpaper => {
            // Create IPC channel and start server
            let (ipc_tx, ipc_rx) = tokio::sync::mpsc::channel::<ipc::IpcCommand>(32);

            tokio::spawn(async move {
                if let Err(e) = ipc::start_server(ipc_tx).await {
                    tracing::warn!("IPC server error: {}", e);
                }
            });

            display::wallpaper::run(config, ipc_rx).await?;

            // Clean up socket on exit
            let _ = std::fs::remove_file(ipc::socket_path());
        }
    }

    Ok(())
}
