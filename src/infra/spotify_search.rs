//! YouTube Music InnerTube search client restricted to songs.
//!
//! Maps search results to [`SongCandidate`] and [`MatchCandidate`] for Spotify imports.

use anyhow::Context;
use serde_json::Value;

use super::catalog_parse_common::{best_thumbnail, flex_text, parse_duration_secs};
use super::catalog_requests::{client_context, search_url};
use crate::domain::{MatchCandidate, MusicVideoType};
use crate::i18n::Locale;

/// Typed failure when searching songs on InnerTube.
#[derive(Debug)]
pub enum SearchFailure {
    RateLimited,
    Unavailable(anyhow::Error),
}

impl std::fmt::Display for SearchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited => write!(f, "rate limited"),
            Self::Unavailable(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SearchFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RateLimited => None,
            Self::Unavailable(err) => Some(err.as_ref()),
        }
    }
}

/// Filter params restricting YouTube Music InnerTube search to songs.
pub const SONGS_FILTER_PARAMS: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";

/// Timeout in seconds for songs search requests.
pub const SEARCH_TIMEOUT_SECS: u64 = 20;

/// Builds the minimal WEB_REMIX search payload for `query` with Songs filter params.
pub fn songs_search_body(query: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "query": query,
        "params": SONGS_FILTER_PARAMS,
    })
}

/// Status classification of an HTTP response from InnerTube.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStatus {
    Ok,
    RateLimited,
    ClientError,
    ServerError,
}

/// Classifies an HTTP status code into a [`SearchStatus`].
pub fn classify_status(status: u16) -> SearchStatus {
    match status {
        429 | 503 => SearchStatus::RateLimited,
        400..=499 => SearchStatus::ClientError,
        500..=599 => SearchStatus::ServerError,
        _ => SearchStatus::Ok,
    }
}

/// Classifies an HTTP status code into an optional [`SearchFailure`].
pub fn classify_failure(status: u16) -> Option<SearchFailure> {
    match classify_status(status) {
        SearchStatus::Ok => None,
        SearchStatus::RateLimited => Some(SearchFailure::RateLimited),
        SearchStatus::ClientError | SearchStatus::ServerError => {
            Some(SearchFailure::Unavailable(anyhow::anyhow!("search failed with HTTP {status}")))
        }
    }
}

/// A song hit parsed from YouTube Music search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongCandidate {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
    pub thumb_url: Option<String>,
    pub video_type: MusicVideoType,
}

impl SongCandidate {
    /// Maps this song candidate to a domain [`MatchCandidate`].
    pub fn to_match_candidate(&self) -> MatchCandidate {
        MatchCandidate {
            title: self.title.clone(),
            channel: self.artist.clone(),
            duration_secs: self.duration_secs,
            video_type: self.video_type,
        }
    }
}

fn non_empty_trimmed(s: Option<&str>) -> Option<String> {
    let trimmed = s?.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn extract_video_id(renderer: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/playlistItemData/videoId",
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId",
        "/navigationEndpoint/watchEndpoint/videoId",
    ];
    for path in PATHS {
        if let Some(id) = non_empty_trimmed(renderer.pointer(path).and_then(Value::as_str)) {
            return Some(id);
        }
    }
    None
}

/// Maps an InnerTube `musicVideoType` string to a [`MusicVideoType`].
pub fn video_type_from(raw: Option<&str>) -> MusicVideoType {
    let Some(raw) = raw else {
        return MusicVideoType::Other;
    };
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "MUSIC_VIDEO_TYPE_ATV" | "ATV" => MusicVideoType::Atv,
        "MUSIC_VIDEO_TYPE_OMV" | "OMV" => MusicVideoType::Omv,
        "MUSIC_VIDEO_TYPE_UGC" | "UGC" => MusicVideoType::Ugc,
        _ => MusicVideoType::Other,
    }
}

fn extract_video_type(renderer: &Value) -> MusicVideoType {
    const PATHS: &[&str] = &[
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType",
        "/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType",
    ];
    let raw = PATHS.iter().find_map(|p| renderer.pointer(p).and_then(Value::as_str));
    video_type_from(raw)
}

fn extract_artist(runs: &[Value]) -> String {
    runs.first()
        .and_then(|r| r.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}

fn extract_duration(runs: &[Value]) -> Option<u32> {
    for run in runs.iter().rev() {
        if let Some(text) = run.get("text").and_then(Value::as_str) {
            if let Some(secs) = parse_duration_secs(text) {
                return Some(secs);
            }
        }
    }
    None
}

fn is_album_run(run: &Value) -> bool {
    run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .is_some_and(|pt| pt == "MUSIC_PAGE_TYPE_ALBUM")
}

fn is_artist_run(run: &Value) -> bool {
    run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .is_some_and(|pt| pt == "MUSIC_PAGE_TYPE_ARTIST")
}

fn is_separator(text: &str) -> bool {
    let t = text.trim();
    t == "\u{2022}" || t == "\u{00b7}" || t == "&"
}

fn fallback_album(runs: &[Value]) -> Option<String> {
    for run in &runs[1..] {
        if is_artist_run(run) {
            continue;
        }
        let Some(text) = run.get("text").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if text.is_empty() || is_separator(text) || parse_duration_secs(text).is_some() {
            continue;
        }
        return Some(text.to_owned());
    }
    None
}

fn extract_album(runs: &[Value]) -> Option<String> {
    if runs.len() <= 1 {
        return None;
    }
    for run in &runs[1..] {
        if is_album_run(run) {
            if let Some(text) = run.get("text").and_then(Value::as_str).map(str::trim) {
                if !text.is_empty() {
                    return Some(text.to_owned());
                }
            }
        }
    }
    fallback_album(runs)
}

fn parse_song_item(item: &Value) -> Option<SongCandidate> {
    let renderer = item.get("musicResponsiveListItemRenderer").unwrap_or(item);
    let video_id = extract_video_id(renderer)?;
    let title = flex_text(renderer, 0)?;
    let thumb_url = best_thumbnail(renderer);
    let video_type = extract_video_type(renderer);

    let empty = [];
    let runs = renderer
        .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&empty);

    let artist = extract_artist(runs);
    let album = extract_album(runs);
    let duration_secs = extract_duration(runs);

    Some(SongCandidate { video_id, title, artist, album, duration_secs, thumb_url, video_type })
}

fn section_shelf_contents(section: &Value) -> Option<&[Value]> {
    section.pointer("/musicShelfRenderer/contents").and_then(Value::as_array).map(Vec::as_slice)
}

fn tab_shelf_contents(tab: &Value) -> Option<&[Value]> {
    let sections = tab.pointer("/tabRenderer/content/sectionListRenderer/contents")?.as_array()?;
    sections.iter().find_map(section_shelf_contents)
}

fn find_music_shelf_contents(value: &Value) -> Option<&[Value]> {
    if let Some(c) = value.pointer("/musicShelfRenderer/contents").and_then(Value::as_array) {
        return Some(c.as_slice());
    }
    let tabs = value.pointer("/contents/tabbedSearchResultsRenderer/tabs")?.as_array()?;
    tabs.iter().find_map(tab_shelf_contents)
}

/// Pure parser that extracts song candidates from an InnerTube search response payload.
pub fn parse_song_candidates(value: &Value) -> Vec<SongCandidate> {
    let Some(items) = find_music_shelf_contents(value) else {
        return Vec::new();
    };
    items.iter().filter_map(parse_song_item).collect()
}

/// Client for searching YouTube Music restricted to songs.
#[derive(Clone, Debug)]
pub struct SpotifySongSearch {
    client: reqwest::blocking::Client,
}

impl SpotifySongSearch {
    /// Creates a new search client with default user agent and timeout.
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .user_agent(format!("moggyt/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client }
    }

    /// Searches YouTube Music for `query` restricted to songs.
    pub fn search(&self, query: &str, locale: Locale) -> Result<Vec<SongCandidate>, SearchFailure> {
        let url = search_url();
        let body = songs_search_body(query, locale);
        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .context("transport failure during songs search")
            .map_err(SearchFailure::Unavailable)?;
        let status = response.status();
        let code = status.as_u16();
        if let Some(failure) = classify_failure(code) {
            return Err(failure);
        }
        let payload: Value = response
            .json()
            .context("failed to parse InnerTube search response as JSON")
            .map_err(SearchFailure::Unavailable)?;
        Ok(parse_song_candidates(&payload))
    }
}

impl Default for SpotifySongSearch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixture_parsing() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("catalog_search_songs.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let candidates = parse_song_candidates(&payload);
        assert_eq!(candidates.len(), 5, "fixture must contain exactly 5 candidates");

        let first = &candidates[0];
        assert_eq!(first.title, "Loser");
        assert_eq!(first.artist, "Tame Impala");
        assert_eq!(first.album, Some("Deadbeat".to_owned()));
        assert_eq!(first.duration_secs, Some(224));
        assert!(!first.video_id.is_empty());
        assert_eq!(first.video_type, MusicVideoType::Atv);

        for (i, c) in candidates.iter().enumerate() {
            assert!(!c.title.is_empty(), "candidate {i} title must not be empty");
            assert!(!c.video_id.is_empty(), "candidate {i} video_id must not be empty");
            assert!(c.duration_secs.is_some(), "candidate {i} duration must parse to seconds");
        }
    }

    #[test]
    fn test_video_type_from_variants() {
        assert_eq!(video_type_from(Some("MUSIC_VIDEO_TYPE_OMV")), MusicVideoType::Omv);
        assert_eq!(video_type_from(Some("MUSIC_VIDEO_TYPE_UGC")), MusicVideoType::Ugc);
        assert_eq!(video_type_from(Some("MUSIC_VIDEO_TYPE_ATV")), MusicVideoType::Atv);
        assert_eq!(video_type_from(None), MusicVideoType::Other);
        assert_eq!(video_type_from(Some("UNKNOWN_STRING")), MusicVideoType::Other);
        assert_eq!(video_type_from(Some("music_video_type_omv")), MusicVideoType::Omv);
        assert_eq!(video_type_from(Some("music_video_type_ugc")), MusicVideoType::Ugc);
        assert_eq!(video_type_from(Some("music_video_type_atv")), MusicVideoType::Atv);
        assert_eq!(video_type_from(Some("atv")), MusicVideoType::Atv);
    }

    #[test]
    fn test_parse_song_candidates_skips_and_tolerance() {
        let missing_title = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicShelfRenderer": {
                                            "contents": [{
                                                "musicResponsiveListItemRenderer": {
                                                    "playlistItemData": { "videoId": "v123" },
                                                    "flexColumns": []
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert!(parse_song_candidates(&missing_title).is_empty());

        let missing_video_id = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicShelfRenderer": {
                                            "contents": [{
                                                "musicResponsiveListItemRenderer": {
                                                    "flexColumns": [{
                                                        "musicResponsiveListItemFlexColumnRenderer": {
                                                            "text": { "runs": [{ "text": "Song Title" }] }
                                                        }
                                                    }]
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert!(parse_song_candidates(&missing_video_id).is_empty());

        let empty_contents = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicShelfRenderer": {
                                            "contents": []
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert!(parse_song_candidates(&empty_contents).is_empty());

        let no_shelf = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": []
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert!(parse_song_candidates(&no_shelf).is_empty());

        let malformed = serde_json::json!({});
        assert!(parse_song_candidates(&malformed).is_empty());
    }

    #[test]
    fn test_songs_search_body_structure() {
        let body = songs_search_body("Tame Impala Loser", Locale::Spanish);
        assert_eq!(body["query"], "Tame Impala Loser");
        assert_eq!(body["params"], SONGS_FILTER_PARAMS);
        assert_eq!(body["context"]["client"]["hl"], "es");
    }

    #[test]
    fn test_classify_status_table() {
        let cases = [
            (200, SearchStatus::Ok),
            (204, SearchStatus::Ok),
            (400, SearchStatus::ClientError),
            (403, SearchStatus::ClientError),
            (404, SearchStatus::ClientError),
            (429, SearchStatus::RateLimited),
            (500, SearchStatus::ServerError),
            (503, SearchStatus::RateLimited),
            (504, SearchStatus::ServerError),
        ];
        for (status, expected) in cases {
            assert_eq!(
                classify_status(status),
                expected,
                "status {status} should classify as {expected:?}"
            );
        }
    }

    #[test]
    fn test_to_match_candidate_mapping() {
        let candidate = SongCandidate {
            video_id: "test-vid".to_owned(),
            title: "Test Song".to_owned(),
            artist: "Test Artist".to_owned(),
            album: Some("Test Album".to_owned()),
            duration_secs: Some(180),
            thumb_url: Some("https://example.com/thumb.jpg".to_owned()),
            video_type: MusicVideoType::Atv,
        };
        let match_candidate = candidate.to_match_candidate();
        assert_eq!(match_candidate.title, "Test Song");
        assert_eq!(match_candidate.channel, "Test Artist");
        assert_eq!(match_candidate.duration_secs, Some(180));
        assert_eq!(match_candidate.video_type, MusicVideoType::Atv);
    }

    #[test]
    fn test_classify_failure_variants() {
        assert!(classify_failure(200).is_none());
        assert!(matches!(classify_failure(429), Some(SearchFailure::RateLimited)));
        assert!(matches!(classify_failure(503), Some(SearchFailure::RateLimited)));

        let err_400 = classify_failure(400);
        match err_400 {
            Some(SearchFailure::Unavailable(err)) => {
                assert!(err.to_string().contains("400"));
            }
            other => panic!("expected Unavailable for 400, got {other:?}"),
        }

        let err_500 = classify_failure(500);
        match err_500 {
            Some(SearchFailure::Unavailable(_)) => {}
            other => panic!("expected Unavailable for 500, got {other:?}"),
        }

        let rate_limited = SearchFailure::RateLimited;
        assert_eq!(rate_limited.to_string(), "rate limited");
        use std::error::Error;
        assert!(rate_limited.source().is_none());

        let unavail = SearchFailure::Unavailable(anyhow::anyhow!("backend unreachable"));
        assert!(unavail.source().is_some());
    }

    #[test]
    fn test_spotify_song_search_construction() {
        let searcher = SpotifySongSearch::new();
        assert!(format!("{searcher:?}").contains("SpotifySongSearch"));
        let default_searcher = SpotifySongSearch::default();
        assert!(format!("{default_searcher:?}").contains("SpotifySongSearch"));
    }
}
