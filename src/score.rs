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
    pub name: String,
    pub clef: String,
    pub relative: String,
    pub midi_instrument: Option<String>,
    pub notes: String,
    /// "lilypond" = native pass-through, "lilypondx" = parsed, "test" = expected output
    pub syntax: String,
}

#[derive(Debug, Clone)]
pub struct Score {
    pub metadata: ScoreMetadata,
    pub tracks: Vec<Track>,
}
