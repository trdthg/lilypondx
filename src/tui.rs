use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEvent, MouseEventKind};
use crossterm::terminal::{self as term};
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
use crate::sparkline::{self, SparklineConfig, GRID_X_OFFSET};
use crate::TICKS_PER_BEAT;

/// Application state for the TUI.
pub struct App {
    pub score: Score,
    pub player: Option<AudioPlayer>,
    parsed_cache: Vec<(String, ParsedTrack)>,
    cache_dirty: bool,
    /// Last playback progress captured before stop (so it doesn't jump to 0).
    final_progress: f64,
    pub reload_error: Option<String>,
    /// Hovered grid column (mouse).
    pub hover_col: Option<usize>,
    /// Screen rect of the grid area: x = left edge, width = total_cols.
    pub grid_rect: Rect,
    /// Total grid columns (cached for mouse mapping).
    pub total_cols: usize,
}

impl App {
    pub fn new(score: Score) -> Self {
        Self {
            score,
            player: None,
            parsed_cache: Vec::new(),
            cache_dirty: true,
            final_progress: 0.0,
            reload_error: None,
            hover_col: None,
            grid_rect: Rect::ZERO,
            total_cols: 0,
        }
    }

    pub fn parsed_tracks(&mut self) -> &[(String, ParsedTrack)] {
        if self.cache_dirty {
            self.parsed_cache = self
                .score
                .tracks
                .iter()
                .map(|t| {
                    let parsed = note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT);
                    (t.name.clone(), parsed)
                })
                .collect();
            self.cache_dirty = false;
        }
        &self.parsed_cache
    }

    pub fn start_playback(&mut self) -> Result<(), LilypondxError> {
        self.stop_playback();
        let tempo_bpm = tempo_from(&self.score);
        let events = crate::audio::generate_events_direct(&self.score, TICKS_PER_BEAT);
        if events.is_empty() {
            return Ok(());
        }
        let player = AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
        player.play_background()?;
        self.player = Some(player);
        self.final_progress = 0.0;
        Ok(())
    }

    pub fn stop_playback(&mut self) {
        if let Some(p) = &self.player {
            self.final_progress = p.progress();
            p.stop();
        }
        self.player = None;
    }

    pub fn is_playing(&self) -> bool {
        self.player.is_some()
    }

    pub fn progress(&self) -> f64 {
        match &self.player {
            Some(p) => p.progress(),
            None => self.final_progress,
        }
    }

    pub fn set_score(&mut self, score: Score) {
        self.score = score;
        self.cache_dirty = true;
    }

    /// Seek to a fractional position; restart playback from there.
    pub fn seek(&mut self, fraction: f64) -> Result<(), LilypondxError> {
        self.stop_playback();
        let tempo_bpm = tempo_from(&self.score);
        let events = crate::audio::generate_events_direct(&self.score, TICKS_PER_BEAT);
        if events.is_empty() {
            return Ok(());
        }
        let total = events.iter().map(|e| e.tick).max().unwrap_or(0);
        let target_tick = ((fraction.clamp(0.0, 1.0) * total as f64).round() as u64).min(total);
        let player = AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
        player.play_background_from(target_tick)?;
        self.player = Some(player);
        Ok(())
    }
}

fn tempo_from(score: &Score) -> u32 {
    score
        .metadata
        .tempo
        .as_deref()
        .and_then(|t| t.split('=').nth(1))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(120)
}

fn beats_per_bar_from(score: &Score) -> Option<u32> {
    score
        .metadata
        .time
        .as_deref()?
        .split('/')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// RAII guard: restores the terminal on drop (even on panic).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = term::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            term::LeaveAlternateScreen
        );
    }
}

/// Run the TUI watch loop. Blocks until the user quits.
pub fn run_tui(file: PathBuf, _width: usize, _rows: usize) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(&file)?;
    let mut app = App::new(score);
    app.start_playback()?;

    term::enable_raw_mode().map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    let _guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, term::EnterAlternateScreen, crossterm::event::EnableMouseCapture)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    let _ = terminal.hide_cursor();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res
                && matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                )
            {
                let _ = tx.send(Instant::now());
            }
        })
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let canonical = file.canonicalize()?;
    watcher
        .watch(&canonical, notify::RecursiveMode::NonRecursive)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let debounce = Duration::from_millis(150);
    let mut last_change: Option<Instant> = None;
    let mut prev_hover_col: Option<usize> = None;
    let mut needs_redraw = true;
    let mut running = true;
    while running {
        if needs_redraw {
            terminal.draw(|f| draw_ui(f, &mut app)).ok();
            needs_redraw = false;
        }

        // Drain the entire event queue each iteration so a burst of mouse-move
        // events doesn't accumulate (each frame would otherwise only consume
        // one, causing lag).
        let mut got_events = false;
        while event::poll(Duration::from_millis(0))
            .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?
        {
            let ev = event::read().map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
            got_events = true;
            match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => running = false,
                    KeyCode::Char(' ') => {
                        if app.is_playing() {
                            app.stop_playback();
                        } else {
                            let _ = app.start_playback();
                        }
                        needs_redraw = true;
                    }
                    _ => {}
                },
                Event::Mouse(MouseEvent { kind, column, .. }) => match kind {
                    MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                        app.hover_col = screen_x_to_col(column, app.grid_rect);
                    }
                    MouseEventKind::Down(_) => {
                        if let Some(col) = screen_x_to_col(column, app.grid_rect) {
                            let frac = if app.total_cols > 0 {
                                col as f64 / app.total_cols as f64
                            } else {
                                0.0
                            };
                            let _ = app.seek(frac);
                            needs_redraw = true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        // Only redraw on hover change (not on every mouse-move event that
        // maps to the same column — common when the cursor is stationary).
        if app.hover_col != prev_hover_col {
            prev_hover_col = app.hover_col;
            needs_redraw = true;
        }

        // Watcher events.
        while let Ok(ts) = rx.try_recv() {
            last_change = Some(ts);
        }
        if let Some(ts) = last_change
            && ts.elapsed() >= debounce
        {
            last_change = None;
            match parser::parse_markdown(&canonical) {
                Ok(new_score) => {
                    app.reload_error = None;
                    app.set_score(new_score);
                    let _ = app.start_playback();
                    needs_redraw = true;
                }
                Err(e) => app.reload_error = Some(format!("{e}")),
            }
        }

        // Playback progress always advances while playing — redraw to animate
        // the playback head. But cap the rate: only redraw if playing AND
        // enough time passed since the last draw (avoid spinning the CPU).
        if app.is_playing() {
            needs_redraw = true;
        }

        // Auto-stop when playback finishes.
        if let Some(p) = &app.player
            && p.progress() >= 1.0
        {
            app.stop_playback();
            needs_redraw = true;
        }

        if !got_events && !needs_redraw {
            // No work to do; sleep briefly to avoid busy-looping.
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    app.stop_playback();
    drop(watcher);
    Ok(())
}

fn screen_x_to_col(x: u16, grid_rect: Rect) -> Option<usize> {
    if x < grid_rect.x || x >= grid_rect.x + grid_rect.width {
        return None;
    }
    Some((x - grid_rect.x) as usize)
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, chunks[0], app);
    draw_sparkline_area(f, chunks[1], app);
    draw_status(f, chunks[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &mut App) {
    let title = &app.score.metadata.title;
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));

    let visible: Vec<&str> = app
        .score
        .tracks
        .iter()
        .filter(|t| t.syntax != "test")
        .map(|t| t.name.as_str())
        .collect();
    let text = if let Some(err) = &app.reload_error {
        format!("{title}  —  [reload error: {err}]")
    } else if visible.is_empty() {
        format!("{title}  —  (no tracks)")
    } else {
        format!("{title}  —  {}", visible.join(" · "))
    };
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_sparkline_area(f: &mut Frame, area: Rect, app: &mut App) {
    let visible: Vec<(String, String)> = app
        .score
        .tracks
        .iter()
        .filter(|t| t.syntax != "test")
        .map(|t| (t.name.clone(), t.clef.clone()))
        .collect();
    let n_tracks = visible.len();
    if n_tracks == 0 {
        return;
    }

    let parsed_snapshot: Vec<(String, ParsedTrack)> = app.parsed_tracks().to_vec();
    let progress = app.progress();
    let beats_per_bar = beats_per_bar_from(&app.score);
    let shared_total_ticks = parsed_snapshot.iter().map(|(_, t)| t.total_ticks).max();
    let total_cols = shared_total_ticks.map_or(0, |t| sparkline::total_cols(t, beats_per_bar));
    app.total_cols = total_cols;

    let row_counts: Vec<usize> = visible
        .iter()
        .map(|(name, _)| {
            parsed_snapshot
                .iter()
                .find(|(n, _)| n == name)
                .map_or(0, |(_, t)| sparkline::row_count(t))
        })
        .collect();

    let track_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            row_counts
                .iter()
                .map(|&r| Constraint::Length(r as u16))
                .collect::<Vec<_>>(),
        )
        .split(area);

    let n_visible = visible.len();
    for (i, (name, _clef)) in visible.iter().enumerate() {
        if i >= track_areas.len() {
            break;
        }
        let parsed_track = parsed_snapshot.iter().find(|(n, _)| n == name);
        let Some((_, parsed_track)) = parsed_track else { continue };

        let config = SparklineConfig {
            progress: Some(progress),
            beats_per_bar,
            total_ticks_override: shared_total_ticks,
            hover_col: app.hover_col,
            show_progress_bar: i == n_visible - 1,
        };
        let (text, _) = sparkline::render_sparkline_widget(parsed_track, &config);

        if i == 0 {
            app.grid_rect = Rect {
                x: track_areas[i].x + GRID_X_OFFSET,
                y: track_areas[i].y,
                width: total_cols as u16,
                height: track_areas[i].height,
            };
        }

        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(Paragraph::new(text).block(block), track_areas[i]);
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &mut App) {
    let play_state = if app.is_playing() { "▶ Playing" } else { "⏸ Paused" };
    let backend = app
        .player
        .as_ref()
        .map(|p| p.backend_name())
        .unwrap_or_default();
    let progress = app.progress();

    let beats_per_bar = beats_per_bar_from(&app.score);
    let shared_total_ticks = app
        .parsed_tracks()
        .iter()
        .map(|(_, t)| t.total_ticks)
        .max()
        .unwrap_or(0);
    let total_bars = beats_per_bar
        .filter(|&b| b > 0)
        .map(|b| (shared_total_ticks as f64 / (b as f64 * TICKS_PER_BEAT as f64)).ceil() as u32)
        .unwrap_or(0);
    let current_bar = if total_bars > 0 {
        ((progress * total_bars as f64).floor() as u32 + 1).min(total_bars)
    } else {
        0
    };

    let tempo_bpm = tempo_from(&app.score) as f64;
    let total_sec = if tempo_bpm > 0.0 {
        shared_total_ticks as f64 / TICKS_PER_BEAT as f64 * (60.0 / tempo_bpm)
    } else {
        0.0
    };
    let current_sec = total_sec * progress;

    let mut text = format!(
        " [q]uit  [space] play/pause  [click] seek  |  {play_state}  |  {:.0}%",
        progress * 100.0
    );
    if total_bars > 0 {
        text.push_str(&format!("  |  bar {}/{}", current_bar, total_bars));
    }
    if total_sec > 0.0 {
        text.push_str(&format!(
            "  |  {:02}:{:02} / {:02}:{:02}",
            (current_sec as u32) / 60, (current_sec as u32) % 60,
            (total_sec as u32) / 60, (total_sec as u32) % 60,
        ));
    }
    if !backend.is_empty() {
        text.push_str(&format!("  |  {backend}"));
    }
    if let Some(p) = &app.player
        && let Some(err) = p.last_error()
    {
        text.push_str(&format!("  |  ! {err}"));
    }
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
