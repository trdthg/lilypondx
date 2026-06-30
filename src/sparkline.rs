use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

use crate::note::{self, ParsedTrack};
use crate::score::Score;
use crate::TICKS_PER_BEAT;

/// A color theme for sparkline backgrounds and foregrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Black & white: white keys = white bg, black keys = black bg.
    #[default]
    Mono,
    /// Soft pastel rainbow (original).
    Macaron,
    /// Piano-style: white keys = light gray, black keys = dark gray.
    Piano,
}

impl Theme {
    pub fn all() -> &'static [&'static str] {
        &["Mono", "Macaron", "Piano"]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "mono" => Some(Theme::Mono),
            "macaron" => Some(Theme::Macaron),
            "piano" => Some(Theme::Piano),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Theme::Mono => "Mono",
            Theme::Macaron => "Macaron",
            Theme::Piano => "Piano",
        }
    }

    /// Cycle to the next theme (for click-to-toggle in the settings popup).
    pub fn next(self) -> Self {
        match self {
            Theme::Mono => Theme::Macaron,
            Theme::Macaron => Theme::Piano,
            Theme::Piano => Theme::Mono,
        }
    }
}

/// Background color for a pitch, based on the active theme.
pub fn bg_color(midi: u8, theme: Theme) -> Color {
    match theme {
        Theme::Mono => bg_color_mono(midi),
        Theme::Macaron => bg_color_macaron(midi),
        Theme::Piano => bg_color_piano(midi),
    }
}

/// Black & white: white keys = light gray, black keys = dark gray.
fn bg_color_mono(midi: u8) -> Color {
    match midi % 12 {
        0 | 2 | 4 | 5 | 7 | 9 | 11 => Color::Rgb(230, 230, 230),  // white keys: light gray
        _ => Color::Rgb(40, 40, 40),                               // black keys: dark gray
    }
}

/// Piano-style: white keys (C D E F G A B) = light gray, black keys = dark gray.
fn bg_color_piano(midi: u8) -> Color {
    match midi % 12 {
        0 | 2 | 4 | 5 | 7 | 9 | 11 => Color::Rgb(200, 200, 205),  // white keys
        _ => Color::Rgb(80, 80, 85),                                // black keys
    }
}

/// Soft pastel rainbow (macaron palette) — natural notes only.
/// Accidentals (C#/Db, D#/Eb, ...) have NO background color.
fn bg_color_macaron(midi: u8) -> Color {
    match midi % 12 {
        0 => Color::Rgb(120, 140, 180),   // C
        2 => Color::Rgb(180, 130, 144),   // D
        4 => Color::Rgb(140, 172, 124),   // E
        5 => Color::Rgb(176, 148, 104),   // F
        7 => Color::Rgb(124, 160, 168),   // G
        9 => Color::Rgb(160, 128, 176),   // A
        11 => Color::Rgb(140, 140, 148),  // B
        _ => Color::Reset,                // accidentals: no color
    }
}

/// Which pitch rows to show in the sparkline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleMode {
    /// Show only notes of the given scale (major/minor).  The tonic + mode
    /// are encoded as a bitmask of 7 pitch classes.
    Diatonic(u16),
    /// Show all semitone rows (current default behavior).
    #[default]
    Chromatic,
}

/// Configuration for sparkline rendering.
#[derive(Default)]
pub struct SparklineConfig {
    /// Override the total timeline length (in ticks). When rendering multiple
    /// voices, pass the max total_ticks so every voice shares one time axis.
    pub total_ticks_override: Option<u64>,
    /// Playback progress 0.0–1.0 (drives the bottom progress bar).
    pub progress: Option<f64>,
    /// Beats per bar (from time signature, e.g. 4 for 4/4). Draws bar lines.
    pub beats_per_bar: Option<u32>,
    /// Column index to highlight (mouse hover). Vertical `▌` through rows.
    pub hover_col: Option<usize>,
    /// Draw the progress bar below the grid (only one voice should set this).
    pub show_progress_bar: bool,
    /// Which pitch rows to show.
    pub scale_mode: ScaleMode,
    /// Color theme.
    pub theme: Theme,
}

/// Major scale interval pattern (semitone offsets from tonic): W W H W W W H
/// → 0, 2, 4, 5, 7, 9, 11
const MAJOR_PITCHES: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
/// Natural minor: 0, 2, 3, 5, 7, 8, 10
const MINOR_PITCHES: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// Build a 12-bit pitch-class mask for a scale starting at `tonic`.
pub fn scale_mask(tonic: u8, pitches: &[u8; 7]) -> u16 {
    pitches.iter().map(|&p| 1u16 << ((tonic + p) % 12)).fold(0, |a, b| a | b)
}

/// Detect a scale that contains every pitch class used in `track`.
/// Returns `(tonic, mask, is_major)` for the best-fitting major or minor scale.
pub fn detect_scale(track: &ParsedTrack) -> Option<(u8, u16, bool)> {
    let used: u16 = track
        .notes
        .iter()
        .flat_map(|n| n.pitches.iter().copied())
        .map(|p| 1u16 << (p % 12))
        .fold(0u16, |a, b| a | b);

    // Try every tonic for major and minor; keep scales whose mask is a
    // superset of `used`.  Among matches, prefer the one with the fewest
    // extra pitch classes (tightest fit).
    let mut best: Option<(u8, u16, bool, u32)> = None;
    for tonic in 0..12u8 {
        for (pitches, is_major) in [(&MAJOR_PITCHES, true), (&MINOR_PITCHES, false)] {
            let mask = scale_mask(tonic, pitches);
            if used & !mask == 0 {
                // All used pitches are in this scale.
                let extra = (mask & !used).count_ones();
                match best {
                    None => best = Some((tonic, mask, is_major, extra)),
                    Some((_, _, _, e)) if extra < e => best = Some((tonic, mask, is_major, extra)),
                    _ => {}
                }
            }
        }
    }
    best.map(|(t, m, maj, _)| (t, m, maj))
}

/// Parse a LilyPond-style key string like `c \major` or `a \minor` into a
/// `ScaleMode::Diatonic` with the corresponding pitch-class mask.
pub fn parse_key(key_str: &str) -> Option<ScaleMode> {
    // Lowercase, strip backslashes, split into [tonic, mode].
    let s = key_str.to_lowercase().replace('\\', " ").replace("major", " major").replace("minor", " minor");
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let tonic = match parts[0] {
        "c" => 0, "cis" => 1, "d" => 2, "dis" => 3, "e" => 4, "f" => 5,
        "fis" => 6, "g" => 7, "gis" => 8, "a" => 9, "ais" => 10, "b" => 11,
        _ => return None,
    };
    let is_major = parts.iter().rposition(|&p| p == "major" || p == "minor")
        .map(|i| parts[i] == "major")
        .unwrap_or(true);
    let mask = scale_mask(tonic, if is_major { &MAJOR_PITCHES } else { &MINOR_PITCHES });
    Some(ScaleMode::Diatonic(mask))
}

/// Built grid + metadata, shared by both renderers.
struct GridData {
    /// One string per row, each char is a cell: ` `, `━`, `┊`.
    rows: Vec<String>,
    /// Pitch (MIDI) for each row, highest first.
    label_pitches: Vec<u8>,
    total_cols: usize,
    total_ticks: u64,
    /// Column of the playback head (for widget styling), if progress is set.
    playhead_col: Option<usize>,
}

/// Resolve a `--scale` CLI argument (or "auto") into a `ScaleMode`.
/// Shared by the CLI `dump` command and the TUI.
pub fn resolve_scale_mode(score: &Score, arg: &str) -> ScaleMode {
    match arg.trim() {
        "chromatic" => ScaleMode::Chromatic,
        "auto" => {
            // 1. Try frontmatter `key`, then auto-detect from all tracks' pitches.
            if let Some(k) = &score.metadata.key
                && let Some(mode) = parse_key(k)
            {
                return mode;
            }
            // 2. Auto-detect from all tracks' pitches.
            let parsed: Vec<ParsedTrack> = score
                .tracks
                .iter()
                .map(|t| note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT))
                .collect();
            let combined = ParsedTrack {
                notes: parsed.iter().flat_map(|p| p.notes.iter().cloned()).collect(),
                total_ticks: parsed.iter().map(|p| p.total_ticks).max().unwrap_or(0),
            };
            detect_scale(&combined)
                .map(|(_, mask, _)| ScaleMode::Diatonic(mask))
                .unwrap_or(ScaleMode::Chromatic)
        }
        key_str => parse_key(key_str).unwrap_or(ScaleMode::Chromatic),
    }
}

/// Width of the left gutter before grid content: pitch label (3) + " │" (2) = 5.
pub const GRID_X_OFFSET: u16 = 5;

fn build_grid(track: &ParsedTrack, config: &SparklineConfig) -> Option<GridData> {
    if track.notes.is_empty() {
        return None;
    }
    let pitches: Vec<u8> = track.notes.iter().flat_map(|n| n.pitches.iter().copied()).collect();
    if pitches.is_empty() {
        return None; // "(no pitched notes)" handled by callers
    }
    let min_p = *pitches.iter().min().unwrap();
    let max_p = *pitches.iter().max().unwrap();

    // Decide which pitch classes to show.
    let scale_mask = match config.scale_mode {
        ScaleMode::Chromatic => None,
        ScaleMode::Diatonic(mask) => Some(mask),
    };

    // Collect the set of pitch classes that actually appear in the track.
    // These are always shown (even if outside the scale) so out-of-key notes
    // render on their own row instead of being snapped to a nearby in-key row.
    let used_pcs: u16 = pitches.iter().fold(0u16, |acc, &p| acc | (1 << (p % 12)));

    // Build the list of row pitches: every semitone between (min-1) and
    // (max+1), but skip rows that are neither in the scale nor actually
    // used in the track.
    let lo = min_p.saturating_sub(1);
    let hi = max_p.saturating_add(1);
    let mut label_pitches: Vec<u8> = Vec::new();
    for p in (lo..=hi).rev() {
        let in_scale = scale_mask.is_none_or(|m| (m & (1 << (p % 12))) != 0);
        let is_used = (used_pcs & (1 << (p % 12))) != 0;
        if in_scale || is_used {
            label_pitches.push(p);
        }
    }
    let rows = label_pitches.len();

    let pitch_to_row = |pitch: u8| -> usize {
        label_pitches
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (pitch as i32 - **p as i32).unsigned_abs())
            .map(|(i, _)| i)
            .unwrap_or(0)
    };

    let total_ticks = config.total_ticks_override.unwrap_or(track.total_ticks).max(1);
    let ticks_per_beat = TICKS_PER_BEAT as u64;
    let ticks_per_col = ticks_per_beat / 4; // quarter-beat per column (16th note = 1 col)

    // Bar boundaries (in ticks). Each gets a dedicated extra column in the
    // grid, so bar lines don't steal space from notes — every bar still has
    // its full `bpb * 2` columns of note area.
    let bar_ticks: Vec<u64> = config
        .beats_per_bar
        .filter(|&b| b > 0)
        .map(|b| {
            let bar_len = b as u64 * TICKS_PER_BEAT as u64;
            let mut v = Vec::new();
            let mut t = bar_len;
            while t < total_ticks {
                v.push(t);
                t += bar_len;
            }
            v
        })
        .unwrap_or_default();

    // Map a tick to a grid column: note columns come from tick/tpc, plus one
    // extra column inserted at each bar boundary. The bar line column sits at
    // the boundary; notes starting at the boundary begin one column later.
    let tick_to_col = |tick: u64| -> usize {
        let base = (tick / ticks_per_col) as usize;
        // Notes at or after a bar boundary get +1 so the bar line has its own
        // column to the left of the note.
        let bars_at_or_before = bar_ticks.iter().filter(|&&bt| bt <= tick).count();
        base + bars_at_or_before
    };

    // total_cols includes the inserted bar columns.
    let total_cols = tick_to_col(total_ticks).max(1);
    // Bar line columns: at each bar boundary bt, the bar line occupies the
    // column that would correspond to bt if notes didn't get the +1 shift,
    // i.e. base + (bars strictly before bt).
    let bar_cols: std::collections::HashSet<usize> = bar_ticks
        .iter()
        .map(|&bt| {
            let base = (bt / ticks_per_col) as usize;
            let bars_before = bar_ticks.iter().filter(|&&b| b < bt).count();
            base + bars_before
        })
        .collect();

    // Build the grid as a Vec<String> (one per row) for easy overlay.
    let mut grid = vec![vec![' '; total_cols]; rows];
    // Draw bar lines first (as full vertical `┊` columns) so notes overlay on top.
    for &col in &bar_cols {
        for row in grid.iter_mut() {
            row[col] = '┊';
        }
    }
    for note in &track.notes {
        let note_tick = note.start_tick;
        let start_col = tick_to_col(note_tick);
        // For the end column, use strict `<` so a note ending exactly at a
        // bar boundary doesn't paint over the bar line column.
        let end_tick = note_tick + note.duration as u64;
        let end_base = (end_tick / ticks_per_col) as usize;
        let end_bars = bar_ticks.iter().filter(|&&bt| bt < end_tick).count();
        let end_col = (end_base + end_bars).max(start_col + 1).min(total_cols);
        let start_col = start_col.min(total_cols.saturating_sub(1));
        for &pitch in &note.pitches {
            let row = pitch_to_row(pitch);
            for (_col, cell) in grid[row].iter_mut().enumerate().take(end_col).skip(start_col) {
                *cell = '━';
            }
        }
    }

    // Hover highlight (overlays on top, but keeps note glyphs).
    if let Some(hc) = config.hover_col
        && hc < total_cols
    {
        for row in grid.iter_mut() {
        if row[hc] == ' ' || row[hc] == '┊' {
            row[hc] = '|';
        }
        }
    }

    // Playback head column: computed from progress, snapping to note spans.
    //  - While a pitched note sounds: track the exact tick within [start, end),
    //    so the head walks along long notes.
    //  - During a rest: snap to the next pitched note's onset column.
    //  - Past the last note: hold at the last pitched note's onset.
    // The head is NOT drawn into the grid (that would erase `━` glyphs);
    // instead we return its column and the widget renderer styles it yellow.
    // The head never sits on a bar-line column — if it would, it snaps to the
    // previous note column instead.
    let playhead_col = config.progress.and_then(|p| {
        let p = p.clamp(0.0, 1.0);
        let current_tick = (p * total_ticks as f64).round() as u64;
        let mut snap: Option<usize> = None;
        let mut last_pitched: Option<usize> = None;
        for note in &track.notes {
            let start = note.start_tick;
            let end = start + note.duration as u64;
            if !note.pitches.is_empty() {
                last_pitched = Some(tick_to_col(start));
            }
            if snap.is_none() && current_tick < end {
                if !note.pitches.is_empty() {
                    let note_start_col = tick_to_col(start);
                    let col = tick_to_col(current_tick);
                    snap = Some(if col < note_start_col || bar_cols.contains(&col) {
                        note_start_col
                    } else {
                        col
                    });
                }
                break;
            }
        }
        // Past the last note: hold at the last column (end of piece).
        if snap.is_none() && current_tick >= total_ticks {
            snap = Some(total_cols.saturating_sub(1));
        }
        // Rest: snap to next upcoming pitched note.
        if snap.is_none() {
            for note in &track.notes {
                if current_tick <= note.start_tick && !note.pitches.is_empty() {
                    snap = Some(tick_to_col(note.start_tick));
                    break;
                }
            }
        }
        snap.or(last_pitched).filter(|&c| c < total_cols)
    });

    let rows: Vec<String> = grid.into_iter().map(|r| r.into_iter().collect()).collect();
    Some(GridData { rows, label_pitches, total_cols, total_ticks, playhead_col })
}

/// Render a sparkline as plain text (no styling). For `dump` and tests.
pub fn render_sparkline(track: &ParsedTrack, config: &SparklineConfig) -> String {
    let Some(g) = build_grid(track, config) else {
        return if track.notes.is_empty() { String::new() } else { "(no pitched notes)\n".into() };
    };

    let mut out = String::new();
    for (row, &pitch) in g.rows.iter().zip(&g.label_pitches) {
        out.push_str(&pitch_label(pitch));
        out.push_str(" │");
        out.push_str(row);
        out.push('\n');
    }

    append_progress_bar(&mut out, &g, config);
    // Trim trailing whitespace so output matches test fixtures (which are trimmed).
    out.trim_end().to_string()
}

/// Render a sparkline as a ratatui `Text` with per-cell styling (rainbow
/// backgrounds, hover highlight). Returns `(text, total_cols)` so the TUI can
/// map screen X → grid column for mouse interaction.
///
/// `scroll_offset` and `visible_width` control horizontal clipping:
/// set both to 0 to render the full grid (no clipping).
pub fn render_sparkline_widget<'a>(
    track: &ParsedTrack,
    config: &SparklineConfig,
    scroll_offset: usize,
    visible_width: usize,
) -> (Text<'a>, usize) {
    let Some(g) = build_grid(track, config) else {
        return (
            Text::from(Line::from(if track.notes.is_empty() {
                String::new()
            } else {
                "(no pitched notes)".into()
            })),
            0,
        );
    };

    let mut lines: Vec<Line> = Vec::with_capacity(g.rows.len() + 2);

    // Determine clipping window.
    let start_col = if visible_width > 0 {
        scroll_offset.min(g.total_cols.saturating_sub(visible_width))
    } else {
        0
    };
    let end_col = if visible_width > 0 {
        (start_col + visible_width).min(g.total_cols)
    } else {
        g.total_cols
    };
    let vis_cols = end_col - start_col;

    for (row, &pitch) in g.rows.iter().zip(&g.label_pitches) {
        let bg = bg_color(pitch, config.theme);
        let label = pitch_label(pitch);
        // Left gutter (label + │) is NOT colored — uses the terminal default.
        let mut spans = vec![Span::raw(format!("{label} │"))];
        for (abs_col, ch) in row.chars().enumerate().skip(start_col).take(vis_cols) {
            // Playhead column gets a bright yellow background, keeping the
            // underlying glyph (`━`, ` `, `┊`, `▌`) visible.
            let style = if Some(abs_col) == g.playhead_col {
                Style::default().bg(Color::Yellow).fg(Color::Black)
            } else if ch == '|' {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if ch == '━' {
                // Note bar color: blue on Mono, black on Macaron/Piano.
                let note_fg = match config.theme {
                    Theme::Mono => Color::Blue,
                    _ => Color::Black,
                };
                Style::default().bg(bg).fg(note_fg)
            } else if ch == '▌' {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                // Empty cells and bar lines use the themed bg, no foreground.
                Style::default().bg(bg)
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        lines.push(Line::from(spans));
    }

    if config.show_progress_bar
        && let Some(p) = config.progress
    {
        let p = p.clamp(0.0, 1.0);
        let marker_col = (p * (g.total_cols.saturating_sub(1)) as f64) as usize;
        let mut s = String::from("   ");
        for col in start_col..end_col {
            if col == marker_col {
                s.push('▶');
            } else if col < marker_col {
                s.push('━');
            } else {
                s.push('─');
            }
        }
        let pct = (p * 100.0) as u32;
        if g.total_ticks > 0 {
            let total_sec = g.total_ticks as f64 / TICKS_PER_BEAT as f64 * (60.0 / 120.0);
            let current_sec = total_sec * p;
            s.push_str(&format!(
                "  {:02}:{:02} / {:02}:{:02}  {}%",
                (current_sec as u32) / 60, (current_sec as u32) % 60,
                (total_sec as u32) / 60, (total_sec as u32) % 60, pct,
            ));
        }
        lines.push(Line::from(Span::raw(s)));
    }

    (Text::from(lines), g.total_cols)
}

// Old single-arg bg_color removed — use `bg_color(midi, theme)` instead.

/// How many rows (lines) a track's sparkline will occupy (pitch rows only),
/// excluding the optional progress bar. For TUI height allocation per voice.
pub fn row_count(track: &ParsedTrack) -> usize {
    row_count_with_scale(track, ScaleMode::Chromatic)
}

/// Same as `row_count` but honoring a scale mode (diatonic rows only).
pub fn row_count_with_scale(track: &ParsedTrack, mode: ScaleMode) -> usize {
    if track.notes.is_empty() {
        return 0;
    }
    let pitches: Vec<u8> = track.notes.iter().flat_map(|n| n.pitches.iter().copied()).collect();
    if pitches.is_empty() {
        return 1;
    }
    let min_p = *pitches.iter().min().unwrap();
    let max_p = *pitches.iter().max().unwrap();
    let lo = min_p.saturating_sub(1);
    let hi = max_p.saturating_add(1);
    let scale_mask = match mode {
        ScaleMode::Chromatic => None,
        ScaleMode::Diatonic(m) => Some(m),
    };
    let used_pcs: u16 = pitches.iter().fold(0u16, |acc, &p| acc | (1 << (p % 12)));
    (lo..=hi)
        .filter(|&p| {
            let in_scale = scale_mask.is_none_or(|m| (m & (1 << (p % 12))) != 0);
            let is_used = (used_pcs & (1 << (p % 12))) != 0;
            in_scale || is_used
        })
        .count()
}

/// Count grid columns for a timeline (shared util for mouse mapping).
/// Count grid columns for a timeline, including inserted bar-line columns.
pub fn total_cols(total_ticks: u64, beats_per_bar: Option<u32>) -> usize {
    let tpc = TICKS_PER_BEAT as u64 / 4;
    let base = (total_ticks.max(1) / tpc) as usize;
    let bar_count = beats_per_bar
        .filter(|&b| b > 0)
        .map(|b| {
            let bar_len = b as u64 * TICKS_PER_BEAT as u64;
            let mut n = 0;
            let mut t = bar_len;
            while t < total_ticks {
                n += 1;
                t += bar_len;
            }
            n
        })
        .unwrap_or(0);
    base + bar_count
}

fn append_progress_bar(out: &mut String, g: &GridData, config: &SparklineConfig) {
    if !config.show_progress_bar {
        return;
    }
    let Some(p) = config.progress else { return };
    let p = p.clamp(0.0, 1.0);
    let marker_col = (p * (g.total_cols.saturating_sub(1)) as f64) as usize;
    out.push('\n');
    out.push_str("   ");
    for col in 0..g.total_cols {
        if col == marker_col {
            out.push('▶');
        } else if col < marker_col {
            out.push('━');
        } else {
            out.push('─');
        }
    }
    let pct = (p * 100.0) as u32;
    if g.total_ticks > 0 {
        let total_sec = g.total_ticks as f64 / TICKS_PER_BEAT as f64 * (60.0 / 120.0);
        let current_sec = total_sec * p;
        out.push_str(&format!(
            "  {:02}:{:02} / {:02}:{:02}  {}%",
            (current_sec as u32) / 60, (current_sec as u32) % 60,
            (total_sec as u32) / 60, (total_sec as u32) % 60, pct,
        ));
    }
}

/// Fixed-width 3-char label: [accidental][note][octave]. ` C4`, `#F4`.
pub fn pitch_label(midi: u8) -> String {
    let notes = ['C', 'C', 'D', 'D', 'E', 'F', 'F', 'G', 'G', 'A', 'A', 'B'];
    let accs = [' ', '#', ' ', '#', ' ', ' ', '#', ' ', '#', ' ', '#', ' '];
    let idx = (midi % 12) as usize;
    let octave = (midi as i32 / 12) - 1;
    format!("{}{}{}", accs[idx], notes[idx], octave)
}