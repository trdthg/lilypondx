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
                notes.push(Note { pitches: Vec::new(), duration: dur, tie: false });
                current_tick += dur as u64;
            }
            '<' => {
                chars.next();
                // Collect all pitches inside <...>
                let mut chord_pitches: Vec<u8> = Vec::new();
                while let Some(&c) = chars.peek() {
                    if c == '>' {
                        chars.next();
                        break;
                    }
                    if matches!(c, 'a'..='g') {
                        let (raw_pitch, octave_shift) = parse_pitch_with_octave(&mut chars, relative);
                        let pitch = if relative {
                            relative_pitch(raw_pitch, octave_shift, prev_pitch)
                        } else {
                            raw_pitch
                        };
                        chord_pitches.push(pitch);
                        prev_pitch = pitch;
                    } else {
                        chars.next();
                    }
                }
                if chord_pitches.is_empty() {
                    chord_pitches.push(prev_pitch);
                }
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;
                let mut tie = false;
                if let Some(&'~') = chars.peek() {
                    chars.next();
                    tie = true;
                }
                notes.push(Note { pitches: chord_pitches, duration: dur, tie });
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

                notes.push(Note { pitches: vec![pitch], duration: dur, tie });
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

    for &oct_offset in &[-12i32, 12, -24, 24] {
        let candidate = ((raw_pitch as i32) + oct_offset).clamp(0, 127) as u8;
        let dist = (candidate as i32 - prev_pitch as i32).unsigned_abs();
        if dist < best_dist {
            best = candidate;
            best_dist = dist;
        }
    }

    ((best as i32) + octave_shift * 12).clamp(0, 127) as u8
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
    while let Some(&'.') = chars.peek() {
        chars.next();
        dur += dur / 2;
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

/// Serialize parsed notes to a compact test-friendly format:
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
