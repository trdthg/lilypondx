//! End-to-end pipeline tests: parse → notes → sparkline → MIDI events.

use lilypondx::audio::generate_events_direct;
use lilypondx::note::parse_notes_relative;
use lilypondx::parser;
use lilypondx::sparkline::{render_sparkline, SparklineConfig};
use lilypondx::TICKS_PER_BEAT;

#[test]
fn multitrack_pipeline() {
    let score = parser::parse_markdown("tests/data/pipeline_multitrack.md")
        .expect("parse");
    assert_eq!(score.tracks.len(), 2, "should have RH and LH");

    for track in &score.tracks {
        let parsed = parse_notes_relative(&track.notes, &track.relative, TICKS_PER_BEAT);
        assert!(!parsed.notes.is_empty(), "track {} should parse notes", track.name);
        assert!(parsed.total_ticks > 0, "track {} should have duration", track.name);

        let spark = render_sparkline(&parsed, &SparklineConfig::default());
        assert!(!spark.is_empty(), "track {} should render", track.name);
    }

    let events = generate_events_direct(&score, TICKS_PER_BEAT);
    assert!(!events.is_empty(), "should generate MIDI events");
    assert!(
        events.iter().any(|e| e.command == 0x90),
        "should have at least one NoteOn"
    );

    // Events should be sorted by tick.
    for w in events.windows(2) {
        assert!(w[0].tick <= w[1].tick, "events should be sorted by tick");
    }
}

#[test]
fn lilypond_test_blocks_excluded_from_playback() {
    // A file with a `lilypond-test` block: it must be parsed (for assertions)
    // but must NOT contribute MIDI events to playback.
    let score = parser::parse_markdown("tests/data/render_ascending.md")
        .expect("parse");
    let events = generate_events_direct(&score, TICKS_PER_BEAT);
    // The single `lilypond` block has 4 notes → at least 4 NoteOns.
    let note_ons = events.iter().filter(|e| e.command == 0x90).count();
    assert_eq!(note_ons, 4, "test blocks must not produce MIDI events");
}
