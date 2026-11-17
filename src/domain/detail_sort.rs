//! Sorting model for the detail playlist table.

use std::cmp::Ordering;

use crate::domain::Track;

/// Column of the detail playlist table that can order its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    DateAdded,
    Duration,
}

impl SortColumn {
    /// Maps a UI column index to its sortable column.
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Title),
            1 => Some(Self::Artist),
            2 => Some(Self::Album),
            3 => Some(Self::DateAdded),
            4 => Some(Self::Duration),
            _ => None,
        }
    }

    /// Returns the UI column index of this column.
    pub fn to_index(self) -> i32 {
        match self {
            Self::Title => 0,
            Self::Artist => 1,
            Self::Album => 2,
            Self::DateAdded => 3,
            Self::Duration => 4,
        }
    }
}

/// Direction of the active sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Active sort of the playlist table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailSort {
    pub column: SortColumn,
    pub direction: SortDirection,
}

/// Cycles the sort state for a clicked column: none, ascending, descending, none.
pub fn next_sort(current: Option<DetailSort>, clicked: SortColumn) -> Option<DetailSort> {
    match current {
        None => Some(DetailSort { column: clicked, direction: SortDirection::Ascending }),
        Some(sort) if sort.column != clicked => {
            Some(DetailSort { column: clicked, direction: SortDirection::Ascending })
        }
        Some(sort) if sort.direction == SortDirection::Ascending => {
            Some(DetailSort { column: clicked, direction: SortDirection::Descending })
        }
        Some(_) => None,
    }
}

/// Returns the stable display order of `tracks` for the active sort.
pub fn ordered_indices(tracks: &[Track], sort: Option<DetailSort>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..tracks.len()).collect();
    let Some(sort) = sort else {
        return order;
    };
    order.sort_by(|&left, &right| compare_tracks(&tracks[left], &tracks[right], sort));
    order
}

fn compare_tracks(left: &Track, right: &Track, sort: DetailSort) -> Ordering {
    match sort.column {
        SortColumn::Title => compare_text(&left.title, &right.title, sort.direction),
        SortColumn::Artist => compare_text(&left.artist, &right.artist, sort.direction),
        SortColumn::Album => compare_text(album_text(left), album_text(right), sort.direction),
        SortColumn::DateAdded => {
            compare_optional(left.added_at_secs, right.added_at_secs, sort.direction)
        }
        SortColumn::Duration => {
            compare_optional(left.duration_secs, right.duration_secs, sort.direction)
        }
    }
}

fn album_text(track: &Track) -> &str {
    track.album.as_deref().unwrap_or("")
}

fn compare_text(left: &str, right: &str, direction: SortDirection) -> Ordering {
    apply_direction(left.to_lowercase().cmp(&right.to_lowercase()), direction)
}

fn compare_optional<T: Ord>(
    left: Option<T>,
    right: Option<T>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => apply_direction(left.cmp(&right), direction),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artist: &str) -> Track {
        Track::new("id", title, artist)
    }

    #[test]
    fn column_index_round_trip() {
        for column in [
            SortColumn::Title,
            SortColumn::Artist,
            SortColumn::Album,
            SortColumn::DateAdded,
            SortColumn::Duration,
        ] {
            assert_eq!(SortColumn::from_index(column.to_index()), Some(column));
        }
        assert_eq!(SortColumn::from_index(-1), None);
        assert_eq!(SortColumn::from_index(5), None);
    }

    #[test]
    fn next_sort_cycles_three_states_per_column() {
        let title = SortColumn::Title;
        let none = None;
        let ascending = next_sort(none, title).expect("ascending");
        assert_eq!(ascending.direction, SortDirection::Ascending);
        let descending = next_sort(Some(ascending), title).expect("descending");
        assert_eq!(descending.direction, SortDirection::Descending);
        assert_eq!(next_sort(Some(descending), title), None);
        let other = next_sort(Some(descending), SortColumn::Album).expect("restart");
        assert_eq!(other.column, SortColumn::Album);
        assert_eq!(other.direction, SortDirection::Ascending);
    }

    #[test]
    fn no_sort_keeps_catalog_order() {
        let tracks = vec![track("B", "X"), track("A", "Y")];
        assert_eq!(ordered_indices(&tracks, None), vec![0, 1]);
    }

    #[test]
    fn title_sort_is_case_insensitive_and_reversible() {
        let tracks = vec![track("beta", "X"), track("Alpha", "Y"), track("gamma", "Z")];
        let ascending =
            DetailSort { column: SortColumn::Title, direction: SortDirection::Ascending };
        let descending =
            DetailSort { column: SortColumn::Title, direction: SortDirection::Descending };
        assert_eq!(ordered_indices(&tracks, Some(ascending)), vec![1, 0, 2]);
        assert_eq!(ordered_indices(&tracks, Some(descending)), vec![2, 0, 1]);
    }

    #[test]
    fn artist_and_album_sort_ascending() {
        let tracks =
            vec![track("T1", "Zed").with_album("Beta"), track("T2", "Ana").with_album("Alpha")];
        let artist = DetailSort { column: SortColumn::Artist, direction: SortDirection::Ascending };
        let album = DetailSort { column: SortColumn::Album, direction: SortDirection::Ascending };
        assert_eq!(ordered_indices(&tracks, Some(artist)), vec![1, 0]);
        assert_eq!(ordered_indices(&tracks, Some(album)), vec![1, 0]);
    }

    #[test]
    fn date_added_keeps_missing_last_in_both_directions() {
        let tracks = vec![
            track("old", "X").with_added("old", 100),
            track("missing", "Y"),
            track("new", "Z").with_added("new", 300),
        ];
        let ascending =
            DetailSort { column: SortColumn::DateAdded, direction: SortDirection::Ascending };
        let descending =
            DetailSort { column: SortColumn::DateAdded, direction: SortDirection::Descending };
        assert_eq!(ordered_indices(&tracks, Some(ascending)), vec![0, 2, 1]);
        assert_eq!(ordered_indices(&tracks, Some(descending)), vec![2, 0, 1]);
    }

    #[test]
    fn duration_keeps_missing_last_in_both_directions() {
        let tracks = vec![
            track("long", "X").with_duration(300),
            track("missing", "Y"),
            track("short", "Z").with_duration(100),
        ];
        let ascending =
            DetailSort { column: SortColumn::Duration, direction: SortDirection::Ascending };
        let descending =
            DetailSort { column: SortColumn::Duration, direction: SortDirection::Descending };
        assert_eq!(ordered_indices(&tracks, Some(ascending)), vec![2, 0, 1]);
        assert_eq!(ordered_indices(&tracks, Some(descending)), vec![0, 2, 1]);
    }

    #[test]
    fn equal_keys_keep_original_order() {
        let tracks = vec![track("Same", "X"), track("Same", "Y"), track("Same", "Z")];
        let sort = DetailSort { column: SortColumn::Title, direction: SortDirection::Ascending };
        assert_eq!(ordered_indices(&tracks, Some(sort)), vec![0, 1, 2]);
    }
}
