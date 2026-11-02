//! Pure playback queue: ordering rules without I/O or clocks.

use serde::{Deserialize, Serialize};

use super::search::Track;

/// Traversal repetition policy for a playback queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatMode {
    /// Advance stops at tail, retreat stops at head.
    #[default]
    Off,
    /// Queue loops continuously in both directions.
    All,
    /// Advance repeats current track, retreat saturates at head.
    One,
}

/// Ordered playback queue with a cursor over the current track.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackQueue {
    tracks: Vec<Track>,
    cursor: usize,
    repeat: RepeatMode,
}

impl PlaybackQueue {
    /// Creates an empty queue with default repeat policy.
    pub fn new() -> Self {
        Self { tracks: Vec::new(), cursor: 0, repeat: RepeatMode::default() }
    }

    /// Appends a track to the tail.
    pub fn push(&mut self, track: Track) {
        self.tracks.push(track);
    }

    /// Number of queued tracks.
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the queue holds no tracks.
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// All tracks in order (borrowed view for rendering).
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Active repeat policy.
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Updates the active repeat policy.
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    /// Track under the cursor, if any.
    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(self.cursor)
    }

    /// Active cursor index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Clamps the cursor to valid bounds within the track list.
    ///
    /// Resets cursor to 0 if the queue is empty; otherwise clamps to `tracks.len() - 1`.
    pub fn clamp_cursor(&mut self) {
        if self.tracks.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.tracks.len() {
            self.cursor = self.tracks.len() - 1;
        }
    }

    fn next_cursor(&self) -> Option<usize> {
        let len = self.tracks.len();
        if len == 0 || self.repeat == RepeatMode::One {
            return None;
        }
        match self.repeat {
            RepeatMode::All if len > 1 => Some((self.cursor + 1) % len),
            RepeatMode::Off if self.cursor + 1 < len => Some(self.cursor + 1),
            _ => None,
        }
    }

    /// Returns the track that [`advance`](Self::advance) would move to without mutating the queue.
    ///
    /// Returns `None` when the queue is empty or when advancing would keep the same cursor
    /// (`RepeatMode::One`, `RepeatMode::Off` at the tail, or single-track `RepeatMode::All`).
    pub fn peek_next(&self) -> Option<&Track> {
        let idx = self.next_cursor()?;
        self.tracks.get(idx)
    }

    /// Advances the cursor according to the active repeat policy.
    pub fn advance(&mut self) {
        if let Some(next) = self.next_cursor() {
            self.cursor = next;
        }
    }

    /// Moves the cursor back according to the active repeat policy.
    pub fn retreat(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        if self.repeat == RepeatMode::All && self.cursor == 0 {
            self.cursor = self.tracks.len().saturating_sub(1);
            return;
        }
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Clears all tracks and resets the cursor to 0, retaining repeat mode.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.cursor = 0;
    }

    /// Reorders tracks according to the provided index permutation.
    ///
    /// Callers inject randomness by generating `order` outside the domain.
    /// Returns `false` if `order` is not a valid permutation of `0..self.len()`,
    /// leaving the queue unchanged. Resets the cursor to 0 on success.
    pub fn shuffle_with(&mut self, order: &[usize]) -> bool {
        if !is_valid_permutation(order, self.tracks.len()) {
            return false;
        }
        self.apply_permutation(order);
        self.cursor = 0;
        true
    }

    fn apply_permutation(&mut self, order: &[usize]) {
        let old = std::mem::take(&mut self.tracks);
        let mut lookup: Vec<Option<Track>> = old.into_iter().map(Some).collect();
        self.tracks =
            order.iter().filter_map(|&idx| lookup.get_mut(idx).and_then(Option::take)).collect();
    }
}

fn is_valid_permutation(order: &[usize], len: usize) -> bool {
    if order.len() != len {
        return false;
    }
    let mut seen = vec![false; len];
    for &idx in order {
        if !mark_seen(&mut seen, idx) {
            return false;
        }
    }
    true
}

fn mark_seen(seen: &mut [bool], idx: usize) -> bool {
    if idx >= seen.len() || seen[idx] {
        return false;
    }
    seen[idx] = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_from(ids: &[&str], cursor: usize, repeat: RepeatMode) -> PlaybackQueue {
        let mut q = PlaybackQueue::new();
        for &id in ids {
            q.push(Track::new(id, "title", "artist"));
        }
        for _ in 0..cursor {
            q.advance();
        }
        q.set_repeat(repeat);
        q
    }

    fn track_ids(q: &PlaybackQueue) -> Vec<&str> {
        q.tracks().iter().map(|t| t.id.as_str()).collect()
    }

    fn current_id(q: &PlaybackQueue) -> Option<&str> {
        q.current().map(|t| t.id.as_str())
    }

    #[test]
    fn traversal_and_repeat() {
        let mut q = PlaybackQueue::new();
        assert_eq!(q.repeat(), RepeatMode::Off);
        assert!(q.is_empty());
        assert_eq!(current_id(&q), None);

        // Empty traversal safety
        q.set_repeat(RepeatMode::All);
        q.advance();
        q.retreat();
        assert_eq!(current_id(&q), None);

        // (mode, is_advance, initial_cursor, expected_current)
        let cases = [
            (RepeatMode::Off, true, 1, Some("b")),  // tail stops
            (RepeatMode::Off, false, 0, Some("a")), // head saturates
            (RepeatMode::All, true, 1, Some("a")),  // tail wraps to head
            (RepeatMode::All, false, 0, Some("b")), // head wraps to tail
            (RepeatMode::One, true, 0, Some("a")),  // stays on current
            (RepeatMode::One, false, 0, Some("a")), // head saturates
        ];
        for (mode, is_advance, start, expected) in cases {
            let mut q = queue_from(&["a", "b"], start, mode);
            if is_advance {
                q.advance();
            } else {
                q.retreat();
            }
            assert_eq!(current_id(&q), expected);
        }
    }

    #[test]
    fn clear_preserves_repeat() {
        let mut q = queue_from(&["a", "b"], 1, RepeatMode::All);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(current_id(&q), None);
        assert_eq!(q.repeat(), RepeatMode::All);
    }

    #[test]
    fn shuffle_with() {
        // Invalid permutations leave queue untouched
        let mut q = queue_from(&["a", "b"], 1, RepeatMode::Off);
        for invalid in [&[0][..], &[0, 1, 2][..], &[0, 2][..], &[1, 1][..]] {
            assert!(!q.shuffle_with(invalid));
            assert_eq!(track_ids(&q), &["a", "b"]);
            assert_eq!(current_id(&q), Some("b"));
        }

        // Valid permutation reorders and resets cursor to 0
        let mut q = queue_from(&["a", "b", "c"], 2, RepeatMode::Off);
        assert!(q.shuffle_with(&[2, 0, 1]));
        assert_eq!(track_ids(&q), &["c", "a", "b"]);
        assert_eq!(current_id(&q), Some("c"));

        // Empty queue
        let mut empty = PlaybackQueue::new();
        assert!(empty.shuffle_with(&[]));
        assert!(!empty.shuffle_with(&[0]));
        assert_eq!(current_id(&empty), None);
    }

    #[test]
    fn serde_round_trip_with_cursor_and_repeat() {
        let mut queue = PlaybackQueue::new();
        queue.push(Track::new("id-1", "Title 1", "Artist 1"));
        queue.push(Track::new("id-2", "Title 2", "Artist 2"));
        queue.advance();
        queue.set_repeat(RepeatMode::All);

        let json = serde_json::to_string(&queue).expect("serialization succeeds");
        let deserialized: PlaybackQueue =
            serde_json::from_str(&json).expect("deserialization succeeds");

        assert_eq!(queue, deserialized);
        assert_eq!(deserialized.cursor(), 1);
        assert_eq!(deserialized.repeat(), RepeatMode::All);
        assert_eq!(deserialized.len(), 2);
    }

    #[test]
    fn cursor_clamp_on_empty_and_out_of_bounds() {
        let mut empty = PlaybackQueue::new();
        empty.cursor = 10;
        empty.clamp_cursor();
        assert_eq!(empty.cursor(), 0);

        let mut queue = PlaybackQueue::new();
        queue.push(Track::new("id-1", "Title 1", "Artist 1"));
        queue.push(Track::new("id-2", "Title 2", "Artist 2"));
        queue.cursor = 10;
        queue.clamp_cursor();
        assert_eq!(queue.cursor(), 1);

        let json =
            r#"{"tracks":[{"id":"t1","title":"T1","artist":"A1"}],"cursor":99,"repeat":"Off"}"#;
        let mut loaded: PlaybackQueue =
            serde_json::from_str(json).expect("deserialization succeeds");
        assert_eq!(loaded.cursor(), 99);
        loaded.clamp_cursor();
        assert_eq!(loaded.cursor(), 0);
    }

    #[test]
    fn peek_next_policies_and_non_mutating() {
        let empty = PlaybackQueue::new();
        assert_eq!(empty.peek_next(), None);

        let q_off_middle = queue_from(&["a", "b", "c"], 0, RepeatMode::Off);
        assert_eq!(q_off_middle.peek_next().map(|t| t.id.as_str()), Some("b"));
        assert_eq!(q_off_middle.cursor(), 0);

        let q_off_last = queue_from(&["a", "b", "c"], 2, RepeatMode::Off);
        assert_eq!(q_off_last.peek_next(), None);
        assert_eq!(q_off_last.cursor(), 2);

        let q_all_wrap = queue_from(&["a", "b", "c"], 2, RepeatMode::All);
        assert_eq!(q_all_wrap.peek_next().map(|t| t.id.as_str()), Some("a"));
        assert_eq!(q_all_wrap.cursor(), 2);

        let q_all_single = queue_from(&["a"], 0, RepeatMode::All);
        assert_eq!(q_all_single.peek_next(), None);
        assert_eq!(q_all_single.cursor(), 0);

        let q_one = queue_from(&["a", "b", "c"], 1, RepeatMode::One);
        assert_eq!(q_one.peek_next(), None);
        assert_eq!(q_one.cursor(), 1);
    }
}
