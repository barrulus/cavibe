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

    // Spectrogram history buffer (persists across frames)
    let mut spectrogram_history: Vec<Vec<f32>> = Vec::new();

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

        // Update spectrogram history
        spectrogram_history.push(audio.frequencies.clone());
        if spectrogram_history.len() > height as usize {
            let excess = spectrogram_history.len() - height as usize;
            spectrogram_history.drain(..excess);
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
            &spectrogram_history,
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

/// Bundled rendering context for direct mode, avoiding excessive function arguments
struct DirectRenderCtx<'a> {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    audio: &'a Arc<AudioData>,
    color_scheme: &'a ColorScheme,
    config: &'a VisualizerConfig,
    spectrogram_history: &'a [Vec<f32>],
}

/// Render a single frame directly to stdout using ANSI escape codes
#[allow(clippy::too_many_arguments)]
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
    spectrogram_history: &[Vec<f32>],
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

    let ctx = DirectRenderCtx {
        x: bars_x,
        y: bars_y,
        width: bars_w,
        height: bars_h,
        audio,
        color_scheme,
        config: &visualizer.visualizer_config,
        spectrogram_history,
    };

    // Render bars with current style
    render_bars_direct(stdout, &ctx, visualizer.current_style)?;

    // Render text area
    render_text_direct(stdout, text_x, text_y, text_w, text_height, track, audio, color_scheme, visualizer.time)?;

    stdout.flush()?;
    Ok(())
}

/// Render bars directly using ANSI codes
fn render_bars_direct(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
    style: usize,
) -> Result<()> {
    if ctx.width == 0 || ctx.height == 0 || ctx.audio.frequencies.is_empty() {
        return Ok(());
    }

    let bar_count = ctx.audio.frequencies.len().min(ctx.width as usize);

    // Clear the entire bar area first
    for row in 0..ctx.height {
        for col in 0..ctx.width {
            execute!(
                stdout,
                MoveTo(ctx.x + col, ctx.y + row),
                SetBackgroundColor(Color::Reset),
                Print(" ")
            )?;
        }
    }

    match style {
        1 => render_direct_mirrored(stdout, ctx, bar_count),
        2 => render_direct_wave(stdout, ctx, bar_count),
        3 => render_direct_dots(stdout, ctx, bar_count),
        4 => render_direct_blocks(stdout, ctx, bar_count),
        5 => render_direct_oscilloscope(stdout, ctx),
        6 => render_direct_spectrogram(stdout, ctx),
        7 => render_direct_radial(stdout, ctx),
        _ => render_direct_classic(stdout, ctx, bar_count),
    }
}

/// Style 0: Classic vertical bars from bottom
fn render_direct_classic(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
    bar_count: usize,
) -> Result<()> {
    for (i, &magnitude) in ctx.audio.frequencies.iter().take(bar_count).enumerate() {
        let bar_height = (magnitude * ctx.height as f32) as u16;
        let x_start = bar_x_position(i, bar_count, ctx.width);
        let x_end = bar_x_position(i + 1, bar_count, ctx.width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = ctx.x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, ctx.config);

        for offset in 0..bar_height.min(ctx.height) {
            let row = ctx.y + ctx.height - 1 - offset;
            let intensity = offset as f32 / ctx.height as f32;
            let (r, g, b) = ctx.color_scheme.get_color(position, intensity);

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
    ctx: &DirectRenderCtx,
    bar_count: usize,
) -> Result<()> {
    let center = ctx.y + ctx.height / 2;

    for (i, &magnitude) in ctx.audio.frequencies.iter().take(bar_count).enumerate() {
        let half_height = (magnitude * ctx.height as f32 / 2.0) as u16;
        let x_start = bar_x_position(i, bar_count, ctx.width);
        let x_end = bar_x_position(i + 1, bar_count, ctx.width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = ctx.x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, ctx.config);

        for offset in 0..half_height.min(ctx.height / 2) {
            let intensity = offset as f32 / (ctx.height as f32 / 2.0);
            let (r, g, b) = ctx.color_scheme.get_color(position, intensity);

            // Upper half
            let row_up = center.saturating_sub(offset);
            if row_up >= ctx.y {
                for bx in 0..draw_width {
                    execute!(stdout, MoveTo(bar_x + bx, row_up), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
                }
            }

            // Lower half
            let row_down = center + offset;
            if row_down < ctx.y + ctx.height {
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
    ctx: &DirectRenderCtx,
    bar_count: usize,
) -> Result<()> {
    let center = ctx.y + ctx.height / 2;

    for (i, &magnitude) in ctx.audio.frequencies.iter().take(bar_count).enumerate() {
        let wave_height = (magnitude * (ctx.height as f32 / 2.0)) as i16;
        let col = ctx.x + bar_x_position(i, bar_count, ctx.width);
        if col >= ctx.x + ctx.width {
            break;
        }
        let position = i as f32 / bar_count as f32;

        for offset in -wave_height..=wave_height {
            let row = (center as i16 + offset) as u16;
            if row >= ctx.y && row < ctx.y + ctx.height {
                let intensity = 1.0 - (offset.unsigned_abs() as f32 / wave_height.max(1) as f32);
                let (r, g, b) = ctx.color_scheme.get_color(position, intensity);
                execute!(stdout, MoveTo(col, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("│"))?;
            }
        }
    }
    Ok(())
}

/// Style 3: Dots at peak with trailing dots below
fn render_direct_dots(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
    bar_count: usize,
) -> Result<()> {
    for (i, &magnitude) in ctx.audio.frequencies.iter().take(bar_count).enumerate() {
        let col = ctx.x + bar_x_position(i, bar_count, ctx.width);
        if col >= ctx.x + ctx.width {
            break;
        }
        let position = i as f32 / bar_count as f32;
        let dot_y = ctx.y + ctx.height - 1 - (magnitude * (ctx.height - 1) as f32) as u16;

        // Draw dot
        if dot_y >= ctx.y && dot_y < ctx.y + ctx.height {
            let (r, g, b) = ctx.color_scheme.get_color(position, magnitude);
            execute!(stdout, MoveTo(col, dot_y), SetForegroundColor(Color::Rgb { r, g, b }), Print("●"))?;
        }

        // Draw trail
        for trail_y in (dot_y + 1)..(ctx.y + ctx.height) {
            let trail_intensity = 1.0 - ((trail_y - dot_y) as f32 / (ctx.height as f32 / 2.0));
            if trail_intensity <= 0.0 {
                break;
            }
            let (r, g, b) = ctx.color_scheme.get_color(position, trail_intensity * magnitude);
            execute!(stdout, MoveTo(col, trail_y), SetForegroundColor(Color::Rgb { r, g, b }), Print("·"))?;
        }
    }
    Ok(())
}

/// Style 4: Blocks with partial unicode block characters
fn render_direct_blocks(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
    bar_count: usize,
) -> Result<()> {
    let block_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    for (i, &magnitude) in ctx.audio.frequencies.iter().take(bar_count).enumerate() {
        let x_start = bar_x_position(i, bar_count, ctx.width);
        let x_end = bar_x_position(i + 1, bar_count, ctx.width);
        let slot_width = (x_end - x_start).max(1);
        let bar_x = ctx.x + x_start;
        let position = i as f32 / bar_count as f32;
        let draw_width = calculate_bar_dimensions(slot_width, ctx.config);

        let full_blocks = (magnitude * ctx.height as f32) as u16;
        let partial = ((magnitude * ctx.height as f32) % 1.0 * 8.0) as usize;

        // Draw full blocks
        for b in 0..full_blocks.min(ctx.height) {
            let row = ctx.y + ctx.height - 1 - b;
            let intensity = b as f32 / ctx.height as f32;
            let (r, g, b) = ctx.color_scheme.get_color(position, intensity);

            for bx in 0..draw_width {
                execute!(stdout, MoveTo(bar_x + bx, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
            }
        }

        // Draw partial block on top
        if full_blocks < ctx.height && partial > 0 {
            let row = ctx.y + ctx.height - 1 - full_blocks;
            let intensity = full_blocks as f32 / ctx.height as f32;
            let (r, g, b) = ctx.color_scheme.get_color(position, intensity);
            let ch = block_chars[partial.min(7)];

            for bx in 0..draw_width {
                execute!(stdout, MoveTo(bar_x + bx, row), SetForegroundColor(Color::Rgb { r, g, b }), Print(ch))?;
            }
        }
    }
    Ok(())
}

/// Style 5: Oscilloscope — raw waveform as a continuous line
fn render_direct_oscilloscope(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
) -> Result<()> {
    if ctx.audio.waveform.is_empty() || ctx.width == 0 || ctx.height == 0 {
        return Ok(());
    }

    let num_samples = ctx.audio.waveform.len();
    let center_y = ctx.y + ctx.height / 2;
    let half_height = ctx.height as f32 / 2.0;

    let mut prev_row: Option<u16> = None;

    for col_offset in 0..ctx.width {
        let col = ctx.x + col_offset;
        // Map column to sample index
        let sample_idx = (col_offset as usize * num_samples) / ctx.width as usize;
        let sample = ctx.audio.waveform[sample_idx.min(num_samples - 1)];

        // Map sample (-1..1) to row
        let row = ((center_y as f32 - sample * half_height) as u16)
            .max(ctx.y)
            .min(ctx.y + ctx.height - 1);

        let position = col_offset as f32 / ctx.width as f32;
        let intensity = sample.abs().min(1.0);
        let (r, g, b) = ctx.color_scheme.get_color(position, intensity.max(0.3));

        // Fill vertically between prev_row and row for smoothness
        let (y_min, y_max) = if let Some(pr) = prev_row {
            (pr.min(row), pr.max(row))
        } else {
            (row, row)
        };

        for fill_row in y_min..=y_max {
            if fill_row >= ctx.y && fill_row < ctx.y + ctx.height {
                let ch = if fill_row == row { '●' } else { '│' };
                execute!(stdout, MoveTo(col, fill_row), SetForegroundColor(Color::Rgb { r, g, b }), Print(ch))?;
            }
        }

        prev_row = Some(row);
    }
    Ok(())
}

/// Style 6: Spectrogram — scrolling 2D heatmap (X=frequency, Y=time)
fn render_direct_spectrogram(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
) -> Result<()> {
    let history = ctx.spectrogram_history;
    if history.is_empty() || ctx.width == 0 || ctx.height == 0 {
        return Ok(());
    }

    let num_rows = history.len().min(ctx.height as usize);

    for (row_idx, slice) in history.iter().rev().take(num_rows).enumerate() {
        // row_idx 0 = newest (bottom), draw from bottom up
        let row = ctx.y + ctx.height - 1 - row_idx as u16;
        if row < ctx.y {
            break;
        }

        let num_freqs = slice.len();
        if num_freqs == 0 {
            continue;
        }

        for col in 0..ctx.width {
            let freq_idx = (col as usize * num_freqs) / ctx.width as usize;
            let magnitude = slice[freq_idx.min(num_freqs - 1)];
            let position = col as f32 / ctx.width as f32;
            let (r, g, b) = ctx.color_scheme.get_color(position, magnitude);

            execute!(
                stdout,
                MoveTo(ctx.x + col, row),
                SetForegroundColor(Color::Rgb { r, g, b }),
                Print("█")
            )?;
        }
    }
    Ok(())
}

/// Style 7: Radial — frequency bars radiating outward from a circle
fn render_direct_radial(
    stdout: &mut impl Write,
    ctx: &DirectRenderCtx,
) -> Result<()> {
    if ctx.width == 0 || ctx.height == 0 || ctx.audio.frequencies.is_empty() {
        return Ok(());
    }

    let bar_count = ctx.audio.frequencies.len();
    // Account for terminal character aspect ratio (~2:1 height:width)
    let char_aspect = 2.0_f32;
    let cx = ctx.width as f32 / 2.0;
    let cy = ctx.height as f32 / 2.0;
    // Effective dimensions accounting for aspect ratio
    let effective_w = ctx.width as f32;
    let effective_h = ctx.height as f32 * char_aspect;
    let half_dim = effective_w.min(effective_h) / 2.0;
    let base_radius = half_dim * 0.35;
    let max_radius = half_dim * 0.95;

    // Draw base circle
    let circle_steps = (base_radius * std::f32::consts::TAU).ceil() as usize;
    for step in 0..circle_steps {
        let angle = (step as f32 / circle_steps as f32) * std::f32::consts::TAU;
        let px = (cx + angle.cos() * base_radius).round() as u16;
        let py = (cy + angle.sin() * base_radius / char_aspect).round() as u16;
        let col = ctx.x + px;
        let row = ctx.y + py;
        if px < ctx.width && py < ctx.height {
            let position = ((angle + std::f32::consts::FRAC_PI_2) / std::f32::consts::TAU).rem_euclid(1.0);
            let (r, g, b) = ctx.color_scheme.get_color(position, 0.3);
            execute!(stdout, MoveTo(col, row), SetForegroundColor(Color::Rgb { r, g, b }), Print("·"))?;
        }
    }

    // Draw radial bars
    for i in 0..bar_count {
        let magnitude = ctx.audio.frequencies[i];
        if magnitude < 0.01 {
            continue;
        }
        let angle = -std::f32::consts::FRAC_PI_2
            + (i as f32 / bar_count as f32) * std::f32::consts::TAU;
        let bar_length = magnitude * (max_radius - base_radius);
        let position = i as f32 / bar_count as f32;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let steps = (bar_length.ceil() as usize).max(1);
        for s in 0..=steps {
            let r_dist = base_radius + (s as f32 / steps as f32) * bar_length;
            let px = (cx + cos_a * r_dist).round() as u16;
            let py = (cy + sin_a * r_dist / char_aspect).round() as u16;
            if px < ctx.width && py < ctx.height {
                let intensity = (r_dist - base_radius) / (max_radius - base_radius);
                let (r, g, b) = ctx.color_scheme.get_color(position, magnitude * 0.5 + intensity * 0.5);
                execute!(stdout, MoveTo(ctx.x + px, ctx.y + py), SetForegroundColor(Color::Rgb { r, g, b }), Print("█"))?;
            }
        }
    }
    Ok(())
}

/// Render text directly using ANSI codes
#[allow(clippy::too_many_arguments)]
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
