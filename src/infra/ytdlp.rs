//! yt-dlp sidecar stream resolver and provisioning adapter.
//!
//! Provides direct stream URL resolution by delegating to a bundled or
//! locally provisioned yt-dlp binary. Implements the [`StreamResolver`] port
//! from [`super::stream`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use anyhow::Context;

use super::stream::{
    AudioQuality, StreamResolver, audio_quality_degraded_format, audio_quality_format,
};
use super::stream_url_cache::StreamUrlCache;

/// Update interval for checking yt-dlp release freshness (3 days in seconds).
pub const YTDLP_UPDATE_INTERVAL_SECS: u64 = 3 * 24 * 60 * 60;

/// Cooldown period in seconds between forced refresh attempts on resolution failures (1 hour).
pub const YTDLP_REFRESH_COOLDOWN_SECS: u64 = 60 * 60;

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

/// Evaluates whether a refresh is due given the elapsed duration in seconds since the last refresh.
///
/// An elapsed duration of `None` indicates unknown or never refreshed, which is considered due.
pub fn is_refresh_due(elapsed_secs: Option<u64>) -> bool {
    elapsed_secs.is_none_or(|s| s >= YTDLP_REFRESH_COOLDOWN_SECS)
}

/// Formats the CLI arguments passed to yt-dlp for extracting the audio URL using the default quality.
pub fn format_args(video_id: &str) -> Vec<String> {
    format_args_with_quality(video_id, AudioQuality::default())
}

/// Formats the CLI arguments passed to yt-dlp for extracting the audio URL with a quality preference.
pub fn format_args_with_quality(video_id: &str, quality: AudioQuality) -> Vec<String> {
    format_args_with_selector(video_id, audio_quality_format(quality))
}

/// Formats the CLI arguments passed to yt-dlp for extracting the audio URL with an explicit selector.
pub fn format_args_with_selector(video_id: &str, selector: &str) -> Vec<String> {
    vec!["-f".to_string(), selector.to_string(), "-g".to_string(), watch_url(video_id)]
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

/// Returns the public download URL for the latest yt-dlp release binary asset.
pub fn release_download_url() -> String {
    format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}", release_asset_name())
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
    cache: StreamUrlCache,
}

impl YtDlpResolver {
    /// Creates a resolver with default timeout (30 seconds) and standard quality.
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            timeout_secs: 30,
            quality: AudioQuality::default(),
            cache: StreamUrlCache::default(),
        }
    }

    /// Borrows the internal stream URL cache.
    pub fn cache(&self) -> &StreamUrlCache {
        &self.cache
    }

    /// Prefetches and caches the stream URL for `video_id`.
    ///
    /// Returns `true` if resolution succeeds or if the URL is already cached.
    /// Returns `false` on blank video IDs or resolution failures.
    pub fn prefetch(&self, video_id: &str) -> bool {
        let Ok(id) = validate_video_id(video_id) else {
            return false;
        };
        if self.cache.get(id, self.quality, Instant::now()).is_some() {
            return true;
        }
        self.fetch_audio_url(id).is_ok()
    }

    /// Invalidates cached stream URLs for `video_id`.
    pub fn invalidate(&self, video_id: &str) {
        self.cache.invalidate(video_id);
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
        if let Some(url) = self.cache.get(id, self.quality, Instant::now()) {
            return Ok(url);
        }
        let url = self.resolve_uncached(id)?;
        self.cache.insert(id, self.quality, url.clone(), Instant::now());
        Ok(url)
    }

    fn resolve_uncached(&self, id: &str) -> anyhow::Result<String> {
        match self.extract_url(id) {
            Ok(url) => Ok(url),
            Err(first_err) => {
                if refresh_sidecar_if_due(&self.binary) {
                    self.extract_url(id)
                } else {
                    Err(first_err)
                }
            }
        }
    }

    fn extract_url(&self, video_id: &str) -> anyhow::Result<String> {
        let selector = audio_quality_format(self.quality);
        match spawn_ytdlp(&self.binary, video_id, selector) {
            Ok(output) => extract_stream_url(&output.stdout),
            Err(err) if is_format_unavailable(&err.to_string()) => {
                self.extract_url_degraded(video_id, err)
            }
            Err(err) => Err(err),
        }
    }

    fn extract_url_degraded(
        &self,
        video_id: &str,
        primary_err: anyhow::Error,
    ) -> anyhow::Result<String> {
        let Some(selector) = audio_quality_degraded_format(self.quality) else {
            return Err(primary_err);
        };
        tracing::warn!(
            video_id = %video_id,
            "no stream matches the compact tier; retrying with the best available audio"
        );
        let output = spawn_ytdlp(&self.binary, video_id, selector)?;
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

/// Returns whether a yt-dlp failure means the requested format selector matched nothing.
///
/// yt-dlp prints this message when `-f` selects no available stream. A spawn error, a network
/// failure, or a bot check must propagate instead of triggering the degraded retry.
fn is_format_unavailable(stderr: &str) -> bool {
    stderr.contains("Requested format is not available")
}

fn spawn_ytdlp(
    binary: &Path,
    video_id: &str,
    selector: &str,
) -> anyhow::Result<std::process::Output> {
    let output = std::process::Command::new(binary)
        .arg("--no-playlist")
        .args(format_args_with_selector(video_id, selector))
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

/// Fetches the sidecar release payload from `url`.
fn fetch_sidecar_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let response = reqwest::blocking::get(url)
        .with_context(|| format!("failed to request yt-dlp release from {url}"))?
        .error_for_status()
        .with_context(|| format!("http error downloading yt-dlp release from {url}"))?;

    let bytes =
        response.bytes().with_context(|| format!("failed to read payload bytes from {url}"))?;

    Ok(bytes.to_vec())
}

/// Writes the payload to a temporary file, marks it executable and moves it into place.
fn write_sidecar_atomically(target_path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temp_path = target_path.with_extension("download");
    std::fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write payload to {temp_path:?}"))?;

    set_executable_permissions(&temp_path)?;

    std::fs::rename(&temp_path, target_path)
        .with_context(|| format!("failed to rename {temp_path:?} to {target_path:?}"))?;

    Ok(())
}

/// Downloads a binary release from `url` directly to `target_path`.
fn download_sidecar(target_path: &Path, url: &str) -> anyhow::Result<()> {
    let bytes = fetch_sidecar_bytes(url)?;
    write_sidecar_atomically(target_path, &bytes)
}

#[cfg(unix)]
fn is_executable_mode(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable_mode(_metadata: &std::fs::Metadata) -> bool {
    true
}

fn is_non_empty_file(metadata: &std::fs::Metadata) -> bool {
    if !metadata.is_file() {
        return false;
    }
    metadata.len() > 0
}

fn has_html_prefix(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return true;
    };
    let mut buffer = [0u8; 512];
    let Ok(read_bytes) = file.read(&mut buffer) else {
        return true;
    };
    buffer[..read_bytes].iter().find(|b| !b.is_ascii_whitespace()).is_some_and(|b| *b == b'<')
}

fn spawn_version_probe(path: &Path) -> Option<std::process::Child> {
    std::process::Command::new(path)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

fn wait_child_bounded(mut child: std::process::Child, timeout: std::time::Duration) -> bool {
    let start = Instant::now();
    let step = std::time::Duration::from_millis(20);
    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(step),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

fn probe_version(path: &Path) -> bool {
    let Some(child) = spawn_version_probe(path) else {
        return false;
    };
    wait_child_bounded(child, std::time::Duration::from_secs(5))
}

/// Returns whether a sidecar candidate is a usable executable.
pub fn is_valid_sidecar(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !is_non_empty_file(&metadata) {
        return false;
    }
    if has_html_prefix(path) {
        return false;
    }
    if !is_executable_mode(&metadata) {
        return false;
    }
    probe_version(path)
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn warn_invalid_existing(path: &Path) {
    if path.exists() {
        tracing::warn!(path = %path.display(), "existing yt-dlp sidecar is invalid; re-downloading");
    }
}

/// Provisions the yt-dlp sidecar without requiring freshness.
///
/// A valid existing binary is returned immediately (no download, no refresh).
/// A missing or invalid binary is downloaded once and validated.
pub fn provision_sidecar(data_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create sidecar data directory at {data_dir:?}"))?;

    let path = data_dir.join(sidecar_binary_name());
    if is_valid_sidecar(&path) {
        return Ok(path);
    }

    warn_invalid_existing(&path);

    let url = release_download_url();
    download_sidecar(&path, &url)?;

    if !is_valid_sidecar(&path) {
        anyhow::bail!("downloaded yt-dlp sidecar at '{}' is invalid", path.display());
    }

    Ok(path)
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn refresh_thread_worker(binary: &Path) {
    let outcome = refresh_sidecar_if_due(binary);
    tracing::info!(binary = %binary.display(), refreshed = outcome, "sidecar background refresh finished");
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
/// Refreshes the sidecar in the background when it is stale; never blocks startup.
pub fn spawn_sidecar_refresh(binary: PathBuf) {
    if !sidecar_file_needs_update(&binary) {
        return;
    }

    let spawn_result = std::thread::Builder::new()
        .name("ytdlp-refresh".to_string())
        .spawn(move || refresh_thread_worker(&binary));

    if let Err(err) = spawn_result {
        tracing::warn!(error = ?err, "failed to spawn sidecar refresh thread");
    }
}

/// Global lock recording the timestamp of the last forced-refresh attempt.
static REFRESH_STATE: Mutex<Option<Instant>> = Mutex::new(None);

fn file_age_secs(path: &Path) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    Some(elapsed.as_secs())
}

fn can_refresh(last_attempt: Option<Instant>, binary: &Path) -> bool {
    let attempt_elapsed = last_attempt.map(|t| t.elapsed().as_secs());
    is_refresh_due(attempt_elapsed) && is_refresh_due(file_age_secs(binary))
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn execute_refresh(binary: &Path) -> bool {
    let url = release_download_url();
    match download_sidecar(binary, &url) {
        Ok(()) => {
            tracing::info!(binary = %binary.display(), "refreshed yt-dlp sidecar binary");
            true
        }
        Err(err) => {
            tracing::warn!(
                binary = %binary.display(),
                error = ?err,
                "failed to refresh yt-dlp sidecar binary"
            );
            false
        }
    }
}

/// Attempts to force-refresh the sidecar binary from the latest GitHub release if due.
///
/// Refreshes only when the binary file exists on disk (a missing binary is a provisioning
/// failure owned by [`provision_sidecar`] at startup, not an extraction breakage).
///
/// Holding [`REFRESH_STATE`] during download ensures concurrent resolvers wait and skip
/// redundant downloads.
fn refresh_sidecar_if_due(binary: &Path) -> bool {
    if !binary.is_file() {
        return false;
    }

    let mut guard = REFRESH_STATE.lock().unwrap_or_else(PoisonError::into_inner);
    if !can_refresh(*guard, binary) {
        return false;
    }

    *guard = Some(Instant::now());
    execute_refresh(binary)
}

#[cfg(test)]
#[path = "ytdlp_tests.rs"]
mod tests;
