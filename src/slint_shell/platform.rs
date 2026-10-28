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

/// Flattened display row for the Slint lyrics list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricDisplayLine {
    pub index: i32,
    pub text: String,
}

impl LyricDisplayLine {
    pub fn new(index: i32, text: impl Into<String>) -> Self {
        Self { index, text: text.into() }
    }
}

pub fn lyrics_display_lines(lyrics: &LyricsState) -> Vec<LyricDisplayLine> {
    lyrics.lines.iter().enumerate().map(|(i, (_, t))| LyricDisplayLine::new(i as i32, t)).collect()
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

/// Synced lyrics state with line timestamp indexation and time offset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricsState {
    pub lines: Vec<(u64, String)>,
    pub offset_ms: i64,
}

impl LyricsState {
    pub fn new(lines: Vec<(u64, String)>) -> Self {
        Self { lines, offset_ms: 0 }
    }

    pub fn set_offset_ms(&mut self, offset_ms: i64) {
        self.offset_ms = offset_ms;
    }

    pub fn line_at(&self, ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let effective_ms = (ms as i64).saturating_add(self.offset_ms);
        if effective_ms < 0 {
            return None;
        }
        let target = effective_ms as u64;
        if target < self.lines[0].0 {
            return None;
        }
        let idx = self.lines.partition_point(|(line_ms, _)| *line_ms <= target);
        if idx == 0 { None } else { Some(idx - 1) }
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

/// In-memory platform state holding covers, lyrics, and presence models.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformState {
    pub cover: Option<CoverArtData>,
    pub lyrics: LyricsState,
    pub presence: Option<PresenceSnapshot>,
}

impl PlatformState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_cover(&mut self, url: impl Into<String>, rgba: Vec<u8>, width: u32, height: u32) {
        self.cover = Some(CoverArtData::new(url, rgba, width, height));
    }

    pub fn clear_cover(&mut self) {
        self.cover = None;
    }

    pub fn set_lyrics(&mut self, lyrics: LyricsState) {
        self.lyrics = lyrics;
    }

    pub fn update_presence(&mut self, track: &Track, is_playing: bool) {
        self.presence = Some(PresenceSnapshot::from_track(track, is_playing));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportCommand {
    Play(String),
    Pause,
    Next,
    Prev,
    Seek(u64),
    SetVolume(u8),
}

/// Narrow transport boundary mirroring audio and playback commands.
pub trait Transport {
    fn play(&mut self, track_id: &str);
    fn pause(&mut self);
    fn next(&mut self);
    fn prev(&mut self);
    fn seek(&mut self, position_secs: u64);
    fn set_volume(&mut self, volume: u8);
    fn volume(&self) -> u8;
    fn is_playing(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportStub {
    pub is_playing: bool,
    pub volume: u8,
    pub commands: Vec<TransportCommand>,
}

impl Default for TransportStub {
    fn default() -> Self {
        Self { is_playing: false, volume: 100, commands: Vec::new() }
    }
}

impl TransportStub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commands(&self) -> &[TransportCommand] {
        &self.commands
    }
}

impl Transport for TransportStub {
    fn play(&mut self, track_id: &str) {
        self.is_playing = true;
        self.commands.push(TransportCommand::Play(track_id.to_string()));
    }

    fn pause(&mut self) {
        self.is_playing = false;
        self.commands.push(TransportCommand::Pause);
    }

    fn next(&mut self) {
        self.commands.push(TransportCommand::Next);
    }

    fn prev(&mut self) {
        self.commands.push(TransportCommand::Prev);
    }

    fn seek(&mut self, position_secs: u64) {
        self.commands.push(TransportCommand::Seek(position_secs));
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.commands.push(TransportCommand::SetVolume(self.volume));
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn is_playing(&self) -> bool {
        self.is_playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lyrics_line_at_boundaries() {
        assert_eq!(LyricsState::new(vec![]).line_at(1000), None);
        let s = LyricsState::new(vec![
            (1000, "L1".to_string()),
            (3000, "L2".to_string()),
            (5000, "L3".to_string()),
        ]);
        for ms in [0, 999] {
            assert_eq!(s.line_at(ms), None);
        }
        for ms in [1000, 1500] {
            assert_eq!(s.line_at(ms), Some(0));
        }
        for ms in [3000, 4500] {
            assert_eq!(s.line_at(ms), Some(1));
        }
        for ms in [5000, 9999] {
            assert_eq!(s.line_at(ms), Some(2));
        }
    }

    #[test]
    fn test_lyrics_offset_shifting() {
        let mut s = LyricsState::new(vec![(1000, "L1".to_string()), (3000, "L2".to_string())]);
        s.set_offset_ms(500);
        for (ms, exp) in [(499, None), (500, Some(0)), (2500, Some(1))] {
            assert_eq!(s.line_at(ms), exp);
        }
        s.set_offset_ms(-500);
        for (ms, exp) in [(1000, None), (1500, Some(0))] {
            assert_eq!(s.line_at(ms), exp);
        }
        s.set_offset_ms(-10000);
        assert_eq!(s.line_at(1000), None);
    }

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
    fn test_cover_cache_key_replacement() {
        let mut s = PlatformState::new();
        assert_eq!(s.cover, None);
        s.set_cover("u1", vec![1, 2, 3, 4], 1, 1);
        let c1 = s.cover.as_ref().unwrap();
        assert_eq!(
            (c1.url.as_str(), c1.rgba.as_slice(), c1.width, c1.height),
            ("u1", &[1, 2, 3, 4][..], 1, 1)
        );
        s.set_cover("u2", vec![5, 6, 7, 8], 1, 1);
        assert_eq!(s.cover.as_ref().unwrap().url, "u2");
        s.clear_cover();
        assert_eq!(s.cover, None);
    }

    #[test]
    fn test_transport_stub_call_recording() {
        let mut stub = TransportStub::new();
        assert!(!stub.is_playing() && stub.volume() == 100);
        stub.play("track-1");
        stub.pause();
        stub.next();
        stub.prev();
        stub.seek(60);
        stub.set_volume(80);
        stub.set_volume(120);
        assert_eq!(stub.volume(), 100);
        assert_eq!(
            stub.commands(),
            &[
                TransportCommand::Play("track-1".into()),
                TransportCommand::Pause,
                TransportCommand::Next,
                TransportCommand::Prev,
                TransportCommand::Seek(60),
                TransportCommand::SetVolume(80),
                TransportCommand::SetVolume(100),
            ]
        );
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
    fn test_rgba_to_image_and_lyrics_display_lines() {
        assert!(rgba_to_image(&[], 0, 0).is_none());
        assert!(rgba_to_image(&[0, 0, 0], 1, 1).is_none());
        assert!(rgba_to_image(&[255, 0, 0, 255], 1, 1).is_some());
        let lines =
            lyrics_display_lines(&LyricsState::new(vec![(0, "A".into()), (1000, "B".into())]));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], LyricDisplayLine::new(0, "A"));
        assert_eq!(lines[1], LyricDisplayLine::new(1, "B"));
    }
}
