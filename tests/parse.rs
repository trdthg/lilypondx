//! Parse-level tests: frontmatter, syntax detection, and exact note-serialization
//! assertions against `lilypond-test` blocks.

mod common;

use lilypondx::note::{parse_notes_relative, serialize_notes};
use lilypondx::parser;
use lilypondx::TICKS_PER_BEAT;

/// Helper: load pairs from a data file and assert each input's serialized notes
/// equal its `lilypond-test` block.
fn assert_pairs_serialize(path: &str) {
    let pairs = common::load_pairs(std::path::Path::new(path));
    assert!(!pairs.is_empty(), "{path}: no pairs loaded");
    for (i, pair) in pairs.iter().enumerate() {
        let parsed = parse_notes_relative(&pair.input.notes, &pair.input.relative, TICKS_PER_BEAT);
        let actual = serialize_notes(&parsed);
        assert_eq!(
            actual, pair.expected,
            "{path} pair #{i} ({}) mismatch",
            pair.input.name
        );
    }
}

#[test]
fn parse_relative_basics() {
    assert_pairs_serialize("tests/data/parse_relative.md");
}

#[test]
fn twinkle_little_star() {
    let pairs = common::load_pairs(std::path::Path::new("tests/data/twinkle.md"));
    assert_eq!(pairs.len(), 2, "should have RH and LH pairs");

    for (i, pair) in pairs.iter().enumerate() {
        let parsed =
            parse_notes_relative(&pair.input.notes, &pair.input.relative, TICKS_PER_BEAT);
        let actual = serialize_notes(&parsed);
        assert_eq!(
            actual, pair.expected,
            "twinkle.md pair #{i} ({}) mismatch",
            pair.input.name
        );
    }
}

#[test]
fn parse_octave_rest_tie() {
    assert_pairs_serialize("tests/data/parse_octave_rest_tie.md");
}

#[test]
fn parse_rests_and_bars() {
    assert_pairs_serialize("tests/data/parse_rests_bars.md");
}

#[test]
fn frontmatter_metadata() {
    let score = parser::parse_markdown(std::path::Path::new("tests/data/frontmatter.md"))
        .expect("parse");
    assert_eq!(score.metadata.title, "frontmatter test");
    assert_eq!(score.metadata.composer.as_deref(), Some("青木望"));
    assert_eq!(score.metadata.subtitle.as_deref(), Some("銀河鉄道 999"));
    assert_eq!(score.metadata.tempo.as_deref(), Some("4 = 70"));
    assert_eq!(score.metadata.key.as_deref(), Some("c \\major"));
    assert_eq!(score.metadata.time.as_deref(), Some("4/4"));
    assert_eq!(score.tracks.len(), 1);
    assert_eq!(score.tracks[0].syntax, "lilypond");
    assert_eq!(score.tracks[0].clef, "treble");
    assert_eq!(score.tracks[0].relative, "c");
}

#[test]
fn lilypondx_syntax_detected() {
    let score =
        parser::parse_markdown(std::path::Path::new("tests/data/pipeline_multitrack.md"))
            .expect("parse");
    assert_eq!(score.tracks.len(), 2);
    for t in &score.tracks {
        assert_eq!(t.syntax, "lilypondx", "track {} should be lilypondx", t.name);
    }
}

#[test]
fn native_lilypond_syntax_detected() {
    let score =
        parser::parse_markdown(std::path::Path::new("tests/data/frontmatter.md"))
            .expect("parse");
    for t in &score.tracks {
        assert_eq!(t.syntax, "lilypond");
    }
}

#[test]
fn bare_fences_yield_no_tracks() {
    let score = parser::parse_markdown(std::path::Path::new("tests/data/no_tracks.md"))
        .expect("parse");
    assert!(score.tracks.is_empty(), "bare fences should produce 0 tracks");
}
