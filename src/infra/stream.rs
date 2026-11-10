//! Stream resolution port, quality preferences, and adapter re-exports.

use std::io::Read;

use serde::{Deserialize, Serialize};

pub use super::stream_reqwest::open_audio_stream;
pub use super::stream_stub::StreamStub;

/// Audio stream quality preference: Compact (<=64k), Standard (default), High (highest available AAC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioQuality {
    Compact,
    #[default]
    Standard,
    High,
}

/// Maps an [`AudioQuality`] preference to yt-dlp format selector (SDD Q-3).
///
/// [`AudioQuality::High`] shares the standard selector because unauthenticated
/// YouTube Music serves AAC itag 140 (~128-130 kbps) as its bitrate ceiling: an
/// `abr` floor above it never matches and would only add a dead branch. Higher
/// tiers (AAC itag 141, ~256 kbps) require Premium authentication, and Opus itag
/// 251 is not decodable by the rodio/symphonia stack.
pub const fn audio_quality_format(quality: AudioQuality) -> &'static str {
    match quality {
        AudioQuality::Compact => "bestaudio[ext=m4a][abr<=64]/bestaudio[ext=m4a]/bestaudio/best",
        AudioQuality::Standard | AudioQuality::High => "bestaudio[ext=m4a]/bestaudio/best",
    }
}

/// Identifies which resolver tier produced a cached stream entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolverSource {
    InnerTube,
    YtDlp,
}

/// Resolved stream location and optional stream metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStream {
    pub url: String,
    pub mime_type: Option<String>,
    pub loudness_db: Option<f32>,
}

impl ResolvedStream {
    pub fn from_url(url: impl Into<String>) -> Self {
        Self { url: url.into(), mime_type: None, loudness_db: None }
    }
}

pub trait StreamResolver {
    fn resolve(&self, video_id: &str) -> Option<String>;

    /// Resolves a stream with optional provider metadata. Defaults to the URL-only contract.
    fn resolve_stream(&self, video_id: &str) -> Option<ResolvedStream> {
        self.resolve(video_id).map(ResolvedStream::from_url)
    }
}

/// Response returned by a [`RangeTransport`].
pub enum RangeResponse {
    /// Server supports range requests or supplied the requested byte slice.
    Partial { data: Vec<u8>, total_len: Option<u64> },
    /// Server ignored the Range header and supplied the full stream body sequentially.
    Full(Box<dyn Read + Send + Sync>),
}

/// Abstraction for fetching audio byte ranges over a transport seam.
pub trait RangeTransport: Send + Sync + 'static {
    /// Fetches a byte range `[start, start + length - 1]`.
    fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_resolves_non_blank_and_rejects_blank() {
        let resolver = StreamStub::new();
        for (id, exp) in [("v-1", Some("https://stream.local/v-1")), ("", None), ("   ", None)] {
            assert_eq!(resolver.resolve(id).as_deref(), exp);
        }
    }

    #[test]
    fn resolve_stream_default_resolves_non_blank_and_rejects_blank() {
        let resolver = StreamStub::new();
        let non_blank =
            resolver.resolve_stream("v-1").expect("non-blank id resolves with default provider");
        assert_eq!(non_blank.url, "https://stream.local/v-1");
        assert_eq!(non_blank.mime_type, None);
        assert_eq!(non_blank.loudness_db, None);
        assert_eq!(resolver.resolve_stream(""), None);
        assert_eq!(resolver.resolve_stream("   "), None);
    }

    #[test]
    fn resolved_stream_from_url_sets_empty_metadata() {
        let stream = ResolvedStream::from_url("https://stream.local/test");
        assert_eq!(stream.url, "https://stream.local/test");
        assert_eq!(stream.mime_type, None);
        assert_eq!(stream.loudness_db, None);
    }

    #[test]
    fn audio_quality_format_mapping_table() {
        let cases = [
            (
                AudioQuality::Compact,
                "bestaudio[ext=m4a][abr<=64]/bestaudio[ext=m4a]/bestaudio/best",
            ),
            (AudioQuality::Standard, "bestaudio[ext=m4a]/bestaudio/best"),
            (AudioQuality::High, "bestaudio[ext=m4a]/bestaudio/best"),
        ];
        for (quality, expected) in cases {
            assert_eq!(audio_quality_format(quality), expected);
        }
    }

    #[test]
    fn audio_quality_high_never_requests_an_unreachable_bitrate_floor() {
        let high = audio_quality_format(AudioQuality::High);
        assert_eq!(high, audio_quality_format(AudioQuality::Standard));
        assert!(
            !high.contains("abr>="),
            "High must not request an abr floor above the AAC ceiling (itag 140)"
        );
    }

    #[test]
    fn audio_quality_prefers_m4a_before_unconstrained_fallback() {
        for quality in [AudioQuality::Compact, AudioQuality::Standard, AudioQuality::High] {
            let format = audio_quality_format(quality);
            let selectors: Vec<&str> = format.split('/').collect();
            let first_unconstrained = selectors
                .iter()
                .position(|&s| !s.contains("ext=m4a"))
                .expect("must contain at least one unconstrained fallback");
            let first_m4a = selectors
                .iter()
                .position(|&s| s.contains("ext=m4a"))
                .expect("must prefer m4a format");
            assert!(first_m4a < first_unconstrained);
            assert!(format.ends_with("/bestaudio/best"));
        }
    }

    #[test]
    fn audio_quality_default_is_standard() {
        assert_eq!(AudioQuality::default(), AudioQuality::Standard);
    }

    #[test]
    fn audio_quality_serde_roundtrip_snake_case() {
        for (quality, json_str) in [
            (AudioQuality::Compact, "\"compact\""),
            (AudioQuality::Standard, "\"standard\""),
            (AudioQuality::High, "\"high\""),
        ] {
            let serialized = serde_json::to_string(&quality).expect("serialization succeeds");
            assert_eq!(serialized, json_str);
            let deserialized: AudioQuality =
                serde_json::from_str(json_str).expect("deserialization succeeds");
            assert_eq!(deserialized, quality);
        }
    }
}
