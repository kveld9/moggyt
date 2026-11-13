//! Music-video ordering policy for catalog result lists.
//!
//! Invariant: pure list transformation; no I/O, network, filesystem, or clocks.

use super::library::SearchResult;

/// Returns true when `item` is an official or user-uploaded music video track.
fn is_video(item: &SearchResult) -> bool {
    matches!(item, SearchResult::Track(track) if track.is_music_video())
}

/// Stable-partitions `items` so songs and non-track hits keep their relative
/// order ahead of music videos, which also keep their relative order.
pub fn songs_first(items: Vec<SearchResult>) -> Vec<SearchResult> {
    let (songs, videos): (Vec<_>, Vec<_>) = items.into_iter().partition(|item| !is_video(item));
    songs.into_iter().chain(videos).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, MusicVideoType, Track};

    fn track(id: &str, video_type: Option<MusicVideoType>) -> SearchResult {
        let mut track = Track::new(id, id, "Artist");
        if let Some(video_type) = video_type {
            track = track.with_video_type(video_type);
        }
        SearchResult::Track(track)
    }

    #[test]
    fn songs_keep_relative_order_ahead_of_videos() {
        let items = vec![
            track("v1", Some(MusicVideoType::Omv)),
            track("s1", Some(MusicVideoType::Atv)),
            track("u1", Some(MusicVideoType::Ugc)),
            track("s2", None),
        ];
        let ordered = songs_first(items);
        assert_eq!(
            ordered,
            vec![
                track("s1", Some(MusicVideoType::Atv)),
                track("s2", None),
                track("v1", Some(MusicVideoType::Omv)),
                track("u1", Some(MusicVideoType::Ugc)),
            ]
        );
    }

    #[test]
    fn non_track_hits_keep_their_position_ahead_of_videos() {
        let album = SearchResult::Album(Album::new("a1", "Album", "Artist", None));
        let items = vec![track("v1", Some(MusicVideoType::Omv)), album.clone()];
        assert_eq!(songs_first(items), vec![album, track("v1", Some(MusicVideoType::Omv))]);
    }

    #[test]
    fn ordering_empty_and_single_lists_is_identity() {
        assert!(songs_first(Vec::new()).is_empty());
        let only_song = vec![track("s1", Some(MusicVideoType::Atv))];
        assert_eq!(songs_first(only_song.clone()), only_song);
    }
}
