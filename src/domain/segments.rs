//! Non-music segment skipping models.
//!
//! Stores user-marked skip intervals per video id, kept sorted and non-overlapping.

use serde::{Deserialize, Serialize};

/// Error returned when constructing a segment with invalid bounds (`start >= end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSegment;

impl std::fmt::Display for InvalidSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "start position must be strictly less than end position")
    }
}

impl std::error::Error for InvalidSegment {}

/// A single playback skip segment with validated half-open bounds `[start_secs, end_secs)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkipSegment {
    /// Starting boundary in seconds (inclusive).
    pub start_secs: u32,
    /// Ending boundary in seconds (exclusive).
    pub end_secs: u32,
}

impl SkipSegment {
    /// Constructs a validated skip segment where `start_secs < end_secs`.
    pub fn new(start_secs: u32, end_secs: u32) -> Result<Self, InvalidSegment> {
        if start_secs >= end_secs {
            return Err(InvalidSegment);
        }
        Ok(Self { start_secs, end_secs })
    }
}

/// Collection of sorted, non-overlapping skip segments for a track.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkipRanges(pub Vec<SkipSegment>);

fn merge_segment_into(mut acc: Vec<SkipSegment>, seg: SkipSegment) -> Vec<SkipSegment> {
    let Some(last) = acc.last_mut() else {
        acc.push(seg);
        return acc;
    };
    if seg.start_secs <= last.end_secs {
        last.end_secs = last.end_secs.max(seg.end_secs);
    } else {
        acc.push(seg);
    }
    acc
}

impl SkipRanges {
    /// Creates an empty collection of skip ranges.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Returns a slice of the stored segments.
    pub fn segments(&self) -> &[SkipSegment] {
        &self.0
    }

    /// Returns true if there are no segments stored.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of segments stored.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Inserts a segment, maintaining sorted order and merging overlapping or contiguous ranges.
    pub fn insert(&mut self, segment: SkipSegment) {
        self.0.push(segment);
        self.0.sort_by_key(|s| (s.start_secs, s.end_secs));
        let segments = std::mem::take(&mut self.0);
        self.0 = segments.into_iter().fold(Vec::new(), merge_segment_into);
    }

    /// Removes a segment at `index`, returning the removed segment if valid.
    pub fn remove(&mut self, index: usize) -> Option<SkipSegment> {
        if index < self.0.len() { Some(self.0.remove(index)) } else { None }
    }

    /// Returns jump target if `pos_secs` falls strictly inside any range: `start <= pos < end`.
    pub fn jump_target(&self, pos_secs: u32) -> Option<u32> {
        self.0
            .iter()
            .find(|seg| pos_secs >= seg.start_secs && pos_secs < seg.end_secs)
            .map(|seg| seg.end_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_rejects_invalid_bounds() {
        assert_eq!(SkipSegment::new(10, 10), Err(InvalidSegment));
        assert_eq!(SkipSegment::new(15, 10), Err(InvalidSegment));
        assert_eq!(SkipSegment::new(0, 0), Err(InvalidSegment));
        assert_eq!(SkipSegment::new(5, 10), Ok(SkipSegment { start_secs: 5, end_secs: 10 }));
    }

    #[test]
    fn test_insert_merges_overlaps_and_sorts() {
        let mut ranges = SkipRanges::new();
        assert!(ranges.is_empty());
        assert_eq!(ranges.len(), 0);

        ranges.insert(SkipSegment::new(20, 30).unwrap());
        ranges.insert(SkipSegment::new(5, 10).unwrap());
        assert_eq!(
            ranges.segments(),
            &[
                SkipSegment { start_secs: 5, end_secs: 10 },
                SkipSegment { start_secs: 20, end_secs: 30 }
            ]
        );

        // Overlapping range merges [5, 10] and [8, 15] into [5, 15]
        ranges.insert(SkipSegment::new(8, 15).unwrap());
        assert_eq!(
            ranges.segments(),
            &[
                SkipSegment { start_secs: 5, end_secs: 15 },
                SkipSegment { start_secs: 20, end_secs: 30 }
            ]
        );

        // Contiguous bridge range [15, 20] merges [5, 15] and [20, 30] into [5, 30]
        ranges.insert(SkipSegment::new(15, 20).unwrap());
        assert_eq!(ranges.segments(), &[SkipSegment { start_secs: 5, end_secs: 30 }]);

        // Engulfing range [0, 50] absorbs existing range
        ranges.insert(SkipSegment::new(0, 50).unwrap());
        assert_eq!(ranges.segments(), &[SkipSegment { start_secs: 0, end_secs: 50 }]);
        assert_eq!(ranges.len(), 1);
    }

    #[test]
    fn test_jump_target_boundary_semantics() {
        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());

        // Boundary: strictly before range -> does not seek
        assert_eq!(ranges.jump_target(9), None);

        // Boundary: exactly at start -> seeks to range end
        assert_eq!(ranges.jump_target(10), Some(20));

        // Inside range -> seeks to range end
        assert_eq!(ranges.jump_target(15), Some(20));
        assert_eq!(ranges.jump_target(19), Some(20));

        // Boundary: exactly at end -> does not seek
        assert_eq!(ranges.jump_target(20), None);

        // Boundary: strictly after range -> does not seek
        assert_eq!(ranges.jump_target(21), None);
    }

    #[test]
    fn test_remove_at_index_and_out_of_bounds() {
        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(0, 10).unwrap());
        ranges.insert(SkipSegment::new(20, 30).unwrap());

        assert_eq!(ranges.remove(5), None);
        assert_eq!(ranges.len(), 2);

        let removed = ranges.remove(0);
        assert_eq!(removed, Some(SkipSegment { start_secs: 0, end_secs: 10 }));
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.segments(), &[SkipSegment { start_secs: 20, end_secs: 30 }]);
    }
}
