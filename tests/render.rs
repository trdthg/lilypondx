//! Render-level tests: sparkline plain-text output is matched byte-for-byte
//! against `lilypond-test` blocks. Color behavior is asserted structurally on
//! the ascending-scale fixture via the ratatui widget API.

mod common;

use std::path::Path;

use lilypondx::note::parse_notes_relative;
use lilypondx::sparkline::{render_sparkline, render_sparkline_widget, SparklineConfig};
use lilypondx::TICKS_PER_BEAT;

fn assert_pairs_render(path: &str) {
    let pairs = common::load_pairs(Path::new(path));
    assert!(!pairs.is_empty(), "{path}: no pairs loaded");
    for (i, pair) in pairs.iter().enumerate() {
        let parsed = parse_notes_relative(&pair.input.notes, &pair.input.relative, TICKS_PER_BEAT);
        let actual = render_sparkline(&parsed, &SparklineConfig::default());
        // The parser trims leading whitespace from test blocks, so compare
        // line-by-line with trim_start to normalize.
        let actual_trimmed: String = actual.lines()
            .map(|l| l.trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        let expected_trimmed: String = pair.expected.lines()
            .map(|l| l.trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            actual_trimmed, expected_trimmed,
            "{path} pair #{i} ({}) render mismatch",
            pair.input.name
        );
    }
}

#[test]
fn render_ascending() {
    assert_pairs_render("tests/data/render_ascending.md");
}

#[test]
fn render_chromatic() {
    assert_pairs_render("tests/data/render_chromatic.md");
}

#[test]
fn render_color_widget_has_styled_spans() {
    // The widget must produce styled spans (non-default background) for the
    // pitch rows, proving color is applied via ratatui styles (not ANSI).
    let pairs = common::load_pairs(Path::new("tests/data/render_ascending.md"));
    let pair = &pairs[0];
    let parsed = parse_notes_relative(&pair.input.notes, &pair.input.relative, TICKS_PER_BEAT);
    let (text, _) = render_sparkline_widget(&parsed, &SparklineConfig::default(), 0, 0);

    let has_styled_bg = text.lines.iter().any(|line| {
        line.spans
            .iter()
            .any(|s| s.style.bg.is_some())
    });
    assert!(has_styled_bg, "widget must apply background styling to pitch rows");
}

#[test]
fn render_widget_plain_matches_plain_render() {
    // The widget's text content (ignoring styles) must equal the plain render.
    let pairs = common::load_pairs(Path::new("tests/data/render_ascending.md"));
    let pair = &pairs[0];
    let parsed = parse_notes_relative(&pair.input.notes, &pair.input.relative, TICKS_PER_BEAT);

    let plain = render_sparkline(&parsed, &SparklineConfig::default());
    let (text, _) = render_sparkline_widget(&parsed, &SparklineConfig::default(), 0, 0);
    let widget_plain: String = text
        .lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(widget_plain.trim_end(), plain, "widget plain text must equal render_sparkline");
}

#[test]
fn render_empty_input_is_empty() {
    let track = parse_notes_relative("", "c'", TICKS_PER_BEAT);
    assert_eq!(render_sparkline(&track, &SparklineConfig::default()), "");
}

#[test]
fn render_all_rests() {
    let track = parse_notes_relative("r4 r4 r4 r4", "c'", TICKS_PER_BEAT);
    let out = render_sparkline(&track, &SparklineConfig::default());
    assert!(out.contains("(no pitched notes)"));
}
