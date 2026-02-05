use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info};

use crate::color::ColorScheme;
use crate::config::Config;
use crate::visualizer::{VisualizerState, VISUALIZER_STYLES};

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
    Ping { reply: oneshot::Sender<String> },
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
        ["ping"] => Ok(IpcCommand::Ping { reply }),
        _ => Err(anyhow::anyhow!("Unknown command: {}", line)),
    }
}

/// Process an IPC command by mutating render loop state
pub fn process_ipc_command(
    cmd: IpcCommand,
    visualizer: &mut VisualizerState,
    color_scheme: &mut ColorScheme,
    visible: &mut bool,
    opacity: &mut f32,
    config: &mut Config,
) {
    match cmd {
        IpcCommand::StyleNext { reply } => {
            visualizer.next_style();
            let _ = reply.send(format!("ok: {}", visualizer.current_style_name()));
        }
        IpcCommand::StylePrev { reply } => {
            visualizer.prev_style();
            let _ = reply.send(format!("ok: {}", visualizer.current_style_name()));
        }
        IpcCommand::ColorNext { reply } => {
            *color_scheme = color_scheme.next();
            let _ = reply.send(format!("ok: {}", color_scheme.name()));
        }
        IpcCommand::ColorPrev { reply } => {
            *color_scheme = color_scheme.prev();
            let _ = reply.send(format!("ok: {}", color_scheme.name()));
        }
        IpcCommand::Toggle { reply } => {
            *visible = !*visible;
            let state = if *visible { "visible" } else { "hidden" };
            let _ = reply.send(format!("ok: {}", state));
        }
        IpcCommand::SetOpacity { value, reply } => {
            *opacity = value;
            let _ = reply.send(format!("ok: {}", value));
        }
        IpcCommand::Reload { reply } => {
            match Config::load_from_default_path() {
                Some(new_config) => {
                    *color_scheme = new_config.visualizer.color_scheme;
                    *opacity = new_config.visualizer.opacity;
                    *config = new_config;
                    let _ = reply.send("ok: reloaded".to_string());
                }
                None => {
                    let _ = reply.send("err: could not load config".to_string());
                }
            }
        }
        IpcCommand::Status { reply } => {
            let status = format!(
                "ok: style={} color={} visible={} opacity={}",
                visualizer.current_style_name(),
                color_scheme.name(),
                visible,
                opacity,
            );
            let _ = reply.send(status);
        }
        IpcCommand::ListStyles { reply } => {
            let names: Vec<&str> = VISUALIZER_STYLES.iter().map(|s| s.name()).collect();
            let _ = reply.send(format!("ok: {}", names.join(",")));
        }
        IpcCommand::ListColors { reply } => {
            let names: Vec<&str> = ColorScheme::all().iter().map(|c| c.name()).collect();
            let _ = reply.send(format!("ok: {}", names.join(",")));
        }
        IpcCommand::Ping { reply } => {
            let _ = reply.send("ok: pong".to_string());
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

    let (reader, mut writer) = stream.into_split();

    writer.write_all(format!("{}\n", line).as_bytes()).await?;
    writer.shutdown().await?;

    let mut buf_reader = BufReader::new(reader);
    let mut response = String::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        buf_reader.read_line(&mut response),
    )
    .await
    .context("Response timed out")?
    .context("Failed to read response")?;

    Ok(response.trim().to_string())
}
