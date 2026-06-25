/// A parsed musical note with absolute pitch and duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Absolute pitch as MIDI note number (C4 = 60, each semitone ±1).
    /// `None` for rests.
    pub pitch: Option<u8>,
    /// Duration in ticks. With the default resolution, a quarter note = `ticks_per_beat`.
    /// The caller provides the resolution (e.g. 480).
    pub duration: u32,
    /// Whether a tie starts from this note (glues to next note).
    pub tie: bool,
}

/// Parsed bar check (just records position for sparkline grid alignment).
#[derive(Debug, Clone, Copy)]
pub struct BarCheck {
    pub tick: u64,
}

/// Result of parsing a single track's notes.
#[derive(Debug, Clone)]
pub struct ParsedTrack {
    pub notes: Vec<Note>,
    pub bar_checks: Vec<BarCheck>,
    /// Total duration in ticks.
    pub total_ticks: u64,
}

/// Parse LilyPond note syntax in relative mode.
///
/// `input` — raw note string, e.g. `"a8 ais c4 d c | g8 a ais4 c ais |"`
/// `anchor` — the relative anchor, e.g. `"c'''"` or `"c,"`
/// `ticks_per_beat` — resolution (e.g. 480 ticks per quarter note)
pub fn parse_notes_relative(
    input: &str,
    anchor: &str,
    ticks_per_beat: u32,
) -> ParsedTrack {
    let anchor_pitch = parse_anchor(anchor);
    parse_notes_impl(input, anchor_pitch, ticks_per_beat, true)
}

/// Parse LilyPond note syntax in absolute mode.
pub fn parse_notes_absolute(
    input: &str,
    ticks_per_beat: u32,
) -> ParsedTrack {
    parse_notes_impl(input, 60, ticks_per_beat, false)
}

/// Parse the anchor string (e.g. "c'''", "c,", "cis'") into a MIDI pitch.
fn parse_anchor(anchor: &str) -> u8 {
    let (pitch, _octave) = parse_pitch_with_octave(anchor, 0, false);
    pitch
}

fn parse_notes_impl(
    input: &str,
    anchor_pitch: u8,
    ticks_per_beat: u32,
    relative: bool,
) -> ParsedTrack {
    let mut notes = Vec::new();
    let mut bar_checks = Vec::new();
    let mut prev_pitch: u8 = anchor_pitch;
    let mut current_tick: u64 = 0;
    let mut default_duration: u32 = ticks_per_beat / 4; // default to 8th note

    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '|' => {
                chars.next();
                bar_checks.push(BarCheck { tick: current_tick });
            }
            '%' => {
                // Comment — skip to end of line
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            }
            '\\' => {
                // Skip commands like \clef, \key, \time, \tempo, \repeat, etc.
                skip_command(&mut chars);
            }
            's' | 'r' if is_rest_start(ch, &mut chars) => {
                // Rest
                chars.next(); // consume s or r
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;
                notes.push(Note {
                    pitch: None,
                    duration: dur,
                    tie: false,
                });
                current_tick += dur as u64;
            }
            '<' => {
                // Chord — skip for now, read until >
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '>' {
                        break;
                    }
                }
                // Parse duration after chord
                if let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                        default_duration = dur;
                        // Add a placeholder note for the chord
                        notes.push(Note {
                            pitch: Some(prev_pitch),
                            duration: dur,
                            tie: false,
                        });
                        current_tick += dur as u64;
                    }
                }
            }
            'a'..='g' => {
                // Note!
                let (raw_pitch, octave_shift) = parse_pitch_with_octave_str(&mut chars, relative);
                let pitch = if relative {
                    relative_pitch(raw_pitch, octave_shift, prev_pitch)
                } else {
                    raw_pitch
                };
                let dur = parse_duration(&mut chars, default_duration, ticks_per_beat);
                default_duration = dur;

                // Check for tie
                let mut tie = false;
                if let Some(&'~') = chars.peek() {
                    chars.next();
                    tie = true;
                }

                notes.push(Note {
                    pitch: Some(pitch),
                    duration: dur,
                    tie,
                });
                prev_pitch = pitch;
                current_tick += dur as u64;
            }
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            _ => {
                // Unknown char, skip
                chars.next();
            }
        }

        // Also skip whitespace between tokens
        while let Some(&c) = chars.peek() {
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                chars.next();
            } else {
                break;
            }
        }
    }

    ParsedTrack {
        notes,
        bar_checks,
        total_ticks: current_tick,
    }
}

/// Check if 's' or 'r' is a rest start (not part of a word like "ais" or "right").
fn is_rest_start(_ch: char, chars: &std::iter::Peekable<std::str::Chars>) -> bool {
    // Peek ahead: if next char is a letter, it's a word (like "staff", "sustain")
    // We need to clone the iterator to peek without consuming
    let mut peek = chars.clone();
    let _ = peek.next(); // skip current ch (s or r)
    if let Some(&next) = peek.peek() {
        // If followed by space, digit, newline, or end, it's a rest
        // Also if followed by '~' it's a tied rest
        if next.is_whitespace() || next.is_ascii_digit() || next == '~' || next == '|' {
            return true;
        }
        // If next is a letter, it's a word (not a rest)
        if next.is_alphabetic() {
            return false;
        }
    }
    // End of input → it's a rest
    true
}

/// Parse a pitch letter + optional accidentals + optional octave marks.
/// Returns (absolute_pitch_including_octave, octave_shift_relative).
/// The octave_shift_relative is the number of octave marks (' or ,).
fn parse_pitch_with_octave_str(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    relative: bool,
) -> (u8, i32) {
    let note_letter = chars.next().expect("expected note letter");
    let base = match note_letter {
        'c' => 0i32,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => 0,
    };

    // Accidentals
    let accidental = parse_accidental(chars);

    let semitone = base + accidental;

    // Octave marks
    let mut octave_shift: i32 = 0;
    while let Some(&c) = chars.peek() {
        match c {
            '\'' => {
                chars.next();
                octave_shift += 1;
            }
            ',' => {
                chars.next();
                octave_shift -= 1;
            }
            _ => break,
        }
    }

    // In non-relative mode, octave marks determine absolute pitch directly
    // C4 = 60, with each octave = ±12 semitones
    let octave_midi_base: i32 = if relative {
        // In relative mode, the raw pitch is always at base C3.
        // relative_pitch handles octave marks AFTER finding the closest octave.
        48
    } else {
        // In absolute mode, octave marks determine pitch directly.
        // c' = C4 (60), c'' = C5 (72), c = C3 (48), c, = C2 (36)
        48 + octave_shift * 12
    };

    let pitch = (octave_midi_base + semitone).clamp(0, 127) as u8;
    (pitch, octave_shift)
}

/// Parse pitch with octave from an anchor string (non-iterator version).
fn parse_pitch_with_octave(s: &str, default_octave_shift: i32, _relative: bool) -> (u8, i32) {
    let mut chars = s.chars();
    let note_letter = chars.next().unwrap_or('c');
    let base = match note_letter {
        'c' => 0i32,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => 0,
    };

    let rest = chars.collect::<String>();
    let rest_str = rest.as_str();

    // Parse accidental from rest
    let accidental = if rest_str.starts_with("isis") {
        2
    } else if rest_str.starts_with("eses") {
        -2
    } else if rest_str.starts_with("is") {
        1
    } else if rest_str.starts_with("es") {
        -1
    } else if rest_str.starts_with("as") {
        -1
    } else {
        0
    };

    // Count octave marks
    let accidental_len = if accidental == 2 || accidental == -2 {
        if accidental == 2 { 4 } else { 4 } // "isis" or "eses"
    } else if accidental == 1 || accidental == -1 {
        if accidental == 1 && rest_str.starts_with("is") && !rest_str.starts_with("isis") { 2 }
        else if accidental == -1 && rest_str.starts_with("eses") { 4 }
        else if accidental == -1 && rest_str.starts_with("es") { 2 }
        else { 2 } // "as"
    } else {
        0
    };

    let octave_str = &rest[accidental_len..];
    let mut octave_shift = default_octave_shift;
    for c in octave_str.chars() {
        match c {
            '\'' => octave_shift += 1,
            ',' => octave_shift -= 1,
            _ => {}
        }
    }

    let semitone = base + accidental;
    let octave_midi = 48 + octave_shift * 12;
    let pitch = (octave_midi + semitone).clamp(0, 127) as u8;
    (pitch, octave_shift)
}

/// Parse accidentals: "is", "es", "as", "isis", "eses", or nothing.
fn parse_accidental(chars: &mut std::iter::Peekable<std::str::Chars>) -> i32 {
    // Need to peek ahead. Build a small buffer.
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

    let accidental = match buf.as_str() {
        "isis" => { chars.next(); chars.next(); chars.next(); chars.next(); 2 }
        "eses" => { chars.next(); chars.next(); chars.next(); chars.next(); -2 }
        "is" => { chars.next(); chars.next(); 1 }
        "es" => { chars.next(); chars.next(); -1 }
        "as" => { chars.next(); chars.next(); -1 }
        _ => 0,
    };
    accidental
}

/// In relative mode, choose the octave that makes the pitch closest to `prev_pitch`,
/// then apply octave marks (`'` = +1 octave, `,` = -1 octave).
fn relative_pitch(raw_pitch: u8, octave_shift: i32, prev_pitch: u8) -> u8 {
    // Always find the closest octave first (without octave marks)
    let mut best = raw_pitch;
    let mut best_dist = distance(raw_pitch, prev_pitch);

    for oct_offset in &[-12, 12, -24, 24] {
        let candidate = ((raw_pitch as i32) + oct_offset).clamp(0, 127) as u8;
        let dist = distance(candidate, prev_pitch);
        if dist < best_dist {
            best = candidate;
            best_dist = dist;
        }
    }

    // Then apply octave marks on top
    ((best as i32) + octave_shift * 12).clamp(0, 127) as u8
}

fn distance(a: u8, b: u8) -> u32 {
    (a as i32 - b as i32).unsigned_abs()
}

/// Parse a duration number (and optional dots) after a note.
fn parse_duration(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    default_dur: u32,
    ticks_per_beat: u32,
) -> u32 {
    // Parse digits
    let mut num_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_str.push(c);
            chars.next();
        } else {
            break;
        }
    }

    let base_dur: u32 = if num_str.is_empty() {
        default_dur
    } else {
        let n: u32 = num_str.parse().unwrap_or(4);
        // LilyPond duration: 1=whole, 2=half, 4=quarter, 8=eighth, ...
        ticks_per_beat * 4 / n
    };

    // Dots add half the previous duration
    let mut dur = base_dur;
    while let Some(&'.') = chars.peek() {
        chars.next();
        dur += dur / 2; // Each dot adds half of current value
    }

    dur
}

/// Skip a LilyPond command (starts with \).
fn skip_command(chars: &mut std::iter::Peekable<std::str::Chars>) {
    chars.next(); // consume the backslash
    // Read the command word
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            break;
        }
        if c == '{' {
            chars.next();
            // Skip to matching }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_relative() {
        // a8 ais c4 d c — relative to c' (middle C = 60)
        let result = parse_notes_relative("a8 ais c4 d c", "c'", 480);
        assert_eq!(result.notes.len(), 5);

        // a8 (relative to c: a below c = A3 = 57)
        assert_eq!(result.notes[0].pitch, Some(57)); // A3
        assert_eq!(result.notes[0].duration, 240);   // 8th note = 480*4/8 = 240

        // ais (next to a=57: ais above a=58)
        assert_eq!(result.notes[1].pitch, Some(58));
        assert_eq!(result.notes[1].duration, 240);

        // c4 (ais→c: c=60 is closer than 48 or 72 from 58: 60-58=2)
        assert_eq!(result.notes[2].pitch, Some(60));
        assert_eq!(result.notes[2].duration, 480);

        // d (c=60→d=62) — inherits quarter-note duration from c4
        assert_eq!(result.notes[3].pitch, Some(62));
        assert_eq!(result.notes[3].duration, 480);

        // c (d=62→c=60) — inherits quarter-note duration
        assert_eq!(result.notes[4].pitch, Some(60));
        assert_eq!(result.notes[4].duration, 480);
    }

    #[test]
    fn test_parse_relative_with_octave_marks() {
        // Test that explicit octave marks are respected
        let result = parse_notes_relative("c' g", "c", 480);
        assert_eq!(result.notes[0].pitch, Some(60)); // c' = C4 = 60
        // g relative to c'=60: closest g is 55 (G3, 5 semitones below vs 7 above)
        assert_eq!(result.notes[1].pitch, Some(55));
    }

    #[test]
    fn test_parse_rest() {
        let result = parse_notes_relative("c4 r8 d4", "c'", 480);
        assert_eq!(result.notes.len(), 3);
        assert_eq!(result.notes[0].pitch, Some(60));
        assert_eq!(result.notes[1].pitch, None); // rest
        assert_eq!(result.notes[1].duration, 240); // 8th
        assert_eq!(result.notes[2].pitch, Some(62));
    }

    #[test]
    fn test_parse_s_rest() {
        let result = parse_notes_relative("c4 s8 d4", "c", 480);
        assert_eq!(result.notes.len(), 3);
        assert_eq!(result.notes[1].pitch, None); // s rest
    }

    #[test]
    fn test_parse_tie() {
        let result = parse_notes_relative("c4~ c8 d4", "c", 480);
        assert_eq!(result.notes.len(), 3);
        assert_eq!(result.notes[0].tie, true);
        assert_eq!(result.notes[1].tie, false);
    }

    #[test]
    fn test_parse_bar_check() {
        let result = parse_notes_relative("c4 d4 | e4 f4", "c", 480);
        assert_eq!(result.notes.len(), 4);
        assert_eq!(result.bar_checks.len(), 1);
        assert_eq!(result.bar_checks[0].tick, 960); // after two quarter notes
    }

    #[test]
    fn test_parse_dotted() {
        let result = parse_notes_relative("c4. d8", "c", 480);
        assert_eq!(result.notes.len(), 2);
        // c4. = 480 + 240 = 720
        assert_eq!(result.notes[0].duration, 720);
        assert_eq!(result.notes[1].duration, 240);
    }

    #[test]
    fn test_parse_anchor() {
        assert_eq!(parse_anchor("c'''"), 84); // C6 = 48 + 3*12 = 84
        assert_eq!(parse_anchor("c,"), 36);   // C2 = 48 - 12 = 36
        assert_eq!(parse_anchor("c'"), 60);   // C4 = 48 + 12 = 60
        assert_eq!(parse_anchor("c"), 48);    // C3 = 48 + 0*12 = 48
    }

    #[test]
    fn test_duration_default() {
        // First note sets default, second inherits
        let result = parse_notes_relative("c8 d e4", "c", 480);
        assert_eq!(result.notes[0].duration, 240); // 8th
        assert_eq!(result.notes[1].duration, 240); // inherits 8th
        assert_eq!(result.notes[2].duration, 480); // quarter
    }
}
