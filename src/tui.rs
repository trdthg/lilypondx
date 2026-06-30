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
    /// Cached MIDI events + tempo so resume_playback doesn't recompile.
    cached_events: Option<(Vec<crate::audio::MidiEvent>, u32)>,
    /// Pre-parsed tracks from MIDI (for .ly files).  When set, `parsed_tracks()`
    /// returns this instead of calling `parse_notes_relative`.
    midi_parsed: Option<Vec<(String, ParsedTrack)>>,
    /// Last playback progress captured before stop (so it doesn't jump to 0).
    final_progress: f64,
    pub reload_error: Option<String>,
    /// Hovered grid column (mouse).
    pub hover_col: Option<usize>,
    /// Screen rect of the grid area: x = left edge, width = total_cols.
    pub grid_rect: Rect,
    /// Y ranges of each framed track (for mouse hit-testing).
    pub track_rects: Vec<Rect>,
    /// Total grid columns (cached for mouse mapping).
    pub total_cols: usize,
    /// Horizontal scroll offset (in grid columns) for the sparkline area.
    pub scroll_offset: usize,
    /// Which pitch rows to show in the sparkline.
    pub scale_mode: sparkline::ScaleMode,
    /// The CLI `--scale` argument (stored for recomputing on reload).
    pub scale_arg: String,
    /// Screen rects of clickable buttons in the header.
    pub button_rects: Vec<(String, Rect)>,
    /// Index of the button currently hovered by the mouse.
    pub hover_button: Option<usize>,
    /// Auto-follow: scroll the sparkline to keep the playhead visible.
    pub auto_follow: bool,
    /// Color theme.
    pub theme: sparkline::Theme,
    /// Settings popup open?
    pub settings_open: bool,
    /// Hovered item in the settings popup.
    pub settings_hover: Option<usize>,
    /// Clickable setting rows.
    pub settings_rects: Vec<(String, Rect)>,
}

impl App {
    pub fn new(score: Score) -> Self {
        Self {
            score,
            player: None,
            parsed_cache: Vec::new(),
            cache_dirty: true,
            cached_events: None,
            midi_parsed: None,
            final_progress: 0.0,
            reload_error: None,
            hover_col: None,
            grid_rect: Rect::ZERO,
            track_rects: Vec::new(),
            total_cols: 0,
            scroll_offset: 0,
            scale_mode: sparkline::ScaleMode::Chromatic,
            scale_arg: String::from("auto"),
            button_rects: Vec::new(),
            hover_button: None,
            auto_follow: true,
            theme: sparkline::Theme::default(),
            settings_open: false,
            settings_hover: None,
            settings_rects: Vec::new(),
        }
    }

    pub fn new_midi(title: String, events: Vec<crate::audio::MidiEvent>, tempo_bpm: u32) -> Self {
        let parsed = crate::note::midi_events_to_parsed_track(&events);
        let score = Score {
            metadata: crate::score::ScoreMetadata {
                title,
                tempo: Some(format!("4 = {tempo_bpm}")),
                ..Default::default()
            },
            tracks: vec![crate::score::Track {
                name: "MIDI".into(),
                clef: "treble".into(),
                relative: "c'".into(),
                midi_instrument: None,
                notes: String::new(),
                syntax: "ly".into(),
            }],
        };
        let mut app = Self::new(score);
        app.cached_events = Some((events, tempo_bpm));
        app.midi_parsed = Some(vec![("MIDI".into(), parsed)]);
        app
    }

    pub fn parsed_tracks(&mut self) -> &[(String, ParsedTrack)] {
        if let Some(ref mp) = self.midi_parsed {
            return mp.as_slice();
        }
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
        let (events, tempo_bpm) = self.get_events()?;
        if events.is_empty() {
            return Ok(());
        }
        let player = AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
        player.play_background()?;
        self.player = Some(player);
        self.final_progress = 0.0;
        Ok(())
    }

    /// Resume playback from the paused position (final_progress).
    pub fn resume_playback(&mut self) -> Result<(), LilypondxError> {
        let (events, tempo_bpm) = self.get_events()?;
        if events.is_empty() {
            return Ok(());
        }
        let total = self.player.as_ref().map_or(0, |p| p.total_ticks());
        let total = if total > 0 { total } else { events.iter().map(|e| e.tick).max().unwrap_or(0) };
        let target_tick = ((self.final_progress.clamp(0.0, 1.0) * total as f64).round() as u64).min(total);
        let player = AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
        player.play_background_from(target_tick)?;
        self.player = Some(player);
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
        self.cached_events = None;
    }

    /// Seek to a fractional position; restart playback from there.
    pub fn seek(&mut self, fraction: f64) -> Result<(), LilypondxError> {
        self.stop_playback();
        let (events, tempo_bpm) = self.get_events()?;
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

    /// Get events from cache or generate fresh.
    fn get_events(&mut self) -> Result<(Vec<crate::audio::MidiEvent>, u32), LilypondxError> {
        if let Some(ref cached) = self.cached_events {
            Ok(cached.clone())
        } else {
            let (ev, bpm) = crate::audio::generate_events(&self.score, TICKS_PER_BEAT)?;
            self.cached_events = Some((ev.clone(), bpm));
            Ok((ev, bpm))
        }
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

/// Compute the number of ticks per bar from the time signature.
/// e.g. 4/4 → 4 × 480 = 1920, 6/8 → 6 × 240 = 1440, 3/4 → 3 × 480 = 1440.
/// The beat unit depends on the denominator (4 = quarter, 8 = eighth, 2 = half).
fn ticks_per_bar_from(score: &Score) -> Option<u64> {
    let time = score.metadata.time.as_deref()?;
    let (num, den) = time.split_once('/')?;
    let num: u32 = num.trim().parse().ok()?;
    let den: u32 = den.trim().parse().ok()?;
    if den == 0 {
        return None;
    }
    let ticks_per_beat_unit = TICKS_PER_BEAT as u64 * 4 / den as u64;
    Some(num as u64 * ticks_per_beat_unit)
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
pub fn run_tui(file: PathBuf, _width: usize, _rows: usize, no_watch: bool, scale: String) -> Result<(), LilypondxError> {
    let source = file.to_string_lossy().to_string();
    let is_url = source.starts_with("http://") || source.starts_with("https://");
    let score = parser::parse_markdown(&source)?;
    let mut app = App::new(score);
    app.scale_arg = scale.clone();
    app.scale_mode = sparkline::resolve_scale_mode(&app.score, &scale);
    app.start_playback()?;

    run_tui_loop(app, file, is_url, !no_watch)
}

/// Run TUI with a pre-built App (for .ly files where App is constructed from MIDI).
pub fn run_tui_with_app(mut app: App) -> Result<(), LilypondxError> {
    app.start_playback()?;
    run_tui_loop(app, PathBuf::new(), false, false)
}

fn run_tui_loop(mut app: App, file: PathBuf, is_url: bool, should_watch_local: bool) -> Result<(), LilypondxError> {
    let source = file.to_string_lossy().to_string();
    term::enable_raw_mode().map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    let _guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, term::EnterAlternateScreen, crossterm::event::EnableMouseCapture)
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
    let _ = terminal.hide_cursor();

    let debounce = Duration::from_millis(150);
    let mut last_change: Option<Instant> = None;
    let mut prev_hover_col: Option<usize> = None;
    let mut needs_redraw = true;
    let mut prev_playhead_col: Option<usize> = None;
    let mut running = true;

    // File watcher for local files; disabled for HTTP URLs or --no-watch.
    let mut watcher: Option<notify::RecommendedWatcher> = None;
    let (tx, rx) = std::sync::mpsc::channel();
    let should_watch = !is_url && should_watch_local;
    if should_watch {
        let canonical = file.canonicalize()?;
        let mut w = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
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
        w.watch(&canonical, notify::RecursiveMode::NonRecursive)
            .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
        watcher = Some(w);
    }
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
                    KeyCode::Char('f') => {
                        app.auto_follow = !app.auto_follow;
                        needs_redraw = true;
                    }
                    KeyCode::Char(' ') => {
                        if app.is_playing() {
                            app.stop_playback();
                        } else if app.final_progress >= 1.0 {
                            let _ = app.start_playback();
                        } else {
                            let _ = app.resume_playback();
                        }
                        needs_redraw = true;
                    }
                    KeyCode::Left => {
                        let step = (app.total_cols.max(20) / 10).max(4);
                        app.scroll_offset = app.scroll_offset.saturating_sub(step);
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
                    }
                    KeyCode::Right => {
                        let step = (app.total_cols.max(20) / 10).max(4);
                        let max_scroll = app.total_cols.saturating_sub(visible_cols(&app));
                        app.scroll_offset = (app.scroll_offset + step).min(max_scroll);
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
                    }
                    KeyCode::Home => {
                        app.scroll_offset = 0;
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
                    }
                    KeyCode::End => {
                        let max_scroll = app.total_cols.saturating_sub(visible_cols(&app));
                        app.scroll_offset = max_scroll;
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
                    }
                    _ => {}
                },
                Event::Mouse(MouseEvent { kind, column, row, .. }) => match kind {
                    MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                        // If settings popup is open, check popup items.
                        if app.settings_open {
                            let mut new_hover: Option<usize> = None;
                            for (i, (_, rect)) in app.settings_rects.iter().enumerate() {
                                if column >= rect.x && column < rect.x + rect.width
                                    && row >= rect.y && row < rect.y + rect.height
                                {
                                    new_hover = Some(i);
                                    break;
                                }
                            }
                            if new_hover != app.settings_hover {
                                app.settings_hover = new_hover;
                                needs_redraw = true;
                            }
                        } else {
                            // Check if hovering over a header button.
                            let mut new_hover: Option<usize> = None;
                            for (i, (_, rect)) in app.button_rects.iter().enumerate() {
                                if column >= rect.x && column < rect.x + rect.width
                                    && row >= rect.y && row < rect.y + rect.height
                                {
                                    new_hover = Some(i);
                                    break;
                                }
                            }
                            if new_hover != app.hover_button {
                                app.hover_button = new_hover;
                                needs_redraw = true;
                            }
                            if app.hover_button.is_none() {
                                app.hover_col = screen_x_to_col(column, row, &app.track_rects, app.grid_rect, app.scroll_offset);
                            } else {
                                app.hover_col = None;
                            }
                        }
                    }
                    MouseEventKind::Down(_) => {
                        // If settings popup is open, check popup items first.
                        if app.settings_open {
                            let mut clicked_row: Option<&str> = None;
                            for (name, rect) in &app.settings_rects {
                                if column >= rect.x && column < rect.x + rect.width
                                    && row >= rect.y && row < rect.y + rect.height
                                {
                                    clicked_row = Some(name.as_str());
                                    break;
                                }
                            }
                            if let Some(row_name) = clicked_row {
                                // Click-to-toggle: cycle the theme to the next one.
                                if row_name == "theme" {
                                    app.theme = app.theme.next();
                                }
                                needs_redraw = true;
                            } else {
                                // Click outside popup → close it.
                                app.settings_open = false;
                                app.settings_hover = None;
                                needs_redraw = true;
                            }
                        } else {
                            // Check if clicked a header button.
                            let mut clicked_btn: Option<&str> = None;
                            for (name, rect) in &app.button_rects {
                                if column >= rect.x && column < rect.x + rect.width
                                    && row >= rect.y && row < rect.y + rect.height
                                {
                                    clicked_btn = Some(name.as_str());
                                    break;
                                }
                            }
                            match clicked_btn {
                            Some("play") => {
                                if app.is_playing() {
                                    app.stop_playback();
                                } else if app.final_progress >= 1.0 {
                                    let _ = app.start_playback();
                                } else {
                                    let _ = app.resume_playback();
                                }
                                needs_redraw = true;
                            }
                            Some("follow") => {
                                app.auto_follow = !app.auto_follow;
                                needs_redraw = true;
                            }
                            Some("settings") => {
                                app.settings_open = !app.settings_open;
                                needs_redraw = true;
                            }
                            Some("quit") => {
                                running = false;
                            }
                            _ => {
                                // Click on sparkline → seek.
                                if let Some(col) = screen_x_to_col(column, row, &app.track_rects, app.grid_rect, app.scroll_offset) {
                                    // Use the SAME total as the playhead (audio
                                    // engine's total_ticks) so click↔playhead
                                    // stay in the same column space.
                                    let total = app.player.as_ref()
                                        .map(|p| p.total_ticks())
                                        .filter(|&t| t > 0)
                                        .unwrap_or_else(|| {
                                            app.parsed_tracks().iter()
                                                .map(|(_, t)| t.total_ticks).max().unwrap_or(0)
                                        });
                                    let frac = if app.total_cols > 0 && total > 0 {
                                        let tick = sparkline::col_to_tick(col, ticks_per_bar_from(&app.score), total);
                                        tick as f64 / total as f64
                                    } else { 0.0 };
                                    let _ = app.seek(frac);
                                    needs_redraw = true;
                                }
                            }
                        }
                        } // end else (settings not open)
                    }
                    // Map vertical scroll wheel to horizontal timeline scroll.
                    // Terminal protocols don't expose horizontal swipe, so the
                    // trackpad's two-finger scroll (whatever axis) surfaces as
                    // ScrollUp/ScrollDown here.
                    MouseEventKind::ScrollUp => {
                        let step = visible_cols(&app).max(8) / 4;
                        app.scroll_offset = app.scroll_offset.saturating_sub(step);
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
                    }
                    MouseEventKind::ScrollDown => {
                        let step = visible_cols(&app).max(8) / 4;
                        let max_scroll = app.total_cols.saturating_sub(visible_cols(&app));
                        app.scroll_offset = (app.scroll_offset + step).min(max_scroll);
                        app.hover_col = None;
                        app.auto_follow = false;
                        needs_redraw = true;
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

        // Watcher events (local files only).
        while let Ok(ts) = rx.try_recv() {
            last_change = Some(ts);
        }
        if let Some(ts) = last_change
            && ts.elapsed() >= debounce
        {
            last_change = None;
            match parser::parse_markdown(&source) {
                Ok(new_score) => {
                    app.reload_error = None;
                    app.scroll_offset = 0;
                    app.set_score(new_score);
                    app.scale_mode = sparkline::resolve_scale_mode(&app.score, &app.scale_arg);
                    let _ = app.start_playback();
                    needs_redraw = true;
                }
                Err(e) => app.reload_error = Some(format!("{e}")),
            }
        }

        // Playback progress advances while playing — redraw only when the
        // *playhead column* actually changes. This avoids burning CPU drawing
        // identical frames between column transitions (most commonly on long
        // notes where the head sits on the same column for several frames).
        if app.is_playing() {
            let p = app.progress();
            let total = app.total_cols.max(1);
            let cur = (p * (total.saturating_sub(1)) as f64).round() as usize;
            if Some(cur) != prev_playhead_col {
                prev_playhead_col = Some(cur);
                needs_redraw = true;
            }

            // Auto-follow: when playhead reaches 80% of visible width,
            // jump so it sits at 20% from the left.
            if app.auto_follow && total > visible_cols(&app) {
                let vis = visible_cols(&app);
                let playhead_in_view = cur.saturating_sub(app.scroll_offset);
                if playhead_in_view >= vis * 7 / 8 {
                    let target = cur.saturating_sub(vis / 8);
                    let max_scroll = total.saturating_sub(vis);
                    let target = target.min(max_scroll);
                    if target != app.scroll_offset {
                        app.scroll_offset = target;
                        app.hover_col = None;
                        needs_redraw = true;
                    }
                }
            }
        } else {
            prev_playhead_col = None;
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

/// Number of grid columns visible in the terminal (clamped to total_cols).
fn visible_cols(app: &App) -> usize {
    app.total_cols.min(app.grid_rect.width as usize)
}

/// Helpers to clamp scroll_offset so it never exceeds the maximum.
fn clamp_scroll(app: &mut App) {
    let max = app.total_cols.saturating_sub(visible_cols(app));
    app.scroll_offset = app.scroll_offset.min(max);
}

fn screen_x_to_col(x: u16, y: u16, track_rects: &[Rect], grid_rect: Rect, scroll_offset: usize) -> Option<usize> {
    // Only respond to clicks inside a framed track area.
    if !track_rects.iter().any(|r| y >= r.y && y < r.y + r.height) {
        return None;
    }
    // Use grid_rect (label gutter end + border offset) for x mapping.
    if x < grid_rect.x || x >= grid_rect.x + grid_rect.width {
        return None;
    }
    Some((x - grid_rect.x) as usize + scroll_offset)
}

/// Resolve the `--scale` CLI argument into a `ScaleMode`.
/// - "auto": detect from frontmatter `key`, or infer from notes.
/// - "chromatic": show all rows.
/// - key string like "c major": parse and use that scale.
fn draw_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
        ])
        .split(f.area());

    draw_header(f, chunks[0], app);
    draw_sparkline_area(f, chunks[1], app);

    if app.settings_open {
        draw_settings_popup(f, app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &mut App) {
    use ratatui::text::{Line, Span, Text};

    let meta = &app.score.metadata;
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));

    let mut parts: Vec<String> = Vec::new();
    if !meta.title.is_empty() { parts.push(meta.title.clone()); }
    if let Some(c) = &meta.composer { parts.push(c.clone()); }
    if let Some(t) = &meta.tempo { parts.push(format!("♪ {}", t)); }
    if let Some(k) = &meta.key { parts.push(format!("key: {}", k.replace("\\", ""))); }
    if let Some(t) = &meta.time { parts.push(t.to_string()); }

    let left_text = if let Some(err) = &app.reload_error {
        format!("{}  —  [reload error: {err}]", parts.join("  |  "))
    } else if parts.is_empty() {
        "(no metadata)".into()
    } else {
        parts.join("  |  ")
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Left side: metadata.
    f.render_widget(
        Paragraph::new(left_text).style(Style::default().fg(Color::Cyan)),
        inner,
    );

    // Right side: clickable buttons + progress info.
    let play_label = if app.is_playing() { "⏸ Pause" } else { "▶ Play" };
    let follow_label = if app.auto_follow { "📎 Following" } else { "📎 Fixed" };
    let progress = app.progress();

    let shared_total_ticks = app
        .parsed_tracks()
        .iter()
        .map(|(_, t)| t.total_ticks)
        .max()
        .unwrap_or(0);
    let tempo_bpm = tempo_from(&app.score) as f64;
    let total_sec = if tempo_bpm > 0.0 {
        shared_total_ticks as f64 / TICKS_PER_BEAT as f64 * (60.0 / tempo_bpm)
    } else { 0.0 };
    let current_sec = total_sec * progress;

    let progress_text = if total_sec > 0.0 {
        format!(
            "  {:02}:{:02} / {:02}:{:02}  {:.0}%",
            (current_sec as u32) / 60, (current_sec as u32) % 60,
            (total_sec as u32) / 60, (total_sec as u32) % 60,
            progress * 100.0,
        )
    } else { String::new() };

    let quit_label = "[q]uit";

    // Build styled spans for the right side.  Buttons get backgrounds; hover
    // changes the background to a brighter color.
    let normal_bg = Color::DarkGray;
    let hover_bg = Color::LightBlue;
    let active_bg = Color::Blue;
    let hover_idx = app.hover_button.unwrap_or(usize::MAX);

    let play_len = play_label.chars().count() as u16;
    let follow_len = follow_label.chars().count() as u16;
    let quit_len = quit_label.chars().count() as u16;
    let prog_len = progress_text.chars().count() as u16;
    let spacing: u16 = 2;

    let right_total = prog_len + spacing + play_len + spacing + follow_len + spacing + 1 + spacing + quit_len;
    let start_x = inner.x + inner.width.saturating_sub(right_total);

    let mut spans: Vec<Span> = Vec::new();
    let mut button_rects: Vec<(String, Rect)> = Vec::new();
    let mut x = start_x;

    // Progress text (not a button) — sits left of the play/pause button.
    spans.push(Span::raw(progress_text));
    x += prog_len;

    // Spacing.
    spans.push(Span::raw("  "));
    x += spacing;

    // Play/Pause button (index 0).
    let play_bg = if hover_idx == 0 { hover_bg } else { normal_bg };
    spans.push(Span::styled(
        play_label,
        Style::default().bg(play_bg).fg(Color::White),
    ));
    button_rects.push(("play".into(), Rect { x, y: inner.y, width: play_len, height: 1 }));
    x += play_len;

    // Spacing.
    spans.push(Span::raw("  "));
    x += spacing;

    // Follow button (index 1).
    let follow_bg = if hover_idx == 1 {
        hover_bg
    } else if app.auto_follow {
        active_bg
    } else {
        normal_bg
    };
    spans.push(Span::styled(
        follow_label,
        Style::default().bg(follow_bg).fg(Color::White),
    ));
    button_rects.push(("follow".into(), Rect { x, y: inner.y, width: follow_len, height: 1 }));
    x += follow_len;

    // Spacing.
    spans.push(Span::raw("  "));
    x += spacing;

    // Settings button (index 2).
    let settings_label = "⚙ setting";
    let settings_len: u16 = settings_label.chars().count() as u16;
    let settings_bg = if hover_idx == 2 { hover_bg }
        else if app.settings_open { active_bg }
        else { normal_bg };
    spans.push(Span::styled(
        settings_label,
        Style::default().bg(settings_bg).fg(Color::White),
    ));
    button_rects.push(("settings".into(), Rect { x, y: inner.y, width: settings_len, height: 1 }));
    x += settings_len;

    // Spacing.
    spans.push(Span::raw("  "));
    x += spacing;

    // Quit button (index 3).
    let quit_bg = if hover_idx == 3 { hover_bg } else { normal_bg };
    spans.push(Span::styled(
        quit_label,
        Style::default().bg(quit_bg).fg(Color::White),
    ));
    button_rects.push(("quit".into(), Rect { x, y: inner.y, width: quit_len, height: 1 }));

    app.button_rects = button_rects;

    let right_area = Rect { x: start_x, y: inner.y, width: right_total, height: 1 };
    f.render_widget(
        Paragraph::new(Text::from(Line::from(spans))),
        right_area,
    );
}

/// Draw the settings popup (centered overlay).
/// Layout: title "setting", then a list of rows where the left side is the
/// option name and the right side is the current value. Click a row to toggle.
fn draw_settings_popup(f: &mut Frame, app: &mut App) {
    use ratatui::widgets::{Clear, Block, Borders, Paragraph};
    use ratatui::text::{Line, Span, Text};

    // Each setting row: (label, value_string, is_bool, bool_value).
    // For now we only have Theme (a cyclic toggle, not a bool).
    let rows: Vec<(&str, String, bool, bool)> = vec![
        ("Theme", app.theme.name().to_string(), false, false),
    ];

    let popup_height = 3 + rows.len() as u16; // border(2) + title-line + rows
    let popup_width = 28u16;
    let area = f.area();
    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" setting ")
        .style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    let mut settings_rects: Vec<(String, Rect)> = Vec::new();

    for (i, (label, value, is_bool, bool_val)) in rows.iter().enumerate() {
        let is_hovered = app.settings_hover == Some(i);
        let style = if is_hovered {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Left: option name. Right: value ([x]/[ ] for bools, "Name >" for cyclic).
        let value_str = if *is_bool {
            if *bool_val { "[x]" } else { "[ ]" }.to_string()
        } else {
            format!("{} >", value)
        };
        let pad = (inner.width as usize)
            .saturating_sub(label.len() + value_str.chars().count())
            .max(1);
        let line_str = format!("{}{}{}", label, " ".repeat(pad), value_str);
        let line_len = line_str.chars().count() as u16;
        lines.push(Line::from(vec![Span::styled(line_str, style)]));

        settings_rects.push((
            "theme".to_string(),
            Rect {
                x: inner.x,
                y: inner.y + i as u16,
                width: line_len.max(inner.width),
                height: 1,
            },
        ));
    }

    app.settings_rects = settings_rects;

    f.render_widget(
        Paragraph::new(Text::from(lines)),
        inner,
    );
}

/// Context for drawing a track's top border.
struct TrackBorderCtx<'a> {
    track_name: &'a str,
    show_bar_numbers: bool,
    ticks_per_bar: Option<u64>,
    total_ticks: Option<u64>,
    visible_width: usize,
    scroll_offset: usize,
}

/// Draw the top border of a track, with the track name at the start and
/// optional bar numbers (first track only).
fn draw_track_top_border(f: &mut Frame, area: Rect, ctx: TrackBorderCtx) {
    let line_width = area.width.saturating_sub(2) as usize;
    let mut buf: Vec<char> = vec!['─'; line_width];

    // Insert track name at the beginning: `─ RH ─`.
    let label = format!(" {} ", ctx.track_name);
    for (i, c) in label.chars().enumerate() {
        if i < line_width {
            buf[i] = c;
        }
    }

    // Bar numbers (first track only).
    if ctx.show_bar_numbers
        && let (Some(bpb), Some(total)) = (ctx.ticks_per_bar, ctx.total_ticks)
        && bpb > 0
        && total > 0
    {
        let bar_ticks = sparkline::bar_tick_list(Some(bpb), total);
        for (i, &bt) in bar_ticks.iter().enumerate() {
            let bar_number = i + 2;
            if bar_number % 4 != 0 {
                continue;
            }
            let col = sparkline::tick_to_col(bt, &bar_ticks);
            if col < ctx.scroll_offset || col >= ctx.scroll_offset + ctx.visible_width {
                continue;
            }
            let vis_col = col - ctx.scroll_offset;
            let pos = GRID_X_OFFSET as usize + vis_col;
            let num_str = bar_number.to_string();
            let num_len = num_str.len();
            for (offset, digit) in num_str.chars().rev().enumerate() {
                let p = pos.saturating_sub(offset);
                if p < line_width {
                    buf[p] = digit;
                }
            }
            let before = pos.saturating_sub(num_len);
            if before < line_width {
                buf[before] = ' ';
            }
            let after = pos + 1;
            if after < line_width {
                buf[after] = ' ';
            }
        }
    }

    let line: String = buf.iter().collect();
    let border = format!("┌{line}┐");
    f.render_widget(
        Paragraph::new(border).style(Style::default().fg(Color::DarkGray)),
        area,
    );
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
    let ticks_per_bar = ticks_per_bar_from(&app.score);
    // Use the audio engine's total_ticks (max MIDI event tick) for playhead
    // mapping — it stays in sync with what the audio is actually playing.
    let shared_total_ticks = app
        .player
        .as_ref()
        .map(|p| p.total_ticks())
        .filter(|&t| t > 0)
        .or_else(|| parsed_snapshot.iter().map(|(_, t)| t.total_ticks).max());
    let total_cols = shared_total_ticks.map_or(0, |t| sparkline::total_cols(t, ticks_per_bar));
    app.total_cols = total_cols;
    // Reserve 2 for left/right border + GRID_X_OFFSET for the label gutter.
    let visible_width = total_cols.min(area.width.saturating_sub(GRID_X_OFFSET + 2) as usize);
    app.grid_rect.width = visible_width as u16;
    clamp_scroll(app);
    let scroll_offset = app.scroll_offset;
    let scale_mode = app.scale_mode;

    // Compute the playhead column ONCE from progress + shared timeline,
    // so all voices show the head at the same horizontal position.
    let playhead_col = shared_total_ticks.and_then(|total| {
        if total == 0 || total_cols == 0 {
            return None;
        }
        let p = progress.clamp(0.0, 1.0);
        let current_tick = (p * total as f64).round() as u64;
        // Use the shared tick_to_col so playhead stays in sync with build_grid.
        let bar_ticks = sparkline::bar_tick_list(ticks_per_bar, total);
        let col = sparkline::tick_to_col(current_tick, &bar_ticks);
        Some(col.min(total_cols.saturating_sub(1)))
    });

    let row_counts: Vec<usize> = visible
        .iter()
        .map(|(name, _)| {
            parsed_snapshot
                .iter()
                .find(|(n, _)| n == name)
                .map_or(0, |(_, t)| sparkline::row_count_with_scale(t, scale_mode))
        })
        .collect();

    // Each track is bordered (2 rows for top/bottom border + r content rows).
    // No separator lines between tracks — the borders themselves visually
    // separate adjacent tracks.
    let mut constraints: Vec<Constraint> = Vec::new();
    for &r in row_counts.iter() {
        constraints.push(Constraint::Length((r + 2) as u16)); // +2 for borders
    }
    let track_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut track_rects: Vec<Rect> = Vec::new();
    for (i, (name, _clef)) in visible.iter().enumerate() {
        if i >= track_areas.len() {
            break;
        }
        let track_outer = track_areas[i];

        let parsed_track = parsed_snapshot.iter().find(|(n, _)| n == name);
        let Some((_, parsed_track)) = parsed_track else { continue };

        // All tracks: draw left/right/bottom borders, custom top border.
        let border_block = Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .style(Style::default().fg(Color::DarkGray));
        let track_inner_raw = border_block.inner(track_outer);
        f.render_widget(border_block, track_outer);

        // Custom top border with track name (+ bar numbers for first track).
        let top_rect = Rect {
            x: track_outer.x,
            y: track_outer.y,
            width: track_outer.width,
            height: 1,
        };
        draw_track_top_border(
            f,
            top_rect,
            TrackBorderCtx {
                track_name: name,
                show_bar_numbers: i == 0,
                ticks_per_bar,
                total_ticks: shared_total_ticks,
                visible_width,
                scroll_offset,
            },
        );

        // Skip the top-border row in the inner area.
        let track_inner = Rect {
            y: track_inner_raw.y + 1,
            height: track_inner_raw.height.saturating_sub(1),
            ..track_inner_raw
        };

        let config = SparklineConfig {
            progress: Some(progress),
            ticks_per_bar,
            total_ticks_override: shared_total_ticks,
            hover_col: app.hover_col,
            playhead_col,
            scale_mode,
            theme: app.theme,
        };
        let (text, _) = sparkline::render_sparkline_widget(parsed_track, &config, scroll_offset, visible_width);

        // Record the grid rect for mouse mapping (use inner area).
        let grid_rect = Rect {
            x: track_inner.x + GRID_X_OFFSET,
            y: track_inner.y,
            width: visible_width as u16,
            height: track_inner.height,
        };
        track_rects.push(track_inner);

        if i == 0 {
            app.grid_rect = grid_rect;
        }

        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::Gray)),
            track_inner,
        );
    }
    app.track_rects = track_rects;
}
