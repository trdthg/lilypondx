use std::path::PathBuf;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ScoreMetadata {
    pub title: String,
    #[serde(default)]
    pub composer: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub dedication: Option<String>,
    #[serde(default)]
    pub poet: Option<String>,
    #[serde(default)]
    pub tempo: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Track {
    /// Track identifier, e.g. "RH", "LH"
    pub name: String,
    /// Clef, e.g. "treble", "bass"
    pub clef: String,
    /// Relative pitch anchor, e.g. "c'''" or "c,"
    pub relative: String,
    /// LilyPond instrument name for MIDI, e.g. "acoustic grand", "acoustic guitar (nylon)"
    pub midi_instrument: Option<String>,
    /// Raw LilyPond note content from the code block
    pub notes: String,
    /// Syntax variant: "lilypond" = native pass-through, "lilypondx" = parsed by our engine
    pub syntax: String,
}

#[derive(Debug, Clone)]
pub struct Score {
    pub metadata: ScoreMetadata,
    pub tracks: Vec<Track>,
    /// Source file path (for watch mode)
    pub source: PathBuf,
}
