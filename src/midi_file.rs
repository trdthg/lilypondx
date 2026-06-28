use std::io::Read;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::audio::MidiEvent;
use crate::error::LilypondxError;
use crate::ly_gen;
use crate::score::Score;

/// Strip common comment styles from LilyPond note text before passing to the compiler.
/// Handles `;;`, `//`, and `%` line comments.
fn strip_comments(notes: &str) -> String {
    notes
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with(";;") || trimmed.starts_with("//"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compile `lilypond` syntax tracks via the real LilyPond compiler.
/// Returns (events, tempo_bpm, ticks_per_beat).
pub fn compile_lilypond_tracks(score: &Score) -> Result<(Vec<MidiEvent>, u32, u32), LilypondxError> {
    let lilypond_tracks: Vec<_> = score.tracks.iter().filter(|t| t.syntax == "lilypond").collect();
    if lilypond_tracks.is_empty() {
        return Ok((Vec::new(), 120, 480));
    }

    let tmp = TempDir::new().map_err(|e| LilypondxError::Io(e))?;
    let ly_path = tmp.path().join("score.ly");

    // Reuse ly_gen to produce a full .ly file, but we need to limit to lilypond tracks.
    // Build a sub-score with only lilypond tracks, stripping non-LilyPond comments.
    let sub_tracks: Vec<_> = lilypond_tracks
        .iter()
        .map(|t| {
            let mut track = (*t).clone();
            track.notes = strip_comments(&track.notes);
            track
        })
        .collect();
    let sub_score = Score {
        metadata: score.metadata.clone(),
        tracks: sub_tracks,
    };
    let ly_content = ly_gen::generate_ly(&sub_score);
    std::fs::write(&ly_path, &ly_content)?;

    let output = Command::new("lilypond")
        .arg("-o")
        .arg(tmp.path().join("score"))
        .arg(&ly_path)
        .output()
        .map_err(|_| LilypondxError::LilypondNotFound)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LilypondxError::LilypondError(stderr.to_string()));
    }

    // Find the generated .midi file (named after \bookOutputName in the .ly)
    let midi_path = find_midi_in_dir(tmp.path())
        .ok_or_else(|| LilypondxError::LilypondError("no .midi output produced".into()))?;

    parse_midi_file(&midi_path)
}

/// Find a .midi file in the given directory.
fn find_midi_in_dir(dir: &Path) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "midi") {
            return Some(path);
        }
    }
    None
}

/// Compile a raw `.ly` file directly to MIDI (without our `ly_gen` wrapper).
/// Returns (events, tempo_bpm, ticks_per_beat).
pub fn compile_ly_file(path: &Path) -> Result<(Vec<MidiEvent>, u32, u32), LilypondxError> {
    let tmp = TempDir::new().map_err(|e| LilypondxError::Io(e))?;
    let output_base = tmp.path().join("output");

    let result = Command::new("lilypond")
        .arg("-o")
        .arg(&output_base)
        .arg(path)
        .output()
        .map_err(|_| LilypondxError::LilypondNotFound)?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(LilypondxError::LilypondError(stderr.to_string()));
    }

    let midi_path = find_midi_in_dir(tmp.path())
        .ok_or_else(|| LilypondxError::LilypondError("no .midi output produced".into()))?;

    parse_midi_file(&midi_path)
}

/// Parse a Standard MIDI File into MidiEvents and extract tempo / ticks-per-beat.
pub fn parse_midi_file(path: &Path) -> Result<(Vec<MidiEvent>, u32, u32), LilypondxError> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    let smf = midly::Smf::parse(&buf)
        .map_err(|e| LilypondxError::MidiParse(format!("{e}")))?;

    let ticks_per_beat = match smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int() as u32,
        _ => 480,
    };

    let mut events = Vec::new();
    let mut tempo_micros_per_qn: u64 = 500_000; // default 120 BPM

    for track_data in &smf.tracks {
        let mut abs_tick: u64 = 0;
        for event in track_data {
            abs_tick += event.delta.as_int() as u64;
            match event.kind {
                midly::TrackEventKind::Midi { channel, message } => {
                    match message {
                        midly::MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            events.push(MidiEvent {
                                tick: abs_tick,
                                channel: channel.as_int(),
                                command: 0x90,
                                data1: key.as_int(),
                                data2: vel.as_int(),
                            });
                        }
                        midly::MidiMessage::NoteOff { key, vel }
                        | midly::MidiMessage::NoteOn { key, vel } if vel.as_int() == 0 => {
                            events.push(MidiEvent {
                                tick: abs_tick,
                                channel: channel.as_int(),
                                command: 0x80,
                                data1: key.as_int(),
                                data2: vel.as_int(),
                            });
                        }
                        _ => {}
                    }
                }
                midly::TrackEventKind::Meta(meta) => {
                    if let midly::MetaMessage::Tempo(micros) = meta {
                        tempo_micros_per_qn = micros.as_int() as u64;
                    }
                }
                _ => {}
            }
        }
    }

    // Normalize tick base to our TICKS_PER_BEAT (480)
    let target_tpb: u32 = 480;
    if ticks_per_beat != target_tpb {
        let ratio = target_tpb as f64 / ticks_per_beat as f64;
        for e in &mut events {
            e.tick = (e.tick as f64 * ratio) as u64;
        }
    }

    let bpm = (60_000_000.0 / tempo_micros_per_qn as f64).round() as u32;
    events.sort_by_key(|e| e.tick);

    Ok((events, bpm.max(1), target_tpb))
}
