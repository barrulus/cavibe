use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use crate::audio::{self, AudioData};
use crate::color::ColorScheme;
use crate::config::{Config, VisualizerConfig};
use crate::metadata::{self, TrackInfo};
use crate::visualizer::VisualizerState;

/// Calculate evenly distributed x position using integer math to avoid truncation artifacts.
#[inline]
fn bar_x_position(i: usize, count: usize, width: u16) -> u16 {
    if count == 0 {
        return 0;
    }
    ((i * width as usize) / count) as u16
}

/// Calculate the bar width based on config proportions.
#[inline]
fn calculate_bar_dimensions(slot_width: u16, config: &VisualizerConfig) -> u16 {
    if slot_width == 0 {
        return 0;
    }
    let bar_ratio = config.bar_width as f32 / (config.bar_width + config.bar_spacing) as f32;
    ((slot_width as f32 * bar_ratio).round() as u16).max(1).min(slot_width)
}

/// Check if running under Wayland
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// Wallpaper/overlay mode
///
/// On Wayland: Uses wlr-layer-shell protocol to render as a background layer.
/// On X11: Renders to stdout with ANSI escape sequences for use with xwinwrap.
///
/// Usage with xwinwrap (X11):
/// ```bash
/// xwinwrap -fs -fdt -ni -b -nf -un -o 1.0 -st -- \
///   cavibe --mode wallpaper
/// ```
pub async fn run(config: Config) -> Result<()> {
    info!("Wallpaper mode requested");

    if is_wayland() {
        // Use native Wayland layer-shell
        #[cfg(feature = "wayland")]
        {
            return super::wayland::run(config).await;
        }

        #[cfg(not(feature = "wayland"))]
        {
            return run_wayland_instructions().await;
        }
    }

    // For X11 or unknown, try to run in direct terminal mode
    // This works with xwinwrap, transparent terminals, etc.
    run_direct_mode(config).await
}

/// Run in direct mode - renders to stdout with ANSI codes
/// Works with xwinwrap, transparent terminals, etc.
async fn run_direct_mode(config: Config) -> Result<()> {
    let mut stdout = stdout();

    // Setup terminal
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide, Clear(ClearType::All))?;

    // Create shutdown handler
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let shutdown_tx_clone = shutdown_tx.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_tx_clone.send(true).ok();
    });

    // Initialize audio capture
    let (_audio_capture, mut audio_rx) = audio::create_audio_pipeline(
        config.visualizer.bars,
        config.audio.smoothing,
        config.audio.sensitivity,
    )?;

    // Initialize metadata source
    let mut metadata_rx = metadata::start_watcher();

    // Initialize visualizer
    let mut visualizer = VisualizerState::new(config.visualizer.clone(), config.text.clone());
    let color_scheme = &config.visualizer.color_scheme;

    let target_fps = 60;
    let frame_duration = Duration::from_secs_f64(1.0 / target_fps as f64);
    let mut last_frame = Instant::now();

    // Style rotation
    let mut style_timer = Instant::now();
    let rotation_interval = Duration::from_secs(config.display.rotation_interval_secs);

    loop {
        // Check for shutdown
        if *shutdown_rx.borrow() {
            break;
        }

        // Get terminal size
        let (width, height) = terminal::size()?;
        if width == 0 || height == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // Update audio data
        let audio = audio_rx.borrow_and_update().clone();
        let track = metadata_rx.borrow_and_update().clone();

        // Update visualizer
        let dt = last_frame.elapsed().as_secs_f32();
        last_frame = Instant::now();
        visualizer.update(dt);

        // Auto-rotate styles if enabled
        if config.display.rotate_styles && style_timer.elapsed() >= rotation_interval {
            visualizer.next_style();
            style_timer = Instant::now();
        }

        // Render frame directly to stdout
        render_frame(&mut stdout, width, height, &visualizer, &audio, &track, color_scheme)?;

        // Rate limiting
        let elapsed = last_frame.elapsed();
        if elapsed < frame_duration {
            tokio::time::sleep(frame_duration - elapsed).await;
        }
    }

    // Cleanup
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

/// Render a single frame directly to stdout using ANSI escape codes
fn render_frame(
    stdout: &mut impl Write,
    width: u16,
    height: u16,
    visualizer: &VisualizerState,
    audio: &Arc<AudioData>,
    track: &Arc<TrackInfo>,
    color_scheme: &ColorScheme,
) -> Result<()> {
    // Calculate layout - simple split between bars and text
    let text_height = 3u16;
    let bars_height = height.saturating_sub(text_height);

    // Render bars
    render_bars_direct(stdout, 0, 0, width, bars_height, audio, color_scheme, &visualizer.visualizer_config)?;

    // Render text area
    render_text_direct(stdout, 0, bars_height, width, text_height, track, audio, color_scheme, visualizer.time)?;

    stdout.flush()?;
    Ok(())
}

/// Render bars directly using ANSI codes
fn render_bars_direct(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    config: &VisualizerConfig,
) -> Result<()> {
    if width == 0 || height == 0 || audio.frequencies.is_empty() {
        return Ok(());
    }

    let bar_count = audio.frequencies.len().min(width as usize);

    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let bar_height = (magnitude * height as f32) as u16;

        // Calculate bar position and width using integer math for even spacing
        let x_start = bar_x_position(i, bar_count, width);
        let x_end = bar_x_position(i + 1, bar_count, width);
        let slot_width = (x_end - x_start).max(1);

        let bar_x = x + x_start;
        let position = i as f32 / bar_count as f32;

        // Use config ratio for bar vs spacing
        let draw_width = calculate_bar_dimensions(slot_width, config);

        // Clear column first
        for bx in 0..draw_width {
            for row in 0..height {
                execute!(
                    stdout,
                    MoveTo(bar_x + bx, y + row),
                    SetBackgroundColor(Color::Reset),
                    Print(" ")
                )?;
            }
        }

        // Draw bar from bottom
        for offset in 0..bar_height.min(height) {
            let row = y + height - 1 - offset;
            let intensity = offset as f32 / height as f32;
            let (r, g, b) = color_scheme.get_color(position, intensity);

            for bx in 0..draw_width {
                execute!(
                    stdout,
                    MoveTo(bar_x + bx, row),
                    SetForegroundColor(Color::Rgb { r, g, b }),
                    Print("█")
                )?;
            }
        }
    }

    Ok(())
}

/// Render text directly using ANSI codes
fn render_text_direct(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    _height: u16,
    track: &Arc<TrackInfo>,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    time: f32,
) -> Result<()> {
    // Build display text
    let text = match (&track.title, &track.artist) {
        (Some(title), Some(artist)) => format!("{} - {}", title, artist),
        (Some(title), None) => title.clone(),
        (None, Some(artist)) => artist.clone(),
        (None, None) => "♪ cavibe ♪".to_string(),
    };

    // Center the text
    let text_len = text.chars().count() as u16;
    let start_x = x + width.saturating_sub(text_len) / 2;

    // Clear the line
    execute!(stdout, MoveTo(x, y))?;
    for _ in 0..width {
        execute!(stdout, Print(" "))?;
    }

    // Render text with gradient
    let colors = color_scheme.get_text_gradient(text.len(), audio.intensity, time);
    for (i, ch) in text.chars().enumerate() {
        let char_x = start_x + i as u16;
        if char_x >= x + width {
            break;
        }

        let (r, g, b) = colors.get(i).copied().unwrap_or((255, 255, 255));
        execute!(
            stdout,
            MoveTo(char_x, y),
            SetForegroundColor(Color::Rgb { r, g, b }),
            Print(ch)
        )?;
    }

    // Render intensity bar
    let bar_y = y + 1;
    let bar_width = (audio.intensity * width as f32) as u16;

    execute!(stdout, MoveTo(x, bar_y))?;
    for i in 0..width {
        if i < bar_width {
            let pos = i as f32 / width as f32;
            let (r, g, b) = color_scheme.get_color(pos, audio.intensity);
            execute!(
                stdout,
                SetForegroundColor(Color::Rgb { r, g, b }),
                Print("▀")
            )?;
        } else {
            execute!(stdout, Print(" "))?;
        }
    }

    Ok(())
}

/// Print instructions for Wayland users (when wayland feature is disabled)
#[allow(dead_code)]
async fn run_wayland_instructions() -> Result<()> {
    println!("Cavibe Wallpaper Mode - Wayland Detected");
    println!("=========================================");
    println!();
    println!("Direct wallpaper mode is not supported on Wayland due to protocol restrictions.");
    println!();
    println!("Recommended alternatives:");
    println!();
    println!("1. **Using swww (animated wallpapers):**");
    println!("   - Install swww: https://github.com/LGFae/swww");
    println!("   - swww can display animated content as wallpaper");
    println!();
    println!("2. **Using mpvpaper:**");
    println!("   - Install mpvpaper for video/animation wallpapers");
    println!("   - Can be combined with cavibe output to a video file");
    println!();
    println!("3. **Using a transparent terminal:**");
    println!("   - Configure your terminal (kitty, alacritty, foot) with transparency");
    println!("   - Use your compositor's layer rules to place it at desktop level");
    println!("   - Example for Hyprland:");
    println!("     windowrulev2 = float,class:^(cavibe)$");
    println!("     windowrulev2 = pin,class:^(cavibe)$");
    println!("     windowrulev2 = nofocus,class:^(cavibe)$");
    println!("   - Run: cavibe --mode terminal");
    println!();
    println!("4. **Using wlr-layer-shell directly:**");
    println!("   - Requires compositor support (wlroots-based compositors)");
    println!("   - Future versions of cavibe may add native support");
    println!();

    Ok(())
}
