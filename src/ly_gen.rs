use crate::score::Score;

/// Generate a complete LilyPond `.ly` file from a `Score`.
pub fn generate_ly(score: &Score) -> String {
    let mut out = String::new();

    // Version
    out.push_str("\\version \"2.24.4\"\n\n");

    // Header
    let meta = &score.metadata;
    out.push_str("\\header {\n");
    out.push_str(&format!("  title = \"{}\"\n", escape(meta.title.as_str())));
    if let Some(ref v) = meta.composer {
        out.push_str(&format!("  composer = \"{}\"\n", escape(v)));
    }
    if let Some(ref v) = meta.subtitle {
        out.push_str(&format!("  subtitle = \"{}\"\n", escape(v)));
    }
    if let Some(ref v) = meta.dedication {
        out.push_str(&format!("  dedication = \"{}\"\n", escape(v)));
    }
    if let Some(ref v) = meta.poet {
        out.push_str(&format!("  poet = \"{}\"\n", escape(v)));
    }
    out.push_str("}\n\n");

    // Paper
    out.push_str("\\paper {\n");
    out.push_str("  indent = 10\\mm\n");
    out.push_str("  markup-system-spacing.padding = #5\n");
    out.push_str("}\n\n");

    // Per-track variables
    for track in &score.tracks {
        out.push_str(&format!(
            "{} = \\relative {} {{\n",
            track.name, track.relative
        ));
        out.push_str(&format!("  \\clef {}\n", track.clef));
        if let Some(ref k) = meta.key {
            out.push_str(&format!("  \\key {}\n", k));
        }
        if let Some(ref t) = meta.time {
            out.push_str(&format!("  \\time {}\n", t));
        }
        if let Some(ref t) = meta.tempo {
            out.push_str(&format!("  \\tempo {}\n", t));
        }
        out.push_str(&format!("  {}\n", track.notes));
        out.push_str("}\n\n");
    }

    // Score with layout
    out.push_str("\\score {\n");
    if score.tracks.len() == 1 {
        out.push_str(&format!("  \\new Staff = \"{}\" \\{}\n", score.tracks[0].name, score.tracks[0].name));
    } else {
        out.push_str("  \\new PianoStaff <<\n");
        for track in &score.tracks {
            out.push_str(&format!("    \\new Staff = \"{}\" \\{}\n", track.name, track.name));
        }
        out.push_str("  >>\n");
    }
    out.push_str("  \\layout { }\n");
    out.push_str("}\n\n");

    // MIDI block — use ASCII-safe identifier
    let midi_base: String = score
        .metadata
        .title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let midi_name = if midi_base.is_empty() {
        "score_midi".to_string()
    } else {
        format!("{}_midi", midi_base.to_lowercase())
    };

    out.push_str(&format!("{} = {{\n", midi_name));
    if score.tracks.len() == 1 {
        let track = &score.tracks[0];
        let inst = track
            .midi_instrument
            .as_deref()
            .unwrap_or("acoustic grand");
        out.push_str(&format!(
            "  \\new Staff = \"{}\" {{\n    \\set Staff.midiInstrument = #\"{}\"\n    \\{}\n  }}\n",
            track.name, inst, track.name
        ));
    } else {
        out.push_str("  \\new PianoStaff <<\n");
        for track in &score.tracks {
            let inst = track
                .midi_instrument
                .as_deref()
                .unwrap_or("acoustic grand");
            out.push_str(&format!(
                "    \\new Staff = \"{}\" {{\n      \\set Staff.midiInstrument = #\"{}\"\n      \\{}\n    }}\n",
                track.name, inst, track.name
            ));
        }
        out.push_str("  >>\n");
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "\\book {{\n  \\bookOutputName \"{}\"\n  \\score {{ \\{} \\midi {{ }} }}\n}}\n",
        midi_name, midi_name
    ));

    out
}

/// Generate a MIDI-only `.ly` file — no header, no paper, no layout.
/// LilyPond skips all engraving, producing MIDI in ~50ms.
pub fn generate_ly_midi(score: &Score) -> String {
    let mut out = String::new();
    out.push_str("\\version \"2.24.4\"\n\n");

    // Per-track variables (no header, no paper)
    let meta = &score.metadata;
    for track in &score.tracks {
        out.push_str(&format!(
            "{} = \\relative {} {{\n",
            track.name, track.relative
        ));
        out.push_str(&format!("  \\clef {}\n", track.clef));
        if let Some(ref k) = meta.key {
            out.push_str(&format!("  \\key {}\n", k));
        }
        if let Some(ref t) = meta.time {
            out.push_str(&format!("  \\time {}\n", t));
        }
        if let Some(ref t) = meta.tempo {
            out.push_str(&format!("  \\tempo {}\n", t));
        }
        out.push_str(&format!("  {}\n", track.notes));
        out.push_str("}\n\n");
    }

    // MIDI book — no layout anywhere
    let midi_base: String = score
        .metadata
        .title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let midi_name = if midi_base.is_empty() {
        "score_midi".to_string()
    } else {
        format!("{}_midi", midi_base.to_lowercase())
    };

    out.push_str(&format!("{} = {{\n", midi_name));
    if score.tracks.len() == 1 {
        let track = &score.tracks[0];
        let inst = track.midi_instrument.as_deref().unwrap_or("acoustic grand");
        out.push_str(&format!(
            "  \\new Staff = \"{}\" {{\n    \\set Staff.midiInstrument = #\"{}\"\n    \\{}\n  }}\n",
            track.name, inst, track.name
        ));
    } else {
        out.push_str("  \\new PianoStaff <<\n");
        for track in &score.tracks {
            let inst = track.midi_instrument.as_deref().unwrap_or("acoustic grand");
            out.push_str(&format!(
                "    \\new Staff = \"{}\" {{\n      \\set Staff.midiInstrument = #\"{}\"\n      \\{}\n    }}\n",
                track.name, inst, track.name
            ));
        }
        out.push_str("  >>\n");
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "\\score {{ \\{} \\midi {{ }} }}\n",
        midi_name
    ));

    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_markdown;
    use std::path::PathBuf;

    #[test]
    fn test_generate_ly_from_first_md() {
        let path = PathBuf::from("tests/claire_of_glass.md");
        let score = parse_markdown(&path).expect("should parse");
        let ly = generate_ly(&score);

        // Should contain essential elements
        assert!(ly.contains("\\version \"2.24.4\""));
        assert!(ly.contains("title = \"ガラスのクレア\""));
        assert!(ly.contains("\\clef treble"));
        assert!(ly.contains("\\relative c"));
        assert!(ly.contains("a8 ais c4 d c"));
        assert!(ly.contains("\\midi"));
        assert!(ly.contains("\\layout"));
    }
}
