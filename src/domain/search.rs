//! Search models: immutable query and catalog hit types.

/// Category filter for catalog search queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFilter {
    /// All categories.
    #[default]
    All,
    /// Tracks only.
    Tracks,
    /// Albums only.
    Albums,
    /// Artists only.
    Artists,
    /// Playlists only.
    Playlists,
}

/// Validated search query (trimmed, non-empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    text: String,
    filter: SearchFilter,
}

impl SearchQuery {
    /// Parses raw input at the boundary; rejects blank queries.
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        Some(Self { text, filter: SearchFilter::All })
    }

    /// Query text (already trimmed).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Active category filter.
    pub fn filter(&self) -> SearchFilter {
        self.filter
    }

    /// Sets the category filter, consuming self.
    pub fn with_filter(mut self, filter: SearchFilter) -> Self {
        self.filter = filter;
        self
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
    /// Highest-resolution catalog thumbnail URL when known.
    pub artwork: Option<String>,
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
            artwork: None,
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
            artwork: None,
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

    /// Sets the optional artwork URL, consuming self.
    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        self.artwork = Some(artwork.into());
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
    fn query_filter_defaults_to_all_and_builder_updates_filter() {
        let query = SearchQuery::parse("rock").expect("valid query");
        assert_eq!(query.filter(), SearchFilter::All);

        let filtered = query.with_filter(SearchFilter::Albums);
        assert_eq!(filtered.filter(), SearchFilter::Albums);
        assert_eq!(filtered.text(), "rock");
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
        assert_eq!(base.artwork, None);

        let built =
            base.with_album("Album").with_duration(210).with_artwork("https://artwork.local/art");
        let full = Track::new_full("id", "Title", "Artist", Some("Album".to_owned()), Some(210))
            .with_artwork("https://artwork.local/art");
        assert_eq!(built, full);
        assert_eq!(built.album.as_deref(), Some("Album"));
        assert_eq!(built.duration_secs, Some(210));
        assert_eq!(built.artwork.as_deref(), Some("https://artwork.local/art"));
        assert_eq!(built.display(), "Title - Artist");
    }
}
