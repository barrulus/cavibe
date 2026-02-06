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
use crate::config::{Config, TextPosition, VisualizerConfig, WallpaperAnchor, WallpaperConfig};
use crate::ipc::IpcCommand;
use crate::metadata::{self, TrackInfo};
use crate::visualizer::VisualizerState;
use tokio::sync::mpsc;

/// Calculate the render area based on wallpaper config
fn calculate_render_area(
    config: &WallpaperConfig,
    term_width: u16,
    term_height: u16,
) -> (u16, u16, u16, u16) {
    // Get effective margins
    let (margin_top, margin_right, margin_bottom, margin_left) = config.effective_margins();
    let margin_top = margin_top.max(0) as u16;
    let margin_right = margin_right.max(0) as u16;
    let margin_bottom = margin_bottom.max(0) as u16;
    let margin_left = margin_left.max(0) as u16;

    // Calculate available area after margins
    let available_width = term_width.saturating_sub(margin_left + margin_right);
    let available_height = term_height.saturating_sub(margin_top + margin_bottom);

    // Get configured size or use available area
    let (width, height) = if let Some((w, h)) = config.get_size(term_width as u32, term_height as u32) {
        (
            (w as u16).min(available_width),
            (h as u16).min(available_height),
        )
    } else {
        (available_width, available_height)
    };

    // Calculate position based on anchor
    let (x, y) = match config.anchor {
        WallpaperAnchor::TopLeft => (margin_left, margin_top),
        WallpaperAnchor::Top => (
            margin_left + (available_width.saturating_sub(width)) / 2,
            margin_top,
        ),
        WallpaperAnchor::TopRight => (
            margin_left + available_width.saturating_sub(width),
            margin_top,
        ),
        WallpaperAnchor::Left => (
            margin_left,
            margin_top + (available_height.saturating_sub(height)) / 2,
        ),
        WallpaperAnchor::Center => (
            margin_left + (available_width.saturating_sub(width)) / 2,
            margin_top + (available_height.saturating_sub(height)) / 2,
        ),
        WallpaperAnchor::Right => (
            margin_left + available_width.saturating_sub(width),
            margin_top + (available_height.saturating_sub(height)) / 2,
        ),
        WallpaperAnchor::BottomLeft => (
            margin_left,
            margin_top + available_height.saturating_sub(height),
        ),
        WallpaperAnchor::Bottom => (
            margin_left + (available_width.saturating_sub(width)) / 2,
            margin_top + available_height.saturating_sub(height),
        ),
        WallpaperAnchor::BottomRight => (
            margin_left + available_width.saturating_sub(width),
            margin_top + available_height.saturating_sub(height),
        ),
        WallpaperAnchor::Fullscreen => (0, 0),
    };

    // For fullscreen, use full terminal size
    if config.anchor == WallpaperAnchor::Fullscreen {
        (0, 0, term_width, term_height)
    } else {
        (x, y, width, height)
    }
}

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
pub async fn run(config: Config, ipc_rx: mpsc::Receiver<IpcCommand>) -> Result<()> {
    info!("Wallpaper mode requested");

    if is_wayland() {
        // Use native Wayland layer-shell
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

    // For X11 or unknown, try to run in direct terminal mode
    // This works with xwinwrap, transparent terminals, etc.
    run_direct_mode(config, ipc_rx).await
}

/// Run in direct mode - renders to stdout with ANSI codes
/// Works with xwinwrap, transparent terminals, etc.
async fn run_direct_mode(config: Config, mut ipc_rx: mpsc::Receiver<IpcCommand>) -> Result<()> {
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
    let (mut _audio_capture, mut audio_rx) = audio::create_audio_pipeline(
        config.visualizer.bars,
        config.audio.smoothing,
        config.audio.sensitivity,
        config.audio.device.clone(),
    )?;

    // Initialize metadata source
    let mut metadata_rx = metadata::start_watcher();

    // Initialize visualizer
    let mut visualizer = VisualizerState::new(config.visualizer.clone(), config.text.clone());
    let mut color_scheme = config.visualizer.color_scheme;
    let mut visible = true;
    let mut opacity = config.visualizer.opacity;
    let mut config = config;

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

        // Process IPC commands (non-blocking)
        while let Ok(cmd) = ipc_rx.try_recv() {
            // Intercept audio commands before generic handler
            match cmd {
                IpcCommand::ListSources { reply } => {
                    let response = match audio::list_sources() {
                        Ok(sources) => {
                            let list: Vec<String> = sources
                                .iter()
                                .map(|(name, s)| format!("{} ({})", name, s))
                                .collect();
                            format!("ok: {}", list.join(", "))
                        }
                        Err(e) => format!("err: {}", e),
                    };
                    let _ = reply.send(response);
                }
                IpcCommand::SetSource { name, reply } => {
                    let result = if name == "default" {
                        audio::create_audio_pipeline(
                            config.visualizer.bars,
                            config.audio.smoothing,
                            config.audio.sensitivity,
                            config.audio.device.clone(),
                        )
                    } else {
                        audio::create_audio_pipeline_with_source(
                            config.visualizer.bars,
                            config.audio.smoothing,
                            config.audio.sensitivity,
                            name.clone(),
                        )
                    };
                    match result {
                        Ok((capture, rx)) => {
                            _audio_capture = capture;
                            audio_rx = rx;
                            let _ = reply.send(format!("ok: {}", name));
                        }
                        Err(e) => {
                            let _ = reply.send(format!("err: {}", e));
                        }
                    }
                }
                cmd => {
                    crate::ipc::process_ipc_command(
                        cmd,
                        &mut visualizer,
                        &mut color_scheme,
                        &mut visible,
                        &mut opacity,
                        &mut config,
                        &[], // No monitor info in X11/direct mode
                    );
                }
            }
        }

        if !visible {
            // Clear screen once and sleep
            execute!(stdout, Clear(ClearType::All))?;
            tokio::time::sleep(frame_duration).await;
            last_frame = Instant::now();
            continue;
        }

        // Get terminal size
        let (term_width, term_height) = terminal::size()?;
        if term_width == 0 || term_height == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // Calculate render area based on wallpaper config
        let (render_x, render_y, width, height) =
            calculate_render_area(&config.wallpaper, term_width, term_height);

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
        render_frame(
            &mut stdout,
            render_x,
            render_y,
            width,
            height,
            &visualizer,
            &audio,
            &track,
            &color_scheme,
        )?;

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
    offset_x: u16,
    offset_y: u16,
    width: u16,
    height: u16,
    visualizer: &VisualizerState,
    audio: &Arc<AudioData>,
    track: &Arc<TrackInfo>,
    color_scheme: &ColorScheme,
) -> Result<()> {
    let text_height = 3u16;
    let position = visualizer.text_animator.position();

    // Calculate layout based on text position
    let (bars_x, bars_y, bars_w, bars_h, text_x, text_y, text_w) = match position {
        TextPosition::Top => {
            let bh = height.saturating_sub(text_height);
            (offset_x, offset_y + text_height, width, bh, offset_x, offset_y, width)
        }
        TextPosition::Bottom => {
            let bh = height.saturating_sub(text_height);
            (offset_x, offset_y, width, bh, offset_x, offset_y + bh, width)
        }
        TextPosition::Center => {
            let ty = offset_y + (height.saturating_sub(text_height)) / 2;
            (offset_x, offset_y, width, height, offset_x, ty, width)
        }
        TextPosition::Coordinates { x, y } => {
            let tx = (x.resolve(width as usize) as u16).min(width.saturating_sub(1)) + offset_x;
            let ty = (y.resolve(height as usize) as u16).min(height.saturating_sub(text_height)) + offset_y;
            (offset_x, offset_y, width, height, tx, ty, width.saturating_sub(tx - offset_x))
        }
    };

    // Render bars with current style
    render_bars_direct(stdout, bars_x, bars_y, bars_w, bars_h, audio, color_scheme, &visualizer.visualizer_config, visualizer.current_style)?;

    // Render text area
    render_text_direct(stdout, text_x, text_y, text_w, text_height, track, audio, color_scheme, visualizer.time)?;

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
    style: usize,
) -> Result<()> {
    if width == 0 || height == 0 || audio.frequencies.is_empty() {
        return Ok(());
    }

    let bar_count = audio.frequencies.len().min(width as usize);

    // Clear the entire bar area first
    for row in 0..height {
        for col in 0..width {
            execute!(
                stdout,
                MoveTo(x + col, y + row),
                SetBackgroundColor(Color::Reset),
                Print(" ")
            )?;
        }
    }

    match style {
        1 => render_direct_mirrored(stdout, x, y, width, height, audio, color_scheme, config, bar_count),
        2 => render_direct_wave(stdout, x, y, width, height, audio, color_scheme, config, bar_count),
        3 => render_direct_dots(stdout, x, y, width, height, audio, color_scheme, config, bar_count),
        4 => render_direct_blocks(stdout, x, y, width, height, audio, color_scheme, config, bar_count),
        _ => render_direct_classic(stdout, x, y, width, height, audio, color_scheme, config, bar_count),
    }
}

/// Style 0: Classic vertical bars from bottom
fn render_direct_classic(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    config: &VisualizerConfig,
    bar_count: usize,
) -> Result<()> {
    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let bar_height = (magnitude * height as f32) as u16;
        let x_start = bar_x_position(i, bar_count, width);
        let x_end = bar_x_position(i + 1, bar_count, width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, config);

        for offset in 0..bar_height.min(height) {
            let row = y + height - 1 - offset;
            let intensity = offset as f32 / height as f32;
            let (r, g, b) = color_scheme.get_color(position, intensity);

            for bx in 0..draw_width {
                execute!(stdout, MoveTo(bar_x + bx, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
            }
        }
    }
    Ok(())
}

/// Style 1: Mirrored bars growing from center
fn render_direct_mirrored(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    config: &VisualizerConfig,
    bar_count: usize,
) -> Result<()> {
    let center = y + height / 2;

    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let half_height = (magnitude * height as f32 / 2.0) as u16;
        let x_start = bar_x_position(i, bar_count, width);
        let x_end = bar_x_position(i + 1, bar_count, width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, config);

        for offset in 0..half_height.min(height / 2) {
            let intensity = offset as f32 / (height as f32 / 2.0);
            let (r, g, b) = color_scheme.get_color(position, intensity);

            // Upper half
            let row_up = center.saturating_sub(offset);
            if row_up >= y {
                for bx in 0..draw_width {
                    execute!(stdout, MoveTo(bar_x + bx, row_up), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
                }
            }

            // Lower half
            let row_down = center + offset;
            if row_down < y + height {
                for bx in 0..draw_width {
                    execute!(stdout, MoveTo(bar_x + bx, row_down), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
                }
            }
        }
    }
    Ok(())
}

/// Style 2: Wave lines centered on middle row
fn render_direct_wave(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    _config: &VisualizerConfig,
    bar_count: usize,
) -> Result<()> {
    let center = y + height / 2;

    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let wave_height = (magnitude * (height as f32 / 2.0)) as i16;
        let col = x + bar_x_position(i, bar_count, width);
        if col >= x + width {
            break;
        }
        let position = i as f32 / bar_count as f32;

        for offset in -wave_height..=wave_height {
            let row = (center as i16 + offset) as u16;
            if row >= y && row < y + height {
                let intensity = 1.0 - (offset.unsigned_abs() as f32 / wave_height.max(1) as f32);
                let (r, g, b) = color_scheme.get_color(position, intensity);
                execute!(stdout, MoveTo(col, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("│"))?;
            }
        }
    }
    Ok(())
}

/// Style 3: Dots at peak with trailing dots below
fn render_direct_dots(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    _config: &VisualizerConfig,
    bar_count: usize,
) -> Result<()> {
    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let col = x + bar_x_position(i, bar_count, width);
        if col >= x + width {
            break;
        }
        let position = i as f32 / bar_count as f32;
        let dot_y = y + height - 1 - (magnitude * (height - 1) as f32) as u16;

        // Draw dot
        if dot_y >= y && dot_y < y + height {
            let (r, g, b) = color_scheme.get_color(position, magnitude);
            execute!(stdout, MoveTo(col, dot_y), SetForegroundColor(Color::Rgb { r, g, b }), Print("●"))?;
        }

        // Draw trail
        for trail_y in (dot_y + 1)..(y + height) {
            let trail_intensity = 1.0 - ((trail_y - dot_y) as f32 / (height as f32 / 2.0));
            if trail_intensity <= 0.0 {
                break;
            }
            let (r, g, b) = color_scheme.get_color(position, trail_intensity * magnitude);
            execute!(stdout, MoveTo(col, trail_y), SetForegroundColor(Color::Rgb { r, g, b }), Print("·"))?;
        }
    }
    Ok(())
}

/// Style 4: Blocks with partial unicode block characters
fn render_direct_blocks(
    stdout: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &Arc<AudioData>,
    color_scheme: &ColorScheme,
    config: &VisualizerConfig,
    bar_count: usize,
) -> Result<()> {
    let block_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    for (i, &magnitude) in audio.frequencies.iter().take(bar_count).enumerate() {
        let x_start = bar_x_position(i, bar_count, width);
        let x_end = bar_x_position(i + 1, bar_count, width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, config);

        let full_blocks = (magnitude * height as f32) as u16;
        let partial = ((magnitude * height as f32) % 1.0 * 8.0) as usize;

        // Draw full blocks
        for b in 0..full_blocks.min(height) {
            let row = y + height - 1 - b;
            let intensity = b as f32 / height as f32;
            let (r, g, b) = color_scheme.get_color(position, intensity);

            for bx in 0..draw_width {
                execute!(stdout, MoveTo(bar_x + bx, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
            }
        }

        // Draw partial block on top
        if full_blocks < height && partial > 0 {
            let row = y + height - 1 - full_blocks;
            let intensity = full_blocks as f32 / height as f32;
            let (r, g, b) = color_scheme.get_color(position, intensity);
            let ch = block_chars[partial.min(7)];

            for bx in 0..draw_width {
                execute!(stdout, MoveTo(bar_x + bx, row), SetForegroundColor(Color::Rgb { r, g, b }), Print(ch))?;
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
