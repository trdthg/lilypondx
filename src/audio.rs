use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::error::LilypondxError;
use crate::midi_file;
use crate::note;
use crate::score::Score;
use crate::synth::{self, Synth};

/// A parsed MIDI event ready for the synthesizer.
#[derive(Debug, Clone)]
pub struct MidiEvent {
    pub tick: u64,
    pub channel: u8,
    pub command: u8,
    pub data1: u8,
    pub data2: u8,
}

/// Generate MIDI events for all tracks.
/// - `lilypondx` tracks: use internal parser (no external deps)
/// - `lilypond` tracks: call the real LilyPond compiler
/// - `lilypond-test` blocks: skipped
/// Returns (events, tempo_bpm).
pub fn generate_events(score: &Score, ticks_per_beat: u32) -> Result<(Vec<MidiEvent>, u32), LilypondxError> {
    let mut events = Vec::new();

    // ── lilypondx tracks: internal parser ──────────────────────────────
    let lx_tracks: Vec<_> = score.tracks.iter().filter(|t| t.syntax == "lilypondx").collect();
    let transpose = score.metadata.transpose.unwrap_or(0);
    for (ch, track) in lx_tracks.iter().enumerate() {
        let channel = ch as u8;
        let parsed = note::parse_notes_relative(&track.notes, &track.relative, ticks_per_beat);

        let program = midi_program(track.midi_instrument.as_deref().unwrap_or("acoustic grand"));
        events.push(MidiEvent { tick: 0, channel, command: 0xC0, data1: program, data2: 0 });

        for n in &parsed.notes {
            let start = n.start_tick;
            // Staccato: shorten note-off to ~50% of duration.
            let off_tick = if n.staccato {
                start + (n.duration as u64 / 2)
            } else {
                start + n.duration as u64
            };
            for &pitch in &n.pitches {
                let tpitch = (pitch as i32 + transpose).clamp(0, 127) as u8;
                events.push(MidiEvent {
                    tick: start, channel, command: 0x90, data1: tpitch, data2: 80,
                });
                events.push(MidiEvent {
                    tick: off_tick,
                    channel,
                    command: 0x80,
                    data1: tpitch,
                    data2: 64,
                });
            }
        }
    }

    // ── lilypond tracks: external LilyPond compiler ────────────────────
    let lp_tracks: Vec<_> = score.tracks.iter().filter(|t| t.syntax == "lilypond").collect();
    let tempo_bpm = if !lp_tracks.is_empty() {
        let (mut lp_events, lp_bpm, _) = midi_file::compile_lilypond_tracks(score)?;

        // Offset channels to avoid overlap with lilypondx tracks
        let channel_offset = lx_tracks.len() as u8;
        for e in &mut lp_events {
            e.channel += channel_offset;
        }

        events.extend(lp_events);
        lp_bpm
    } else {
        score
            .metadata
            .tempo
            .as_deref()
            .and_then(|t| t.split('=').nth(1).and_then(|s| s.trim().parse().ok()))
            .unwrap_or(120)
    };

    events.sort_by_key(|e| e.tick);
    Ok((events, tempo_bpm))
}

/// Deprecated: kept for compatibility.
/// Use `generate_events` instead.
pub fn generate_events_direct(score: &Score, ticks_per_beat: u32) -> Vec<MidiEvent> {
    generate_events(score, ticks_per_beat)
        .map(|(ev, _)| ev)
        .unwrap_or_default()
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
        _ => 0,
    }
}

// ── Audio player ───────────────────────────────────────────────────────────

pub struct AudioPlayer {
    pub events: Vec<MidiEvent>,
    pub ticks_per_beat: u32,
    pub tempo_bpm: u32,
    playing: Arc<AtomicBool>,
    current_tick: Arc<AtomicU64>,
    pub total_ticks: u64,
    last_error: Arc<Mutex<Option<String>>>,
    backend_slot: Arc<Mutex<String>>,
}

impl AudioPlayer {
    pub fn new(events: Vec<MidiEvent>, ticks_per_beat: u32, tempo_bpm: u32) -> Self {
        let total_ticks = events.iter().map(|e| e.tick).max().unwrap_or(0);
        Self {
            events,
            ticks_per_beat,
            tempo_bpm,
            playing: Arc::new(AtomicBool::new(true)),
            current_tick: Arc::new(AtomicU64::new(0)),
            total_ticks,
            last_error: Arc::new(Mutex::new(None)),
            backend_slot: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Play in a background thread from `start_tick` (0 = beginning).
    pub fn play_background_from(&self, start_tick: u64) -> Result<(), LilypondxError> {
        let events = self.events.clone();
        let ticks_per_beat = self.ticks_per_beat;
        let tempo_bpm = self.tempo_bpm;
        let playing = self.playing.clone();
        let current_tick = self.current_tick.clone();
        let last_error = self.last_error.clone();
        let backend_out = self.backend_slot.clone();

        current_tick.store(start_tick, Ordering::Relaxed);
        std::thread::spawn(move || {
            if let Err(e) = play_impl(
                &events, ticks_per_beat, tempo_bpm, &playing, &current_tick,
                &last_error, &backend_out, start_tick,
            ) {
                *last_error.lock().unwrap() = Some(format!("{e}"));
            }
        });
        Ok(())
    }

    /// Play in a background thread from the beginning.
    pub fn play_background(&self) -> Result<(), LilypondxError> {
        self.play_background_from(0)
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    pub fn progress(&self) -> f64 {
        if self.total_ticks == 0 {
            return 0.0;
        }
        (self.current_tick.load(Ordering::Relaxed) as f64 / self.total_ticks as f64).min(1.0)
    }

    /// Last playback error captured by the audio thread, if any.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    /// Synth backend name once known ("SoundFont" / "Built-in").
    pub fn backend_name(&self) -> String {
        self.backend_slot.lock().unwrap().clone()
    }

    /// Play blocking from the beginning.
    pub fn play(&self) -> Result<(), LilypondxError> {
        play_impl(
            &self.events, self.ticks_per_beat, self.tempo_bpm, &self.playing, &self.current_tick,
            &self.last_error, &self.backend_slot, 0,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn play_impl(
    events: &[MidiEvent],
    ticks_per_beat: u32,
    tempo_bpm: u32,
    playing: &AtomicBool,
    current_tick: &AtomicU64,
    last_error: &Arc<Mutex<Option<String>>>,
    backend_out: &Arc<Mutex<String>>,
    start_tick: u64,
) -> Result<(), LilypondxError> {
    let synth = synth::create_synth()?;
    *backend_out.lock().unwrap() = synth.name().to_string();

    // Advance event_index past events before start_tick.
    let mut event_index = 0;
    for (i, e) in events.iter().enumerate() {
        if e.tick < start_tick {
            event_index = i + 1;
        }
    }

    let samples_per_tick = (44100.0 * 60_000_000.0 / tempo_bpm.max(1) as f64)
        / (ticks_per_beat as f64 * 1_000_000.0);
    let start_sample = (start_tick as f64 * samples_per_tick) as u64;

    let sample_rate: u64 = 44100;
    let state = Arc::new(Mutex::new(PlaybackState {
        synth,
        events: events.to_vec(),
        event_index,
        sample_count: start_sample,
        samples_per_tick,
        left_buf: vec![0.0; 1024],
        right_buf: vec![0.0; 1024],
        // Fade in over ~30ms to avoid pops/clicks when seeking.
        ramp_samples: if start_tick > 0 { (sample_rate / 30).max(1) } else { 0 },
    }));

    // Clear any hanging notes from a previous synth session.
    {
        let mut st = state.lock().unwrap();
        for ch in 0..16 {
            st.synth
                .process_midi_message(ch, 0xB0, 123, 0);
        }
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| LilypondxError::Audio("No output device found".into()))?;

    let config = device
        .default_output_config()
        .map_err(|e| LilypondxError::Audio(format!("Failed to get output config: {e}")))?;

    let err_slot = last_error.clone();
    let state_clone = state.clone();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let config: cpal::StreamConfig = config.into();
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    audio_callback(data, &state_clone);
                },
                move |err| *err_slot.lock().unwrap() = Some(format!("Audio stream error: {err}")),
                None,
            )
        }
        _ => return Err(LilypondxError::Audio("Unsupported sample format".into())),
    }
    .map_err(|e| LilypondxError::Audio(format!("Failed to create audio stream: {e}")))?;

    stream
        .play()
        .map_err(|e| LilypondxError::Audio(format!("Failed to play audio stream: {e}")))?;

    while playing.load(Ordering::Relaxed) {
        let (idx, total) = {
            let st = state.lock().unwrap();
            (st.event_index, st.events.len())
        };
        // Compute the actual playback position from sample_count (the audio
        // device's current position), NOT from the last processed event's tick.
        // The callback processes events in advance (buffering), so
        // events[idx-1].tick is ahead of what's actually being heard.
        {
            let st = state.lock().unwrap();
            let actual_tick = (st.sample_count as f64 / st.samples_per_tick) as u64;
            current_tick.store(actual_tick, Ordering::Relaxed);
        }
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
    synth: Box<dyn Synth>,
    events: Vec<MidiEvent>,
    event_index: usize,
    sample_count: u64,
    samples_per_tick: f64,
    left_buf: Vec<f32>,
    right_buf: Vec<f32>,
    /// Remaining fade-in samples for seek-start pop suppression.
    ramp_samples: u64,
}

fn audio_callback(data: &mut [f32], state: &Arc<Mutex<PlaybackState>>) {
    let mut st = state.lock().unwrap();
    let half = data.len() / 2;
    if st.left_buf.len() != half {
        st.left_buf.resize(half, 0.0);
        st.right_buf.resize(half, 0.0);
    }

    let end_sample = st.sample_count + half as u64;
    let spt = st.samples_per_tick;
    while st.event_index < st.events.len() {
        let event_sample = (st.events[st.event_index].tick as f64 * spt) as u64;
        if event_sample > end_sample {
            break;
        }
        let ev = st.events[st.event_index].clone();
        st.synth.process_midi_message(
            ev.channel as i32,
            ev.command as i32,
            ev.data1 as i32,
            ev.data2 as i32,
        );
        st.event_index += 1;
    }

    let PlaybackState { synth, left_buf, right_buf, ramp_samples, .. } = &mut *st;
    synth.render(&mut left_buf[..half], &mut right_buf[..half]);

    let ramp = *ramp_samples;
    if ramp > 0 {
        let n = half.min(ramp as usize);
        for i in 0..n {
            let gain = i as f64 / ramp as f64;
            let gain = (gain * gain) as f32; // quadratic ease-out
            left_buf[i] *= gain;
            right_buf[i] *= gain;
        }
        *ramp_samples = ramp.saturating_sub(half as u64);
    }

    for i in 0..half {
        data[i * 2] = left_buf[i];
        data[i * 2 + 1] = right_buf[i];
    }

    st.sample_count = end_sample;
}
