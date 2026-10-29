//! yt-dlp sidecar stream resolver and provisioning adapter.
//!
//! Provides direct stream URL resolution by delegating to a bundled or
//! locally provisioned yt-dlp binary. Implements the [`StreamResolver`] port
//! from [`super::stream`].

use std::path::{Path, PathBuf};

use anyhow::Context;

use super::stream::{AudioQuality, StreamResolver, audio_quality_format};

/// Update interval for checking yt-dlp release freshness (3 days in seconds).
pub const YTDLP_UPDATE_INTERVAL_SECS: u64 = 3 * 24 * 60 * 60;

/// Default format selector for audio streams (alias for standard quality preference).
pub const YTDLP_AUDIO_FORMAT: &str = audio_quality_format(AudioQuality::Standard);

/// Constructs a canonical YouTube watch URL for a video ID.
pub fn watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// Parses the first non-blank trimmed line of yt-dlp output.
pub fn parse_version(output: &str) -> Option<String> {
    output.lines().map(str::trim).find(|line| !line.is_empty()).map(ToOwned::to_owned)
}

/// Evaluates whether the sidecar binary requires an update given its age in seconds.
pub fn sidecar_needs_update(age_secs: u64) -> bool {
    age_secs >= YTDLP_UPDATE_INTERVAL_SECS
}

/// Formats the CLI arguments passed to yt-dlp for extracting the audio URL using the default quality.
pub fn format_args(video_id: &str) -> Vec<String> {
    format_args_with_quality(video_id, AudioQuality::default())
}

/// Formats the CLI arguments passed to yt-dlp for extracting the audio URL with a quality preference.
pub fn format_args_with_quality(video_id: &str, quality: AudioQuality) -> Vec<String> {
    vec![
        "-f".to_string(),
        audio_quality_format(quality).to_string(),
        "-g".to_string(),
        watch_url(video_id),
    ]
}

/// Returns the platform-specific release binary asset name for yt-dlp standalone releases.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_asset_name() -> &'static str {
    "yt-dlp_linux"
}

/// Returns the platform-specific release binary asset name for yt-dlp standalone releases.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub fn release_asset_name() -> &'static str {
    "yt-dlp.exe"
}

/// Returns the platform-specific release binary asset name for yt-dlp standalone releases.
///
/// Note: macOS is documented unsupported for native builds, but asset name is retained for portability.
#[cfg(target_os = "macos")]
pub fn release_asset_name() -> &'static str {
    "yt-dlp_macos"
}

/// Returns the platform-specific release binary asset name for yt-dlp standalone releases.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64"),
    target_os = "macos"
)))]
pub fn release_asset_name() -> &'static str {
    "yt-dlp_linux"
}

/// Target executable binary filename for the current operating system.
pub fn sidecar_binary_name() -> &'static str {
    if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" }
}

/// yt-dlp stream resolver invoking an external binary.
#[derive(Debug, Clone)]
pub struct YtDlpResolver {
    binary: PathBuf,
    timeout_secs: u64,
    quality: AudioQuality,
}

impl YtDlpResolver {
    /// Creates a resolver with default timeout (30 seconds) and standard quality.
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self { binary: binary.into(), timeout_secs: 30, quality: AudioQuality::default() }
    }

    /// Mutates the preferred audio quality in place.
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
    }

    /// Returns the currently configured audio quality preference.
    pub fn quality(&self) -> AudioQuality {
        self.quality
    }

    /// Path to the configured yt-dlp binary.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Configured execution timeout in seconds.
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// Resolves `video_id` to a direct stream URL.
    ///
    /// Soft-fails to `None` on blank video IDs, missing binaries, or process failures.
    pub fn resolve(&self, video_id: &str) -> Option<String> {
        self.fetch_audio_url(video_id).ok()
    }

    /// Resolves `video_id` to a stream URL with explicit error details.
    pub fn fetch_audio_url(&self, video_id: &str) -> anyhow::Result<String> {
        let id = validate_video_id(video_id)?;
        let output = spawn_ytdlp(&self.binary, id, self.quality)?;
        extract_stream_url(&output.stdout)
    }
}

fn validate_video_id(video_id: &str) -> anyhow::Result<&str> {
    let trimmed = video_id.trim();
    if trimmed.is_empty() {
        anyhow::bail!("cannot resolve audio url for blank video id");
    }
    Ok(trimmed)
}

fn spawn_ytdlp(
    binary: &Path,
    video_id: &str,
    quality: AudioQuality,
) -> anyhow::Result<std::process::Output> {
    let output = std::process::Command::new(binary)
        .arg("--no-playlist")
        .args(format_args_with_quality(video_id, quality))
        .output()
        .with_context(|| format!("failed to spawn yt-dlp at {binary:?}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("yt-dlp exited with status {}: {}", output.status, stderr.trim());
    }

    Ok(output)
}

fn extract_stream_url(stdout_bytes: &[u8]) -> anyhow::Result<String> {
    let stdout = std::str::from_utf8(stdout_bytes).context("yt-dlp stdout is not valid utf-8")?;
    parse_version(stdout).ok_or_else(|| anyhow::anyhow!("yt-dlp returned no stream url in stdout"))
}

impl StreamResolver for YtDlpResolver {
    fn resolve(&self, video_id: &str) -> Option<String> {
        YtDlpResolver::resolve(self, video_id)
    }
}

/// Checks whether an existing sidecar binary at `path` needs updating.
fn sidecar_file_needs_update(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return true;
    };
    sidecar_needs_update(elapsed.as_secs())
}

/// Sets executable permissions on Unix systems (mode 0o755).
#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set executable permissions on {path:?}"))
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Downloads a binary release from `url` directly to `target_path`.
fn download_sidecar(target_path: &Path, url: &str) -> anyhow::Result<()> {
    let response = reqwest::blocking::get(url)
        .with_context(|| format!("failed to request yt-dlp release from {url}"))?
        .error_for_status()
        .with_context(|| format!("http error downloading yt-dlp release from {url}"))?;

    let bytes =
        response.bytes().with_context(|| format!("failed to read payload bytes from {url}"))?;

    let temp_path = target_path.with_extension("download");
    std::fs::write(&temp_path, &bytes)
        .with_context(|| format!("failed to write payload to {temp_path:?}"))?;

    set_executable_permissions(&temp_path)?;

    std::fs::rename(&temp_path, target_path)
        .with_context(|| format!("failed to rename {temp_path:?} to {target_path:?}"))?;

    Ok(())
}

/// Ensures the yt-dlp sidecar binary is provisioned and up to date in `data_dir`.
///
/// If the binary does not exist or its last modification age exceeds [`YTDLP_UPDATE_INTERVAL_SECS`],
/// it is downloaded from the latest GitHub release and marked executable.
pub fn ensure_sidecar(data_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create sidecar data directory at {data_dir:?}"))?;

    let binary_path = data_dir.join(sidecar_binary_name());
    if binary_path.exists() && !sidecar_file_needs_update(&binary_path) {
        return Ok(binary_path);
    }

    let asset = release_asset_name();
    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}");
    download_sidecar(&binary_path, &url)?;

    Ok(binary_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_url_shape() {
        assert_eq!(watch_url("dQw4w9WgXcQ"), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(watch_url("abc-123_XYZ"), "https://www.youtube.com/watch?v=abc-123_XYZ");
    }

    #[test]
    fn parse_version_variants() {
        let cases = [
            ("2024.10.07\n", Some("2024.10.07".to_string())),
            ("\n  \n2025.01.15\nsecond line\n", Some("2025.01.15".to_string())),
            ("", None),
            ("   \n\t  \r\n  ", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_version(input), expected, "failed on input {input:?}");
        }
    }

    #[test]
    fn sidecar_needs_update_edges() {
        let cases = [
            (0, false),
            (YTDLP_UPDATE_INTERVAL_SECS - 1, false),
            (YTDLP_UPDATE_INTERVAL_SECS, true),
            (YTDLP_UPDATE_INTERVAL_SECS + 1, true),
            (u64::MAX, true),
        ];

        for (age, expected) in cases {
            assert_eq!(sidecar_needs_update(age), expected, "failed for age {age}");
        }
    }

    #[test]
    fn format_args_exact_vec() {
        let args = format_args("dQw4w9WgXcQ");
        let expected = vec![
            "-f".to_string(),
            "bestaudio[ext=m4a]/bestaudio/best".to_string(),
            "-g".to_string(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn release_asset_name_non_empty_and_valid() {
        let name = release_asset_name();
        assert!(!name.is_empty());
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(name, "yt-dlp_linux");
    }

    #[test]
    fn resolve_soft_fails_on_blank_id_without_spawning() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        assert_eq!(resolver.resolve(""), None);
        assert_eq!(resolver.resolve("   "), None);
    }

    #[test]
    fn resolve_soft_fails_on_missing_binary() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        assert_eq!(resolver.resolve("dQw4w9WgXcQ"), None);
        assert!(resolver.fetch_audio_url("dQw4w9WgXcQ").is_err());
    }

    #[test]
    fn format_args_with_quality_table() {
        for (q, expected_fmt) in [
            (
                AudioQuality::Compact,
                "bestaudio[ext=m4a][abr<=64]/bestaudio[ext=m4a]/bestaudio/best",
            ),
            (AudioQuality::Standard, "bestaudio[ext=m4a]/bestaudio/best"),
            (AudioQuality::High, "bestaudio[ext=m4a][abr>=192]/bestaudio[ext=m4a]/bestaudio/best"),
        ] {
            let args = format_args_with_quality("v123", q);
            assert_eq!(
                args,
                vec![
                    "-f".to_string(),
                    expected_fmt.to_string(),
                    "-g".to_string(),
                    "https://www.youtube.com/watch?v=v123".to_string(),
                ]
            );
        }
    }

    #[test]
    fn resolver_default_quality_is_standard_and_mutators_work() {
        let mut resolver = YtDlpResolver::new("/bin/dummy");
        assert_eq!(resolver.quality(), AudioQuality::Standard);

        resolver.set_quality(AudioQuality::High);
        assert_eq!(resolver.quality(), AudioQuality::High);

        resolver.set_quality(AudioQuality::Compact);
        assert_eq!(resolver.quality(), AudioQuality::Compact);
    }
}
