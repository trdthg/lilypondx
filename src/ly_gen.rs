use crate::score::Score;

/// Generate a complete LilyPond `.ly` file from a `Score`.
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

    for track in &score.tracks {
        out.push_str(&format!("{} = \\relative {} {{\n", track.name, track.relative));
        out.push_str(&format!("  \\clef {}\n", track.clef));
        if let Some(k) = &meta.key { out.push_str(&format!("  \\key {}\n", k)); }
        if let Some(t) = &meta.time { out.push_str(&format!("  \\time {}\n", t)); }
        if let Some(t) = &meta.tempo { out.push_str(&format!("  \\tempo {}\n", t)); }
        out.push_str(&format!("  {}\n", track.notes));
        out.push_str("}\n\n");
    }

    out.push_str("\\score {\n");
    write_staff_block(&mut out, score, true);
    out.push_str("  \\layout { }\n}\n\n");

    let midi_name = midi_output_name(&score.metadata.title);
    out.push_str(&format!("{} = {{\n", midi_name));
    write_staff_block(&mut out, score, false);
    out.push_str("}\n\n");

    out.push_str(&format!(
        "\\book {{\n  \\bookOutputName \"{}\"\n  \\score {{ \\{} \\midi {{ }} }}\n}}\n",
        midi_name, midi_name
    ));

    out
}

/// Write a staff block (layout or MIDI). `layout` controls indentation/structure.
fn write_staff_block(out: &mut String, score: &Score, layout: bool) {
    if layout {
        if score.tracks.len() == 1 {
            let t = &score.tracks[0];
            out.push_str(&format!("  \\new Staff = \"{}\" \\{}\n", t.name, t.name));
        } else {
            out.push_str("  \\new PianoStaff <<\n");
            for t in &score.tracks {
                out.push_str(&format!("    \\new Staff = \"{}\" \\{}\n", t.name, t.name));
            }
            out.push_str("  >>\n");
        }
    } else {
        if score.tracks.len() == 1 {
            let t = &score.tracks[0];
            let inst = t.midi_instrument.as_deref().unwrap_or("acoustic grand");
            out.push_str(&format!(
                "  \\new Staff = \"{}\" {{\n    \\set Staff.midiInstrument = #\"{}\"\n    \\{}\n  }}\n",
                t.name, inst, t.name
            ));
        } else {
            out.push_str("  \\new PianoStaff <<\n");
            for t in &score.tracks {
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

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
