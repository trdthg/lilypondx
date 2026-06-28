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
}

impl App {
    pub fn new(score: Score) -> Self {
        Self {
            score,
            player: None,
            parsed_cache: Vec::new(),
            cache_dirty: true,
            cached_events: None,
            final_progress: 0.0,
            reload_error: None,
            hover_col: None,
            grid_rect: Rect::ZERO,
            track_rects: Vec::new(),
            total_cols: 0,
            scroll_offset: 0,
            scale_mode: sparkline::ScaleMode::Chromatic,
            scale_arg: String::from("auto"),
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
        let (events, tempo_bpm) = crate::audio::generate_events(&self.score, TICKS_PER_BEAT)?;
        self.cached_events = Some((events.clone(), tempo_bpm));
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
        let (events, tempo_bpm) = if let Some(ref cached) = self.cached_events {
            cached.clone()
        } else {
            let (ev, bpm) = crate::audio::generate_events(&self.score, TICKS_PER_BEAT)?;
            self.cached_events = Some((ev.clone(), bpm));
            (ev, bpm)
        };
        if events.is_empty() {
            return Ok(());
        }
        let total = events.iter().map(|e| e.tick).max().unwrap_or(0);
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
        let (events, tempo_bpm) = if let Some(ref cached) = self.cached_events {
            cached.clone()
        } else {
            let (ev, bpm) = crate::audio::generate_events(&self.score, TICKS_PER_BEAT)?;
            self.cached_events = Some((ev.clone(), bpm));
            (ev, bpm)
        };
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
pub fn run_tui(file: PathBuf, _width: usize, _rows: usize, no_watch: bool, scale: String) -> Result<(), LilypondxError> {
    let source = file.to_string_lossy().to_string();
    let is_url = source.starts_with("http://") || source.starts_with("https://");
    let score = parser::parse_markdown(&source)?;
    let mut app = App::new(score);
    app.scale_arg = scale.clone();
    app.scale_mode = resolve_scale_mode(&app.score, &scale);
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

    let debounce = Duration::from_millis(150);
    let mut last_change: Option<Instant> = None;
    let mut prev_hover_col: Option<usize> = None;
    let mut needs_redraw = true;
    let mut running = true;

    // File watcher for local files; disabled for HTTP URLs or --no-watch.
    let mut watcher: Option<notify::RecommendedWatcher> = None;
    let (tx, rx) = std::sync::mpsc::channel();
    let should_watch = !is_url && !no_watch;
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
                        needs_redraw = true;
                    }
                    KeyCode::Right => {
                        let step = (app.total_cols.max(20) / 10).max(4);
                        let max_scroll = app.total_cols.saturating_sub(visible_cols(&app));
                        app.scroll_offset = (app.scroll_offset + step).min(max_scroll);
                        app.hover_col = None;
                        needs_redraw = true;
                    }
                    KeyCode::Home => {
                        app.scroll_offset = 0;
                        app.hover_col = None;
                        needs_redraw = true;
                    }
                    KeyCode::End => {
                        let max_scroll = app.total_cols.saturating_sub(visible_cols(&app));
                        app.scroll_offset = max_scroll;
                        app.hover_col = None;
                        needs_redraw = true;
                    }
                    _ => {}
                },
                Event::Mouse(MouseEvent { kind, column, row, .. }) => match kind {
                    MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                        app.hover_col = screen_x_to_col(column, row, &app.track_rects, app.grid_rect, app.scroll_offset);
                    }
                    MouseEventKind::Down(_) => {
                        if let Some(col) = screen_x_to_col(column, row, &app.track_rects, app.grid_rect, app.scroll_offset) {
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
                    app.scale_mode = resolve_scale_mode(&app.score, &app.scale_arg);
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
fn resolve_scale_mode(score: &Score, arg: &str) -> sparkline::ScaleMode {
    match arg.trim() {
        "chromatic" => sparkline::ScaleMode::Chromatic,
        "auto" => {
            // 1. Try frontmatter `key`.
            if let Some(k) = &score.metadata.key {
                if let Some(mode) = sparkline::parse_key(k) {
                    return mode;
                }
            }
            // 2. Auto-detect from all tracks' pitches.
            let parsed: Vec<crate::note::ParsedTrack> = score
                .tracks
                .iter()
                .map(|t| note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT))
                .collect();
            // Combine all pitches and detect.
            let combined = crate::note::ParsedTrack {
                notes: parsed.iter().flat_map(|p| p.notes.iter().cloned()).collect(),
                total_ticks: parsed.iter().map(|p| p.total_ticks).max().unwrap_or(0),
            };
            sparkline::detect_scale(&combined)
                .map(|(_, mask, _)| sparkline::ScaleMode::Diatonic(mask))
                .unwrap_or(sparkline::ScaleMode::Chromatic)
        }
        key_str => {
            sparkline::parse_key(key_str).unwrap_or(sparkline::ScaleMode::Chromatic)
        }
    }
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
    let meta = &app.score.metadata;
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

    let mut parts: Vec<String> = Vec::new();
    if !meta.title.is_empty() {
        parts.push(meta.title.clone());
    }
    if let Some(c) = &meta.composer {
        parts.push(c.clone());
    }
    if let Some(t) = &meta.tempo {
        parts.push(format!("♪ {}", t));
    }
    if let Some(k) = &meta.key {
        parts.push(format!("key: {}", k.replace("\\", "")));
    }
    if let Some(t) = &meta.time {
        parts.push(format!("{}/bar", t));
    }
    if !visible.is_empty() {
        parts.push(visible.join(" · "));
    }

    let text = if let Some(err) = &app.reload_error {
        format!("{}  —  [reload error: {err}]", parts.join("  |  "))
    } else if parts.is_empty() {
        "(no metadata)".into()
    } else {
        parts.join("  |  ")
    };
    f.render_widget(Paragraph::new(text).block(block), area);
}

/// Draw the top border of a track, with the track name at the start and
/// optional bar numbers (first track only).
fn draw_track_top_border(
    f: &mut Frame,
    area: Rect,
    track_name: &str,
    show_bar_numbers: bool,
    beats_per_bar: Option<u32>,
    total_ticks: Option<u64>,
    visible_width: usize,
    scroll_offset: usize,
) {
    let line_width = area.width.saturating_sub(2) as usize;
    let mut buf: Vec<char> = vec!['─'; line_width];

    // Insert track name at the beginning: `─ RH ─`.
    let label = format!(" {} ", track_name);
    for (i, c) in label.chars().enumerate() {
        if i < line_width {
            buf[i] = c;
        }
    }

    // Bar numbers (first track only).
    if show_bar_numbers {
        if let (Some(bpb), Some(total)) = (beats_per_bar, total_ticks) {
            if bpb > 0 && total > 0 {
                let tpc = TICKS_PER_BEAT as u64 / 2;
                let bar_len = bpb as u64 * TICKS_PER_BEAT as u64;
                let bar_ticks: Vec<u64> = {
                    let mut v = Vec::new();
                    let mut t = bar_len;
                    while t < total {
                        v.push(t);
                        t += bar_len;
                    }
                    v
                };
                for (i, &bt) in bar_ticks.iter().enumerate() {
                    let bar_number = (i + 2) as usize;
                    if bar_number % 4 != 0 {
                        continue;
                    }
                    let base = (bt / tpc) as usize;
                    let bars_before = bar_ticks.iter().filter(|&&b| b < bt).count();
                    let col = base + bars_before;
                    if col < scroll_offset || col >= scroll_offset + visible_width {
                        continue;
                    }
                    let vis_col = col - scroll_offset;
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
    let beats_per_bar = beats_per_bar_from(&app.score);
    let shared_total_ticks = parsed_snapshot.iter().map(|(_, t)| t.total_ticks).max();
    let total_cols = shared_total_ticks.map_or(0, |t| sparkline::total_cols(t, beats_per_bar));
    app.total_cols = total_cols;
    // Reserve 2 for left/right border + GRID_X_OFFSET for the label gutter.
    let visible_width = total_cols.min(area.width.saturating_sub(GRID_X_OFFSET + 2) as usize);
    app.grid_rect.width = visible_width as u16;
    clamp_scroll(app);
    let scroll_offset = app.scroll_offset;
    let scale_mode = app.scale_mode;

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

    let n_visible = visible.len();
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
            name,
            i == 0,
            beats_per_bar,
            shared_total_ticks,
            visible_width,
            scroll_offset,
        );

        // Skip the top-border row in the inner area.
        let track_inner = Rect {
            y: track_inner_raw.y + 1,
            height: track_inner_raw.height.saturating_sub(1),
            ..track_inner_raw
        };

        let config = SparklineConfig {
            progress: Some(progress),
            beats_per_bar,
            total_ticks_override: shared_total_ticks,
            hover_col: app.hover_col,
            show_progress_bar: i == n_visible - 1,
            scale_mode,
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
        " [q]uit  [space] play/pause  [←→] scroll  [click] seek  |  {play_state}  |  {:.0}%",
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
