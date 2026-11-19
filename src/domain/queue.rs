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

/// Origin of a queued track, distinguishing user actions from transient autoplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QueueOrigin {
    /// Context playback (e.g. albums, playlists, searches).
    #[default]
    Context,
    /// Explicitly inserted next by the user.
    User,
    /// Transient radio continuation recommendation.
    Autoplay,
}

/// Ordered playback queue with a cursor over the current track.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackQueue {
    tracks: Vec<Track>,
    cursor: usize,
    repeat: RepeatMode,
    #[serde(default, skip)]
    origins: Vec<QueueOrigin>,
}

impl PartialEq for PlaybackQueue {
    fn eq(&self, other: &Self) -> bool {
        self.tracks == other.tracks
            && self.cursor == other.cursor
            && self.repeat == other.repeat
            && (0..self.tracks.len()).all(|i| self.origin_at(i) == other.origin_at(i))
    }
}

impl Eq for PlaybackQueue {}

impl PlaybackQueue {
    /// Creates an empty queue with default repeat policy.
    pub fn new() -> Self {
        Self { tracks: Vec::new(), cursor: 0, repeat: RepeatMode::default(), origins: Vec::new() }
    }

    /// Appends a track to the tail with Context origin.
    pub fn push(&mut self, track: Track) {
        self.ensure_origins_aligned();
        self.tracks.push(track);
        self.origins.push(QueueOrigin::Context);
    }

    /// Appends a track to the tail with Autoplay origin.
    pub fn push_autoplay(&mut self, track: Track) {
        self.ensure_origins_aligned();
        self.tracks.push(track);
        self.origins.push(QueueOrigin::Autoplay);
    }

    /// Inserts a track immediately after the current cursor, keeping the cursor on the same track.
    ///
    /// An empty queue receives the track and places the cursor on it.
    pub fn insert_next(&mut self, track: Track) {
        if self.tracks.is_empty() {
            self.tracks.push(track);
            self.origins = vec![QueueOrigin::User];
            self.cursor = 0;
            return;
        }
        self.ensure_origins_aligned();
        let at = (self.cursor + 1).min(self.tracks.len());
        self.tracks.insert(at, track);
        self.origins.insert(at, QueueOrigin::User);
    }

    /// Origin of the track at the specified index, defaulting to Context if missing.
    pub fn origin_at(&self, index: usize) -> QueueOrigin {
        self.origins.get(index).copied().unwrap_or_default()
    }

    /// Whether the track at the specified index originated from autoplay.
    pub fn is_autoplay_at(&self, index: usize) -> bool {
        self.origin_at(index) == QueueOrigin::Autoplay
    }

    /// Number of entries whose origin is not Autoplay.
    pub fn visible_len(&self) -> usize {
        (0..self.tracks.len()).filter(|&i| !self.is_autoplay_at(i)).count()
    }

    /// Returns a clone of the queue with Autoplay entries removed and cursor clamped.
    pub fn persistable(&self) -> PlaybackQueue {
        let mut persistable_tracks = Vec::new();
        let mut persistable_origins = Vec::new();
        for (idx, track) in self.tracks.iter().enumerate() {
            if !self.is_autoplay_at(idx) {
                persistable_tracks.push(track.clone());
                persistable_origins.push(self.origin_at(idx));
            }
        }
        let mut queue = PlaybackQueue {
            tracks: persistable_tracks,
            cursor: self.cursor,
            repeat: self.repeat,
            origins: persistable_origins,
        };
        queue.clamp_cursor();
        queue
    }

    fn ensure_origins_aligned(&mut self) {
        if self.origins.len() < self.tracks.len() {
            self.origins.resize(self.tracks.len(), QueueOrigin::Context);
        } else if self.origins.len() > self.tracks.len() {
            self.origins.truncate(self.tracks.len());
        }
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

    /// Sets the cursor to target index, clamping to valid queue bounds.
    ///
    /// Resets cursor to 0 if the queue is empty; otherwise clamps `index` to `tracks.len() - 1`.
    /// Ignores and preserves the active repeat policy.
    pub fn set_cursor(&mut self, index: usize) {
        if self.tracks.is_empty() {
            self.cursor = 0;
            return;
        }
        self.cursor = index.min(self.tracks.len() - 1);
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
        self.origins.clear();
        self.cursor = 0;
    }

    /// Replaces every entry in the queue with the provided tracks and sets the cursor.
    ///
    /// Preserves the active repeat mode. An empty list resets the cursor to 0.
    pub fn replace(&mut self, tracks: Vec<Track>, cursor: usize) {
        let len = tracks.len();
        self.tracks = tracks;
        self.origins = vec![QueueOrigin::Context; len];
        self.set_cursor(cursor);
    }

    /// Keeps only the track under the cursor as the sole entry and resets the cursor to 0.
    ///
    /// Preserves the active repeat mode. Has no effect if the queue is empty or has no current track.
    pub fn retain_current(&mut self) {
        let Some(track) = self.current().cloned() else {
            return;
        };
        let origin = self.origin_at(self.cursor);
        self.tracks = vec![track];
        self.origins = vec![origin];
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
        self.ensure_origins_aligned();
        let old_tracks = std::mem::take(&mut self.tracks);
        let mut track_lookup: Vec<Option<Track>> = old_tracks.into_iter().map(Some).collect();
        self.tracks = order
            .iter()
            .filter_map(|&idx| track_lookup.get_mut(idx).and_then(Option::take))
            .collect();

        let old_origins = std::mem::take(&mut self.origins);
        let mut origin_lookup: Vec<Option<QueueOrigin>> =
            old_origins.into_iter().map(Some).collect();
        self.origins = order
            .iter()
            .filter_map(|&idx| origin_lookup.get_mut(idx).and_then(Option::take))
            .collect();
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
    fn retain_current_keeps_only_cursor_track_and_preserves_repeat() {
        let mut q = queue_from(&["a", "b", "c"], 1, RepeatMode::All);
        q.retain_current();
        assert_eq!(track_ids(&q), ["b"]);
        assert_eq!(q.cursor(), 0);
        assert_eq!(current_id(&q), Some("b"));
        assert_eq!(q.repeat(), RepeatMode::All);

        let mut empty = PlaybackQueue::new();
        empty.set_repeat(RepeatMode::One);
        empty.retain_current();
        assert!(empty.is_empty());
        assert_eq!(empty.cursor(), 0);
        assert_eq!(empty.repeat(), RepeatMode::One);
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

    #[test]
    fn insert_next_on_empty_queue_places_cursor_on_the_track() {
        let mut q = PlaybackQueue::new();
        q.insert_next(Track::new("a", "title", "artist"));
        assert_eq!(q.len(), 1);
        assert_eq!(q.cursor(), 0);
        assert_eq!(current_id(&q), Some("a"));
    }

    #[test]
    fn insert_next_after_middle_cursor_keeps_current_track() {
        let mut q = queue_from(&["a", "b", "c"], 1, RepeatMode::Off);
        q.insert_next(Track::new("x", "title", "artist"));
        assert_eq!(track_ids(&q), ["a", "b", "x", "c"]);
        assert_eq!(current_id(&q), Some("b"));
    }

    #[test]
    fn insert_next_at_tail_appends_and_keeps_current_track() {
        let mut q = queue_from(&["a", "b"], 1, RepeatMode::Off);
        q.insert_next(Track::new("x", "title", "artist"));
        assert_eq!(track_ids(&q), ["a", "b", "x"]);
        assert_eq!(current_id(&q), Some("b"));
    }

    #[test]
    fn set_cursor_moves_under_every_repeat_mode() {
        for mode in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            let mut q = queue_from(&["a", "b", "c"], 0, mode);
            assert_eq!(q.cursor(), 0);
            assert_eq!(q.repeat(), mode);

            q.set_cursor(1);
            assert_eq!(q.cursor(), 1);
            assert_eq!(current_id(&q), Some("b"));
            assert_eq!(q.repeat(), mode);

            q.set_cursor(2);
            assert_eq!(q.cursor(), 2);
            assert_eq!(current_id(&q), Some("c"));
            assert_eq!(q.repeat(), mode);

            q.set_cursor(0);
            assert_eq!(q.cursor(), 0);
            assert_eq!(current_id(&q), Some("a"));
            assert_eq!(q.repeat(), mode);
        }
    }

    #[test]
    fn set_cursor_clamps_and_handles_empty_queue() {
        let mut empty = PlaybackQueue::new();
        empty.set_repeat(RepeatMode::One);
        empty.set_cursor(5);
        assert_eq!(empty.cursor(), 0);
        assert_eq!(empty.repeat(), RepeatMode::One);

        let mut q = queue_from(&["a", "b", "c"], 0, RepeatMode::All);
        q.set_cursor(10);
        assert_eq!(q.cursor(), 2);
        assert_eq!(current_id(&q), Some("c"));
        assert_eq!(q.repeat(), RepeatMode::All);

        q.set_cursor(1);
        assert_eq!(q.cursor(), 1);
        assert_eq!(current_id(&q), Some("b"));
        assert_eq!(q.repeat(), RepeatMode::All);
    }

    #[test]
    fn replace_replaces_non_empty_queue_with_new_list() {
        let mut q = queue_from(&["old1", "old2"], 1, RepeatMode::Off);
        let new_tracks = vec![
            Track::new("new1", "Title 1", "Artist 1"),
            Track::new("new2", "Title 2", "Artist 2"),
        ];
        q.replace(new_tracks, 1);
        assert_eq!(track_ids(&q), ["new1", "new2"]);
        assert_eq!(q.cursor(), 1);
        assert_eq!(current_id(&q), Some("new2"));
    }

    #[test]
    fn replace_clamps_out_of_range_cursor() {
        let mut q = queue_from(&["old"], 0, RepeatMode::Off);
        let new_tracks =
            vec![Track::new("a", "Title A", "Artist A"), Track::new("b", "Title B", "Artist B")];
        q.replace(new_tracks, 99);
        assert_eq!(track_ids(&q), ["a", "b"]);
        assert_eq!(q.cursor(), 1);
        assert_eq!(current_id(&q), Some("b"));
    }

    #[test]
    fn replace_empty_input_empties_queue() {
        let mut q = queue_from(&["old1", "old2"], 1, RepeatMode::Off);
        q.replace(Vec::new(), 3);
        assert!(q.is_empty());
        assert_eq!(q.cursor(), 0);
        assert_eq!(current_id(&q), None);
    }

    #[test]
    fn replace_preserves_repeat_policy() {
        for mode in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            let mut q = queue_from(&["old"], 0, mode);
            q.replace(vec![Track::new("n", "N", "A")], 0);
            assert_eq!(q.repeat(), mode);

            let mut empty_q = queue_from(&["old"], 0, mode);
            empty_q.replace(Vec::new(), 0);
            assert_eq!(empty_q.repeat(), mode);
        }
    }

    #[test]
    fn autoplay_entries_marked_and_counted_out_of_visible_len() {
        let mut q = PlaybackQueue::new();
        q.push(Track::new("c1", "Context 1", "Artist"));
        q.push_autoplay(Track::new("a1", "Autoplay 1", "Artist"));
        q.push(Track::new("c2", "Context 2", "Artist"));
        q.push_autoplay(Track::new("a2", "Autoplay 2", "Artist"));

        assert_eq!(q.len(), 4);
        assert_eq!(q.visible_len(), 2);
        assert_eq!(q.origin_at(0), QueueOrigin::Context);
        assert_eq!(q.origin_at(1), QueueOrigin::Autoplay);
        assert_eq!(q.origin_at(2), QueueOrigin::Context);
        assert_eq!(q.origin_at(3), QueueOrigin::Autoplay);
        assert_eq!(q.origin_at(99), QueueOrigin::Context);

        assert!(!q.is_autoplay_at(0));
        assert!(q.is_autoplay_at(1));
        assert!(!q.is_autoplay_at(2));
        assert!(q.is_autoplay_at(3));
        assert!(!q.is_autoplay_at(99));
    }

    #[test]
    fn persistable_drops_autoplay_and_clamps_cursor_when_context_exhausted() {
        let mut q = PlaybackQueue::new();
        q.push(Track::new("c1", "Context 1", "Artist"));
        q.push(Track::new("c2", "Context 2", "Artist"));
        q.push_autoplay(Track::new("a1", "Autoplay 1", "Artist"));
        q.push_autoplay(Track::new("a2", "Autoplay 2", "Artist"));
        q.set_cursor(3);
        q.set_repeat(RepeatMode::All);

        let p = q.persistable();
        assert_eq!(track_ids(&p), ["c1", "c2"]);
        assert_eq!(p.len(), 2);
        assert_eq!(p.visible_len(), 2);
        assert_eq!(p.cursor(), 1);
        assert_eq!(p.repeat(), RepeatMode::All);
        assert_eq!(p.origin_at(0), QueueOrigin::Context);
        assert_eq!(p.origin_at(1), QueueOrigin::Context);
    }

    #[test]
    fn persistable_drops_autoplay_and_clamps_cursor_with_no_context_left() {
        let mut q = PlaybackQueue::new();
        q.push_autoplay(Track::new("a1", "Autoplay 1", "Artist"));
        q.push_autoplay(Track::new("a2", "Autoplay 2", "Artist"));
        q.set_cursor(1);

        let p = q.persistable();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        assert_eq!(p.visible_len(), 0);
        assert_eq!(p.cursor(), 0);
    }

    #[test]
    fn mutations_keep_origins_aligned() {
        let mut q_empty = PlaybackQueue::new();
        q_empty.insert_next(Track::new("u1", "User 1", "Artist"));
        assert_eq!(q_empty.origin_at(0), QueueOrigin::User);
        assert_eq!(q_empty.len(), 1);

        let mut q = PlaybackQueue::new();
        q.push(Track::new("c1", "Context 1", "Artist"));
        q.push_autoplay(Track::new("a1", "Autoplay 1", "Artist"));
        q.set_cursor(0);
        q.insert_next(Track::new("u2", "User 2", "Artist"));
        assert_eq!(track_ids(&q), ["c1", "u2", "a1"]);
        assert_eq!(q.origin_at(0), QueueOrigin::Context);
        assert_eq!(q.origin_at(1), QueueOrigin::User);
        assert_eq!(q.origin_at(2), QueueOrigin::Autoplay);

        q.replace(
            vec![Track::new("r1", "Replace 1", "Artist"), Track::new("r2", "Replace 2", "Artist")],
            0,
        );
        assert_eq!(q.origin_at(0), QueueOrigin::Context);
        assert_eq!(q.origin_at(1), QueueOrigin::Context);

        let mut q_shuf = PlaybackQueue::new();
        q_shuf.push(Track::new("c", "C", "Artist"));
        q_shuf.push_autoplay(Track::new("a", "A", "Artist"));
        q_shuf.insert_next(Track::new("u", "U", "Artist"));
        assert_eq!(q_shuf.origin_at(0), QueueOrigin::Context);
        assert_eq!(q_shuf.origin_at(1), QueueOrigin::User);
        assert_eq!(q_shuf.origin_at(2), QueueOrigin::Autoplay);
        assert!(q_shuf.shuffle_with(&[2, 0, 1]));
        assert_eq!(track_ids(&q_shuf), ["a", "c", "u"]);
        assert_eq!(q_shuf.origin_at(0), QueueOrigin::Autoplay);
        assert_eq!(q_shuf.origin_at(1), QueueOrigin::Context);
        assert_eq!(q_shuf.origin_at(2), QueueOrigin::User);

        q_shuf.set_cursor(2);
        q_shuf.retain_current();
        assert_eq!(track_ids(&q_shuf), ["u"]);
        assert_eq!(q_shuf.origin_at(0), QueueOrigin::User);

        let mut q_auto = PlaybackQueue::new();
        q_auto.push_autoplay(Track::new("a", "A", "Artist"));
        q_auto.retain_current();
        assert_eq!(track_ids(&q_auto), ["a"]);
        assert_eq!(q_auto.origin_at(0), QueueOrigin::Autoplay);

        q_auto.clear();
        assert!(q_auto.is_empty());
        assert_eq!(q_auto.origin_at(0), QueueOrigin::Context);
    }

    #[test]
    fn deserializing_without_origins_yields_context_for_every_entry() {
        let json = r#"{"tracks":[{"id":"t1","title":"T1","artist":"A1"},{"id":"t2","title":"T2","artist":"A2"}],"cursor":0,"repeat":"Off"}"#;
        let loaded: PlaybackQueue = serde_json::from_str(json).expect("deserialization succeeds");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.visible_len(), 2);
        assert_eq!(loaded.origin_at(0), QueueOrigin::Context);
        assert_eq!(loaded.origin_at(1), QueueOrigin::Context);
        assert_eq!(loaded.origin_at(99), QueueOrigin::Context);
        assert!(!loaded.is_autoplay_at(0));
        assert!(!loaded.is_autoplay_at(1));
    }
}
