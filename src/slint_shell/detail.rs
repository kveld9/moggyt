//! Slint detail page state and view-model bridge.
//! Hexagonal invariant: pure in-memory with zero I/O, network, audio, or async.

use crate::domain::{Detail, DetailKind};

pub fn detail_kind_label(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Album => "Album",
        DetailKind::Artist => "Artist",
        DetailKind::Playlist => "Playlist",
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;

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
