use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use notify::Watcher;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::audio::AudioPlayer;
use crate::error::LilypondxError;
use crate::note::{self, ParsedTrack};
use crate::parser;
use crate::score::Score;
use crate::sparkline::{self, SparklineConfig};

/// Application state for the TUI.
pub struct App {
    pub score: Score,
    pub player: Option<AudioPlayer>,
    pub sf2_path: PathBuf,
    pub sparkline_width: usize,
    pub sparkline_rows: usize,
    /// Index of selected track
    pub selected_track: usize,
}

impl App {
    pub fn new(score: Score, sf2_path: PathBuf, width: usize, rows: usize) -> Self {
        Self {
            score,
            player: None,
            sf2_path,
            sparkline_width: width,
            sparkline_rows: rows,
            selected_track: 0,
        }
    }

    /// Parse all tracks into `ParsedTrack` structs.
    pub fn parsed_tracks(&self) -> Vec<(String, ParsedTrack)> {
        self.score
            .tracks
            .iter()
            .map(|t| {
                let parsed =
                    note::parse_notes_relative(&t.notes, &t.relative, 480);
                (t.name.clone(), parsed)
            })
            .collect()
    }

    /// Start or restart playback.
    pub fn start_playback(&mut self) -> Result<(), LilypondxError> {
        // Stop any existing playback
        self.stop_playback();

        let ticks_per_beat = 480;
        let tempo_bpm: u32 = self
            .score
            .metadata
            .tempo
            .as_deref()
            .and_then(|t| t.split('=').nth(1).and_then(|s| s.trim().parse().ok()))
            .unwrap_or(120);

        let events = crate::audio::generate_events_direct(&self.score, ticks_per_beat);
        let player = AudioPlayer::new(events, ticks_per_beat, tempo_bpm);
        player.play_background(&self.sf2_path)?;
        self.player = Some(player);
        Ok(())
    }

    pub fn stop_playback(&mut self) {
        if let Some(ref p) = self.player {
            p.stop();
        }
        self.player = None;
    }

    pub fn is_playing(&self) -> bool {
        self.player.is_some()
    }

    pub fn progress(&self) -> f64 {
        self.player.as_ref().map(|p| p.progress()).unwrap_or(0.0)
    }
}

/// Run the TUI watch loop. Blocks until the user quits.
pub fn run_tui(file: PathBuf, width: usize, rows: usize) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(&file)?;
    let sf2_path = crate::audio::find_soundfont()?;

    let mut app = App::new(score, sf2_path, width, rows);
    app.start_playback()?;

    // Setup terminal
    crossterm::terminal::enable_raw_mode()
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    // Watch for file changes
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, notify::EventKind::Modify(_)) {
                    let _ = tx.send(());
                }
            }
        })
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let canonical = file.canonicalize()?;
    watcher
        .watch(&canonical, notify::RecursiveMode::NonRecursive)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    // Main event loop
    let tick_rate = Duration::from_millis(66); // ~15fps
    loop {
        // Draw
        terminal
            .draw(|f| draw_ui(f, &app))
            .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

        // Check for events with timeout
        if event::poll(tick_rate).map_err(|e| LilypondxError::Io(std::io::Error::other(e)))? {
            if let Event::Key(key) =
                event::read().map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?
            {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char(' ') => {
                            if app.is_playing() {
                                app.stop_playback();
                            } else {
                                let _ = app.start_playback();
                            }
                        }
                        KeyCode::Tab => {
                            let n = app.score.tracks.len();
                            app.selected_track = (app.selected_track + 1) % n.max(1);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Check for file changes (non-blocking)
        if let Ok(()) = rx.try_recv() {
            std::thread::sleep(Duration::from_millis(50)); // debounce
            // Re-parse
            if let Ok(new_score) = parser::parse_markdown(&canonical) {
                app.score = new_score;
                let _ = app.start_playback(); // restart on file change
            }
        }

        // Auto-stop when playback ends
        if let Some(ref p) = app.player {
            if p.progress() >= 1.0 {
                app.stop_playback();
            }
        }
    }

    // Cleanup
    app.stop_playback();
    drop(watcher);
    crossterm::terminal::disable_raw_mode()
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    Ok(())
}

fn draw_ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(10),   // sparkline area
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    // Header
    draw_header(f, chunks[0], app);

    // Sparkline
    draw_sparkline_area(f, chunks[1], app);

    // Status bar
    draw_status(f, chunks[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let title = &app.score.metadata.title;
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));

    let track = &app.score.tracks[app.selected_track.min(app.score.tracks.len().saturating_sub(1))];
    let text = format!(
        "{}  —  {} ({})  [{}]",
        title, track.name, track.clef, track.syntax
    );
    let p = Paragraph::new(text).block(block);
    f.render_widget(p, area);
}

fn draw_sparkline_area(f: &mut Frame, area: Rect, app: &App) {
    let visible: Vec<_> = app.score.tracks.iter()
        .filter(|t| t.syntax != "test")
        .collect();
    let n_tracks = visible.len();
    if n_tracks == 0 { return; }

    let track_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Ratio(1, n_tracks as u32); n_tracks])
        .split(area);

    let parsed_tracks = app.parsed_tracks();
    let progress = app.progress();

    for (i, track) in visible.iter().enumerate() {
        if i >= track_areas.len() { break; }
        let parsed = parsed_tracks.iter()
            .find(|(name, _)| name == &track.name)
            .map(|(_, p)| p);
        let Some(parsed) = parsed else { continue; };

        let config = SparklineConfig {
            rows: app.sparkline_rows,
            width: track_areas[i].width as usize - 8,
            progress: Some(progress),
            color: true,
            ..Default::default()
        };
        let spark = sparkline::render_sparkline(parsed, &config);

        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().fg(if i == app.selected_track { Color::Yellow } else { Color::Gray }));

        let p = Paragraph::new(spark).block(block);
        f.render_widget(p, track_areas[i]);
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let play_state = if app.is_playing() { "▶ Playing" } else { "⏸ Paused" };
    let text = format!(
        " [q]uit  [space] play/pause  [tab] track  |  {}  |  progress: {:.0}%",
        play_state,
        app.progress() * 100.0
    );
    let p = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(p, area);
}
