//! Slint platform integration: covers, lyrics, presence, and audio transport.
//! Hexagonal invariant: pure in-memory with zero I/O, audio hardware, or D-Bus.

use crate::domain::Track;

/// Dispatches a closure to execute on the Slint main event loop.
///
/// Thread boundary: all background UI mutations must go through this helper.
/// Call pattern:
/// `dispatch_to_ui(move || { if let Some(ui) = weak.upgrade() { ui.set_has_cover(true); } }).ok();`
pub fn dispatch_to_ui<F>(f: F) -> Result<(), slint::EventLoopError>
where
    F: FnOnce() + Send + 'static,
{
    slint::invoke_from_event_loop(f)
}

/// Converts raw RGBA pixel buffer to a Slint Image at the platform boundary.
pub fn rgba_to_image(rgba: &[u8], width: u32, height: u32) -> Option<slint::Image> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if rgba.len() != expected {
        return None;
    }
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba, width, height);
    Some(slint::Image::from_rgba8(buf))
}

/// Decoded cover art payload holding image cache key, RGBA bytes, and dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverArtData {
    pub url: String,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl CoverArtData {
    pub fn new(url: impl Into<String>, rgba: Vec<u8>, width: u32, height: u32) -> Self {
        Self { url: url.into(), rgba, width, height }
    }
}

/// Immutable snapshot of playback presence for Discord, MPRIS, and desktop presence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresenceSnapshot {
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
}

impl PresenceSnapshot {
    pub fn new(title: impl Into<String>, artist: impl Into<String>, is_playing: bool) -> Self {
        Self { title: title.into(), artist: artist.into(), is_playing }
    }

    pub fn from_track(track: &Track, is_playing: bool) -> Self {
        Self { title: track.title.clone(), artist: track.artist.clone(), is_playing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presence_snapshot_fields() {
        let track = Track::new("t1", "Bohemian Rhapsody", "Queen");
        let p = PresenceSnapshot::from_track(&track, true);
        assert_eq!(
            (p.title.as_str(), p.artist.as_str(), p.is_playing),
            ("Bohemian Rhapsody", "Queen", true)
        );
        let paused = PresenceSnapshot::from_track(&track, false);
        assert!(!paused.is_playing);
        let explicit = PresenceSnapshot::new("Starboy", "The Weeknd", true);
        assert_eq!((explicit.title.as_str(), explicit.artist.as_str()), ("Starboy", "The Weeknd"));
    }

    #[test]
    fn test_thread_boundary_channel_dispatch() {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            tx.send(PresenceSnapshot::new("Title", "Artist", true)).unwrap();
        });
        let snap = rx.recv().unwrap();
        handle.join().unwrap();
        assert_eq!(snap.title, "Title");
        assert!(snap.is_playing);
    }

    #[test]
    fn test_rgba_to_image() {
        assert!(rgba_to_image(&[], 0, 0).is_none());
        assert!(rgba_to_image(&[0, 0, 0], 1, 1).is_none());
        assert!(rgba_to_image(&[255, 0, 0, 255], 1, 1).is_some());
    }
}
