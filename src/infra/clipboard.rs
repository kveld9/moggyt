//! Clipboard port and adapters for plain-text writes.
//!
//! Adapter boundary: only this module touches the `arboard` crate.

use std::sync::Mutex;

use anyhow::{Context, Result};

/// Port for writing plain text to the system clipboard.
pub trait Clipboard: Send + Sync {
    /// Writes `text` to the clipboard.
    fn set_text(&self, text: &str) -> Result<()>;
}

/// Clipboard adapter backed by the operating system clipboard.
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn set_text(&self, text: &str) -> Result<()> {
        let mut clipboard =
            arboard::Clipboard::new().context("failed to open the system clipboard")?;
        clipboard.set_text(text.to_owned()).context("failed to write to the system clipboard")
    }
}

/// In-memory clipboard for tests and headless flows.
#[derive(Default)]
pub struct MemoryClipboard {
    last: Mutex<Option<String>>,
}

impl MemoryClipboard {
    /// Last text written through this clipboard, when any.
    pub fn last_text(&self) -> Option<String> {
        self.last.lock().ok().and_then(|guard| guard.clone())
    }
}

impl Clipboard for MemoryClipboard {
    fn set_text(&self, text: &str) -> Result<()> {
        let mut guard =
            self.last.lock().map_err(|_| anyhow::anyhow!("clipboard mutex poisoned"))?;
        *guard = Some(text.to_owned());
        Ok(())
    }
}

/// Writes `text` to the operating system clipboard.
pub fn copy_text(text: &str) -> Result<()> {
    SystemClipboard.set_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_keeps_the_last_text() {
        let clipboard = MemoryClipboard::default();
        assert_eq!(clipboard.last_text(), None);

        clipboard.set_text("https://music.youtube.com/watch?v=first").expect("first write");
        clipboard.set_text("https://music.youtube.com/watch?v=second").expect("second write");

        assert_eq!(
            clipboard.last_text().as_deref(),
            Some("https://music.youtube.com/watch?v=second")
        );
    }
}
