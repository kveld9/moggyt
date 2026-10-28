//! Slint detail page state and view-model bridge.
//! Hexagonal invariant: pure in-memory with zero I/O, network, audio, or async.

use crate::domain::{Detail, DetailKind, Track};

use super::search::{TrackDisplayRow, format_duration};

pub fn detail_kind_label(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Album => "Album",
        DetailKind::Artist => "Artist",
        DetailKind::Playlist => "Playlist",
    }
}

pub fn detail_track_rows(detail: &Detail) -> Vec<TrackDisplayRow> {
    detail
        .tracks
        .iter()
        .map(|t| TrackDisplayRow::new(&t.title, &t.artist, format_duration(t.duration_secs)))
        .collect()
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
            (DetailKind::Album, "Album"),
            (DetailKind::Artist, "Artist"),
            (DetailKind::Playlist, "Playlist"),
        ];
        for (kind, expected) in cases {
            assert_eq!(detail_kind_label(kind), expected);
            let state = DetailState::with_detail(canned_detail(kind));
            assert_eq!(state.header_strings(), ("Canned Title", "Canned Subtitle", expected));
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
}
