use lilypondx::note::parse_notes_relative;
use lilypondx::parser::parse_markdown;
use lilypondx::sparkline::{render_sparkline_widget, SparklineConfig};
use lilypondx::TICKS_PER_BEAT;
use std::path::Path;

fn playhead_col(notes: &str, relative: &str, tick: u64, beats_per_bar: Option<u32>) -> Option<usize> {
    let parsed = parse_notes_relative(notes, relative, TICKS_PER_BEAT);
    let config = SparklineConfig {
        progress: Some(tick as f64 / parsed.total_ticks as f64),
        beats_per_bar,
        total_ticks_override: Some(parsed.total_ticks),
        ..Default::default()
    };
    let (text, _) = render_sparkline_widget(&parsed, &config, 0, 0);
    text.lines.iter().find_map(|line| {
        line.spans.iter().enumerate().find_map(|(idx, span)| {
            (span.style.bg == Some(ratatui::style::Color::Yellow)).then_some(idx.saturating_sub(1))
        })
    })
}

fn tick_to_col(tick: u64, total_ticks: u64, beats_per_bar: Option<u32>) -> usize {
    let tpc = TICKS_PER_BEAT as u64 / 4;
    let bar_ticks: Vec<u64> = beats_per_bar
        .filter(|&b| b > 0)
        .map(|b| {
            let bar_len = b as u64 * TICKS_PER_BEAT as u64;
            let mut ticks = Vec::new();
            let mut t = bar_len;
            while t < total_ticks {
                ticks.push(t);
                t += bar_len;
            }
            ticks
        })
        .unwrap_or_default();
    (tick / tpc) as usize + bar_ticks.iter().filter(|&&bt| bt <= tick).count()
}

#[test]
fn playhead_walks_through_sixteenth_notes() {
    let notes = "g16 gis16 a8 c4";
    let parsed = parse_notes_relative(notes, "c'", TICKS_PER_BEAT);

    for tick in [0, 60, 120, 180, 240, 360, 480, 720, 959] {
        assert_eq!(
            playhead_col(notes, "c'", tick, None),
            Some(tick_to_col(tick, parsed.total_ticks, None)),
            "playhead mismatch at tick {tick}"
        );
    }
}

#[test]
fn mata_ashita_playhead_after_sixteenth_notes() {
    let score = parse_markdown(Path::new("tests/data/Mata Ashita!.md").to_string_lossy().as_ref()).unwrap();
    let rh = score.tracks.iter().find(|t| t.name == "RH").unwrap();
    let parsed = parse_notes_relative(&rh.notes, &rh.relative, TICKS_PER_BEAT);
    let beats_per_bar = Some(6);

    for tick in [44640, 44700, 44760, 44820, 44880, 45120, 45360, 46080] {
        let config = SparklineConfig {
            progress: Some(tick as f64 / parsed.total_ticks as f64),
            beats_per_bar,
            total_ticks_override: Some(parsed.total_ticks),
            ..Default::default()
        };
        let (text, _) = render_sparkline_widget(&parsed, &config, 0, 0);
        let actual = text.lines.iter().find_map(|line| {
            line.spans.iter().enumerate().find_map(|(idx, span)| {
                (span.style.bg == Some(ratatui::style::Color::Yellow)).then_some(idx.saturating_sub(1))
            })
        });
        assert_eq!(
            actual,
            Some(tick_to_col(tick, parsed.total_ticks, beats_per_bar)),
            "Mata Ashita RH playhead mismatch at tick {tick}"
        );
    }
}
