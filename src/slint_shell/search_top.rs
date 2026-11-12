//! Top-result card view model and window synchronization for catalog search.

use crate::domain::SearchResult;
use crate::i18n::tr;
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::TopResultData;
use crate::slint_shell::covers::resolve_row_cover_image;
use crate::slint_shell::search::SearchRow;
use crate::slint_shell::state::SlintAppState;

/// Returns the artwork URL of a catalog hit, when present.
pub(crate) fn artwork_url(item: &SearchResult) -> Option<&str> {
    match item {
        SearchResult::Track(track) => track.artwork.as_deref(),
        SearchResult::Album(album) => album.artwork.as_deref(),
        SearchResult::Artist(artist) => artist.artwork.as_deref(),
        SearchResult::Playlist(playlist) => playlist.artwork.as_deref(),
    }
}

fn kind_label(state: &SlintAppState, item: &SearchResult) -> &'static str {
    match item {
        SearchResult::Track(_) => tr(state.locale, "Song"),
        SearchResult::Album(_) => tr(state.locale, "Album"),
        SearchResult::Artist(_) => tr(state.locale, "Artist"),
        SearchResult::Playlist(_) => tr(state.locale, "Playlist"),
    }
}

fn join_detail(label: &str, detail: &str) -> String {
    if detail.trim().is_empty() { label.to_owned() } else { format!("{label} - {detail}") }
}

fn subtitle_for(state: &SlintAppState, item: &SearchResult) -> String {
    let label = kind_label(state, item);
    match item {
        SearchResult::Track(track) => join_detail(label, &track.artist),
        SearchResult::Album(album) => join_detail(label, &album.artist),
        SearchResult::Artist(_) | SearchResult::Playlist(_) => label.to_owned(),
    }
}

/// Projects the current top-result hit into the Slint card data structure.
pub(crate) fn top_result_data(state: &SlintAppState) -> TopResultData {
    let Some(item) = state.search_top_result.as_ref() else {
        return TopResultData {
            has_result: false,
            heading: "".into(),
            title: "".into(),
            subtitle: "".into(),
            has_cover: false,
            cover: slint::Image::default(),
            circular: false,
        };
    };
    let row = SearchRow::from(item);
    TopResultData {
        has_result: true,
        heading: tr(state.locale, "Top result").into(),
        title: row.title.into(),
        subtitle: subtitle_for(state, item).into(),
        has_cover: row.artwork_url.is_some(),
        cover: resolve_row_cover_image(row.artwork_url.as_deref(), &state.search_covers),
        circular: matches!(item, SearchResult::Artist(_)),
    }
}

/// Pushes the top-result card data into the window properties.
pub(crate) fn sync_top_result(window: &MoggytWindow, state: &SlintAppState) {
    window.set_top_result(top_result_data(state));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, Track};
    use crate::i18n::Locale;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn top_result_data_is_hidden_without_a_match() {
        let state = test_wired();
        let data = top_result_data(&state);
        assert!(!data.has_result);
        assert_eq!(data.title.as_str(), "");
        assert!(!data.circular);
    }

    #[test]
    fn top_result_data_projects_track_album_and_artist() {
        let mut state = test_wired();
        state.search_top_result =
            Some(SearchResult::Track(Track::new("t1", "Celoso", "Santa Fe Klan")));
        let track_data = top_result_data(&state);
        assert!(track_data.has_result);
        assert_eq!(track_data.heading.as_str(), "Top result");
        assert_eq!(track_data.title.as_str(), "Celoso");
        assert_eq!(track_data.subtitle.as_str(), "Song - Santa Fe Klan");
        assert!(!track_data.circular);

        state.search_top_result =
            Some(SearchResult::Album(Album::new("a1", "Album", "Artist", None)));
        let album_data = top_result_data(&state);
        assert_eq!(album_data.subtitle.as_str(), "Album - Artist");
        assert!(!album_data.circular);

        state.search_top_result =
            Some(SearchResult::Artist(Artist::new("ar1", "Los Angeles", None)));
        let artist_data = top_result_data(&state);
        assert_eq!(artist_data.subtitle.as_str(), "Artist");
        assert!(artist_data.circular);
    }

    #[test]
    fn top_result_subtitle_uses_the_locale() {
        let mut state = test_wired();
        state.locale = Locale::Spanish;
        state.search_top_result = Some(SearchResult::Track(Track::new("t1", "Celoso", "")));
        let data = top_result_data(&state);
        assert_eq!(data.heading.as_str(), "Resultado principal");
        assert_eq!(data.subtitle.as_str(), "CANCIÓN");
    }

    #[test]
    fn blank_artist_keeps_only_the_kind_label() {
        let mut state = test_wired();
        state.search_top_result = Some(SearchResult::Track(Track::new("t1", "Celoso", "  ")));
        let data = top_result_data(&state);
        assert_eq!(data.subtitle.as_str(), "Song");
    }

    #[test]
    fn artwork_url_covers_every_variant() {
        let track = Track::new("t1", "T", "A").with_artwork("https://art/t.jpg");
        let album = Album::new("a1", "AL", "A", Some("https://art/a.jpg".into()));
        let artist = Artist::new("ar1", "AR", Some("https://art/ar.jpg".into()));
        let playlist =
            crate::domain::Playlist::new("p1", "PL", 3, Some("https://art/p.jpg".into()));
        assert_eq!(artwork_url(&SearchResult::Track(track)), Some("https://art/t.jpg"));
        assert_eq!(artwork_url(&SearchResult::Album(album)), Some("https://art/a.jpg"));
        assert_eq!(artwork_url(&SearchResult::Artist(artist)), Some("https://art/ar.jpg"));
        assert_eq!(artwork_url(&SearchResult::Playlist(playlist)), Some("https://art/p.jpg"));
        assert_eq!(artwork_url(&SearchResult::Artist(Artist::new("x", "Y", None))), None);
    }
}
