use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

use crate::note::ParsedTrack;
use crate::TICKS_PER_BEAT;

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

    // Chromatic rows: every semitone between (min-1) and (max+1), highest first.
    let lo = min_p.saturating_sub(1);
    let hi = max_p.saturating_add(1);
    let label_pitches: Vec<u8> = (lo..=hi).rev().collect();
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
    let ticks_per_col = ticks_per_beat / 2; // half-beat per column

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
    let mut current_tick: u64 = 0;
    for note in &track.notes {
        let start_col = tick_to_col(current_tick);
        // For the end column, use strict `<` so a note ending exactly at a
        // bar boundary doesn't paint over the bar line column.
        let end_tick = current_tick + note.duration as u64;
        let end_base = (end_tick / ticks_per_col) as usize;
        let end_bars = bar_ticks.iter().filter(|&&bt| bt < end_tick).count();
        let end_col = (end_base + end_bars).max(start_col + 1).min(total_cols);
        let start_col = start_col.min(total_cols.saturating_sub(1));
        for &pitch in &note.pitches {
            let row = pitch_to_row(pitch);
            for (_col, cell) in grid[row].iter_mut().enumerate().take(end_col).skip(start_col) {
                // Notes overlay bar lines — a note's glyph takes precedence.
                *cell = '━';
            }
        }
        current_tick += note.duration as u64;
    }

    // Hover highlight (overlays on top, but keeps note glyphs).
    if let Some(hc) = config.hover_col
        && hc < total_cols
    {
        for row in grid.iter_mut() {
            if row[hc] == ' ' || row[hc] == '┊' {
                row[hc] = '▌';
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
        let mut t: u64 = 0;
        let mut snap: Option<usize> = None;
        let mut last_pitched: Option<usize> = None;
        for note in &track.notes {
            let start = t;
            let end = t + note.duration as u64;
            if !note.pitches.is_empty() {
                last_pitched = Some(tick_to_col(start));
            }
            if snap.is_none() && current_tick < end {
                if !note.pitches.is_empty() {
                    let note_start_col = tick_to_col(start);
                    let col = tick_to_col(current_tick);
                    // The playhead should never be before the note's start
                    // column (can happen due to float rounding in progress→tick),
                    // and should never land on a bar-line column.
                    snap = Some(if col < note_start_col || bar_cols.contains(&col) {
                        note_start_col
                    } else {
                        col
                    });
                }
                break;
            }
            t = end;
        }
        // Past the last note: hold at the last column (end of piece).
        if snap.is_none() && current_tick >= total_ticks {
            snap = Some(total_cols.saturating_sub(1));
        }
        // Rest: snap to next upcoming pitched note.
        if snap.is_none() {
            let mut t: u64 = 0;
            for note in &track.notes {
                if current_tick <= t && !note.pitches.is_empty() {
                    snap = Some(tick_to_col(t));
                    break;
                }
                t += note.duration as u64;
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
        let bg = bg_color(pitch);
        let label = pitch_label(pitch);
        let mut spans = vec![Span::styled(
            format!("{label} │"),
            Style::default().bg(bg).fg(Color::Black),
        )];
        for (abs_col, ch) in row.chars().enumerate().skip(start_col).take(vis_cols) {
            // Playhead column gets a bright yellow background, keeping the
            // underlying glyph (`━`, ` `, `┊`, `▌`) visible.
            let style = if Some(abs_col) == g.playhead_col {
                Style::default().bg(Color::Yellow).fg(Color::Black)
            } else if ch == '▌' {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if ch == '━' {
                // Note glyph: white on the soft background for contrast.
                Style::default().bg(bg).fg(Color::White)
            } else {
                Style::default().bg(bg).fg(Color::Gray)
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

/// Background color for a pitch (rainbow by pitch class). Each accidental
/// shares the natural's color below it so backgrounds are continuous.
/// Uses light, low-saturation RGB pastels that blend softly with the
/// terminal background.
fn bg_color(midi: u8) -> Color {
    match midi % 12 {
        0 | 1 => Color::Rgb(120, 140, 180),  // C  — soft slate blue
        2 | 3 => Color::Rgb(180, 130, 144),  // D  — soft rose
        4 => Color::Rgb(140, 172, 124),      // E  — soft sage
        5 | 6 => Color::Rgb(176, 148, 104),  // F  — soft amber
        7 | 8 => Color::Rgb(124, 160, 168),  // G  — soft teal
        9 | 10 => Color::Rgb(160, 128, 176), // A  — soft lavender
        11 => Color::Rgb(140, 140, 148),      // B  — soft neutral gray
        _ => Color::Reset,
    }
}

/// How many rows (lines) a track's sparkline will occupy (pitch rows only),
/// excluding the optional progress bar. For TUI height allocation per voice.
pub fn row_count(track: &ParsedTrack) -> usize {
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
    hi as usize - lo as usize + 1
}

/// Count grid columns for a timeline (shared util for mouse mapping).
/// Count grid columns for a timeline, including inserted bar-line columns.
pub fn total_cols(total_ticks: u64, beats_per_bar: Option<u32>) -> usize {
    let tpc = TICKS_PER_BEAT as u64 / 2;
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