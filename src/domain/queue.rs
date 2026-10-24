//! Pure playback queue: ordering rules without I/O or clocks.

use super::search::Track;

/// Ordered playback queue with a cursor over the current track.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PlaybackQueue {
    tracks: Vec<Track>,
    cursor: usize,
}

impl PlaybackQueue {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self { tracks: Vec::new(), cursor: 0 }
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

    /// Track under the cursor, if any.
    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(self.cursor)
    }

    /// Advances the cursor; stays at the tail when already there.
    pub fn advance(&mut self) {
        if self.cursor + 1 < self.tracks.len() {
            self.cursor += 1;
        }
    }

    /// Moves the cursor back; stays at the head when already there.
    pub fn retreat(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track::new(id, "title", "artist")
    }

    #[test]
    fn empty_queue_has_no_current() {
        let queue = PlaybackQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.current(), None);
    }

    #[test]
    fn advance_stops_at_tail() {
        let mut queue = PlaybackQueue::new();
        queue.push(track("a"));
        queue.push(track("b"));
        queue.advance();
        queue.advance();
        queue.advance();
        assert_eq!(queue.current().map(|t| t.id.as_str()), Some("b"));
    }

    #[test]
    fn retreat_stops_at_head() {
        let mut queue = PlaybackQueue::new();
        queue.push(track("a"));
        queue.retreat();
        assert_eq!(queue.current().map(|t| t.id.as_str()), Some("a"));
    }
}
