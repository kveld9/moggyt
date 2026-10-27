//! Local lyric override filesystem storage keyed by videoId.
//!
//! Stores user-managed synced lyric files in `<config>/lyrics/<sanitized-id>.lrc`.
//! Overrides take precedence over remote lyrics provider lookups for matched tracks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::lyrics::SyncedLyrics;
use super::lyrics_lrclib::parse_lrc;
use super::settings::default_config_dir;

/// Maximum permitted lyric override file size in bytes (1 MiB).
pub const MAX_OVERRIDE_BYTES: usize = 1024 * 1024;

static TMP_OVERRIDE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn has_path_traversal(id: &str) -> bool {
    id.contains('/') || id.contains('\\') || id.contains("..") || id.starts_with('.')
}

fn has_invalid_chars(id: &str) -> bool {
    id.chars().any(|c| c.is_ascii_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Sanitizes a video ID for safe use as a file stem.
///
/// Path safety sanitization rules:
/// 1. Leading and trailing whitespace is trimmed.
/// 2. Empty strings or strings with only whitespace are rejected (`None`).
/// 3. Path separators (`/` and `\`) and directory traversal sequences (`..`) are rejected (`None`).
/// 4. Hidden file stems starting with `.` are rejected (`None`).
/// 5. Control characters and filesystem-reserved characters (`:`, `*`, `?`, `"`, `<`, `>`, `|`) are rejected (`None`).
/// 6. Safe strings return `Some(sanitized_id)` and map to `<sanitized_id>.lrc`.
pub fn sanitize_video_id(video_id: &str) -> Option<String> {
    let trimmed = video_id.trim();
    if trimmed.is_empty() || has_path_traversal(trimmed) || has_invalid_chars(trimmed) {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Alias for [`sanitize_video_id`].
pub fn sanitize_id(id: &str) -> Option<String> {
    sanitize_video_id(id)
}

/// Resolves standard platform lyrics override directory (`<config>/lyrics`).
pub fn default_lyrics_dir() -> PathBuf {
    default_config_dir().join("lyrics")
}

/// Directory holding local lyric override files (*.lrc) keyed by videoId.
///
/// File convention: `<dir>/<sanitized-video-id>.lrc`.
/// Files are user-managed. Content refreshes on track change like provider content
/// without a background filesystem watcher.
#[derive(Clone, Debug)]
pub struct LyricOverrideDir {
    dir: PathBuf,
}

impl LyricOverrideDir {
    /// Creates a lyric override directory handle pointing to `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Path to the directory backing local lyric overrides.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Computes the absolute path for a track's override file if the video ID is safe.
    pub fn override_path(&self, video_id: &str) -> Option<PathBuf> {
        let sanitized = sanitize_video_id(video_id)?;
        Some(self.dir.join(format!("{sanitized}.lrc")))
    }

    /// Loads synced lyrics from `<dir>/<sanitized-id>.lrc`.
    ///
    /// Missing, unreadable, or invalid files yield `None`. Empty parses yield `None`.
    /// Never panics on corrupt or foreign file input.
    pub fn load(&self, video_id: &str) -> Option<SyncedLyrics> {
        let path = self.override_path(video_id)?;
        let text = std::fs::read_to_string(path).ok()?;
        let lines = parse_lrc(&text);
        if lines.is_empty() {
            return None;
        }
        Some(SyncedLyrics { lines })
    }

    /// Atomically saves LRC text to `<dir>/<sanitized-id>.lrc` via a temporary file.
    ///
    /// Creates parent directories on demand. Caps content size at [`MAX_OVERRIDE_BYTES`] (1 MiB).
    pub fn save(&self, video_id: &str, lrc_text: &str) -> anyhow::Result<()> {
        if lrc_text.len() > MAX_OVERRIDE_BYTES {
            anyhow::bail!(
                "lyric override exceeds maximum size limit of {MAX_OVERRIDE_BYTES} bytes (got {} bytes)",
                lrc_text.len()
            );
        }
        let dest = self
            .override_path(video_id)
            .ok_or_else(|| anyhow::anyhow!("invalid video id for lyric override: {video_id:?}"))?;
        std::fs::create_dir_all(&self.dir)?;
        self.write_atomic(&dest, lrc_text.as_bytes())
    }

    fn write_atomic(&self, dest: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        let file_stem = dest.file_name().and_then(|n| n.to_str()).unwrap_or("override");
        let count = TMP_OVERRIDE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = self.dir.join(format!("{file_stem}.tmp.{}.{count}", std::process::id()));

        if let Err(err) = std::fs::write(&tmp, bytes) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err.into());
        }
        if let Err(err) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err.into());
        }
        Ok(())
    }
}

impl Default for LyricOverrideDir {
    fn default() -> Self {
        Self::new(default_lyrics_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_rejects_traversal_ids() {
        for traversal in ["../evil", "foo/bar", "foo\\bar", "..", ".", ".hidden"] {
            assert!(
                sanitize_video_id(traversal).is_none(),
                "expected traversal ID '{traversal}' to be rejected"
            );
            assert!(
                sanitize_id(traversal).is_none(),
                "expected sanitize_id to reject traversal '{traversal}'"
            );
        }

        for invalid in ["", "   ", "has:colon", "has*star", "has?mark", "has|pipe"] {
            assert!(
                sanitize_video_id(invalid).is_none(),
                "expected invalid ID '{invalid}' to be rejected"
            );
        }

        let dir = LyricOverrideDir::new("/tmp/test-lyrics");
        assert!(dir.override_path("../evil").is_none());
        assert!(dir.override_path("foo/bar").is_none());

        assert_eq!(sanitize_video_id("dQw4w9WgXcQ"), Some("dQw4w9WgXcQ".to_string()));
        assert_eq!(sanitize_video_id("track_123-safe"), Some("track_123-safe".to_string()));
        assert_eq!(
            dir.override_path("dQw4w9WgXcQ"),
            Some(PathBuf::from("/tmp/test-lyrics/dQw4w9WgXcQ.lrc"))
        );
    }

    #[test]
    fn test_save_load_round_trip() {
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let dir = LyricOverrideDir::new(&temp_dir);

        let lrc = "[00:01.00]First line\n[00:05.50]Second line\n";
        dir.save("valid_vid_1", lrc).expect("save valid lrc");

        let loaded = dir.load("valid_vid_1").expect("load saved lyrics");
        assert_eq!(loaded.lines.len(), 2);
        assert_eq!(loaded.lines[0].start_ms, 1000);
        assert_eq!(loaded.lines[0].text, "First line");
        assert_eq!(loaded.lines[1].start_ms, 5500);
        assert_eq!(loaded.lines[1].text, "Second line");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_corrupt_invalid_lrc_yields_none() {
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let dir = LyricOverrideDir::new(&temp_dir);

        let corrupt = "This is not LRC\nNo timestamps anywhere\nJust random sentences";
        dir.save("corrupt_vid", corrupt).expect("save corrupt file");

        assert_eq!(dir.load("corrupt_vid"), None);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_missing_file_yields_none() {
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let dir = LyricOverrideDir::new(&temp_dir);

        assert_eq!(dir.load("non_existent_vid"), None);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_oversize_rejected() {
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-oversize-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let dir = LyricOverrideDir::new(&temp_dir);

        let oversize = "a".repeat(MAX_OVERRIDE_BYTES + 1);
        let res = dir.save("oversize_vid", &oversize);
        assert!(res.is_err());

        assert_eq!(dir.load("oversize_vid"), None);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
