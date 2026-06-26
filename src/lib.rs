pub mod audio;
pub mod error;
pub mod ly_gen;
pub mod note;
pub mod parser;
pub mod score;
pub mod sparkline;
pub mod synth;
pub mod tui;

/// Default MIDI resolution: ticks per quarter note.
pub const TICKS_PER_BEAT: u32 = 480;
