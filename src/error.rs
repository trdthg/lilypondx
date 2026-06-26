use thiserror::Error;

#[derive(Error, Debug)]
pub enum LilypondxError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse YAML frontmatter: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("SoundFont error: {0}")]
    SoundFont(String),

    #[error("lilypond command not found. Install LilyPond or use `lilypondx` syntax blocks")]
    LilypondNotFound,

    #[error("LilyPond compilation failed:\n{0}")]
    LilypondError(String),

    #[error("MIDI file parse error: {0}")]
    MidiParse(String),
}
