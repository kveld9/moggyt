//! Raw model definitions for catalog API search hits and entity containers.

/// Parsed track hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrack {
    /// YouTube video identifier.
    pub video_id: String,
    /// Track title.
    pub title: String,
    /// Artist name.
    pub artist: String,
    /// Optional track duration in seconds.
    pub duration_secs: Option<u32>,
    /// Highest-resolution thumbnail URL when known.
    pub thumb_url: Option<String>,
}

/// Parsed album hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAlbum {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
    pub thumb_url: Option<String>,
}

/// Parsed artist hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawArtist {
    pub browse_id: String,
    pub name: String,
    pub thumb_url: Option<String>,
}

/// Parsed playlist hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPlaylist {
    pub browse_id: String,
    pub title: String,
    pub thumb_url: Option<String>,
}

/// Grouped search hits across catalog item categories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchHits {
    pub tracks: Vec<RawTrack>,
    pub albums: Vec<RawAlbum>,
    pub artists: Vec<RawArtist>,
    pub playlists: Vec<RawPlaylist>,
}

impl SearchHits {
    /// Creates an empty search hits container.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if all categories have zero hits.
    pub fn is_empty(&self) -> bool {
        [self.tracks.len(), self.albums.len(), self.artists.len(), self.playlists.len()] == [0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_hits_empty_and_is_empty() {
        let hits = SearchHits::empty();
        assert!(hits.is_empty());
        assert_eq!(hits, SearchHits::default());

        let mut with_track = SearchHits::empty();
        with_track.tracks.push(RawTrack {
            video_id: "v1".into(),
            title: "T1".into(),
            artist: "A1".into(),
            duration_secs: Some(180),
            thumb_url: None,
        });
        assert!(!with_track.is_empty());

        let mut with_album = SearchHits::empty();
        with_album.albums.push(RawAlbum {
            browse_id: "b1".into(),
            title: "Alb".into(),
            artist: "Art".into(),
            thumb_url: None,
        });
        assert!(!with_album.is_empty());

        let mut with_artist = SearchHits::empty();
        with_artist.artists.push(RawArtist {
            browse_id: "ar1".into(),
            name: "ArtName".into(),
            thumb_url: None,
        });
        assert!(!with_artist.is_empty());

        let mut with_playlist = SearchHits::empty();
        with_playlist.playlists.push(RawPlaylist {
            browse_id: "p1".into(),
            title: "PlayTitle".into(),
            thumb_url: None,
        });
        assert!(!with_playlist.is_empty());
    }
}
