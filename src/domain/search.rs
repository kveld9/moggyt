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
    /// Optional album name.
    pub album: Option<String>,
    /// Optional track duration in seconds.
    pub duration_secs: Option<u32>,
}

impl Track {
    /// Creates a track hit from its parts with absent optional fields.
    pub fn new(id: &str, title: &str, artist: &str) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: None,
            duration_secs: None,
        }
    }

    /// Creates a track hit with all fields specified.
    pub fn new_full(
        id: &str,
        title: &str,
        artist: &str,
        album: Option<String>,
        duration_secs: Option<u32>,
    ) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album,
            duration_secs,
        }
    }

    /// Sets the optional album name, consuming self.
    pub fn with_album(mut self, album: impl Into<String>) -> Self {
        self.album = Some(album.into());
        self
    }

    /// Sets the optional duration in seconds, consuming self.
    pub fn with_duration(mut self, duration_secs: u32) -> Self {
        self.duration_secs = Some(duration_secs);
        self
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

    #[test]
    fn track_evolution_and_builders() {
        let base = Track::new("id", "Title", "Artist");
        assert_eq!(base.album, None);
        assert_eq!(base.duration_secs, None);

        let built = base.with_album("Album").with_duration(210);
        let full = Track::new_full("id", "Title", "Artist", Some("Album".to_owned()), Some(210));
        assert_eq!(built, full);
        assert_eq!(built.album.as_deref(), Some("Album"));
        assert_eq!(built.duration_secs, Some(210));
        assert_eq!(built.display(), "Title - Artist");
    }
}
