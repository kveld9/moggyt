//! Search models: immutable query and catalog hit types.

/// Validated search query (trimmed, non-empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    text: String,
}

impl SearchQuery {
    /// Parses raw input at the boundary; rejects blank queries.
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        Some(Self { text })
    }

    /// Query text (already trimmed).
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A catalog track hit: plain data, no behavior, no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Stable catalog identifier.
    pub id: String,
    /// Track title.
    pub title: String,
    /// Artist display name.
    pub artist: String,
}

impl Track {
    /// Creates a track hit from its parts.
    pub fn new(id: &str, title: &str, artist: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), artist: artist.to_owned() }
    }

    /// Human-facing one-line rendering (`title — artist`).
    pub fn display(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_query_is_rejected() {
        assert_eq!(SearchQuery::parse("   "), None);
    }

    #[test]
    fn query_is_trimmed() {
        let query = SearchQuery::parse("  midnight  ").expect("valid query");
        assert_eq!(query.text(), "midnight");
    }

    #[test]
    fn display_joins_title_and_artist() {
        let track = Track::new("id", "Midnight", "Night Drive");
        assert_eq!(track.display(), "Midnight - Night Drive");
    }
}
