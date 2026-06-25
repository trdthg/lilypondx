use crate::note::ParsedTrack;

/// Configuration for sparkline rendering.
pub struct SparklineConfig {
    /// Number of vertical rows. In diatonic mode, automatically expanded to fit.
    pub rows: usize,
    /// Width in character columns. 0 = auto from data.
    pub width: usize,
    /// Time step per column in ticks. 0 = auto (fit to width).
    pub ticks_per_col: u32,
    /// Min / max pitch (MIDI). `None` = auto from data.
    pub min_pitch: Option<u8>,
    pub max_pitch: Option<u8>,
    /// Playback progress 0.0–1.0.
    pub progress: Option<f64>,
    /// If true, color natural notes C–B with rainbow background.
    pub color: bool,
    /// If true, only natural notes on vertical axis.
    pub diatonic: bool,
}

impl Default for SparklineConfig {
    fn default() -> Self {
        Self {
            rows: 7,
            width: 60,
            ticks_per_col: 0,
            min_pitch: None,
            max_pitch: None,
            progress: None,
            color: false,
            diatonic: false,
        }
    }
}
pub fn render_sparkline(track: &ParsedTrack, config: &SparklineConfig) -> String {
    if track.notes.is_empty() {
        return String::new();
    }

    // Determine pitch range
    let pitches: Vec<u8> = track.notes.iter().filter_map(|n| n.pitch).collect();
    if pitches.is_empty() {
        return "(no pitched notes)\n".to_string();
    }
    let min_p = config.min_pitch.unwrap_or_else(|| *pitches.iter().min().unwrap());
    let max_p = config.max_pitch.unwrap_or_else(|| *pitches.iter().max().unwrap());

    // Build row mapping: either diatonic (natural notes only) or chromatic (evenly spaced)
    let (rows, label_pitches): (usize, Vec<u8>) = if config.diatonic {
        let nats: Vec<u8> = natural_pitches_in_range(min_p, max_p);
        (nats.len().max(1), nats)
    } else {
        // Default: show every semitone between min and max
        let lo = min_p.saturating_sub(1);
        let hi = max_p.saturating_add(1);
        let pitches: Vec<u8> = (lo..=hi).rev().collect();
        (pitches.len().max(1), pitches)
    };
    let rows = rows.max(1);
    let label_pitches = label_pitches; // highest→lowest

    // Map pitch to nearest label row
    let pitch_to_row = |pitch: u8| -> usize {
        label_pitches.iter()
            .enumerate()
            .min_by_key(|(_, p)| (pitch as i32 - **p as i32).unsigned_abs())
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    // Time resolution: half-beat per column.
    // Quarter note = 2 cols (•─), half note = 4 cols (•───), eighth = 1 col (•)
    let ticks_per_beat: u64 = 480;
    let total_ticks = track.total_ticks.max(1);
    let ticks_per_col = if config.ticks_per_col > 0 {
        config.ticks_per_col as u64
    } else {
        ticks_per_beat / 2
    };
    let total_cols = ((total_ticks + ticks_per_col - 1) / ticks_per_col) as usize;

    // Build the grid — line‑chart style
    let mut grid = vec![vec![' '; total_cols]; rows];
    let mut current_tick: u64 = 0;

    // Collect indices of pitched notes with their start ticks
    struct PitchedNote { row: usize, start_col: usize, dur_cols: usize }
    let mut pitched: Vec<PitchedNote> = Vec::new();
    for note in &track.notes {
        let start_col = (current_tick / ticks_per_col as u64) as usize;
        let dur_cols = ((note.duration as u64 + ticks_per_col as u64 - 1) / ticks_per_col as u64) as usize;
        let dur_cols = dur_cols.max(1);
        if let Some(pitch) = note.pitch {
            pitched.push(PitchedNote {
                row: pitch_to_row(pitch),
                start_col,
                dur_cols,
            });
        }
        current_tick += note.duration as u64;
    }

    // Draw horizontal stems — no note heads, just lines
    for (_i, pn) in pitched.iter().enumerate() {
        let sc = pn.start_col.min(total_cols.saturating_sub(1));
        let ec = sc.saturating_add(pn.dur_cols).min(total_cols);
        if ec <= sc { continue; }

        // Fill duration with ━
        for col in sc..ec {
            grid[pn.row][col] = '━';
        }
    }
    // Build output with pitch labels
    let mut out = String::new();
    for (i, row) in grid.iter().enumerate() {
        let pitch = label_pitches.get(i).copied().unwrap_or(60);
        let color = if config.color { bg_color(pitch) } else { "" };
        let reset = if config.color && !color.is_empty() { "\x1b[0m" } else { "" };
        out.push_str(&pitch_label(pitch));
        out.push_str(" │");
        if !color.is_empty() { out.push_str(color); }
        for &ch in row.iter() { out.push(ch); }
        if !reset.is_empty() { out.push_str(reset); }
        out.push('\n');
    }
    // Bottom axis
    out.push_str("    └");
    out.push_str(&"─".repeat(total_cols));

    // Progress bar
    if let Some(p) = config.progress {
        let p = p.clamp(0.0, 1.0);
        let marker_col = (p * (total_cols.saturating_sub(1)) as f64) as usize;
        out.push('\n');
        out.push_str("   ");
        for col in 0..total_cols {
            if col == marker_col { out.push('▶'); }
            else if col < marker_col { out.push('━'); }
            else { out.push('─'); }
        }
        let pct = (p * 100.0) as u32;
        if total_ticks > 0 {
            let total_sec = track.total_ticks as f64 / 480.0 * (60.0 / 120.0);
            let current_sec = total_sec * p;
            out.push_str(&format!("  {:02}:{:02} / {:02}:{:02}  {}%",
                (current_sec as u32) / 60, (current_sec as u32) % 60,
                (total_sec as u32) / 60, (total_sec as u32) % 60, pct));
        }
    }

    out
}

/// Collect all natural-note MIDI pitches in [min, max] (white keys only).
fn natural_pitches_in_range(min: u8, max: u8) -> Vec<u8> {
    let naturals: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
    let min_oct = (min as i32 / 12).max(0);
    let max_oct = (max as i32 / 12).max(0);
    let mut pitches = Vec::new();
    for oct in min_oct..=max_oct {
        for &sem in &naturals {
            let p = (oct * 12 + sem) as u8;
            if p >= min && p <= max { pitches.push(p); }
        }
    }
    pitches.sort_unstable();
    pitches.reverse(); // highest first (row 0 = top)
    pitches
}
/// Fixed-width 3-char label: [accidental][note][octave].  ` C4`, `#F4`.
fn pitch_label(midi: u8) -> String {
    let notes = ['C', 'C', 'D', 'D', 'E', 'F', 'F', 'G', 'G', 'A', 'A', 'B'];
    let accs  = [' ', '#', ' ', '#', ' ', ' ', '#', ' ', '#', ' ', '#', ' '];
    let idx = (midi % 12) as usize;
    let octave = (midi as i32 / 12) - 1;
    format!("{}{}{}", accs[idx], notes[idx], octave)
}

fn bg_color(midi: u8) -> &'static str {
    // 8-color backgrounds: red→magenta rainbow, black foreground
    match midi % 12 {
        0  => "\x1b[41m\x1b[30m",  // C — red bg + black fg
        2  => "\x1b[43m\x1b[30m",  // D — yellow bg + black fg
        4  => "\x1b[42m\x1b[30m",  // E — green bg + black fg
        5  => "\x1b[46m\x1b[30m",  // F — cyan bg + black fg
        7  => "\x1b[44m\x1b[30m",  // G — blue bg + black fg
        9  => "\x1b[45m\x1b[30m",  // A — magenta bg + black fg
        11 => "\x1b[47m\x1b[30m",  // B — white bg + black fg
        _  => "",
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::parse_notes_relative;

    #[test]
    fn test_sparkline_basic() {
        let track = parse_notes_relative("c4 d e f g a b c'", "c'", 480);
        let config = SparklineConfig {
            rows: 5,
            width: 40,
            ..Default::default()
        };
        let result = render_sparkline(&track, &config);
        assert!(result.contains('━'), "should contain horizontal lines");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_sparkline_empty() {
        let track = parse_notes_relative("", "c'", 480);
        let config = SparklineConfig::default();
        let result = render_sparkline(&track, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_sparkline_all_rests() {
        let track = parse_notes_relative("r4 r4 r4 r4", "c'", 480);
        let config = SparklineConfig::default();
        let result = render_sparkline(&track, &config);
        assert!(result.contains("(no pitched notes)"));
    }

    #[test]
    fn test_sparkline_with_rests() {
        let track = parse_notes_relative("c4 r4 d4", "c'", 480);
        let config = SparklineConfig {
            rows: 5,
            width: 20,
            ..Default::default()
        };
        let result = render_sparkline(&track, &config);
        assert!(!result.is_empty());
        assert!(result.contains('━'));
    }

    #[test]
    fn test_pitch_label() {
        assert_eq!(pitch_label(60), " C4");
        assert_eq!(pitch_label(61), "#C4");
        assert_eq!(pitch_label(69), " A4");
        assert_eq!(pitch_label(48), " C3");
        assert_eq!(pitch_label(72), " C5");
    }
}
