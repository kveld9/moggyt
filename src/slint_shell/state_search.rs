//! Search state extensions for SlintAppState.

use super::state::SlintAppState;

impl SlintAppState {
    /// Clears the lyrics-search mode when a new catalog search starts.
    pub fn clear_lyrics_search(&mut self) {
        self.lyrics_search = false;
    }
}

#[cfg(test)]
mod tests {
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_lyrics_search_mode_clearing() {
        let mut state = test_wired();
        state.lyrics_search = true;
        state.clear_lyrics_search();
        assert!(!state.lyrics_search);
        state.lyrics_search = true;
        state.clear_search();
        assert!(!state.lyrics_search);
    }
}
