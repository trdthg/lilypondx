//! Synthesizer backends: SoundFont (rustysynth) with a zero-dependency
//! built-in oscillator fallback so playback works out of the box.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::LilypondxError;

const SAMPLE_RATE: i32 = 44100;

/// Trait unifying the SoundFont and built-in synthesizers.
pub trait Synth: Send {
    fn process_midi_message(&mut self, channel: i32, command: i32, data1: i32, data2: i32);
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    fn name(&self) -> &'static str;
}

/// SoundFont-backed synthesizer (rustysynth).
struct SoundFontSynth(rustysynth::Synthesizer);

impl Synth for SoundFontSynth {
    fn process_midi_message(&mut self, ch: i32, cmd: i32, d1: i32, d2: i32) {
        self.0.process_midi_message(ch, cmd, d1, d2);
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.0.render(l, r);
    }
    fn name(&self) -> &'static str {
        "SoundFont"
    }
}

/// Zero-dependency built-in synthesizer: additive oscillator with ADSR envelope.
/// Used as fallback when no .sf2 file is found.
pub struct BuiltInSynth {
    sample_rate: i32,
    voices: Box<[Option<Voice>; 128]>,
}

struct Voice {
    velocity: f32,
    phase: f64,
    phase_inc: f64,
    note_on: bool,
    age: u64,
    release_age: u64,
}

impl BuiltInSynth {
    pub fn new(sample_rate: i32) -> Self {
        Self {
            sample_rate,
            voices: Box::new(std::array::from_fn(|_| None)),
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let freq = 440.0 * 2.0_f64.powf((pitch as f64 - 69.0) / 12.0);
        self.voices[pitch as usize] = Some(Voice {
            velocity: velocity as f32 / 127.0,
            phase: 0.0,
            phase_inc: freq / self.sample_rate as f64 * std::f64::consts::TAU,
            note_on: true,
            age: 0,
            release_age: 0,
        });
    }

    fn note_off(&mut self, pitch: u8) {
        if let Some(v) = &mut self.voices[pitch as usize] {
            v.note_on = false;
            v.release_age = 0;
        }
    }
}

impl Synth for BuiltInSynth {
    fn process_midi_message(&mut self, _channel: i32, command: i32, data1: i32, data2: i32) {
        match command {
            0x90 if data2 > 0 => self.note_on(data1 as u8, data2 as u8),
            0x80 | 0x90 => self.note_off(data1 as u8),
            _ => {}
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate as u64;
        let attack = (0.005 * sr as f64) as u64;
        let decay = (0.05 * sr as f64) as u64;
        let sustain: f64 = 0.5;
        let release = (0.1 * sr as f64) as u64;

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut sample = 0.0f32;
            for slot in self.voices.iter_mut() {
                let mut done = false;
                if let Some(v) = slot {
                    let amp = if v.note_on {
                        if v.age < attack {
                            v.age as f64 / attack.max(1) as f64
                        } else if v.age < attack + decay {
                            let t = (v.age - attack) as f64 / decay.max(1) as f64;
                            1.0 - t * (1.0 - sustain)
                        } else {
                            sustain
                        }
                    } else {
                        let t = v.release_age as f64 / release.max(1) as f64;
                        (sustain * (1.0 - t)).max(0.0)
                    };

                    if amp <= 0.0 && !v.note_on {
                        done = true;
                    } else {
                        let wave = v.phase.sin() as f32
                            + 0.3 * (2.0 * v.phase).sin() as f32
                            + 0.1 * (3.0 * v.phase).sin() as f32;
                        sample += wave * amp as f32 * v.velocity * 0.15;
                        v.phase += v.phase_inc;
                        if v.phase >= std::f64::consts::TAU {
                            v.phase -= std::f64::consts::TAU;
                        }
                        if v.note_on {
                            v.age += 1;
                        } else {
                            v.release_age += 1;
                        }
                    }
                }
                if done {
                    *slot = None;
                }
            }
            *l = sample;
            *r = sample;
        }
    }

    fn name(&self) -> &'static str {
        "Built-in"
    }
}

/// Find a SoundFont file, if available. Checks env var, bundled, and system locations.
pub fn find_soundfont() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LILYPONDX_SF2")
        && Path::new(&path).exists()
    {
        return Some(PathBuf::from(path));
    }
    let candidates = [
        Path::new("assets/tiny.sf2"),
        Path::new("/usr/share/sounds/sf2/FluidR3_GM.sf2"),
        Path::new("/usr/share/soundfonts/FluidR3_GM.sf2"),
        Path::new("C:/Windows/System32/Drivers/gm.dls"),
    ];
    candidates.iter().find(|p| p.exists()).map(|p| p.to_path_buf())
}

/// Create the best available synthesizer at the default sample rate.
pub fn create_synth() -> Result<Box<dyn Synth>, LilypondxError> {
    create_synth_at(SAMPLE_RATE)
}

/// Create a synthesizer at a specific sample rate (exposed for tests).
pub fn create_synth_at(sample_rate: i32) -> Result<Box<dyn Synth>, LilypondxError> {
    if let Some(path) = find_soundfont() {
        let data = std::fs::read(&path)?;
        let sf = rustysynth::SoundFont::new(&mut std::io::Cursor::new(&data))
            .map_err(|e| LilypondxError::SoundFont(format!("Failed to load SoundFont: {e}")))?;
        let settings = rustysynth::SynthesizerSettings::new(sample_rate);
        let synth = rustysynth::Synthesizer::new(&Arc::new(sf), &settings)
            .map_err(|e| LilypondxError::SoundFont(format!("Failed to create synthesizer: {e}")))?;
        return Ok(Box::new(SoundFontSynth(synth)));
    }
    Ok(Box::new(BuiltInSynth::new(sample_rate)))
}
