//! Album responsive header parser extracting credited artists and release label.

use super::catalog_parse_detail::{detail_header_node, join_runs};
use super::catalog_parse_playlist::playlist_meta;
use serde_json::Value;

const STRAPLINE_PATHS: &[&str] = &[
    "/straplineTextOne/runs",
    "/musicResponsiveHeaderRenderer/straplineTextOne/runs",
    "/header/musicResponsiveHeaderRenderer/straplineTextOne/runs",
];

const SUBTITLE_PATHS: &[&str] = &[
    "/subtitle/runs",
    "/musicResponsiveHeaderRenderer/subtitle/runs",
    "/header/musicResponsiveHeaderRenderer/subtitle/runs",
];

/// Header metadata extracted from an album browse payload.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AlbumHeader {
    /// Credited artists line carried by `straplineTextOne`.
    pub artists: Option<String>,
    /// Release type label and year carried by header `subtitle`.
    pub container_label: Option<String>,
    /// Total number of tracks the album carries, as reported by the server.
    pub track_count: Option<u32>,
    /// Total playback duration in seconds, as reported by the server.
    pub total_duration_secs: Option<u32>,
}

fn runs_from(header: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        if let Some(runs) = header.pointer(path).and_then(Value::as_array) {
            if let Some(text) = join_runs(runs) {
                return Some(text);
            }
        }
    }
    None
}

/// Parses the credited artists and container release label from an album header.
///
/// Carries aggregate track count and duration even when `straplineTextOne` is absent or blank.
pub fn album_header(value: &Value) -> AlbumHeader {
    let Some(header) = detail_header_node(value) else {
        return AlbumHeader::default();
    };
    let meta = playlist_meta(Some(header));
    let Some(artists) = runs_from(header, STRAPLINE_PATHS) else {
        return AlbumHeader {
            artists: None,
            container_label: None,
            track_count: meta.track_count,
            total_duration_secs: meta.total_duration_secs,
        };
    };
    let container_label = runs_from(header, SUBTITLE_PATHS).filter(|label| label != &artists);
    AlbumHeader {
        artists: Some(artists),
        container_label,
        track_count: meta.track_count,
        total_duration_secs: meta.total_duration_secs,
    }
}

#[cfg(test)]
#[path = "catalog_parse_album_header_tests.rs"]
mod tests;
