/// A parsed musical note with absolute pitch(es) and duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Absolute pitches as MIDI note numbers (C4 = 60, each semitone ±1).
    /// Empty for rests. Multiple entries = chord (simultaneous notes).
    pub pitches: Vec<u8>,
    /// Duration in ticks. Quarter note = `ticks_per_beat`.
    pub duration: u32,
    /// Whether a tie starts from this note (glues to next note).
    pub tie: bool,
    /// Absolute start tick (when this note begins).
    pub start_tick: u64,
    /// Staccato articulation (shorten note-off to ~50% of duration).
    pub staccato: bool,
}

/// Result of parsing a single track's notes.
#[derive(Debug, Clone)]
pub struct ParsedTrack {
    pub notes: Vec<Note>,
    /// Total duration in ticks.
    pub total_ticks: u64,
}

/// Parse LilyPond note syntax in relative mode.
///
/// `input` — raw note string, e.g. `"a8 ais c4 d c |"`
/// `anchor` — the relative anchor, e.g. `"c'''"` or `"c,"`
/// `ticks_per_beat` — resolution (e.g. 480 ticks per quarter note)
pub fn parse_notes_relative(input: &str, anchor: &str, ticks_per_beat: u32) -> ParsedTrack {
    let anchor_pitch = parse_anchor(anchor);
    parse_notes_impl(input, anchor_pitch, ticks_per_beat, true)
}

/// Parse the anchor string (e.g. "c'''", "c,", "cis'") into a MIDI pitch.
pub fn parse_anchor(anchor: &str) -> u8 {
    let mut chars = anchor.chars().peekable();
    let (pitch, _) = parse_pitch_with_octave(&mut chars, false);
    pitch
}

fn parse_notes_impl(
    input: &str,
    anchor_pitch: u8,
    ticks_per_beat: u32,
    relative: bool,
) -> ParsedTrack {
    let mut notes = Vec::new();
    let mut prev_pitch: u8 = anchor_pitch;
    let mut current_tick: u64 = 0;
    let mut default_duration: u32 = ticks_per_beat / 4; // default to 8th note

    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '|' => {
                chars.next();
            }
            '%' => {
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            }
            ';' => {
                // ;; line comment — skip rest of line
                if matches!(chars.peek(), Some(';')) {
                    chars.next(); // consume second ;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                // single ; — ignore it
            }
            '/' => {
                if matches!(chars.peek(), Some('/')) {
                    chars.next(); // consume second /
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\n' {
                            break;
                        }
                    }
                }
            }
            '\\' => {
                skip_command(&mut chars);
            }
            's' | 'r' if is_rest_start(&chars) => {
                chars.next();
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;
                notes.push(Note { pitches: Vec::new(), duration: dur, tie: false, start_tick: current_tick, staccato: false });
                current_tick += dur as u64;
            }
            '<' => {
                chars.next();
                // Collect all pitches inside <...>
                let mut chord_pitches: Vec<u8> = Vec::new();
                let mut chord_prev = prev_pitch;
                while let Some(&c) = chars.peek() {
                    if c == '>' {
                        chars.next();
                        break;
                    }
                    if matches!(c, 'a'..='g') {
                        let (raw_pitch, octave_shift) = parse_pitch_with_octave(&mut chars, relative);
                        let pitch = if relative {
                            relative_pitch(raw_pitch, octave_shift, chord_prev)
                        } else {
                            raw_pitch
                        };
                        chord_pitches.push(pitch);
                        // Inside a chord, each note is relative to the previous
                        // note *inside* the chord (per LilyPond docs).
                        chord_prev = pitch;
                    } else {
                        chars.next();
                    }
                }
                if chord_pitches.is_empty() {
                    chord_pitches.push(prev_pitch);
                }
                // After a chord, prev_pitch = first note of the chord
                // (per LilyPond: "the first note of the chord is used as
                // the reference point" for the following note).
                prev_pitch = chord_pitches[0];
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;
                let mut tie = false;
                if let Some(&'~') = chars.peek() {
                    chars.next();
                    tie = true;
                }
                let staccato = parse_staccato(&mut chars);
                notes.push(Note { pitches: chord_pitches, duration: dur, tie, start_tick: current_tick, staccato });
                current_tick += dur as u64;
            }
            'a'..='g' => {
                let (raw_pitch, octave_shift) = parse_pitch_with_octave(&mut chars, relative);
                let pitch = if relative {
                    relative_pitch(raw_pitch, octave_shift, prev_pitch)
                } else {
                    raw_pitch
                };
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;

                let mut tie = false;
                if let Some(&'~') = chars.peek() {
                    chars.next();
                    tie = true;
                }
                let staccato = parse_staccato(&mut chars);

                notes.push(Note { pitches: vec![pitch], duration: dur, tie, start_tick: current_tick, staccato });
                prev_pitch = pitch;
                current_tick += dur as u64;
            }
            _ => {
                chars.next();
            }
        }

        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
    }

    ParsedTrack { notes, total_ticks: current_tick }
}

/// Check if 's' or 'r' is a rest start (not part of a word like "ais").
fn is_rest_start(chars: &std::iter::Peekable<std::str::Chars>) -> bool {
    let mut peek = chars.clone();
    peek.next();
    match peek.peek() {
        Some(&next) => {
            next.is_whitespace() || next.is_ascii_digit() || next == '~' || next == '|'
        }
        None => true,
    }
}

/// Parse a pitch letter + optional accidentals + optional octave marks.
/// Returns (pitch, octave_shift). In relative mode pitch is base C3 (48);
/// in absolute mode octave marks map directly (c'=C4=60, c=C3=48, c,=C2=36).
fn parse_pitch_with_octave(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    relative: bool,
) -> (u8, i32) {
    let note_letter = chars.next().expect("expected note letter");
    let base = match note_letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => 0,
    };

    let accidental = parse_accidental(chars);
    let semitone = base + accidental;

    let mut octave_shift: i32 = 0;
    while let Some(&c) = chars.peek() {
        match c {
            '\'' => { chars.next(); octave_shift += 1; }
            ',' => { chars.next(); octave_shift -= 1; }
            _ => break,
        }
    }

    let octave_midi_base: i32 = if relative {
        48 // relative mode: raw pitch is always C3, relative_pitch handles octave
    } else {
        48 + octave_shift * 12
    };

    let pitch = (octave_midi_base + semitone).clamp(0, 127) as u8;
    (pitch, octave_shift)
}

/// Parse accidentals: "is"(+1), "es"/"as"(-1), "isis"(+2), "eses"(-2), or none.
fn parse_accidental(chars: &mut std::iter::Peekable<std::str::Chars>) -> i32 {
    let mut buf = String::new();
    let mut peek = chars.clone();
    while let Some(&c) = peek.peek() {
        if c.is_alphabetic() {
            buf.push(c);
            peek.next();
        } else {
            break;
        }
    }

    match buf.as_str() {
        "isis" => { chars.next(); chars.next(); chars.next(); chars.next(); 2 }
        "eses" => { chars.next(); chars.next(); chars.next(); chars.next(); -2 }
        "is" => { chars.next(); chars.next(); 1 }
        "es" | "as" => { chars.next(); chars.next(); -1 }
        _ => 0,
    }
}

/// In relative mode, choose the octave that makes the pitch closest to `prev_pitch`,
/// then apply octave marks (`'` = +1 octave, `,` = -1 octave).
fn relative_pitch(raw_pitch: u8, octave_shift: i32, prev_pitch: u8) -> u8 {
    let mut best = raw_pitch;
    let mut best_dist = (raw_pitch as i32 - prev_pitch as i32).unsigned_abs();

    // Try octave shifts up to ±3 (36 semitones) to find the closest pitch.
    // LilyPond's rule is "interval less than a fifth" which can require
    // shifting up to 3 octaves in extreme cases.
    for &oct_offset in &[-48i32, -36, -24, -12, 12, 24, 36, 48] {
        let candidate = ((raw_pitch as i32) + oct_offset).clamp(0, 127) as u8;
        let dist = (candidate as i32 - prev_pitch as i32).unsigned_abs();
        if dist < best_dist {
            best = candidate;
            best_dist = dist;
        }
    }

    ((best as i32) + octave_shift * 12).clamp(0, 127) as u8
}

/// Check for staccato articulation `-.` after a note/chord.
fn parse_staccato(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    let mut peek = chars.clone();
    if peek.next() == Some('-') && peek.next() == Some('.') {
        chars.next();
        chars.next();
        true
    } else {
        false
    }
}

/// Parse a duration number (and optional dots) after a note.
fn parse_duration(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    default_dur: u32,
    ticks_per_beat: u32,
) -> u32 {
    let mut num_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_str.push(c);
            chars.next();
        } else {
            break;
        }
    }

    let base_dur = if num_str.is_empty() {
        default_dur
    } else {
        let n: u32 = num_str.parse().unwrap_or(4);
        ticks_per_beat * 4 / n
    };

    let mut dur = base_dur;
    let mut dot_value = base_dur / 2;
    while let Some(&'.') = chars.peek() {
        chars.next();
        dur += dot_value;
        dot_value /= 2;
    }

    dur
}

/// Skip a LilyPond command (starts with \).
fn skip_command(chars: &mut std::iter::Peekable<std::str::Chars>) {
    chars.next();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            break;
        }
        if c == '{' {
            chars.next();
            let mut depth = 1;
            while let Some(&c) = chars.peek() {
                chars.next();
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            break;
        }
        chars.next();
    }
}

/// Convert raw MIDI events (note_on/note_off pairs) into a `ParsedTrack`.
/// Each note_on with velocity > 0 starts a note; the matching note_off ends it.
/// Multiple simultaneous notes become chords (same start_tick, grouped).
pub fn midi_events_to_parsed_track(events: &[crate::audio::MidiEvent]) -> ParsedTrack {
    use std::collections::HashMap;

    // Collect (pitch, start_tick, end_tick) for each note.
    // Track active notes: (channel, pitch) → start_tick.
    let mut active: HashMap<(u8, u8), u64> = HashMap::new();
    let mut raw_notes: Vec<(u8, u64, u64)> = Vec::new(); // (pitch, start, end)

    for ev in events {
        match ev.command {
            0x90 if ev.data2 > 0 => {
                // Note on.
                active.insert((ev.channel, ev.data1), ev.tick);
            }
            0x80 => {
                // Note off.
                if let Some(start) = active.remove(&(ev.channel, ev.data1)) {
                    raw_notes.push((ev.data1, start, ev.tick));
                }
            }
            0x90 if ev.data2 == 0 => {
                // Note on with velocity 0 = note off.
                if let Some(start) = active.remove(&(ev.channel, ev.data1)) {
                    raw_notes.push((ev.data1, start, ev.tick));
                }
            }
            _ => {}
        }
    }

    // Group notes by start_tick (chords share start_tick).
    raw_notes.sort_by_key(|(_, start, _)| *start);
    let mut notes: Vec<Note> = Vec::new();
    let mut i = 0;
    let total_ticks = raw_notes.iter().map(|(_, _, end)| *end).max().unwrap_or(0);
    while i < raw_notes.len() {
        let start = raw_notes[i].1;
        let mut group: Vec<(u8, u64, u64)> = Vec::new();
        while i < raw_notes.len() && raw_notes[i].1 == start {
            group.push(raw_notes[i]);
            i += 1;
        }
        // Duration = max end - start.
        let max_end = group.iter().map(|(_, _, end)| *end).max().unwrap_or(start);
        let duration = (max_end - start).max(1) as u32;
        let pitches: Vec<u8> = group.iter().map(|(p, _, _)| *p).collect();
        notes.push(Note {
            pitches,
            duration,
            tie: false,
            start_tick: start,
            staccato: false,
        });
    }

    ParsedTrack {
        notes,
        total_ticks,
    }
}
/// `PITCH,DURATION` per note, space-separated. `R` for rests, `~` suffix for ties.
/// Chords are serialized as `[p1+p2+p3],DURATION`.
pub fn serialize_notes(track: &ParsedTrack) -> String {
    track
        .notes
        .iter()
        .map(|n| {
            let pitch = if n.pitches.is_empty() {
                "R".into()
            } else if n.pitches.len() == 1 {
                n.pitches[0].to_string()
            } else {
                format!("[{}]", n.pitches.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("+"))
            };
            let tie = if n.tie { "~" } else { "" };
            format!("{pitch},{}{tie}", n.duration)
        })
        .collect::<Vec<_>>()
        .join(" ")
}
