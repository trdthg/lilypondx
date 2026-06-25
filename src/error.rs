use thiserror::Error;

#[derive(Error, Debug)]
pub enum LilypondxError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse markdown: {0}")]
    MarkdownParse(String),

    #[error("Failed to parse YAML frontmatter: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("LilyPond compilation failed: {0}")]
    LilypondCompile(String),

    #[error("MIDI parse error: {0}")]
    MidiParse(String),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("SoundFont error: {0}")]
    SoundFont(String),
}
