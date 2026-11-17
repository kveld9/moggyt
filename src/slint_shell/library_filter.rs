//! Library section selected by the chips row.

/// Section shown by the library grid behind the chips row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LibraryFilter {
    #[default]
    Playlists,
    Songs,
    Albums,
    Artists,
}

impl LibraryFilter {
    /// Maps a chip index from the UI to a filter.
    pub(crate) fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Songs,
            2 => Self::Albums,
            3 => Self::Artists,
            _ => Self::Playlists,
        }
    }

    /// Stable index exposed to the chips row.
    pub(crate) fn to_index(self) -> i32 {
        match self {
            Self::Playlists => 0,
            Self::Songs => 1,
            Self::Albums => 2,
            Self::Artists => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_indices_round_trip() {
        for filter in [
            LibraryFilter::Playlists,
            LibraryFilter::Songs,
            LibraryFilter::Albums,
            LibraryFilter::Artists,
        ] {
            assert_eq!(LibraryFilter::from_index(filter.to_index()), filter);
        }
    }

    #[test]
    fn unknown_index_falls_back_to_playlists() {
        assert_eq!(LibraryFilter::from_index(-1), LibraryFilter::Playlists);
        assert_eq!(LibraryFilter::from_index(7), LibraryFilter::Playlists);
    }
}
