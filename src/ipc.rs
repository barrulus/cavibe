use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info};

use crate::color::ColorScheme;
use crate::config::{Config, FontStyle, TextAnimation, TextPosition, WallpaperAnchor, WallpaperLayer, WallpaperSize};
use crate::renderer::styles::STYLE_NAMES;
use crate::visualizer::VisualizerState;

/// Pending changes that require action in the render loop
#[derive(Default)]
pub struct PendingChanges {
    /// Layer changed — requires surface recreation
    pub layer_change: bool,
    /// Anchor/margin/size changed — can be applied dynamically
    pub surface_update: bool,
    /// Drag mode changed — update keyboard interactivity
    pub drag_changed: bool,
    /// State changed — save to config file
    pub save_config: bool,
}

/// Commands sent from IPC server to render loop
pub enum IpcCommand {
    StyleNext { reply: oneshot::Sender<String> },
    StylePrev { reply: oneshot::Sender<String> },
    ColorNext { reply: oneshot::Sender<String> },
    ColorPrev { reply: oneshot::Sender<String> },
    Toggle { reply: oneshot::Sender<String> },
    SetOpacity { value: f32, reply: oneshot::Sender<String> },
    Reload { reply: oneshot::Sender<String> },
    Status { reply: oneshot::Sender<String> },
    ListStyles { reply: oneshot::Sender<String> },
    ListColors { reply: oneshot::Sender<String> },
    ListMonitors { reply: oneshot::Sender<String> },
    Ping { reply: oneshot::Sender<String> },
    TextPosition { value: TextPosition, reply: oneshot::Sender<String> },
    TextFont { value: FontStyle, reply: oneshot::Sender<String> },
    TextAnimation { value: TextAnimation, reply: oneshot::Sender<String> },
    TextToggle { reply: oneshot::Sender<String> },
    ListSources { reply: oneshot::Sender<String> },
    SetSource { name: String, reply: oneshot::Sender<String> },
    LayerNext { reply: oneshot::Sender<String> },
    LayerPrev { reply: oneshot::Sender<String> },
    LayerSet { name: String, reply: oneshot::Sender<String> },
    ListLayers { reply: oneshot::Sender<String> },
    AnchorSet { anchor: WallpaperAnchor, reply: oneshot::Sender<String> },
    MarginSet { top: i32, right: i32, bottom: i32, left: i32, reply: oneshot::Sender<String> },
    Resize { width: String, height: String, reply: oneshot::Sender<String> },
    ResizeRelative { delta: i32, is_percent: bool, reply: oneshot::Sender<String> },
    DragToggle { reply: oneshot::Sender<String> },
    DragOn { reply: oneshot::Sender<String> },
    DragOff { reply: oneshot::Sender<String> },
}

/// Get the socket path for IPC
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("cavibe.sock")
    } else {
        PathBuf::from("/tmp/cavibe.sock")
    }
}

/// Parse a protocol line into an IpcCommand
fn parse_command(line: &str, reply: oneshot::Sender<String>) -> Result<IpcCommand> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        ["style", "next"] => Ok(IpcCommand::StyleNext { reply }),
        ["style", "prev"] => Ok(IpcCommand::StylePrev { reply }),
        ["color", "next"] => Ok(IpcCommand::ColorNext { reply }),
        ["color", "prev"] => Ok(IpcCommand::ColorPrev { reply }),
        ["toggle"] => Ok(IpcCommand::Toggle { reply }),
        ["opacity", val] => {
            let v: f32 = val.parse().context("Invalid opacity value")?;
            Ok(IpcCommand::SetOpacity {
                value: v.clamp(0.0, 1.0),
                reply,
            })
        }
        ["reload"] => Ok(IpcCommand::Reload { reply }),
        ["status"] => Ok(IpcCommand::Status { reply }),
        ["list", "styles"] => Ok(IpcCommand::ListStyles { reply }),
        ["list", "colors"] => Ok(IpcCommand::ListColors { reply }),
        ["list", "monitors"] => Ok(IpcCommand::ListMonitors { reply }),
        ["ping"] => Ok(IpcCommand::Ping { reply }),
        ["text", "position", val] => {
            let pos = val.parse::<TextPosition>()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(IpcCommand::TextPosition { value: pos, reply })
        }
        ["text", "font", val] => {
            let font = match *val {
                "normal" => FontStyle::Normal,
                "bold" => FontStyle::Bold,
                "ascii" => FontStyle::Ascii,
                "figlet" => FontStyle::Figlet,
                _ => return Err(anyhow::anyhow!("Unknown font: {} (normal, bold, ascii, figlet)", val)),
            };
            Ok(IpcCommand::TextFont { value: font, reply })
        }
        ["text", "animation", val] => {
            let anim = match *val {
                "scroll" => TextAnimation::Scroll,
                "pulse" => TextAnimation::Pulse,
                "fade" => TextAnimation::Fade,
                "wave" => TextAnimation::Wave,
                "none" => TextAnimation::None,
                _ => return Err(anyhow::anyhow!("Unknown animation: {} (scroll, pulse, fade, wave, none)", val)),
            };
            Ok(IpcCommand::TextAnimation { value: anim, reply })
        }
        ["text", "toggle"] => Ok(IpcCommand::TextToggle { reply }),
        ["list", "sources"] => Ok(IpcCommand::ListSources { reply }),
        ["set", "source", name] => Ok(IpcCommand::SetSource { name: name.to_string(), reply }),
        ["layer", "next"] => Ok(IpcCommand::LayerNext { reply }),
        ["layer", "prev"] => Ok(IpcCommand::LayerPrev { reply }),
        ["layer", name] => {
            if WallpaperLayer::from_name(name).is_some() {
                Ok(IpcCommand::LayerSet { name: name.to_string(), reply })
            } else {
                Err(anyhow::anyhow!("Unknown layer: {} ({})", name, WallpaperLayer::all_names().join(", ")))
            }
        }
        ["list", "layers"] => Ok(IpcCommand::ListLayers { reply }),
        ["anchor", pos] => {
            let anchor: WallpaperAnchor = pos.parse()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(IpcCommand::AnchorSet { anchor, reply })
        }
        ["margin", top, right, bottom, left] => {
            let top: i32 = top.parse().context("Invalid top margin")?;
            let right: i32 = right.parse().context("Invalid right margin")?;
            let bottom: i32 = bottom.parse().context("Invalid bottom margin")?;
            let left: i32 = left.parse().context("Invalid left margin")?;
            Ok(IpcCommand::MarginSet { top, right, bottom, left, reply })
        }
        ["resize", size] => {
            // Check for relative resize: +50, -50, +10%, -10%
            let s = *size;
            if s.starts_with('+') || s.starts_with('-') {
                let is_percent = s.ends_with('%');
                let num_str = if is_percent {
                    &s[..s.len() - 1]
                } else {
                    s
                };
                let delta: i32 = num_str.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid relative size: {} (expected +50, -50, +10%, -10%)", s))?;
                Ok(IpcCommand::ResizeRelative { delta, is_percent, reply })
            } else {
                let parsed = WallpaperSize::parse(size)
                    .ok_or_else(|| anyhow::anyhow!("Invalid size format: {} (expected WxH e.g. 800x600, or +50, -10%)", size))?;
                let w = match parsed.width {
                    crate::config::WallpaperDimension::Pixels(px) => px.to_string(),
                    crate::config::WallpaperDimension::Percentage(pct) => format!("{}%", pct),
                };
                let h = match parsed.height {
                    crate::config::WallpaperDimension::Pixels(px) => px.to_string(),
                    crate::config::WallpaperDimension::Percentage(pct) => format!("{}%", pct),
                };
                Ok(IpcCommand::Resize { width: w, height: h, reply })
            }
        }
        ["drag", "toggle"] => Ok(IpcCommand::DragToggle { reply }),
        ["drag", "on"] => Ok(IpcCommand::DragOn { reply }),
        ["drag", "off"] => Ok(IpcCommand::DragOff { reply }),
        _ => Err(anyhow::anyhow!("Unknown command: {}", line)),
    }
}

/// Process an IPC command by mutating render loop state
#[allow(clippy::too_many_arguments)]
pub fn process_ipc_command(
    cmd: IpcCommand,
    visualizer: &mut VisualizerState,
    color_scheme: &mut ColorScheme,
    visible: &mut bool,
    opacity: &mut f32,
    config: &mut Config,
    monitors: &[(String, bool)],
    pending: &mut PendingChanges,
) {
    match cmd {
        IpcCommand::StyleNext { reply } => {
            visualizer.next_style();
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", visualizer.current_style_name()));
        }
        IpcCommand::StylePrev { reply } => {
            visualizer.prev_style();
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", visualizer.current_style_name()));
        }
        IpcCommand::ColorNext { reply } => {
            *color_scheme = color_scheme.next();
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", color_scheme.name()));
        }
        IpcCommand::ColorPrev { reply } => {
            *color_scheme = color_scheme.prev();
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", color_scheme.name()));
        }
        IpcCommand::Toggle { reply } => {
            *visible = !*visible;
            let state = if *visible { "visible" } else { "hidden" };
            let _ = reply.send(format!("ok: {}", state));
        }
        IpcCommand::SetOpacity { value, reply } => {
            *opacity = value;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", value));
        }
        IpcCommand::Reload { reply } => {
            match Config::load_from_default_path() {
                Ok(Some(new_config)) => {
                    *color_scheme = new_config.visualizer.color_scheme;
                    *opacity = new_config.visualizer.opacity;
                    *config = new_config;
                    let _ = reply.send("ok: reloaded".to_string());
                }
                Ok(None) => {
                    let _ = reply.send("err: config file not found".to_string());
                }
                Err(e) => {
                    let _ = reply.send(format!("err: {}", e));
                }
            }
        }
        IpcCommand::Status { reply } => {
            let (mt, mr, mb, ml) = config.wallpaper.effective_margins();
            let size_str = match (&config.wallpaper.width, &config.wallpaper.height) {
                (Some(w), Some(h)) => format!("{}x{}", w, h),
                _ => "auto".to_string(),
            };
            let status = format!(
                "ok: style={} color={} visible={} opacity={} layer={} anchor={:?} margin={},{},{},{} size={} draggable={}",
                visualizer.current_style_name(),
                color_scheme.name(),
                visible,
                opacity,
                config.wallpaper.layer.name(),
                config.wallpaper.anchor,
                mt, mr, mb, ml,
                size_str,
                config.wallpaper.draggable,
            ).to_lowercase();
            let _ = reply.send(status);
        }
        IpcCommand::ListStyles { reply } => {
            let _ = reply.send(format!("ok: {}", STYLE_NAMES.join(",")));
        }
        IpcCommand::ListColors { reply } => {
            let names: Vec<&str> = ColorScheme::all().iter().map(|c| c.name()).collect();
            let _ = reply.send(format!("ok: {}", names.join(",")));
        }
        IpcCommand::ListMonitors { reply } => {
            if monitors.is_empty() {
                let _ = reply.send("ok: (no monitors)".to_string());
            } else {
                let list: Vec<String> = monitors.iter().map(|(name, active)| {
                    format!("{} ({})", name, if *active { "active" } else { "inactive" })
                }).collect();
                let _ = reply.send(format!("ok: {}", list.join(", ")));
            }
        }
        IpcCommand::Ping { reply } => {
            let _ = reply.send("ok: pong".to_string());
        }
        IpcCommand::TextPosition { value, reply } => {
            config.text.position = value;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", value));
        }
        IpcCommand::TextFont { value, reply } => {
            config.text.font_style = value;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {:?}", value).to_lowercase());
        }
        IpcCommand::TextAnimation { value, reply } => {
            config.text.animation_style = value;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {:?}", value).to_lowercase());
        }
        IpcCommand::TextToggle { reply } => {
            let both_off = !config.text.show_title && !config.text.show_artist;
            if both_off {
                config.text.show_title = true;
                config.text.show_artist = true;
                let _ = reply.send("ok: visible".to_string());
            } else {
                config.text.show_title = false;
                config.text.show_artist = false;
                let _ = reply.send("ok: hidden".to_string());
            }
            pending.save_config = true;
        }
        IpcCommand::LayerNext { reply } => {
            config.wallpaper.layer = config.wallpaper.layer.next();
            pending.layer_change = true;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", config.wallpaper.layer.name()));
        }
        IpcCommand::LayerPrev { reply } => {
            config.wallpaper.layer = config.wallpaper.layer.prev();
            pending.layer_change = true;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {}", config.wallpaper.layer.name()));
        }
        IpcCommand::LayerSet { name, reply } => {
            if let Some(layer) = WallpaperLayer::from_name(&name) {
                config.wallpaper.layer = layer;
                pending.layer_change = true;
                pending.save_config = true;
                let _ = reply.send(format!("ok: {}", layer.name()));
            } else {
                let _ = reply.send(format!("err: unknown layer '{}' ({})", name, WallpaperLayer::all_names().join(", ")));
            }
        }
        IpcCommand::ListLayers { reply } => {
            let current = config.wallpaper.layer.name();
            let list: Vec<String> = WallpaperLayer::all_names().iter().map(|&n| {
                if n == current { format!("{}*", n) } else { n.to_string() }
            }).collect();
            let _ = reply.send(format!("ok: {}", list.join(",")));
        }
        IpcCommand::AnchorSet { anchor, reply } => {
            config.wallpaper.anchor = anchor;
            pending.surface_update = true;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {:?}", anchor).to_lowercase());
        }
        IpcCommand::MarginSet { top, right, bottom, left, reply } => {
            config.wallpaper.margin_top = top;
            config.wallpaper.margin_right = right;
            config.wallpaper.margin_bottom = bottom;
            config.wallpaper.margin_left = left;
            config.wallpaper.margin = 0; // Clear uniform margin
            pending.surface_update = true;
            pending.save_config = true;
            let _ = reply.send(format!("ok: {},{},{},{}", top, right, bottom, left));
        }
        IpcCommand::Resize { .. } => {
            // Handled in wayland.rs main loop (needs surface dimensions for margin adjustment)
            unreachable!("Resize should be intercepted in wayland.rs");
        }
        IpcCommand::DragToggle { reply } => {
            config.wallpaper.draggable = !config.wallpaper.draggable;
            pending.drag_changed = true;
            pending.save_config = true;
            let state = if config.wallpaper.draggable { "on" } else { "off" };
            let mut msg = format!("ok: drag {}", state);
            if config.wallpaper.draggable && config.wallpaper.layer == WallpaperLayer::Background {
                msg.push_str(" (warning: background layer may not receive pointer events)");
            }
            let _ = reply.send(msg);
        }
        IpcCommand::DragOn { reply } => {
            config.wallpaper.draggable = true;
            pending.drag_changed = true;
            pending.save_config = true;
            let mut msg = "ok: drag on".to_string();
            if config.wallpaper.layer == WallpaperLayer::Background {
                msg.push_str(" (warning: background layer may not receive pointer events)");
            }
            let _ = reply.send(msg);
        }
        IpcCommand::DragOff { reply } => {
            config.wallpaper.draggable = false;
            pending.drag_changed = true;
            pending.save_config = true;
            let _ = reply.send("ok: drag off".to_string());
        }
        // ResizeRelative is intercepted in wayland.rs before reaching here
        IpcCommand::ResizeRelative { reply, .. } => {
            let _ = reply.send("err: not supported in this mode".to_string());
        }
        // Audio commands are intercepted in render loops before reaching here
        IpcCommand::ListSources { reply } => {
            let _ = reply.send("err: not supported in this mode".to_string());
        }
        IpcCommand::SetSource { reply, .. } => {
            let _ = reply.send("err: not supported in this mode".to_string());
        }
    }
}

/// Handle a single client connection
async fn handle_client(stream: UnixStream, cmd_tx: mpsc::Sender<IpcCommand>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;
    let line = line.trim();

    if line.is_empty() {
        return Ok(());
    }

    let (reply_tx, reply_rx) = oneshot::channel();

    let command = match parse_command(line, reply_tx) {
        Ok(cmd) => cmd,
        Err(e) => {
            writer
                .write_all(format!("err: {}\n", e).as_bytes())
                .await?;
            return Ok(());
        }
    };

    cmd_tx
        .send(command)
        .await
        .map_err(|_| anyhow::anyhow!("Render loop has shut down"))?;

    let response = reply_rx
        .await
        .unwrap_or_else(|_| "err: internal error".to_string());

    writer
        .write_all(format!("{}\n", response).as_bytes())
        .await?;
    Ok(())
}

/// Start the IPC server, listening for commands on a Unix socket
pub async fn start_server(cmd_tx: mpsc::Sender<IpcCommand>) -> Result<()> {
    let path = socket_path();

    // Remove stale socket from previous run
    let _ = std::fs::remove_file(&path);

    let listener =
        UnixListener::bind(&path).context("Failed to bind IPC socket")?;

    info!("IPC server listening on {}", path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let cmd_tx = cmd_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, cmd_tx).await {
                debug!("IPC client error: {}", e);
            }
        });
    }
}

/// Send a command to a running cavibe instance (client mode)
pub async fn send_command(line: &str) -> Result<String> {
    let path = socket_path();

    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        UnixStream::connect(&path),
    )
    .await
    .context("Connection timed out")?
    .context("Could not connect to cavibe. Is it running in wallpaper mode?")?;

    let (mut reader, mut writer) = stream.into_split();

    writer.write_all(format!("{}\n", line).as_bytes()).await?;
    writer.shutdown().await?;

    let mut response = String::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut response),
    )
    .await
    .context("Response timed out")?
    .context("Failed to read response")?;

    Ok(response.trim().to_string())
}
