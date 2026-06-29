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

impl Synth for BuiltInSynth {
    fn process_midi_message(&mut self, _channel: i32, command: i32, data1: i32, data2: i32) {
        match command {
            0x90 if data2 > 0 => self.note_on(data1 as u8, data2 as u8),
            0x80 | 0x90 => self.note_off(data1 as u8),
            _ => {}
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        BuiltInSynth::render(self, left, right);
    }
    fn name(&self) -> &'static str {
        "Built-in"
    }
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
/// Uses a precomputed wavetable (LUT) + linear interpolation to avoid calling
/// `sin()` per-sample — critical for real-time performance with many voices.
/// Used as fallback when no .sf2 file is found.
pub struct BuiltInSynth {
    sample_rate: i32,
    voices: Box<[Option<Voice>; 128]>,
    active: Vec<u8>,
    /// Wavetable: one period of `sin(x) + 0.3·sin(2x) + 0.1·sin(3x)`,
    /// normalized to [-1, 1]. Indexed by `phase / TAU * LUT.len()`.
    wavetable: Box<[f32; LUT_SIZE]>,
}

/// Wavetable size — must be a power of 2 for fast modulo via bitmask.
const LUT_SIZE: usize = 2048;
const LUT_MASK: usize = LUT_SIZE - 1;

struct Voice {
    velocity: f32,
    /// Phase in [0, 1) — multiply by LUT_SIZE to get the table index.
    phase: f32,
    /// Phase increment per sample, in [0, 1).
    phase_inc: f32,
    note_on: bool,
    age: u64,
    release_age: u64,
}

fn build_wavetable() -> Box<[f32; LUT_SIZE]> {
    let mut tbl = Box::new([0.0f32; LUT_SIZE]);
    let norm = 1.0 + 0.3 + 0.1; // sum of harmonic amplitudes
    for i in 0..LUT_SIZE {
        let phase = i as f32 / LUT_SIZE as f32 * std::f32::consts::TAU;
        let wave = phase.sin() + 0.3 * (2.0 * phase).sin() + 0.1 * (3.0 * phase).sin();
        tbl[i] = wave / norm;
    }
    tbl
}

impl BuiltInSynth {
    pub fn new(sample_rate: i32) -> Self {
        Self {
            sample_rate,
            voices: Box::new(std::array::from_fn(|_| None)),
            active: Vec::new(),
            wavetable: build_wavetable(),
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let freq = 440.0 * 2.0_f32.powf((pitch as f32 - 69.0) / 12.0);
        if self.voices[pitch as usize].is_none() {
            self.active.push(pitch);
        }
        self.voices[pitch as usize] = Some(Voice {
            velocity: velocity as f32 / 127.0,
            phase: 0.0,
            phase_inc: freq / self.sample_rate as f32,
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

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate as u64;
        let attack = (0.005 * sr as f64) as u64;
        let decay = (0.05 * sr as f64) as u64;
        let sustain: f32 = 0.5;
        let release = (0.1 * sr as f64) as u64;

        // Snapshot the active voice indices so we can iterate without
        // aliasing self.active and self.voices.
        let active: Vec<u8> = self.active.clone();
        let mut finished: Vec<u8> = Vec::new();
        // Borrow the wavetable once as a raw pointer so we can read it inside
        // the loop without re-borrowing `self` (which conflicts with the
        // mutable borrow on `self.voices`).
        let lut: *const [f32; LUT_SIZE] = &*self.wavetable;

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut sample = 0.0f32;
            for &pitch in &active {
                let Some(v) = &mut self.voices[pitch as usize] else { continue };
                let amp: f32 = if v.note_on {
                    if v.age < attack {
                        v.age as f32 / attack.max(1) as f32
                    } else if v.age < attack + decay {
                        let t = (v.age - attack) as f32 / decay.max(1) as f32;
                        1.0 - t * (1.0 - sustain)
                    } else {
                        sustain
                    }
                } else {
                    let t = v.release_age as f32 / release.max(1) as f32;
                    (sustain * (1.0 - t)).max(0.0)
                };

                if amp <= 0.0 && !v.note_on {
                    finished.push(pitch);
                } else {
                    // Inline wavetable lookup (can't call self.method here due
                    // to the mutable borrow on self.voices).
                    let pos = v.phase * LUT_SIZE as f32;
                    let i0 = pos as usize & LUT_MASK;
                    let i1 = (i0 + 1) & LUT_MASK;
                    let frac = pos - pos.floor();
                    // SAFETY: `lut` is a valid pointer to our wavetable, which
                    // is not mutated during render. Indices are masked to
                    // LUT_SIZE, so always in bounds.
                    let wave = unsafe {
                        let a = (*lut)[i0];
                        let b = (*lut)[i1];
                        a + (b - a) * frac
                    };
                    sample += wave * amp * v.velocity * 0.15;
                    v.phase += v.phase_inc;
                    if v.phase >= 1.0 {
                        v.phase -= 1.0;
                    }
                    if v.note_on {
                        v.age += 1;
                    } else {
                        v.release_age += 1;
                    }
                }
            }
            *l = sample;
            *r = sample;
        }

        if !finished.is_empty() {
            self.active.retain(|p| !finished.contains(p));
            for p in &finished {
                self.voices[*p as usize] = None;
            }
        }
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
