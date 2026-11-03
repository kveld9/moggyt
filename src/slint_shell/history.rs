//! Account listening history playlist entry projection, detail labeling, and subtitles.

use crate::domain::{Detail, Playlist};
use crate::i18n::{Locale, tr};
use crate::infra::HISTORY_BROWSE_ID;

/// Returns true if the given browse identifier corresponds to the account listening history.
pub(crate) fn is_history_id(id: &str) -> bool {
    id.trim() == HISTORY_BROWSE_ID
}

/// Prepends the account listening history entry to the playlist collection when signed in.
///
/// If the user is unauthenticated or the history entry is already present in `playlists`,
/// the collection is returned unchanged.
pub(crate) fn with_history_entry(
    mut playlists: Vec<Playlist>,
    is_signed_in: bool,
    locale: Locale,
) -> Vec<Playlist> {
    if !is_signed_in || playlists.iter().any(|p| is_history_id(&p.id)) {
        return playlists;
    }
    let entry = Playlist::new(HISTORY_BROWSE_ID, tr(locale, "History"), 0, None);
    playlists.insert(0, entry);
    playlists
}

/// Sets the detail title to the localized history label if the detail title is empty.
pub(crate) fn label_history_detail(mut detail: Detail, locale: Locale) -> Detail {
    if detail.title.trim().is_empty() {
        detail.title = tr(locale, "History").to_owned();
    }
    detail
}

/// Returns the formatted subtitle for a library playlist item.
///
/// Returns an empty string for the account listening history entry; otherwise returns
/// the standard track count display.
pub(crate) fn library_item_subtitle(playlist: &Playlist, _locale: Locale) -> String {
    if is_history_id(&playlist.id) {
        String::new()
    } else {
        format!("{} tracks", playlist.track_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DetailKind, Track};

    #[test]
    fn test_is_history_id() {
        assert!(is_history_id(HISTORY_BROWSE_ID));
        assert!(is_history_id("  FEmusic_history  "));
        assert!(!is_history_id("FEmusic_liked_playlists"));
        assert!(!is_history_id("other_playlist"));
        assert!(!is_history_id(""));
    }

    #[test]
    fn test_with_history_entry_signed_out_returns_unchanged() {
        let initial = vec![Playlist::new("p1", "My Playlist", 5, None)];
        let result = with_history_entry(initial.clone(), false, Locale::English);
        assert_eq!(result, initial);
    }

    #[test]
    fn test_with_history_entry_signed_in_prepends_entry() {
        let initial = vec![Playlist::new("p1", "My Playlist", 5, None)];
        let result = with_history_entry(initial, true, Locale::English);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, HISTORY_BROWSE_ID);
        assert_eq!(result[0].title, "History");
        assert_eq!(result[0].track_count, 0);
        assert_eq!(result[0].artwork, None);
        assert_eq!(result[1].id, "p1");

        let es_result = with_history_entry(Vec::new(), true, Locale::Spanish);
        assert_eq!(es_result.len(), 1);
        assert_eq!(es_result[0].id, HISTORY_BROWSE_ID);
        assert_eq!(es_result[0].title, "Historial");
        assert_eq!(es_result[0].track_count, 0);
        assert_eq!(es_result[0].artwork, None);
    }

    #[test]
    fn test_with_history_entry_never_duplicates() {
        let initial = vec![
            Playlist::new(HISTORY_BROWSE_ID, "History", 0, None),
            Playlist::new("p1", "My Playlist", 5, None),
        ];
        let result = with_history_entry(initial.clone(), true, Locale::English);
        assert_eq!(result, initial);
    }

    #[test]
    fn test_label_history_detail_empty_title() {
        let detail =
            Detail::new(DetailKind::Playlist, "", "Sub", vec![Track::new("t1", "Track", "Artist")]);
        let labeled = label_history_detail(detail, Locale::English);
        assert_eq!(labeled.title, "History");

        let detail_es = Detail::new(DetailKind::Playlist, "  ", "", Vec::new());
        let labeled_es = label_history_detail(detail_es, Locale::Spanish);
        assert_eq!(labeled_es.title, "Historial");
    }

    #[test]
    fn test_label_history_detail_non_empty_title_preserved() {
        let detail = Detail::new(DetailKind::Playlist, "Custom Title", "Sub", Vec::new());
        let labeled = label_history_detail(detail, Locale::English);
        assert_eq!(labeled.title, "Custom Title");
    }

    #[test]
    fn test_library_item_subtitle() {
        let history = Playlist::new(HISTORY_BROWSE_ID, "History", 0, None);
        assert_eq!(library_item_subtitle(&history, Locale::English), "");
        assert_eq!(library_item_subtitle(&history, Locale::Spanish), "");

        let regular = Playlist::new("p1", "Regular", 12, None);
        assert_eq!(library_item_subtitle(&regular, Locale::English), "12 tracks");
        assert_eq!(library_item_subtitle(&regular, Locale::Spanish), "12 tracks");
    }
}
