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
    /// Transposition in semitones (e.g. 12 = up one octave, -12 = down).
    /// Applied to the generated `.ly` via `\transpose`.
    #[serde(default)]
    pub transpose: Option<i32>,
    /// Generate guitar tablature (TabStaff) alongside standard notation.
    #[serde(default)]
    pub tablature: bool,
    /// Part layout: "split" (each track separate) or "combined" (one staff).
    #[serde(default)]
    pub parts: Option<String>,
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
