use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use crate::error::LilypondxError;
use crate::score::Score;
/// Compile a `.ly` file to MIDI using the `lilypond` command-line tool.
/// Returns the path to the generated `.mid` file.
pub fn compile_ly_to_midi(ly_path: &Path) -> Result<PathBuf, LilypondxError> {
    let lilypond = find_lilypond()?;
    let output = std::process::Command::new(&lilypond)
        .arg("-dno-print-pages")
        .arg("-ddelete-intermediate-files")
        .arg("-o")
        .arg(ly_path.parent().unwrap_or(Path::new(".")))
        .arg(ly_path)
        .output()
        .map_err(|e| LilypondxError::LilypondCompile(format!("Failed to run lilypond: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LilypondxError::LilypondCompile(stderr.into_owned()));
    }

    // Find the generated .mid file — LilyPond uses \bookOutputName, not the .ly stem
    let out_dir = ly_path.parent().unwrap_or(Path::new("."));
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Try parsing "MIDI output to `...'" from LilyPond output
    if let Some(midi_name) = stdout
        .lines()
        .find_map(|line| line.strip_prefix("MIDI output to `").and_then(|s| s.strip_suffix("'...")))
    {
        let midi_path = out_dir.join(midi_name);
        if midi_path.exists() {
            return Ok(midi_path);
        }
    }

    // Fallback: scan for any .mid in output dir
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "mid") {
                return Ok(p);
            }
        }
    }

    Err(LilypondxError::LilypondCompile(
        "MIDI file not found after compilation".into(),
    ))
}

/// Generate MIDI events directly from parsed notes — no LilyPond needed.
/// This is the fast path for `play`: microseconds instead of ~50ms process spawn.
pub fn generate_events_direct(
    score: &Score,
    ticks_per_beat: u64,
) -> Vec<MidiEvent> {
    let mut events = Vec::new();

    for (ch, track) in score.tracks.iter().enumerate() {
        let channel = ch as u8;
        let anchor = &track.relative;
        let parsed = crate::note::parse_notes_relative(
            &track.notes,
            anchor,
            ticks_per_beat as u32,
        );

        // Program change (instrument)
        let program = midi_program(track.midi_instrument.as_deref().unwrap_or("acoustic grand"));
        events.push(MidiEvent {
            tick: 0,
            channel,
            command: 0xC0,
            data1: program,
            data2: 0,
        });

        let mut current_tick: u64 = 0;
        for note in &parsed.notes {
            if let Some(pitch) = note.pitch {
                events.push(MidiEvent {
                    tick: current_tick,
                    channel,
                    command: 0x90,
                    data1: pitch,
                    data2: 80,
                });
                let off_tick = current_tick + note.duration as u64;
                events.push(MidiEvent {
                    tick: off_tick,
                    channel,
                    command: 0x80,
                    data1: pitch,
                    data2: 64,
                });
            }
            current_tick += note.duration as u64;
        }
    }

    events.sort_by_key(|e| e.tick);
    events
}

/// Map instrument name to General MIDI program number.
fn midi_program(name: &str) -> u8 {
    match name.to_lowercase().as_str() {
        "acoustic grand" | "piano" => 0,
        "bright acoustic" => 1,
        "electric grand" => 2,
        "honky-tonk" => 3,
        "electric piano 1" => 4,
        "electric piano 2" => 5,
        "harpsichord" => 6,
        "clavinet" => 7,
        "celesta" => 8,
        "glockenspiel" => 9,
        "music box" => 10,
        "vibraphone" => 11,
        "marimba" => 12,
        "xylophone" => 13,
        "tubular bells" => 14,
        "dulcimer" => 15,
        "drawbar organ" => 16,
        "percussive organ" => 17,
        "rock organ" => 18,
        "church organ" => 19,
        "reed organ" => 20,
        "accordion" => 21,
        "harmonica" => 22,
        "tango accordion" => 23,
        "acoustic guitar (nylon)" | "nylon guitar" => 24,
        "acoustic guitar (steel)" | "steel guitar" => 25,
        "electric guitar (jazz)" => 26,
        "electric guitar (clean)" => 27,
        "electric guitar (muted)" => 28,
        "overdriven guitar" => 29,
        "distorted guitar" => 30,
        "guitar harmonics" => 31,
        "acoustic bass" => 32,
        "electric bass (finger)" => 33,
        "electric bass (pick)" => 34,
        "fretless bass" => 35,
        "slap bass 1" => 36,
        "slap bass 2" => 37,
        "synth bass 1" => 38,
        "synth bass 2" => 39,
        "violin" => 40,
        "viola" => 41,
        "cello" => 42,
        "contrabass" => 43,
        "tremolo strings" => 44,
        "pizzicato strings" => 45,
        "orchestral harp" | "harp" => 46,
        "timpani" => 47,
        "string ensemble 1" => 48,
        "string ensemble 2" => 49,
        "synth strings 1" => 50,
        "synth strings 2" => 51,
        "choir aahs" => 52,
        "voice oohs" => 53,
        "synth voice" => 54,
        "orchestra hit" => 55,
        "trumpet" => 56,
        "trombone" => 57,
        "tuba" => 58,
        "muted trumpet" => 59,
        "french horn" => 60,
        "brass section" => 61,
        "synth brass 1" => 62,
        "synth brass 2" => 63,
        "soprano sax" => 64,
        "alto sax" => 65,
        "tenor sax" => 66,
        "baritone sax" => 67,
        "oboe" => 68,
        "english horn" => 69,
        "bassoon" => 70,
        "clarinet" => 71,
        "piccolo" => 72,
        "flute" => 73,
        "recorder" => 74,
        "pan flute" => 75,
        "blown bottle" => 76,
        "shakuhachi" => 77,
        "whistle" => 78,
        "ocarina" => 79,
        "lead 1 (square)" => 80,
        "lead 2 (sawtooth)" => 81,
        _ => 0, // default to acoustic grand
    }
}

/// A parsed MIDI event ready for the synthesizer.
#[derive(Debug, Clone)]
pub struct MidiEvent {
    pub tick: u64,
    pub channel: u8,
    pub command: u8,
    pub data1: u8,
    pub data2: u8,
}

/// Parse a `.mid` file into a vector of `MidiEvent`s.
pub fn parse_midi(path: &Path) -> Result<Vec<MidiEvent>, LilypondxError> {
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    let smf = Smf::parse(&buf)
        .map_err(|e| LilypondxError::MidiParse(format!("Failed to parse MIDI: {e}")))?;

    let _ticks_per_beat = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as u64,
        _ => 480,
    };
    let mut events = Vec::new();

    for track in smf.tracks {
        let mut abs_tick: u64 = 0;
        for event in track {
            abs_tick += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Midi {
                    channel, message, ..
                } => {
                    let (command, data1, data2) = match message {
                        MidiMessage::NoteOn { key, vel } => {
                            if vel == 0 {
                                (0x80, key.as_int(), 64) // NoteOff
                            } else {
                                (0x90, key.as_int(), vel.as_int())
                            }
                        }
                        MidiMessage::NoteOff { key, vel: _ } => (0x80, key.as_int(), 64),
                        MidiMessage::Controller { controller, value } => {
                            (0xB0, controller.as_int(), value.as_int())
                        }
                        MidiMessage::ProgramChange { program } => (0xC0, program.as_int(), 0),
                        _ => continue,
                    };
                    events.push(MidiEvent {
                        tick: abs_tick,
                        channel: channel.as_int(),
                        command,
                        data1,
                        data2,
                    });
                }
                TrackEventKind::Meta(MetaMessage::Tempo(tempo)) => {
                    // Store tempo changes as meta events for timing
                    events.push(MidiEvent {
                        tick: abs_tick,
                        channel: 0,
                        command: 0xFF,
                        data1: 0x51,
                        data2: (tempo.as_int() & 0xFF) as u8,
                    });
                }
                _ => {}
            }
        }
    }

    events.sort_by_key(|e| e.tick);

    // Keep tempo events separate from note events
    // The audio loop will handle tempo
    Ok(events)
}

/// Find a SoundFont file. Checks common locations.
pub fn find_soundfont() -> Result<PathBuf, LilypondxError> {
    let candidates = [
        // Bundled tiny soundfont
        Path::new("assets/tiny.sf2"),
        // Common system locations
        Path::new("/usr/share/sounds/sf2/FluidR3_GM.sf2"),
        Path::new("/usr/share/soundfonts/FluidR3_GM.sf2"),
        // Windows
        Path::new("C:/Windows/System32/Drivers/gm.dls"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.to_path_buf());
        }
    }

    Err(LilypondxError::SoundFont(
        "No SoundFont found. Download a .sf2 file (e.g. FluidR3_GM.sf2) and place it as assets/tiny.sf2 or set LILYPONDX_SF2 env var.\n\
         Quick start: https://musical-artifacts.com/artifacts/3".into(),
    ))
}

/// Find the LilyPond executable. Checks env var, winget install, and PATH.
fn find_lilypond() -> Result<String, LilypondxError> {
    // 1. Check env var
    if let Ok(path) = std::env::var("LILYPONDX_LILYPOND") {
        if Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // 2. Check common Windows winget install
    let winget_base = std::env::var("LOCALAPPDATA")
        .unwrap_or_default();
    let winget_candidate = Path::new(&winget_base)
        .join("Microsoft/WinGet/Packages/LilyPond.LilyPond_Microsoft.Winget.Source_8wekyb3d8bbwe");
    if let Ok(entries) = std::fs::read_dir(&winget_candidate) {
        for entry in entries.flatten() {
            let bin = entry.path().join("bin/lilypond.exe");
            if bin.exists() {
                return Ok(bin.to_string_lossy().into_owned());
            }
        }
    }

    // 3. Fall back to PATH
    Ok("lilypond".to_string())
}
pub struct AudioPlayer {
    pub events: Vec<MidiEvent>,
    pub ticks_per_beat: u64,
    pub tempo_bpm: u32,
    pub playing: Arc<AtomicBool>,
    /// Current playback position in ticks (updated by audio callback).
    pub current_tick: Arc<AtomicU64>,
    /// Total duration in ticks.
    pub total_ticks: u64,
}

impl AudioPlayer {
    pub fn new(
        events: Vec<MidiEvent>,
        ticks_per_beat: u64,
        tempo_bpm: u32,
    ) -> Self {
        let total_ticks = events.iter().map(|e| e.tick).max().unwrap_or(0);
        Self {
            events,
            ticks_per_beat,
            tempo_bpm,
            playing: Arc::new(AtomicBool::new(true)),
            current_tick: Arc::new(AtomicU64::new(0)),
            total_ticks,
        }
    }

    /// Play audio in a background thread — returns immediately.
    /// Progress readable via `self.progress()`; call `stop()` to interrupt.
    pub fn play_background(&self, sf2_path: &Path) -> Result<(), LilypondxError> {
        let sf2_path = sf2_path.to_path_buf();
        let events = self.events.clone();
        let ticks_per_beat = self.ticks_per_beat;
        let tempo_bpm = self.tempo_bpm;
        let playing = self.playing.clone();
        let current_tick = self.current_tick.clone();

        std::thread::spawn(move || {
            if let Err(e) = play_impl(
                &sf2_path, &events, ticks_per_beat, tempo_bpm,
                &playing, &current_tick,
            ) {
                eprintln!("Playback error: {e}");
            }
        });
        Ok(())
    }

    /// Stop background playback.
    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    /// Fraction 0.0–1.0 of playback completed.
    pub fn progress(&self) -> f64 {
        if self.total_ticks == 0 { return 0.0; }
        (self.current_tick.load(Ordering::Relaxed) as f64 / self.total_ticks as f64).min(1.0)
    }

    /// Play blocking (used by `cmd_play`).
    pub fn play(&self, sf2_path: &Path) -> Result<(), LilypondxError> {
        play_impl(
            sf2_path, &self.events, self.ticks_per_beat, self.tempo_bpm,
            &self.playing, &self.current_tick,
        )
    }
}

fn play_impl(
    sf2_path: &Path,
    events: &[MidiEvent],
    ticks_per_beat: u64,
    tempo_bpm: u32,
    playing: &AtomicBool,
    current_tick: &AtomicU64,
) -> Result<(), LilypondxError> {
    let mut sf2_file = File::open(sf2_path)?;
    let mut sf2_data = Vec::new();
    sf2_file.read_to_end(&mut sf2_data)?;

    let sound_font = Arc::new(
        rustysynth::SoundFont::new(&mut std::io::Cursor::new(&sf2_data))
            .map_err(|e| LilypondxError::SoundFont(format!("Failed to load SoundFont: {e}")))?,
    );

    let settings = rustysynth::SynthesizerSettings::new(44100);
    let synthesizer = rustysynth::Synthesizer::new(&sound_font, &settings)
        .map_err(|e| LilypondxError::SoundFont(format!("Failed to create synthesizer: {e}")))?;

    let state = Arc::new(Mutex::new(PlaybackState {
        synthesizer,
        events: events.to_vec(),
        event_index: 0,
        sample_count: 0,
        current_tempo: 60_000_000 / tempo_bpm.max(1),
        ticks_per_beat,
        sample_rate: 44100,
    }));

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| LilypondxError::Audio("No output device found".into()))?;

    let config = device
        .default_output_config()
        .map_err(|e| LilypondxError::Audio(format!("Failed to get output config: {e}")))?;

    let state_clone = state.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let config: cpal::StreamConfig = config.into();
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    audio_callback_f32(data, &state_clone);
                },
                |err| eprintln!("Audio stream error: {err}"),
                None,
            )
        }
        _ => return Err(LilypondxError::Audio("Unsupported sample format".into())),
    }
    .map_err(|e| LilypondxError::Audio(format!("Failed to create audio stream: {e}")))?;

    stream.play().map_err(|e| {
        LilypondxError::Audio(format!("Failed to play audio stream: {e}"))
    })?;

    // Polling loop: track progress
    while playing.load(Ordering::Relaxed) {
        let st = state.lock().unwrap();
        // Estimate tick from last processed event
        if st.event_index > 0 {
            let idx = st.event_index.min(st.events.len()) - 1;
            current_tick.store(st.events[idx].tick, Ordering::Relaxed);
        }
        let total = st.events.len();
        let idx = st.event_index;
        drop(st);

        if idx >= total {
            std::thread::sleep(Duration::from_millis(500));
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    drop(stream);
    Ok(())
}

struct PlaybackState {
    synthesizer: rustysynth::Synthesizer,
    events: Vec<MidiEvent>,
    event_index: usize,
    sample_count: u64,
    current_tempo: u32, // microseconds per beat
    ticks_per_beat: u64,
    sample_rate: u32,
}

fn audio_callback_f32(data: &mut [f32], state: &Mutex<PlaybackState>) {
    let mut st = state.lock().unwrap();
    let samples_per_tick = (st.sample_rate as f64 * st.current_tempo as f64)
        / (st.ticks_per_beat as f64 * 1_000_000.0);

    let mut left_buf = vec![0.0f32; data.len() / 2];
    let mut right_buf = vec![0.0f32; data.len() / 2];

    // Process pending MIDI events for this buffer
    let end_sample = st.sample_count + (data.len() / 2) as u64;
    while st.event_index < st.events.len() {
        let event_sample = (st.events[st.event_index].tick as f64 * samples_per_tick) as u64;
        if event_sample > end_sample {
            break;
        }

        let ev = &st.events[st.event_index];
        let channel = ev.channel;
        let command = ev.command;
        let data1 = ev.data1;
        let data2 = ev.data2;

        if command == 0xFF && data1 == 0x51 {
            st.event_index += 1;
            continue;
        }

        st.synthesizer
            .process_midi_message(channel as i32, command as i32, data1 as i32, data2 as i32);

        st.event_index += 1;
    }

    st.synthesizer.render(&mut left_buf[..], &mut right_buf[..]);

    // Interleave into output buffer
    for (i, (l, r)) in left_buf.iter().zip(right_buf.iter()).enumerate() {
        data[i * 2] = *l;
        data[i * 2 + 1] = *r;
    }


    st.sample_count = end_sample;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_midi() {
        let midi_path = Path::new("/tmp/score_midi.mid");
        if !midi_path.exists() {
            eprintln!("Skipping test: MIDI file not found (run lilypond first)");
            return;
        }
        let events = parse_midi(midi_path).expect("should parse MIDI");
        assert!(!events.is_empty(), "should have MIDI events");

        // Should have at least NoteOn events
        let has_notes = events.iter().any(|e| e.command == 0x90);
        assert!(has_notes, "should have NoteOn events");

        // Events should be sorted by tick
        for w in events.windows(2) {
            assert!(w[0].tick <= w[1].tick, "events should be sorted by tick");
        }
    }
}
