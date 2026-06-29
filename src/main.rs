use std::path::{Path, PathBuf};
use std::process::Command as ShellCmd;

use clap::{Parser, Subcommand, ValueEnum};

use lilypondx::audio;
use lilypondx::error::LilypondxError;
use lilypondx::ly_gen;
use lilypondx::midi_file;
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
    /// Play a score with live TUI preview (default) or headless playback.
    Play {
        /// Path or HTTP(S) URL to the .md score file
        file: PathBuf,
        /// Disable TUI, just play audio and exit
        #[arg(long)]
        no_tui: bool,
        /// Sparkline width in columns (default: terminal width or 60)
        #[arg(short, long)]
        width: Option<usize>,
        /// Disable file watching (auto-disabled for HTTP URLs)
        #[arg(long)]
        no_watch: bool,
        /// Pitch row mode: "auto" (detect scale), "chromatic" (all rows),
        /// or a key like "c major" / "a minor"
        #[arg(short = 'S', long, default_value = "auto")]
        scale: String,
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
        /// Part layout: "split" (each track separate) or "combined" (one staff)
        #[arg(long, default_value = "split")]
        parts: ExportParts,
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
        /// Watch for file changes and auto-re-export
        #[arg(short, long)]
        watch: bool,

        /// Part layout: "split" (each track on its own staff) or "combined"
        /// (all tracks on one staff, e.g. for solo guitar).
        #[arg(long, default_value = "split")]
        parts: ExportParts,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ExportParts {
    Split,
    Combined,
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
        Command::Play { file, no_tui, width, no_watch, scale } =>
            cmd_play(&file, no_tui, width.unwrap_or(0), no_watch, scale),
        Command::Gen { file, output, tablature, parts } => cmd_gen(&file, output, tablature, parts),
        Command::Dump { file, rows: _, scale } => cmd_dump(&file, &scale),
        Command::New { file } => cmd_new(file),
        Command::Export { file, output, format, transpose, tablature, watch, parts } => cmd_export(&file, output, format, transpose, tablature, watch, parts),
    }
}

fn cmd_play(file: &Path, no_tui: bool, width: usize, no_watch: bool, scale: String) -> Result<(), LilypondxError> {
    let source = file.to_string_lossy().to_string();
    let is_url = source.starts_with("http://") || source.starts_with("https://");

    // For .ly files (local or downloaded from URL), compile directly.
    let is_ly = source.ends_with(".ly");
    if is_ly {
        // For URL .ly files, download to a temp file that lives until we're done.
        let _download_tmp: Option<tempfile::TempDir>;
        let ly_path = if is_url {
            let tmp = tempfile::TempDir::new().map_err(LilypondxError::Io)?;
            let p = tmp.path().join("downloaded.ly");
            let content = ureq::get(&source)
                .call()
                .map_err(|e| LilypondxError::Http(format!("{e}")))?
                .into_string()
                .map_err(|e| LilypondxError::Http(format!("{e}")))?;
            std::fs::write(&p, content)?;
            _download_tmp = Some(tmp);
            p
        } else {
            _download_tmp = None;
            file.to_path_buf()
        };

        println!("Compiling {} with LilyPond...", ly_path.display());
        let (events, tempo_bpm, _tpb) = midi_file::compile_ly_file(&ly_path)?;
        println!("Compiled: {} MIDI events ({} BPM)", events.len(), tempo_bpm);

        if no_tui {
            let sf2 = synth::find_soundfont();
            if let Some(p) = &sf2 {
                println!("Using SoundFont: {}", p.display());
            } else {
                println!("No SoundFont found — using built-in oscillator synth");
            }
            let player = audio::AudioPlayer::new(events, TICKS_PER_BEAT, tempo_bpm);
            println!("▶ Playing...");
            player.play()?;
        } else {
            // TUI mode: convert MIDI events → sparkline via midi_events_to_parsed_track.
            let title = ly_path.file_stem().and_then(|s| s.to_str()).unwrap_or("score").to_string();
            let app = lilypondx::tui::App::new_midi(title, events, tempo_bpm);
            tui::run_tui_with_app(app)?;
        }
        return Ok(());
    }

    if no_tui {
        // Headless playback (no TUI, no watching).
        let score = parser::parse_markdown(&file.to_string_lossy())?;
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
    } else {
        // TUI mode with optional file watching.
        tui::run_tui(file.to_path_buf(), width, 0, no_watch, scale)?;
    }
    Ok(())
}

fn cmd_gen(file: &Path, output: Option<PathBuf>, tablature: bool, parts: ExportParts) -> Result<(), LilypondxError> {
    let mut score = parser::parse_markdown(&file.to_string_lossy())?;
    if tablature {
        score.metadata.tablature = true;
    }
    score.metadata.parts = Some(match parts {
        ExportParts::Split => "split",
        ExportParts::Combined => "combined",
    }.into());
    let ly = ly_gen::generate_ly(&score);

    let out_path = output.unwrap_or_else(|| file.with_extension("ly"));
    std::fs::write(&out_path, &ly)?;
    println!("Generated: {}", out_path.display());
    Ok(())
}

fn cmd_new(file: Option<PathBuf>) -> Result<(), LilypondxError> {
    let out_path = file.unwrap_or_else(|| PathBuf::from("twinkle.md"));
    let template = r#"---
title: "=Twinkle, Twinkle, Little Star"
composer: "Traditional"
tempo: "4 = 100"
key: 'c \major'
time: "4/4"
---

# Twinkle, Twinkle, Little Star

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

fn cmd_export(file: &Path, output: Option<PathBuf>, format: ExportFormat, transpose: Option<i32>, tablature: bool, watch: bool, parts: ExportParts) -> Result<(), LilypondxError> {
    use notify::Watcher;
    let parts_str = match parts {
        ExportParts::Split => "split",
        ExportParts::Combined => "combined",
    };
    if watch {
        // Watch loop: re-export on file change.
        let canonical = file.canonicalize()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res
                && matches!(event.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_))
            {
                let _ = tx.send(std::time::Instant::now());
            }
        })
        .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;
        watcher
            .watch(&canonical, notify::RecursiveMode::NonRecursive)
            .map_err(|e| LilypondxError::Io(std::io::Error::other(e)))?;

        // Initial export.
        export_once(file, &output, &format, transpose, tablature, parts_str)?;

        let debounce = std::time::Duration::from_millis(300);
        let mut last_change: Option<std::time::Instant> = None;
        loop {
            while let Ok(ts) = rx.try_recv() {
                last_change = Some(ts);
            }
            if let Some(ts) = last_change
                && ts.elapsed() >= debounce
            {
                last_change = None;
                match export_once(file, &output, &format, transpose, tablature, parts_str) {
                    Ok(()) => {}
                    Err(e) => eprintln!("Export error: {e}"),
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    } else {
        export_once(file, &output, &format, transpose, tablature, parts_str)
    }
}

fn export_once(file: &Path, output: &Option<PathBuf>, format: &ExportFormat, transpose: Option<i32>, tablature: bool, parts: &str) -> Result<(), LilypondxError> {
    let mut score = parser::parse_markdown(&file.to_string_lossy())?;
    if transpose.is_some() {
        score.metadata.transpose = transpose;
    }
    if tablature {
        score.metadata.tablature = true;
    }
    score.metadata.parts = Some(parts.to_string());
    let ly = ly_gen::generate_ly(&score);

    let out_stem = output.clone().unwrap_or_else(|| file.with_extension(""));

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
            "mid" | "midi" if want_midi => {
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
    let score = parser::parse_markdown(&file.to_string_lossy())?;

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
