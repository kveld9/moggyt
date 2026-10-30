//! Search models: immutable query and catalog hit types.

use serde::{Deserialize, Serialize};

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

/// Maximum allowed length (in unicode characters) for a lyrics search snippet.
pub const MAX_LYRICS_SNIPPET_LEN: usize = 200;

/// Validation errors for a lyrics search snippet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsSnippetError {
    /// Snippet is empty or contains only whitespace.
    Empty,
    /// Snippet exceeds the maximum allowed length of [`MAX_LYRICS_SNIPPET_LEN`] characters.
    TooLong,
}

impl std::fmt::Display for LyricsSnippetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "lyrics snippet cannot be blank"),
            Self::TooLong => write!(
                f,
                "lyrics snippet exceeds maximum length of {MAX_LYRICS_SNIPPET_LEN} characters"
            ),
        }
    }
}

impl std::error::Error for LyricsSnippetError {}

/// Validated lyrics snippet for search-by-lyrics lookup (trimmed, non-empty, capped length).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LyricsSnippet(String);

impl LyricsSnippet {
    /// Parses and validates a raw lyrics snippet at the boundary.
    ///
    /// Trims leading and trailing whitespace. Rejects blank queries and
    /// queries exceeding [`MAX_LYRICS_SNIPPET_LEN`] characters.
    pub fn parse(raw: &str) -> Result<Self, LyricsSnippetError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(LyricsSnippetError::Empty);
        }
        if trimmed.chars().count() > MAX_LYRICS_SNIPPET_LEN {
            return Err(LyricsSnippetError::TooLong);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Snippet text (already trimmed).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Snippet text (already trimmed).
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for LyricsSnippet {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for LyricsSnippet {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for LyricsSnippet {
    type Error = LyricsSnippetError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// A catalog track hit: plain data, no behavior, no I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Human-facing one-line rendering (`title - artist`).
    pub fn display(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

/// Maximum number of recent search queries retained in history.
pub const RECENT_SEARCH_LIMIT: usize = 10;

/// Persistent recent search history capped at [`RECENT_SEARCH_LIMIT`] entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSearches(Vec<String>);

impl RecentSearches {
    /// Creates an empty recent search history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ordered entries (newest first).
    pub fn entries(&self) -> &[String] {
        &self.0
    }

    /// Whether history contains no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of retained entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Records a query into history.
    ///
    /// Trims the query, ignores blank input, drops any existing entry equal
    /// ignoring ASCII case, pushes the trimmed text at the front, and truncates
    /// to [`RECENT_SEARCH_LIMIT`].
    pub fn record(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }
        self.0.retain(|entry| !entry.eq_ignore_ascii_case(trimmed));
        self.0.insert(0, trimmed.to_owned());
        self.0.truncate(RECENT_SEARCH_LIMIT);
    }

    /// Removes an entry matching query ignoring ASCII case.
    ///
    /// Returns true if an entry was removed, false otherwise.
    pub fn remove(&mut self, query: &str) -> bool {
        let trimmed = query.trim();
        let initial_len = self.0.len();
        self.0.retain(|entry| !entry.eq_ignore_ascii_case(trimmed));
        self.0.len() < initial_len
    }

    /// Clears all recent searches.
    pub fn clear(&mut self) {
        self.0.clear();
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
    fn blank_lyrics_snippet_is_rejected() {
        assert_eq!(LyricsSnippet::parse(""), Err(LyricsSnippetError::Empty));
        assert_eq!(LyricsSnippet::parse("   "), Err(LyricsSnippetError::Empty));
        assert_eq!(LyricsSnippet::parse("\n\t  "), Err(LyricsSnippetError::Empty));
    }

    #[test]
    fn overlong_lyrics_snippet_is_rejected() {
        let exact = "a".repeat(MAX_LYRICS_SNIPPET_LEN);
        assert!(LyricsSnippet::parse(&exact).is_ok());

        let overlong = "a".repeat(MAX_LYRICS_SNIPPET_LEN + 1);
        assert_eq!(LyricsSnippet::parse(&overlong), Err(LyricsSnippetError::TooLong));
    }

    #[test]
    fn lyrics_snippet_is_trimmed() {
        let snippet = LyricsSnippet::parse("  city lights glowing  ").expect("valid snippet");
        assert_eq!(snippet.as_str(), "city lights glowing");
        assert_eq!(snippet.text(), "city lights glowing");
        assert_eq!(&*snippet, "city lights glowing");
        assert_eq!(snippet.as_ref(), "city lights glowing");
    }

    #[test]
    fn lyrics_snippet_error_display_and_trait() {
        assert_eq!(LyricsSnippetError::Empty.to_string(), "lyrics snippet cannot be blank");
        assert_eq!(
            LyricsSnippetError::TooLong.to_string(),
            format!("lyrics snippet exceeds maximum length of {MAX_LYRICS_SNIPPET_LEN} characters")
        );
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

    #[test]
    fn track_serde_roundtrip() {
        let track = Track::new_full("id-1", "Title", "Artist", Some("Album".into()), Some(180))
            .with_artwork("https://artwork.local/1");
        let json = serde_json::to_string(&track).expect("serialize track");
        let deserialized: Track = serde_json::from_str(&json).expect("deserialize track");
        assert_eq!(track, deserialized);
    }

    #[test]
    fn recent_searches_blank_or_whitespace_ignored() {
        let mut recents = RecentSearches::new();
        recents.record("");
        recents.record("   ");
        recents.record("\t\n ");
        assert!(recents.is_empty());
        assert_eq!(recents.len(), 0);
        assert_eq!(recents.entries(), &[] as &[String]);
    }

    #[test]
    fn recent_searches_duplicate_moves_to_front_and_updates_casing() {
        let mut recents = RecentSearches::new();
        recents.record("Radiohead");
        recents.record("coldplay");
        assert_eq!(recents.entries(), &["coldplay", "Radiohead"]);

        // Duplicate with different case moves to front and keeps newest casing
        recents.record("RADIOHEAD");
        assert_eq!(recents.entries(), &["RADIOHEAD", "coldplay"]);
        assert_eq!(recents.len(), 2);
    }

    #[test]
    fn recent_searches_cap_drops_oldest() {
        let mut recents = RecentSearches::new();
        for i in 1..=12 {
            recents.record(&format!("query {i}"));
        }
        assert_eq!(recents.len(), RECENT_SEARCH_LIMIT);
        assert_eq!(recents.entries().first().map(|s| s.as_str()), Some("query 12"));
        assert_eq!(recents.entries().last().map(|s| s.as_str()), Some("query 3"));
    }

    #[test]
    fn recent_searches_remove_is_case_insensitive_and_reports_status() {
        let mut recents = RecentSearches::new();
        recents.record("Daft Punk");
        assert!(!recents.remove("nonexistent"));
        assert!(recents.remove("daft punk"));
        assert!(!recents.remove("DAFT PUNK"));
        assert!(recents.is_empty());
    }

    #[test]
    fn recent_searches_clear_empties_all() {
        let mut recents = RecentSearches::new();
        recents.record("Pink Floyd");
        assert_eq!(recents.len(), 1);
        recents.clear();
        assert!(recents.is_empty());
        assert_eq!(recents.len(), 0);
    }

    #[test]
    fn recent_searches_serde_roundtrip_and_missing_field_default() {
        let mut recents = RecentSearches::new();
        recents.record("Queen");
        recents.record("ABBA");
        let json = serde_json::to_string(&recents).expect("serialize recent searches");
        assert_eq!(json, r#"["ABBA","Queen"]"#);

        let deserialized: RecentSearches =
            serde_json::from_str(&json).expect("deserialize recent searches");
        assert_eq!(recents, deserialized);

        #[derive(Deserialize)]
        struct Container {
            #[serde(default)]
            history: RecentSearches,
        }
        let container: Container = serde_json::from_str("{}").expect("deserialize missing field");
        assert!(container.history.is_empty());
        assert_eq!(container.history.len(), 0);
    }
}
