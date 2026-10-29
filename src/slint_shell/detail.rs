//! Slint detail page state and view-model bridge.
//! Hexagonal invariant: pure in-memory with zero I/O, network, audio, or async.

use crate::domain::{Detail, DetailKind, Track};
use crate::i18n::{Locale, tr};

use super::search::{TrackDisplayRow, format_duration};

pub fn detail_kind_label(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Album => "Album",
        DetailKind::Artist => "Artist",
        DetailKind::Playlist => "Playlist",
    }
}

pub fn detail_kind_label_with_locale(kind: DetailKind, locale: Locale) -> &'static str {
    tr(locale, detail_kind_label(kind))
}

pub fn detail_track_rows(detail: &Detail) -> Vec<TrackDisplayRow> {
    detail
        .tracks
        .iter()
        .map(|t| TrackDisplayRow::new(&t.title, &t.artist, format_duration(t.duration_secs)))
        .collect()
}

/// Checks whether the detail view has its own header artwork URL.
pub fn detail_has_artwork(detail: &Detail) -> bool {
    detail.artwork.as_deref().is_some_and(|url| !url.trim().is_empty())
}

/// Resolves cover artwork URL for a detail view adhering to preference order:
/// 1. Detail header artwork (e.g. playlist or album header)
/// 2. First track's artwork
/// 3. None (renders default placeholder in UI)
pub fn resolve_detail_cover_url(detail: &Detail) -> Option<&str> {
    detail.artwork.as_deref().filter(|url| !url.trim().is_empty()).or_else(|| {
        detail
            .tracks
            .first()
            .and_then(|t| t.artwork.as_deref())
            .filter(|url| !url.trim().is_empty())
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DetailState {
    pub detail: Option<Detail>,
}

impl DetailState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_detail(detail: Detail) -> Self {
        Self { detail: Some(detail) }
    }

    pub fn set_detail(&mut self, detail: Detail) {
        self.detail = Some(detail);
    }

    pub fn clear(&mut self) {
        self.detail = None;
    }

    pub fn has_detail(&self) -> bool {
        self.detail.is_some()
    }

    pub fn detail(&self) -> Option<&Detail> {
        self.detail.as_ref()
    }

    pub fn selected_track(&self, index: usize) -> Option<&Track> {
        self.detail.as_ref()?.tracks.get(index)
    }

    pub fn header_strings(&self) -> (&str, &str, &str) {
        let Some(detail) = &self.detail else {
            return ("", "", "");
        };
        (detail.title.as_str(), detail.subtitle.as_str(), detail_kind_label(detail.kind))
    }

    pub fn detail_track_rows(&self) -> Vec<TrackDisplayRow> {
        match &self.detail {
            Some(detail) => detail_track_rows(detail),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canned_detail(kind: DetailKind) -> Detail {
        Detail::new(
            kind,
            "Canned Title",
            "Canned Subtitle",
            vec![
                Track::new("t1", "Track One", "Artist One").with_duration(185),
                Track::new("t2", "Track Two", "Artist Two"),
            ],
        )
    }

    #[test]
    fn test_detail_state_lifecycle_and_bounds() {
        let mut state = DetailState::new();
        assert!(!state.has_detail() && state.detail().is_none());
        assert_eq!(state.header_strings(), ("", "", ""));
        assert_eq!(state.selected_track(0), None);

        state.set_detail(canned_detail(DetailKind::Playlist));
        assert!(state.has_detail());
        assert_eq!(state.selected_track(0).map(|t| t.id.as_str()), Some("t1"));
        assert_eq!(state.selected_track(1).map(|t| t.id.as_str()), Some("t2"));
        assert_eq!(state.selected_track(2), None);

        state.clear();
        assert!(!state.has_detail() && state.detail().is_none());
        assert_eq!(state.header_strings(), ("", "", ""));
    }

    #[test]
    fn test_detail_header_strings_for_each_detail_kind() {
        let cases = [
            (DetailKind::Album, "Album", "ÁLBUM"),
            (DetailKind::Artist, "Artist", "ARTISTA"),
            (DetailKind::Playlist, "Playlist", "LISTA"),
        ];
        for (kind, expected_en, expected_es) in cases {
            assert_eq!(detail_kind_label(kind), expected_en);
            assert_eq!(detail_kind_label_with_locale(kind, Locale::English), expected_en);
            assert_eq!(detail_kind_label_with_locale(kind, Locale::Spanish), expected_es);
            let state = DetailState::with_detail(canned_detail(kind));
            assert_eq!(state.header_strings(), ("Canned Title", "Canned Subtitle", expected_en));
        }
    }

    #[test]
    fn test_detail_track_rows_match_domain_data() {
        let detail = canned_detail(DetailKind::Album);
        let rows = detail_track_rows(&detail);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], TrackDisplayRow::new("Track One", "Artist One", "3:05"));
        assert_eq!(rows[1], TrackDisplayRow::new("Track Two", "Artist Two", ""));

        let state = DetailState::with_detail(detail);
        assert_eq!(state.detail_track_rows(), rows);
    }

    #[test]
    fn test_resolve_detail_cover_url_preference_order() {
        let t1 =
            Track::new("t1", "Track 1", "Artist 1").with_artwork("https://img.local/track.jpg");
        let t2 =
            Track::new("t2", "Track 2", "Artist 2").with_artwork("https://img.local/track2.jpg");

        // 1. Header art > track art
        let with_both =
            Detail::new(DetailKind::Playlist, "Title", "Sub", vec![t1.clone(), t2.clone()])
                .with_artwork("https://img.local/header.jpg");
        assert_eq!(resolve_detail_cover_url(&with_both), Some("https://img.local/header.jpg"));

        // 2. Track art fallback when header art is None
        let with_track_only =
            Detail::new(DetailKind::Playlist, "Title", "Sub", vec![t1.clone(), t2.clone()]);
        assert_eq!(resolve_detail_cover_url(&with_track_only), Some("https://img.local/track.jpg"));

        // 3. Track art fallback when header art is blank string
        let with_blank_header =
            Detail::new(DetailKind::Playlist, "Title", "Sub", vec![t1.clone()]).with_artwork("   ");
        assert_eq!(
            resolve_detail_cover_url(&with_blank_header),
            Some("https://img.local/track.jpg")
        );

        // 4. Default None when neither has artwork
        let no_art_track = Track::new("t3", "Track 3", "Artist 3");
        let with_none = Detail::new(DetailKind::Album, "Title", "Sub", vec![no_art_track]);
        assert_eq!(resolve_detail_cover_url(&with_none), None);

        // 5. Empty tracks with header art
        let empty_tracks_with_header = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![])
            .with_artwork("https://img.local/header.jpg");
        assert_eq!(
            resolve_detail_cover_url(&empty_tracks_with_header),
            Some("https://img.local/header.jpg")
        );

        // 6. Empty tracks without header art
        let empty_tracks_no_header = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![]);
        assert_eq!(resolve_detail_cover_url(&empty_tracks_no_header), None);
    }

    #[test]
    fn test_detail_has_artwork() {
        let empty_track = Track::new("t1", "Track 1", "Artist 1");
        let with_art = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![empty_track.clone()])
            .with_artwork("https://img.local/header.jpg");
        assert!(detail_has_artwork(&with_art));

        let without_art =
            Detail::new(DetailKind::Playlist, "Title", "Sub", vec![empty_track.clone()]);
        assert!(!detail_has_artwork(&without_art));

        let with_blank = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![empty_track])
            .with_artwork("   ");
        assert!(!detail_has_artwork(&with_blank));
    }
}
