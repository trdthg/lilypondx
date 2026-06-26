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
}
