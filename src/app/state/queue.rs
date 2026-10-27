//! Queue persistence and state mutations for [`AppState`].

use crate::app::AppState;
use crate::domain::PlaybackQueue;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, QueueStore, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects an alternative queue store implementation and restores the queue.
    pub fn set_queue_store(&mut self, store: Box<dyn QueueStore>) {
        self.queue_store = store;
        self.restore_queue();
    }

    /// Restores the playback queue from storage, clamping cursor to valid bounds.
    pub fn restore_queue(&mut self) {
        let mut queue = self.queue_store.load();
        queue.clamp_cursor();
        self.queue = queue;
    }

    /// Persists the active playback queue to storage, logging errors without panicking.
    pub fn save_queue(&self) {
        if let Err(err) = self.queue_store.save(&self.queue) {
            tracing::warn!("failed to persist queue: {err:#}");
        }
    }

    /// Mutates the playback queue in-place and persists the updated queue.
    ///
    /// Persistence policy (SDD FR-13):
    /// All queue mutations pass through `mutate_queue`, which immediately persists the queue
    /// to `queue_store` (via atomic tmp+rename writes on disk). This ensures crash resilience
    /// without relying on graceful exit hooks or risking lost updates during unexpected termination.
    pub fn mutate_queue<R>(&mut self, f: impl FnOnce(&mut PlaybackQueue) -> R) -> R {
        let result = f(&mut self.queue);
        self.save_queue();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use crate::domain::{RepeatMode, Track};
    use crate::infra::MemoryQueueStore;

    #[test]
    fn test_app_state_queue_persistence_across_instances() {
        let store = MemoryQueueStore::new();

        // Instance 1: populate queue, advance cursor, set repeat mode
        let mut state1 = wired();
        state1.set_queue_store(Box::new(store.clone()));

        state1.mutate_queue(|q| {
            q.push(Track::new("track-1", "Track 1", "Artist 1"));
            q.push(Track::new("track-2", "Track 2", "Artist 2"));
            q.push(Track::new("track-3", "Track 3", "Artist 3"));
            q.advance();
            q.set_repeat(RepeatMode::All);
        });

        assert_eq!(state1.queue.len(), 3);
        assert_eq!(state1.queue.cursor(), 1);
        assert_eq!(state1.queue.repeat(), RepeatMode::All);

        // Instance 2: new state instance injected with the same store
        let mut state2 = wired();
        state2.set_queue_store(Box::new(store));

        assert_eq!(state2.queue.len(), 3);
        assert_eq!(state2.queue.cursor(), 1);
        assert_eq!(state2.queue.repeat(), RepeatMode::All);
        assert_eq!(state2.queue.tracks(), state1.queue.tracks());
    }

    #[test]
    fn test_app_state_queue_mutations_persist_automatically() {
        let store = MemoryQueueStore::new();
        let mut state1 = wired();
        state1.set_queue_store(Box::new(store.clone()));

        state1.search_text = "midnight".to_owned();
        state1.submit_search();
        state1.search_text = "sunrise".to_owned();
        state1.submit_search();

        assert_eq!(state1.queue.len(), 2);
        state1.next_track();
        assert_eq!(state1.queue.cursor(), 1);

        let mut state2 = wired();
        state2.set_queue_store(Box::new(store));
        assert_eq!(state2.queue.len(), 2);
        assert_eq!(state2.queue.cursor(), 1);
        assert_eq!(state2.queue.current().map(|t| t.title.as_str()), Some("sunrise"));
    }
}
