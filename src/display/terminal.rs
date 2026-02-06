use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio;
use crate::color::ColorScheme;
use crate::config::Config;
use crate::metadata::{self, TrackInfo};
use crate::visualizer::VisualizerState;

pub async fn run(config: Config) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_app(&mut terminal, config).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, config: Config) -> Result<()> {
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

        // Render
        terminal.draw(|frame| {
            let area = frame.area();

            // Clear with transparent/reset background for terminal transparency support
            let block = ratatui::widgets::Block::default()
                .style(Style::default().bg(Color::Reset));
            frame.render_widget(block, area);

            // Render visualizer
            visualizer.render(frame, area, &audio_data, &track_info, &color_scheme);

            // Render status bar
            render_status(frame, area, &visualizer, &color_scheme, &track_info);
        })?;

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

fn render_status(
    frame: &mut Frame,
    area: Rect,
    visualizer: &VisualizerState,
    color_scheme: &ColorScheme,
    _track: &Arc<TrackInfo>,
) {
    // Status line at top
    let status = format!(
        " [s]tyle: {} | [c]olor: {:?} | [q]uit ",
        visualizer.current_style_name(),
        color_scheme
    );

    let _status_area = Rect::new(area.x, area.y, area.width, 1);

    for (i, ch) in status.chars().enumerate() {
        if i < area.width as usize {
            let cell = frame.buffer_mut().cell_mut((area.x + i as u16, area.y));
            if let Some(cell) = cell {
                cell.set_char(ch);
                cell.set_fg(Color::DarkGray);
            }
        }
    }
}
