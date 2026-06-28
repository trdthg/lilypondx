//! Test harness: loads `.md` files and pairs each input track with its
//! expected `lilypond-test` output block.
//!
//! Convention: tracks alternate — input (`lilypond` or `lilypondx`) followed
//! by its expected output (`lilypond-test`). The expected block's raw text is
//! the assertion target (compared against either serialized notes or a
//! rendered sparkline, decided by the calling test).

#![allow(dead_code)]

use std::path::Path;

use lilypondx::parser;
use lilypondx::score::{Score, Track};

/// A paired test case: the input track plus the expected raw text from the
/// following `lilypond-test` block.
#[derive(Debug)]
pub struct Pair {
    pub input: Track,
    pub expected: String,
}

/// Parse a `.md` file and split its tracks into input/expected pairs.
///
/// Tracks with syntax `lilypond` or `lilypondx` are inputs; tracks with
/// syntax `test` are expected outputs. They are paired in order of appearance.
/// Panics (via `expect`) if a `lilypond-test` block has no preceding input.
pub fn load_pairs(path: &Path) -> Vec<Pair> {
    let score: Score = parser::parse_markdown(&path.to_string_lossy()).expect("parse markdown");
    let mut pairs = Vec::new();
    let mut pending_input: Option<Track> = None;
    for track in score.tracks {
        match track.syntax.as_str() {
            "lilypond" | "lilypondx" => {
                if let Some(prev) = pending_input.take() {
                    pairs.push(Pair { input: prev, expected: String::new() });
                }
                pending_input = Some(track);
            }
            "test" => {
                let input = pending_input.take().expect(
                    "lilypond-test block must be preceded by a lilypond/lilypondx input block",
                );
                pairs.push(Pair { input, expected: track.notes });
            }
            _ => {}
        }
    }
    if let Some(prev) = pending_input {
        pairs.push(Pair { input: prev, expected: String::new() });
    }
    pairs
}

