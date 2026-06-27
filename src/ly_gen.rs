use crate::score::{Score, Track};

/// Generate a complete LilyPond `.ly` file from a `Score`.
fn active_tracks(score: &Score) -> Vec<&Track> {
    score.tracks.iter().filter(|t| t.syntax != "test").collect()
}

pub fn generate_ly(score: &Score) -> String {
    let mut out = String::new();

    out.push_str("\\version \"2.24.4\"\n\n");

    let meta = &score.metadata;
    out.push_str("\\header {\n");
    out.push_str(&format!("  title = \"{}\"\n", escape(&meta.title)));
    if let Some(v) = &meta.composer {
        out.push_str(&format!("  composer = \"{}\"\n", escape(v)));
    }
    if let Some(v) = &meta.subtitle {
        out.push_str(&format!("  subtitle = \"{}\"\n", escape(v)));
    }
    if let Some(v) = &meta.dedication {
        out.push_str(&format!("  dedication = \"{}\"\n", escape(v)));
    }
    if let Some(v) = &meta.poet {
        out.push_str(&format!("  poet = \"{}\"\n", escape(v)));
    }
    out.push_str("}\n\n");

    out.push_str("\\paper {\n  indent = 10\\mm\n  markup-system-spacing.padding = #5\n}\n\n");

    for track in active_tracks(score) {
        let body = track_body(track, meta);
        if let Some(semitones) = &meta.transpose {
            let target = semitones_to_pitch(*semitones);
            // \transpose must wrap \relative, not the other way around,
            // otherwise relative pitch resolution gives wrong intervals.
            out.push_str(&format!("{} = \\transpose c {} {{\n", track.name, target));
            out.push_str(&format!("  \\relative {} {{\n", track.relative));
            for line in body.lines() {
                out.push_str(&format!("  {}\n", line));
            }
            out.push_str("  }\n");
            out.push_str("}\n\n");
        } else {
            out.push_str(&format!("{} = \\relative {} {{\n", track.name, track.relative));
            out.push_str(&body);
            out.push_str("}\n\n");
        }
    }

    out.push_str("\\score {\n");
    let tablature = score.metadata.tablature;
    write_staff_block(&mut out, score, true, tablature);
    out.push_str("  \\layout { }\n}\n\n");

    let midi_name = midi_output_name(&score.metadata.title);
    out.push_str(&format!("{} = {{\n", midi_name));
    write_staff_block(&mut out, score, false, false);
    out.push_str("}\n\n");

    out.push_str(&format!(
        "\\book {{\n  \\bookOutputName \"{}\"\n  \\score {{ \\{} \\midi {{ }} }}\n}}\n",
        midi_name, midi_name
    ));

    out
}

/// Write a staff block (layout or MIDI). `layout` controls indentation/structure.
fn write_staff_block(out: &mut String, score: &Score, layout: bool, tablature: bool) {
    let tracks: Vec<&Track> = active_tracks(score);
    if layout {
        if tracks.len() == 1 {
            let t = tracks[0];
            if tablature {
                out.push_str("  \\new StaffGroup <<\n");
                out.push_str(&format!("    \\new Staff = \"{}\" \\{}\n", t.name, t.name));
                out.push_str(&format!("    \\new TabStaff = \"{}_tab\" \\{}\n", t.name, t.name));
                out.push_str("  >>\n");
            } else {
                out.push_str(&format!("  \\new Staff = \"{}\" \\{}\n", t.name, t.name));
            }
        } else {
            out.push_str("  \\new PianoStaff <<\n");
            for t in &tracks {
                if tablature {
                    out.push_str(&format!("    \\new StaffGroup <<\n"));
                    out.push_str(&format!("      \\new Staff = \"{}\" \\{}\n", t.name, t.name));
                    out.push_str(&format!("      \\new TabStaff = \"{}_tab\" \\{}\n", t.name, t.name));
                    out.push_str(&format!("    >>\n"));
                } else {
                    out.push_str(&format!("    \\new Staff = \"{}\" \\{}\n", t.name, t.name));
                }
            }
            out.push_str("  >>\n");
        }
    } else {
        if tracks.len() == 1 {
            let t = tracks[0];
            let inst = t.midi_instrument.as_deref().unwrap_or("acoustic grand");
            out.push_str(&format!(
                "  \\new Staff = \"{}\" {{\n    \\set Staff.midiInstrument = #\"{}\"\n    \\{}\n  }}\n",
                t.name, inst, t.name
            ));
        } else {
            out.push_str("  \\new PianoStaff <<\n");
            for t in &tracks {
                let inst = t.midi_instrument.as_deref().unwrap_or("acoustic grand");
                out.push_str(&format!(
                    "    \\new Staff = \"{}\" {{\n      \\set Staff.midiInstrument = #\"{}\"\n      \\{}\n    }}\n",
                    t.name, inst, t.name
                ));
            }
            out.push_str("  >>\n");
        }
    }
}

fn midi_output_name(title: &str) -> String {
    let base: String = title.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if base.is_empty() {
        "score_midi".into()
    } else {
        format!("{}_midi", base.to_lowercase())
    }
}

/// Generate the body of a voice definition (clef, key, time, tempo, notes).
/// When transpose is active this goes inside `\transpose` / `\relative`.
fn track_body(track: &Track, meta: &crate::score::ScoreMetadata) -> String {
    let mut b = String::new();
    b.push_str(&format!("  \\clef {}\n", track.clef));
    if let Some(k) = &meta.key { b.push_str(&format!("  \\key {}\n", k)); }
    if let Some(t) = &meta.time { b.push_str(&format!("  \\time {}\n", t)); }
    if let Some(t) = &meta.tempo { b.push_str(&format!("  \\tempo {}\n", t)); }
    b.push_str(&format!("  {}\n", track.notes));
    b
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Convert a semitone offset to a LilyPond pitch string for `\transpose c <pitch>`.
/// 0 → `c`, 12 → `c'`, -12 → `c,`, 7 → `g`, -5 → `f,`, 1 → `cis`
fn semitones_to_pitch(st: i32) -> String {
    // LilyPond "c" = C3 = MIDI 48.  We compute the note st semitones above that.
    let midi = 48i32 + st;
    let octave_shift = midi.div_euclid(12) - 4; // 4 = octave of C3
    let semitone = midi.rem_euclid(12) as usize;

    let (letter, accidental) = match semitone {
        0 => ('c', ""), 1 => ('c', "is"), 2 => ('d', ""), 3 => ('d', "is"),
        4 => ('e', ""), 5 => ('f', ""), 6 => ('f', "is"), 7 => ('g', ""),
        8 => ('g', "is"), 9 => ('a', ""), 10 => ('a', "is"), 11 => ('b', ""),
        _ => unreachable!(),
    };

    let mut s = String::new();
    s.push(letter);
    s.push_str(accidental);
    for _ in 0..octave_shift {
        s.push('\'');
    }
    for _ in 0..(-octave_shift) {
        s.push(',');
    }
    s
}
