//! Active lyric line and word-timing resolution for the Slint shell.

use super::state::SlintAppState;
use crate::infra::AudioBackend;

impl SlintAppState {
    /// Resolves 0-based active lyric line index for current playback timestamp.
    pub fn active_lyric_index(&self) -> i32 {
        let pos_secs = self.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0);
        self.active_lyric_index_at(pos_secs as u32)
    }

    /// Resolves active lyric line index for an explicitly passed timestamp in seconds.
    pub fn active_lyric_index_at(&self, pos_secs: u32) -> i32 {
        let pos_ms = (pos_secs as u64) * 1000;
        if self.lyrics_lines.is_empty() {
            return -1;
        }
        let idx = self.lyrics_lines.partition_point(|l| l.start_ms <= pos_ms);
        if idx == 0 { -1 } else { (idx - 1) as i32 }
    }

    /// Returns the sung prefix and the remaining suffix of the active lyric line.
    ///
    /// Both strings are empty when no lyric line is active. Lines without word
    /// timings return an empty prefix and the full text, preserving the line-level
    /// highlight.
    pub fn active_lyric_text_split(&self) -> (String, String) {
        let pos_secs = self.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0);
        self.active_lyric_text_split_at(pos_secs * 1000)
    }

    /// Same as [`Self::active_lyric_text_split`] for an explicit position in milliseconds.
    pub fn active_lyric_text_split_at(&self, pos_ms: u64) -> (String, String) {
        let idx = self.active_lyric_index_at((pos_ms / 1000) as u32);
        let Some(line) = usize::try_from(idx).ok().and_then(|i| self.lyrics_lines.get(i)) else {
            return (String::new(), String::new());
        };
        line.split_at_fraction(line.sung_fraction(pos_ms))
    }
}

#[cfg(test)]
mod tests {
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_empty_lyrics_return_negative_one() {
        let state = test_wired();
        assert_eq!(state.active_lyric_index(), -1);
        assert_eq!(state.active_lyric_index_at(0), -1);
        assert_eq!(state.active_lyric_text_split(), (String::new(), String::new()));
        assert_eq!(state.active_lyric_text_split_at(0), (String::new(), String::new()));
    }
}
