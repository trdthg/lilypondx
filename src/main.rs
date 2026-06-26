use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

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
    },

    /// Dump ASCII sparklines to stdout (one-shot, no TUI).
    Dump {
        /// Path to the .md score file
        file: PathBuf,
        /// Number of pitch rows (kept for backward compat, currently unused)
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
        Command::Dump { file, rows: _ } => cmd_dump(&file),
    }
}

fn cmd_play(file: &Path) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;
    println!("Parsed: {} — {} track(s)", score.metadata.title, score.tracks.len());

    let tempo_bpm: u32 = score
        .metadata
        .tempo
        .as_deref()
        .and_then(|t| t.split('=').nth(1).and_then(|s| s.trim().parse().ok()))
        .unwrap_or(120);

    let events = audio::generate_events_direct(&score, TICKS_PER_BEAT);
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

fn cmd_gen(file: &Path, output: Option<PathBuf>) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;
    let ly = ly_gen::generate_ly(&score);

    let out_path = output.unwrap_or_else(|| file.with_extension("ly"));
    std::fs::write(&out_path, &ly)?;
    println!("Generated: {}", out_path.display());
    Ok(())
}

fn cmd_dump(file: &Path) -> Result<(), LilypondxError> {
    let score = parser::parse_markdown(file)?;

    // Parse all non-test tracks first so they can share a time axis.
    let beats_per_bar = score
        .metadata
        .time
        .as_deref()
        .and_then(|t| t.split('/').next())
        .and_then(|s| s.trim().parse::<u32>().ok());
    let parsed: Vec<(&str, &str, note::ParsedTrack)> = score
        .tracks
        .iter()
        .filter(|t| t.syntax != "test")
        .map(|t| {
            let p = note::parse_notes_relative(&t.notes, &t.relative, TICKS_PER_BEAT);
            (t.name.as_str(), t.clef.as_str(), p)
        })
        .collect();
    let shared_total_ticks = parsed.iter().map(|(_, _, p)| p.total_ticks).max();

    let n = parsed.len();
    for (idx, (name, clef, parsed)) in parsed.iter().enumerate() {
        let cfg = sparkline::SparklineConfig {
            beats_per_bar,
            total_ticks_override: shared_total_ticks,
            show_progress_bar: idx == n - 1,
            ..Default::default()
        };
        let spark = sparkline::render_sparkline(parsed, &cfg);
        println!("=== {name} ({clef}) ===");
        println!("{spark}");
        println!();
    }
    Ok(())
}
