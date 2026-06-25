use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::error::LilypondxError;
use crate::score::{Score, ScoreMetadata, Track};

/// Parse a Markdown file into a `Score`.
pub fn parse_markdown(path: &Path) -> Result<Score, LilypondxError> {
    let content = std::fs::read_to_string(path)?;
    parse_markdown_str(&content, path)
}

fn parse_markdown_str(content: &str, source: &Path) -> Result<Score, LilypondxError> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

    let parser = Parser::new_ext(content, options);

    // Collect events to iterate non-consuming
    let events: Vec<Event> = parser.collect();

    let metadata = extract_frontmatter(&events)?;
    let tracks = extract_tracks(&events)?;

    Ok(Score {
        metadata,
        tracks,
        source: source.to_path_buf(),
    })
}

/// Extract YAML frontmatter from the first `---` delimited block in the markdown events.
fn extract_frontmatter(events: &[Event]) -> Result<ScoreMetadata, LilypondxError> {
    let mut in_metadata = false;
    let mut yaml = String::new();

    for event in events {
        match event {
            Event::Start(Tag::MetadataBlock(_)) => {
                in_metadata = true;
            }
            Event::Text(text) if in_metadata => {
                yaml.push_str(text.as_ref());
            }
            Event::End(TagEnd::MetadataBlock(_)) if in_metadata => {
                if !yaml.is_empty() {
                    let meta: ScoreMetadata = serde_yaml::from_str(&yaml)?;
                    return Ok(meta);
                }
                break;
            }
            _ => {}
        }
    }
    Ok(ScoreMetadata::default())
}

/// Extract lilypond code blocks with track attributes.
fn extract_tracks(events: &[Event]) -> Result<Vec<Track>, LilypondxError> {
    let mut tracks = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                // Determine syntax variant
                let (syntax, rest) = if let Some(r) = info.strip_prefix("lilypond-test") {
                    ("test", r)
                } else if let Some(r) = info.strip_prefix("lilypondx") {
                    ("lilypondx", r)
                } else if let Some(r) = info.strip_prefix("lilypond") {
                    ("lilypond", r)
                } else {
                    i += 1;
                    continue;
                };

                let attrs = parse_code_block_attrs(rest);
                let name = attrs
                    .get("track")
                    .cloned()
                    .unwrap_or_else(|| format!("track{}", tracks.len() + 1));
                let clef = attrs.get("clef").cloned().unwrap_or_else(|| "treble".into());
                let relative = attrs
                    .get("relative")
                    .cloned()
                    .unwrap_or_else(|| "c'".into());
                let midi_instrument = attrs.get("midi_instrument").cloned();

                // Collect text events until the End
                let mut notes = String::new();
                let mut j = i + 1;
                while j < events.len() {
                    match &events[j] {
                        Event::Text(text) => {
                            notes.push_str(text.as_ref());
                        }
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                    j += 1;
                }

                tracks.push(Track {
                    name,
                    clef,
                    relative,
                    midi_instrument,
                    notes: notes.trim().to_string(),
                    syntax: syntax.to_string(),
                });
            }
            _ => {}
        }
        i += 1;
    }
    Ok(tracks)
}

/// Parse key=value pairs from the code block info string after "lilypond".
/// e.g. "track=RH clef=treble relative=c'''" → {"track": "RH", "clef": "treble", "relative": "c'''"}
fn parse_code_block_attrs(s: &str) -> std::collections::HashMap<String, String> {
    let mut attrs = std::collections::HashMap::new();
    for part in s.split_whitespace() {
        if let Some((k, v)) = part.split_once('=') {
            attrs.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;


    #[test]
    fn test_parse_first_md() {
        let path = PathBuf::from("tests/claire_of_glass.md");
        let score = parse_markdown(&path).expect("should parse claire_of_glass.md");
        assert_eq!(score.metadata.title, "ガラスのクレア");
        assert_eq!(score.metadata.composer.as_deref(), Some("青木望"));
        assert_eq!(score.metadata.tempo.as_deref(), Some("4 = 70"));
        assert_eq!(score.tracks.len(), 2);
        assert_eq!(score.tracks[0].name, "RH");
        assert_eq!(score.tracks[0].clef, "treble");
        assert_eq!(score.tracks[0].relative, "c");
        assert_eq!(score.tracks[1].name, "LH");
        assert_eq!(score.tracks[1].clef, "bass");
        assert_eq!(score.tracks[1].relative, "c,");
    }
}
