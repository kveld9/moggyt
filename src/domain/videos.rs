//! Music-video ordering and visibility policy for catalog result lists.
//!
//! Invariant: pure list transformations; no I/O, network, filesystem, or clocks.

use super::library::{Detail, SearchResult, Shelf};
use super::search::retain_song_results;

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

/// Applies the browse policy: drops music videos when `hide_videos` is set, then
/// ranks the remaining songs ahead of music videos.
pub fn apply_result_policy(items: Vec<SearchResult>, hide_videos: bool) -> Vec<SearchResult> {
    let items = if hide_videos { retain_song_results(items) } else { items };
    songs_first(items)
}

/// Applies the browse policy to every shelf, dropping shelves left without items.
pub fn apply_shelf_policy(shelves: Vec<Shelf>, hide_videos: bool) -> Vec<Shelf> {
    shelves
        .into_iter()
        .filter_map(|mut shelf| {
            shelf.items = apply_result_policy(shelf.items, hide_videos);
            (!shelf.items.is_empty()).then_some(shelf)
        })
        .collect()
}

/// Drops music videos from a remote detail track list when `hide_videos` is set.
pub fn apply_detail_policy(mut detail: Detail, hide_videos: bool) -> Detail {
    if hide_videos {
        detail.tracks.retain(|track| !track.is_music_video());
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, DetailKind, MusicVideoType, Track};

    fn raw_track(id: &str, video_type: Option<MusicVideoType>) -> Track {
        let mut track = Track::new(id, id, "Artist");
        if let Some(video_type) = video_type {
            track = track.with_video_type(video_type);
        }
        track
    }

    fn song(id: &str) -> Track {
        raw_track(id, Some(MusicVideoType::Atv))
    }

    fn video(id: &str) -> Track {
        raw_track(id, Some(MusicVideoType::Omv))
    }

    fn result(track: Track) -> SearchResult {
        SearchResult::Track(track)
    }

    fn mixed_shelf(title: &str) -> Shelf {
        Shelf { title: title.to_owned(), items: vec![result(video("v1")), result(song("s1"))] }
    }

    #[test]
    fn songs_keep_relative_order_ahead_of_videos() {
        let items = vec![
            result(video("v1")),
            result(song("s1")),
            result(raw_track("u1", Some(MusicVideoType::Ugc))),
            result(raw_track("s2", None)),
        ];
        let ordered = songs_first(items);
        assert_eq!(
            ordered,
            vec![
                result(song("s1")),
                result(raw_track("s2", None)),
                result(video("v1")),
                result(raw_track("u1", Some(MusicVideoType::Ugc))),
            ]
        );
    }

    #[test]
    fn non_track_hits_keep_their_position_ahead_of_videos() {
        let album = SearchResult::Album(Album::new("a1", "Album", "Artist", None));
        let items = vec![result(video("v1")), album.clone()];
        assert_eq!(songs_first(items), vec![album, result(video("v1"))]);
    }

    #[test]
    fn ordering_empty_and_single_lists_is_identity() {
        assert!(songs_first(Vec::new()).is_empty());
        let only_song = vec![result(song("s1"))];
        assert_eq!(songs_first(only_song.clone()), only_song);
    }

    #[test]
    fn result_policy_hides_videos_only_when_requested() {
        let items = vec![result(video("v1")), result(song("s1"))];
        assert_eq!(
            apply_result_policy(items.clone(), false),
            vec![result(song("s1")), result(video("v1"))]
        );
        assert_eq!(apply_result_policy(items, true), vec![result(song("s1"))]);
    }

    #[test]
    fn shelf_policy_applies_to_each_shelf_and_drops_video_only_shelves() {
        let videos_only = Shelf { title: "Videos".to_owned(), items: vec![result(video("v2"))] };
        let shelves = apply_shelf_policy(vec![mixed_shelf("Mixed"), videos_only], true);
        let expected = vec![Shelf { title: "Mixed".to_owned(), items: vec![result(song("s1"))] }];
        assert_eq!(shelves, expected);
    }

    #[test]
    fn detail_policy_drops_video_tracks_only_when_requested() {
        let detail =
            Detail::new(DetailKind::Playlist, "List", "Owner", vec![video("v1"), song("s1")]);
        let hidden = apply_detail_policy(detail.clone(), true);
        assert_eq!(hidden.tracks, vec![song("s1")]);
        let kept = apply_detail_policy(detail, false);
        assert_eq!(kept.tracks, vec![video("v1"), song("s1")]);
    }
}
