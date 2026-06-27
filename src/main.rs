use std::path::{Path, PathBuf};
use std::process::Command as ShellCmd;

use clap::{Parser, Subcommand, ValueEnum};

use lilypondx::audio;
use lilypondx::error::LilypondxError;
use lilypondx::ly_gen;
use lilypondx::note;
use lilypondx::parser;
use lilypondx::sparkline;
use lilypondx::synth;
use lilypondx::tui;
use lilypondx::TICKS_PER_BEAT;

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
        /// Generate guitar tablature (TabStaff) alongside standard notation
        #[arg(short = 'T', long)]
        tablature: bool,
    },

    /// Watch a Markdown score and render live ASCII sparklines on change.
    Watch {
        /// Path to the .md score file
        file: PathBuf,
        /// Sparkline width in columns (default: terminal width or 60)
        #[arg(short, long)]
        width: Option<usize>,
        /// Number of pitch rows (kept for backward compat, currently unused)
        #[arg(short, long, default_value = "7")]
        rows: usize,
        /// Pitch row mode: "auto" (detect scale), "chromatic" (all rows),
        /// or a key like "c major" / "a minor"
        #[arg(short = 'S', long, default_value = "auto")]
        scale: String,
    },

    /// Dump ASCII sparklines to stdout (one-shot, no TUI).
    Dump {
        /// Path to the .md score file
        file: PathBuf,
        /// Number of pitch rows (kept for backward compat, currently unused)
        #[arg(short, long, default_value = "10")]
        rows: usize,
        /// Pitch row mode: "auto" (detect scale), "chromatic" (all rows),
        /// or a key like "c major" / "a minor"
        #[arg(short = 'S', long, default_value = "auto")]
        scale: String,
    },

    /// Create a new .md score from a template.
    New {
        /// Path to write the .md score file (default: ./twinkle.md)
        file: Option<PathBuf>,
    },

    /// Export a score to PDF and/or MIDI via LilyPond.
    Export {
        /// Path to the .md score file
        file: PathBuf,

        /// Output basename (without extension). Defaults to the markdown file name.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Output format(s): pdf, midi, or both
        #[arg(short, long, default_value = "pdf")]
        format: ExportFormat,

        /// Transposition in semitones (overrides frontmatter `transpose`).
        /// e.g. 12 = up one octave (useful for guitar notation).
        #[arg(short, long)]
        transpose: Option<i32>,
        /// Generate guitar tablature (TabStaff) alongside standard notation
        #[arg(short = 'T', long)]
        tablature: bool,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ExportFormat {
    Pdf,
    Midi,
    Both,
}

fn main() -> Result<(), LilypondxError> {
    let cli = Cli::parse();

    match cli.command {
        Command::Play { file } => cmd_play(&file),
        Command::Gen { file, output, tablature } => cmd_gen(&file, output, tablature),
        Command::Watch { file, width, rows, scale } => tui::run_tui(file, width.unwrap_or(0), rows, scale),
        Command::Dump { file, rows: _, scale } => cmd_dump(&file, &scale),
        Command::New { file } => cmd_new(file),
        Command::Export { file, output, format, transpose, tablature } => cmd_export(&file, output, format, transpose, tablature),
    }
}

fn cmd_play(file: &Path) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;
    println!("Parsed: {} — {} track(s)", score.metadata.title, score.tracks.len());

    let (events, tempo_bpm) = audio::generate_events(&score, TICKS_PER_BEAT)?;
    println!("Generated {} MIDI events ({} BPM)", events.len(), tempo_bpm);

    let sf2 = synth::find_soundfont();
    if let Some(p) = &sf2 {
        println!("Using SoundFont: {}", p.display());
    } else {
        println!("No SoundFont found — using built-in oscillator synth");
    }

    let player = audio::AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
    println!("▶ Playing...");
    player.play()?;

    Ok(())
}

fn cmd_gen(file: &Path, output: Option<PathBuf>, tablature: bool) -> Result<(), LilypondxError> {
    let mut score = parser::parse_markdown(file)?;
    if tablature {
        score.metadata.tablature = true;
    }
    let ly = ly_gen::generate_ly(&score);

    let out_path = output.unwrap_or_else(|| file.with_extension("ly"));
    std::fs::write(&out_path, &ly)?;
    println!("Generated: {}", out_path.display());
    Ok(())
}

fn cmd_new(file: Option<PathBuf>) -> Result<(), LilypondxError> {
    let out_path = file.unwrap_or_else(|| PathBuf::from("twinkle.md"));
    let template = r#"---
title: "小星星 (Twinkle, Twinkle, Little Star)"
composer: "Traditional"
tempo: "4 = 100"
key: 'c \major'
time: "4/4"
---

# 小星星

经典的启蒙旋律，用于演示 `lilypondx` 的解析、渲染与播放。
右手 `relative=c'` 锚定 C4; 第一个 `g'` 用八度标记强制上跳，
之后 relative 上下文留在 G4，后续裸 `g` 自然继承。
左手 `relative=c,` 锚定 C2，采用 I-I-IV-V-I 进行，
最后一小节 `g'2 c,2` 形成 G→C 的完满终止 (属到主)。

```lilypondx track=RH clef=treble relative=c'
c4 c g'4 g4 a4 a4 g2 |
f4 f e4 e4 d4 d4 c2 |
```

```lilypondx track=LH clef=bass relative=c,
c1 | c1 | f,1 | g'2 c,2 |
```
"#;
    std::fs::write(&out_path, template)?;
    println!("Created: {}", out_path.display());
    Ok(())
}

fn cmd_export(file: &Path, output: Option<PathBuf>, format: ExportFormat, transpose: Option<i32>, tablature: bool) -> Result<(), LilypondxError> {
    let mut score = parser::parse_markdown(file)?;
    // CLI --transpose overrides frontmatter `transpose`
    if transpose.is_some() {
        score.metadata.transpose = transpose;
    }
    if tablature {
        score.metadata.tablature = true;
    }
    let ly = ly_gen::generate_ly(&score);

    let out_stem = output.unwrap_or_else(|| file.with_extension(""));

    // Write .ly to a temp dir so LilyPond's collateral doesn't pollute cwd.
    let tmp = tempfile::TempDir::new().map_err(|e| LilypondxError::Io(e))?;
    let ly_path = tmp.path().join("score.ly");
    std::fs::write(&ly_path, &ly)?;

    let result = ShellCmd::new("lilypond")
        .arg("-o")
        .arg(tmp.path().join("score"))
        .arg(&ly_path)
        .output()
        .map_err(|_| LilypondxError::LilypondNotFound)?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(LilypondxError::LilypondError(stderr.to_string()));
    }

    let want_pdf = matches!(format, ExportFormat::Pdf | ExportFormat::Both);
    let want_midi = matches!(format, ExportFormat::Midi | ExportFormat::Both);

    for entry in std::fs::read_dir(tmp.path())? {
        let entry = entry?;
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
        match ext {
            "pdf" if want_pdf => {
                let dest = out_stem.with_extension("pdf");
                std::fs::copy(&path, &dest)?;
                println!("Exported: {}", dest.display());
            }
            "midi" if want_midi => {
                let dest = out_stem.with_extension("midi");
                std::fs::copy(&path, &dest)?;
                println!("Exported: {}", dest.display());
            }
            _ => {}
        }
    }

    Ok(())
}

fn cmd_dump(file: &Path, scale: &str) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;

    // Resolve scale mode.
    let scale_mode = resolve_scale_mode(&score, scale);

    // Parse all non-test / non-lilypond tracks for sparkline display.
    // lilypond blocks go through the real LilyPond compiler, not our parser.
    let beats_per_bar = score
        .metadata
        .time
        .as_deref()
        .and_then(|t| t.split('/').next())
        .and_then(|s| s.trim().parse::<u32>().ok());
    let parsed: Vec<(&str, &str, note::ParsedTrack)> = score
        .tracks
        .iter()
        .filter(|t| t.syntax == "lilypondx")
        .map(|t| {
            let p = note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT);
            (t.name.as_str(), t.clef.as_str(), p)
        })
        .collect();

    if parsed.is_empty() {
        println!("(no lilypondx tracks to display)");
        return Ok(());
    }
    let shared_total_ticks = parsed.iter().map(|(_, _, p)| p.total_ticks).max();

    let n = parsed.len();
    for (idx, (name, clef, parsed)) in parsed.iter().enumerate() {
        let cfg = sparkline::SparklineConfig {
            beats_per_bar,
            total_ticks_override: shared_total_ticks,
            show_progress_bar: idx == n - 1,
            scale_mode,
            ..Default::default()
        };
        let spark = sparkline::render_sparkline(parsed, &cfg);
        println!("=== {name} ({clef}) ===");
        println!("{spark}");
        println!();
    }
    Ok(())
}

/// Resolve the `--scale` CLI argument into a `ScaleMode` (mirrors TUI logic).
fn resolve_scale_mode(score: &lilypondx::score::Score, arg: &str) -> lilypondx::sparkline::ScaleMode {
    use lilypondx::note;
    use lilypondx::sparkline;
    use lilypondx::TICKS_PER_BEAT;

    match arg.trim() {
        "chromatic" => sparkline::ScaleMode::Chromatic,
        "auto" => {
            if let Some(k) = &score.metadata.key {
                if let Some(mode) = sparkline::parse_key(k) {
                    return mode;
                }
            }
            let parsed: Vec<note::ParsedTrack> = score
                .tracks
                .iter()
                .map(|t| note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT))
                .collect();
            let combined = note::ParsedTrack {
                notes: parsed.iter().flat_map(|p| p.notes.iter().cloned()).collect(),
                total_ticks: parsed.iter().map(|p| p.total_ticks).max().unwrap_or(0),
            };
            sparkline::detect_scale(&combined)
                .map(|(_, mask, _)| sparkline::ScaleMode::Diatonic(mask))
                .unwrap_or(sparkline::ScaleMode::Chromatic)
        }
        key_str => {
            sparkline::parse_key(key_str).unwrap_or(sparkline::ScaleMode::Chromatic)
        }
    }
}
