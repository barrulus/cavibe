//! Terminal display mode — pixel-accurate half-block rendering.
//!
//! Renders the visualizer to a pixel `Canvas` then converts each pair of
//! vertical pixels into a terminal cell using the upper-half-block character
//! `'▀'` with foreground = top pixel and background = bottom pixel.

use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio;
use crate::color::ColorScheme;
use crate::config::Config;
use crate::metadata::{self, TrackInfo};
use crate::renderer;
use crate::visualizer::VisualizerState;

pub async fn run(config: Config) -> Result<()> {
    let mut stdout = stdout();

    // Setup terminal
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide, Clear(ClearType::All))?;

    let result = run_app(&mut stdout, config).await;

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(stdout, Show, LeaveAlternateScreen)?;

    result
}

async fn run_app(stdout: &mut impl Write, config: Config) -> Result<()> {
    // Start audio capture
    let (_audio_capture, audio_rx) = audio::create_audio_pipeline(
        config.visualizer.bars,
        config.audio.smoothing,
        config.audio.sensitivity,
        config.audio.device.clone(),
    )?;

    // Start metadata watcher
    let metadata_rx = metadata::start_watcher();

    // Initialize visualizer state
    let mut visualizer = VisualizerState::new(config.visualizer.clone(), config.text.clone());
    let mut color_scheme = config.visualizer.color_scheme;

    let mut last_frame = Instant::now();
    let mut style_timer = Instant::now();
    let target_fps = Duration::from_secs_f64(1.0 / 60.0);

    // Reusable pixel canvas
    let mut canvas = renderer::Canvas::new(0, 0);

    // Spectrogram history buffer
    let mut spectrogram_history: Vec<Vec<f32>> = Vec::new();

    loop {
        // Calculate delta time
        let now = Instant::now();
        let dt = now.duration_since(last_frame).as_secs_f32();
        last_frame = now;

        // Auto-rotate styles if enabled
        if config.display.rotate_styles
            && style_timer.elapsed() > Duration::from_secs(config.display.rotation_interval_secs)
        {
            visualizer.next_style();
            style_timer = Instant::now();
        }

        // Update visualizer state
        visualizer.update(dt);

        // Get current audio and metadata
        let audio_data = audio_rx.borrow().clone();
        let track_info = metadata_rx.borrow().clone();

        // Get terminal size
        let (term_width, term_height) = terminal::size()?;
        if term_width == 0 || term_height == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // Canvas: width = terminal cols, height = terminal rows × 2 (half-block)
        let canvas_w = term_width as usize;
        let canvas_h = (term_height as usize).saturating_sub(1) * 2; // -1 row for status bar
        if canvas_w == 0 || canvas_h == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        canvas.resize(canvas_w, canvas_h);

        // Update spectrogram history
        spectrogram_history.push(audio_data.frequencies.clone());
        if spectrogram_history.len() > canvas_h {
            let excess = spectrogram_history.len() - canvas_h;
            spectrogram_history.drain(..excess);
        }

        // Disable bitmap text rendering — the terminal status bar handles text.
        // The bitmap font is designed for high-res pixel buffers, not ~80×50 canvases.
        let mut term_text_config = config.text.clone();
        term_text_config.show_title = false;
        term_text_config.show_artist = false;

        let params = renderer::RenderParams {
            style: visualizer.current_style,
            bar_width: config.visualizer.bar_width as usize,
            bar_spacing: config.visualizer.bar_spacing as usize,
            mirror: config.visualizer.mirror,
            reverse_mirror: config.visualizer.reverse_mirror,
            opacity: 1.0, // terminal doesn't use opacity
            color_scheme: &color_scheme,
            waveform: &audio_data.waveform,
            spectrogram_history: &spectrogram_history,
            text_config: &term_text_config,
        };

        let frame_data = renderer::FrameData {
            frequencies: &audio_data.frequencies,
            intensity: audio_data.intensity,
            track_title: &track_info.title,
            track_artist: &track_info.artist,
            time: visualizer.time,
        };

        renderer::render_frame(&mut canvas, &frame_data, &params);

        // Convert canvas to terminal half-block characters
        canvas_to_terminal(stdout, &canvas, term_width, term_height.saturating_sub(1))?;

        // Render status bar on the last row
        render_status(stdout, term_width, term_height, &visualizer, &color_scheme, &track_info)?;

        stdout.flush()?;

        // Handle input
        if event::poll(target_fps)? {
            if let Event::Key(key) = event::read()? {
                match key {
                    KeyEvent {
                        code: KeyCode::Char('q'),
                        ..
                    }
                    | KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => {
                        break;
                    }
                    KeyEvent {
                        code: KeyCode::Char('s'),
                        ..
                    } => {
                        visualizer.next_style();
                    }
                    KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => {
                        color_scheme = color_scheme.next();
                    }
                    KeyEvent {
                        code: KeyCode::Char('r'),
                        ..
                    } => {
                        // Toggle rotation
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// Convert a pixel canvas to terminal output using half-block characters.
///
/// Each terminal cell represents 2 vertical pixels:
///   - foreground color = top pixel  (via '▀')
///   - background color = bottom pixel
fn canvas_to_terminal(
    stdout: &mut impl Write,
    canvas: &renderer::Canvas,
    term_width: u16,
    term_rows: u16,
) -> Result<()> {
    let cols = (canvas.width as u16).min(term_width);

    for row in 0..term_rows {
        execute!(stdout, MoveTo(0, row))?;

        let top_y = row as usize * 2;
        let bot_y = top_y + 1;

        for col in 0..cols {
            let x = col as usize;
            let (tr, tg, tb, ta) = canvas.get_pixel(x, top_y);
            let (br, bg, bb, ba) = if bot_y < canvas.height {
                canvas.get_pixel(x, bot_y)
            } else {
                (0, 0, 0, 0)
            };

            if ta == 0 && ba == 0 {
                // Both transparent — reset
                execute!(
                    stdout,
                    SetForegroundColor(Color::Reset),
                    SetBackgroundColor(Color::Reset),
                    Print(" ")
                )?;
            } else {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Rgb { r: tr, g: tg, b: tb }),
                    SetBackgroundColor(Color::Rgb { r: br, g: bg, b: bb }),
                    Print("▀")
                )?;
            }
        }

        // Clear rest of line
        if cols < term_width {
            execute!(
                stdout,
                SetForegroundColor(Color::Reset),
                SetBackgroundColor(Color::Reset)
            )?;
            for _ in cols..term_width {
                execute!(stdout, Print(" "))?;
            }
        }
    }

    Ok(())
}

fn render_status(
    stdout: &mut impl Write,
    term_width: u16,
    term_height: u16,
    visualizer: &VisualizerState,
    color_scheme: &ColorScheme,
    _track: &Arc<TrackInfo>,
) -> Result<()> {
    let status = format!(
        " [s]tyle: {} | [c]olor: {:?} | [q]uit ",
        visualizer.current_style_name(),
        color_scheme
    );

    execute!(
        stdout,
        MoveTo(0, term_height - 1),
        SetForegroundColor(Color::DarkGrey),
        SetBackgroundColor(Color::Reset)
    )?;

    for (i, ch) in status.chars().enumerate() {
        if i < term_width as usize {
            execute!(stdout, Print(ch))?;
        }
    }

    // Clear rest of status line
    let status_len = status.chars().count();
    for _ in status_len..term_width as usize {
        execute!(stdout, Print(" "))?;
    }

    Ok(())
}
