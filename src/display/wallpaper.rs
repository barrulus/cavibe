//! Wallpaper mode dispatcher.
//!
//! On Wayland: delegates to the layer-shell backend in `wayland.rs`.
//! On X11/other: prints setup instructions (use terminal mode with a transparent
//! terminal instead).

use anyhow::Result;
use tracing::info;

use crate::config::Config;
use crate::ipc::IpcCommand;
use tokio::sync::mpsc;

/// Check if running under Wayland
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// Wallpaper/overlay mode
///
/// On Wayland: Uses wlr-layer-shell protocol to render as a background layer.
/// On X11/other: Prints instructions for achieving the same effect with a
/// transparent terminal.
pub async fn run(config: Config, ipc_rx: mpsc::Receiver<IpcCommand>) -> Result<()> {
    info!("Wallpaper mode requested");

    if is_wayland() {
        #[cfg(feature = "wayland")]
        {
            return super::wayland::run(config, ipc_rx).await;
        }

        #[cfg(not(feature = "wayland"))]
        {
            drop(ipc_rx);
            return run_wayland_instructions().await;
        }
    }

    // Non-Wayland: print instructions
    drop(ipc_rx);
    run_x11_instructions().await
}

/// Print instructions for X11 users
async fn run_x11_instructions() -> Result<()> {
    println!("Cavibe Wallpaper Mode - X11 Detected");
    println!("=====================================");
    println!();
    println!("Direct wallpaper mode requires Wayland with wlr-layer-shell support.");
    println!();
    println!("To get a wallpaper-like experience on X11, use terminal mode with a");
    println!("transparent terminal and window manager rules:");
    println!();
    println!("  1. Configure your terminal with full transparency");
    println!("  2. Use window manager rules to pin the terminal below other windows");
    println!("  3. Run: cavibe --mode terminal");
    println!();
    println!("Example with xwinwrap:");
    println!("  xwinwrap -fs -fdt -ni -b -nf -un -o 1.0 -st -- \\");
    println!("    cavibe --mode terminal");
    println!();

    Ok(())
}

/// Print instructions for Wayland users when the wayland feature is disabled
#[allow(dead_code)]
async fn run_wayland_instructions() -> Result<()> {
    println!("Cavibe Wallpaper Mode - Wayland Detected");
    println!("=========================================");
    println!();
    println!("The Wayland backend is not compiled in. Rebuild with:");
    println!("  cargo build --features wayland");
    println!();
    println!("Alternatively, use terminal mode with a transparent terminal:");
    println!("  cavibe --mode terminal");
    println!();

    Ok(())
}
