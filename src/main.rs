mod audio;
mod error;
mod ly_gen;
mod note;
mod parser;
mod score;
mod sparkline;
mod tui;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::error::LilypondxError;

/// LilyPondX — Literary music notation with live TUI preview.
#[derive(Parser)]
#[command(name = "lilypondx", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Play a score: parse Markdown, compile to MIDI, and play through built-in synth.
    Play {
        /// Path to the .md score file
        file: PathBuf,
    },

    /// Generate a .ly file from a Markdown score (dry-run without playback).
    Gen {
        /// Path to the .md score file
        file: PathBuf,
        /// Output .ly file (default: derived from input name)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Watch a Markdown score and render live ASCII sparklines on change.
    Watch {
        /// Path to the .md score file
        file: PathBuf,
        /// Sparkline width in columns (default: terminal width or 60)
        #[arg(short, long)]
        width: Option<usize>,
        /// Number of pitch rows (default: 7)
        #[arg(short, long, default_value = "7")]
        rows: usize,
    },

    /// Dump ASCII sparklines to stdout (one-shot, no TUI).
    Dump {
        /// Path to the .md score file
        file: PathBuf,
        /// Number of pitch rows (default: 10)
        #[arg(short, long, default_value = "10")]
        rows: usize,
    },
}

fn main() -> Result<(), LilypondxError> {
    let cli = Cli::parse();

    match cli.command {
        Command::Play { file } => cmd_play(&file),
        Command::Gen { file, output } => cmd_gen(&file, output),
        Command::Watch { file, width, rows } => tui::run_tui(file, width.unwrap_or(0), rows),
        Command::Dump { file, rows } => cmd_dump(&file, rows),
    }
}

fn cmd_play(file: &PathBuf) -> Result<(), LilypondxError> {
    // 1. Parse markdown
    let score = parser::parse_markdown(file)?;
    println!("Parsed: {} — {} track(s)", score.metadata.title, score.tracks.len());

    // Parse tempo from metadata
    let tempo_bpm: u32 = score
        .metadata
        .tempo
        .as_deref()
        .and_then(|t| t.split('=').nth(1).and_then(|s| s.trim().parse().ok()))
        .unwrap_or(120);
    let ticks_per_beat: u64 = 480;

    // 2. Generate MIDI events directly from parsed notes — no LilyPond needed
    let events = audio::generate_events_direct(&score, ticks_per_beat);
    println!("Generated {} MIDI events directly ({} BPM)", events.len(), tempo_bpm);

    // 3. Find SoundFont
    let sf2_path = audio::find_soundfont()?;
    println!("Using SoundFont: {}", sf2_path.display());

    // 4. Play
    let player = audio::AudioPlayer::new(events, ticks_per_beat, tempo_bpm);
    println!("▶ Playing...");
    player.play(&sf2_path)?;

    Ok(())
}

fn cmd_gen(file: &PathBuf, output: Option<PathBuf>) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;
    let ly = ly_gen::generate_ly(&score);

    let out_path = output.unwrap_or_else(|| {
        file.with_extension("ly")
    });

    std::fs::write(&out_path, &ly)?;
    println!("Generated: {}", out_path.display());
    Ok(())
}

fn cmd_dump(file: &PathBuf, rows: usize) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;

    for track in &score.tracks {
        let parsed = note::parse_notes_relative(&track.notes, &track.relative, 480);
        let config = sparkline::SparklineConfig { rows, ..Default::default() };
        let spark = sparkline::render_sparkline(&parsed, &config);
        println!("=== {} ({}) ===", track.name, track.clef);
        println!("{}", spark);
        println!();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_dump_output() {
        let path = PathBuf::from("tests/sparkline_demo.md");
        let result = cmd_dump(&path, 10);
        assert!(result.is_ok(), "dump should succeed");
    }

    #[test]
    fn test_full_pipeline() {
        // Parse → notes → sparkline → MIDI events
        let path = PathBuf::from("tests/claire_of_glass.md");
        let score = parser::parse_markdown(&path).expect("parse");
        assert!(score.tracks.len() >= 2);

        for track in &score.tracks {
            let parsed = note::parse_notes_relative(&track.notes, &track.relative, 480);
            let spark = sparkline::render_sparkline(&parsed, &Default::default());
            assert!(!spark.is_empty(), "sparkline for {}", track.name);

            // Verify MIDI events can be generated
            let events = audio::generate_events_direct(&score, 480);
            assert!(!events.is_empty(), "MIDI events should be generated");
        }
    }

    #[test]
    fn test_lilypondx_syntax_detection() {
        let path = PathBuf::from("tests/lilypondx_demo.md");
        let score = parser::parse_markdown(&path).expect("parse");
        for track in &score.tracks {
            assert_eq!(track.syntax, "lilypondx", "should detect lilypondx syntax");
        }
    }

    #[test]
    fn test_native_lilypond_syntax_detection() {
        let path = PathBuf::from("tests/claire_of_glass.md");
        let score = parser::parse_markdown(&path).expect("parse");
        for track in &score.tracks {
            assert_eq!(track.syntax, "lilypond", "should detect native lilypond syntax");
        }
    }

    #[test]
    fn test_syntax_variants_in_demo() {
        let path = PathBuf::from("tests/sparkline_demo.md");
        let score = parser::parse_markdown(&path).expect("parse");
        let syntaxes: Vec<&str> = score.tracks.iter().map(|t| t.syntax.as_str()).collect();
        assert!(syntaxes.contains(&"lilypondx"), "should have lilypondx blocks");
        assert!(syntaxes.contains(&"test"), "should have test blocks");
        // Test block should be parseable
        for track in &score.tracks {
            if track.syntax == "test" {
                let parsed = note::parse_notes_relative(&track.notes, &track.relative, 480);
                assert!(!parsed.notes.is_empty(), "test block should parse");
            }
        }
    }

    #[test]
    fn test_render_assertions() {
        let path = PathBuf::from("tests/render_test.md");
        let score = parser::parse_markdown(&path).expect("parse");
        assert_eq!(score.tracks.len(), 2, "should have 2 tracks");
        assert_eq!(score.tracks[0].syntax, "lilypond");
        assert_eq!(score.tracks[1].syntax, "test");

        let config = sparkline::SparklineConfig { rows: 8, ..Default::default() };

        // Track A: c4 d e f (relative=c')
        let a = note::parse_notes_relative(&score.tracks[0].notes, &score.tracks[0].relative, 480);
        let a_out = sparkline::render_sparkline(&a, &config);
        assert_eq!(a_out, concat!(
            "#F4 │        \n",
            " F4 │      ━━\n",
            " E4 │    ━━  \n",
            "#D4 │        \n",
            " D4 │  ━━    \n",
            "#C4 │        \n",
            " C4 │━━      \n",
            " B3 │        \n",
            "    └────────",
        ));

        // Track B: g4 a b c' (relative=c') — chromatic, every semitone shown
        let b = note::parse_notes_relative(&score.tracks[1].notes, &score.tracks[1].relative, 480);
        let b_out = sparkline::render_sparkline(&b, &config);
        assert_eq!(b_out, concat!(
            "#C5 │        \n",
            " C5 │      ━━\n",
            " B4 │        \n",
            "#A4 │        \n",
            " A4 │        \n",
            "#G4 │        \n",
            " G4 │        \n",
            "#F4 │        \n",
            " F4 │        \n",
            " E4 │        \n",
            "#D4 │        \n",
            " D4 │        \n",
            "#C4 │        \n",
            " C4 │        \n",
            " B3 │    ━━  \n",
            "#A3 │        \n",
            " A3 │  ━━    \n",
            "#G3 │        \n",
            " G3 │━━      \n",
            "#F3 │        \n",
            "    └────────",
        ));
    }

    #[test]
    fn test_multi_track_render() {
        let path = PathBuf::from("tests/lilypondx_demo.md");
        let score = parser::parse_markdown(&path).expect("parse");
        assert_eq!(score.tracks.len(), 2);

        let config = sparkline::SparklineConfig { rows: 7, ..Default::default() };
        let rh = note::parse_notes_relative(&score.tracks[0].notes, &score.tracks[0].relative, 480);
        let lh = note::parse_notes_relative(&score.tracks[1].notes, &score.tracks[1].relative, 480);

        assert_eq!(sparkline::render_sparkline(&rh, &config), concat!(
            "#C6 │                                \n",
            " C6 │              ━━━━              \n",
            " B5 │                  ━━            \n",
            "#A5 │                                \n",
            " A5 │                    ━━          \n",
            "#G5 │                                \n",
            " G5 │                      ━━        \n",
            "#F5 │                                \n",
            " F5 │                        ━━      \n",
            " E5 │                          ━━    \n",
            "#D5 │                                \n",
            " D5 │                            ━━  \n",
            "#C5 │                                \n",
            " C5 │                              ━━\n",
            " B4 │            ━━                  \n",
            "#A4 │                                \n",
            " A4 │          ━━                    \n",
            "#G4 │                                \n",
            " G4 │        ━━                      \n",
            "#F4 │                                \n",
            " F4 │      ━━                        \n",
            " E4 │    ━━                          \n",
            "#D4 │                                \n",
            " D4 │  ━━                            \n",
            "#C4 │                                \n",
            " C4 │━━                              \n",
            " B3 │                                \n",
            "    └────────────────────────────────",
        ));

        assert_eq!(sparkline::render_sparkline(&lh, &config), concat!(
            "#C2 │                                \n",
            " C2 │━━━━                            \n",
            " B1 │                                \n",
            "#A1 │                                \n",
            " A1 │        ━━━━                    \n",
            "#G1 │                                \n",
            " G1 │    ━━━━    ━━              ━━━━\n",
            "#F1 │                                \n",
            " F1 │              ━━                \n",
            " E1 │                ━━━━            \n",
            "#D1 │                                \n",
            " D1 │                    ━━━━        \n",
            "#C1 │                                \n",
            " C1 │                        ━━━━    \n",
            " B0 │                                \n",
            "    └────────────────────────────────",
        ));
    }

    #[test]
    fn test_color_output() {
        let path = PathBuf::from("tests/render_test.md");
        let score = parser::parse_markdown(&path).expect("parse");
        let parsed = note::parse_notes_relative(&score.tracks[0].notes, &score.tracks[0].relative, 480);

        // Without color
        let nc = sparkline::SparklineConfig { rows: 7, color: false, ..Default::default() };
        let out = sparkline::render_sparkline(&parsed, &nc);
        assert!(!out.contains("\x1b["), "plain output should have no ANSI");

        // With color — full output assertion
        let wc = sparkline::SparklineConfig { rows: 7, color: true, ..Default::default() };
        let out = sparkline::render_sparkline(&parsed, &wc);
        assert_eq!(out, concat!(
            "#F4 │        \n",
            " F4 │\x1b[46m\x1b[30m      ━━\x1b[0m\n",
            " E4 │\x1b[42m\x1b[30m    ━━  \x1b[0m\n",
            "#D4 │        \n",
            " D4 │\x1b[43m\x1b[30m  ━━    \x1b[0m\n",
            "#C4 │        \n",
            " C4 │\x1b[41m\x1b[30m━━      \x1b[0m\n",
            " B3 │\x1b[47m\x1b[30m        \x1b[0m\n",
            "    └────────",
        ));
    }
}

