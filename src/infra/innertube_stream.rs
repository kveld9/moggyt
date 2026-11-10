//! InnerTube VISIONOS stream resolver adapter.
//!
//! Provides direct playback stream extraction via the YouTube InnerTube player endpoint
//! using the VISIONOS client profile.

use std::time::Duration;

use crate::i18n::Locale;
use serde_json::Value;

use super::catalog_requests::CATALOG_CLIENT_GL;
use super::stream::{AudioQuality, ResolvedStream, StreamResolver};

/// InnerTube player endpoint URL.
pub const PLAYER_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";

/// Client name reported in the player context.
pub const CLIENT_NAME: &str = "VISIONOS";

/// Client version reported in the player context.
pub const CLIENT_VERSION: &str = "1.02";

/// InnerTube client ID header value for VISIONOS.
pub const CLIENT_ID: &str = "101";

/// Device make reported in the player context.
pub const DEVICE_MAKE: &str = "Apple";

/// Device model reported in the player context.
pub const DEVICE_MODEL: &str = "RealityDevice17,1";

/// Operating system name reported in the player context.
pub const OS_NAME: &str = "visionOS";

/// Operating system version reported in the player context.
pub const OS_VERSION: &str = "26.5.23O471";

/// Safari user agent string matching the visionOS client.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// HTTP transport timeout for stream resolution requests in seconds.
pub const REQUEST_TIMEOUT_SECS: u64 = 8;

/// Adaptive format itag for low-bitrate AAC (~50 kbps).
pub const ITAG_AAC_COMPACT: u32 = 139;

/// Adaptive format itag for medium-bitrate AAC (~130 kbps).
pub const ITAG_AAC_STANDARD: u32 = 140;

const ORIGIN: &str = "https://www.youtube.com";

/// Selects the preferred InnerTube itag for the given audio quality.
///
/// Returns 139 (AAC ~50 kbps) for [`AudioQuality::Compact`], and 140 (AAC ~130 kbps)
/// for [`AudioQuality::Standard`] and [`AudioQuality::High`].
///
/// Note that itag 251 (Opus) is deliberately excluded because the rodio/symphonia
/// audio stack cannot decode WebM Opus streams.
pub const fn select_itag(quality: AudioQuality) -> u32 {
    match quality {
        AudioQuality::Compact => ITAG_AAC_COMPACT,
        AudioQuality::Standard | AudioQuality::High => ITAG_AAC_STANDARD,
    }
}

/// Reason why InnerTube stream resolution could not produce a valid direct stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FallbackReason {
    Http,
    Timeout,
    Playability,
    NoFormats,
    Ciphered,
    InvalidPayload,
}

impl FallbackReason {
    /// Returns the stable lowercase label for telemetry and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Timeout => "timeout",
            Self::Playability => "playability",
            Self::NoFormats => "no_formats",
            Self::Ciphered => "ciphered",
            Self::InvalidPayload => "invalid_payload",
        }
    }
}

/// Parsed audio stream format candidate from InnerTube player response.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    pub itag: u32,
    pub mime_type: Option<String>,
    pub url: Option<String>,
    pub loudness_db: Option<f32>,
    pub ciphered: bool,
}

/// Builds the JSON request payload for the InnerTube player endpoint.
pub fn player_body(video_id: &str) -> Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": CLIENT_NAME,
                "clientVersion": CLIENT_VERSION,
                "deviceMake": DEVICE_MAKE,
                "deviceModel": DEVICE_MODEL,
                "userAgent": USER_AGENT,
                "osName": OS_NAME,
                "osVersion": OS_VERSION,
                "hl": Locale::English.tag(),
                "gl": CATALOG_CLIENT_GL,
            }
        },
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
    })
}

fn parse_single_format(entry: &Value) -> Option<AudioFormat> {
    let itag = entry.get("itag").and_then(Value::as_u64)? as u32;
    let ciphered = entry.get("signatureCipher").is_some() || entry.get("cipher").is_some();
    let mime_type = entry.get("mimeType").and_then(Value::as_str).map(ToOwned::to_owned);
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let loudness_db = entry.get("loudnessDb").and_then(Value::as_f64).map(|val| val as f32);

    Some(AudioFormat { itag, mime_type, url, loudness_db, ciphered })
}

/// Parses adaptive audio formats from an InnerTube player response payload.
pub fn parse_audio_formats(payload: &Value) -> Vec<AudioFormat> {
    let Some(formats) = payload.pointer("/streamingData/adaptiveFormats").and_then(Value::as_array)
    else {
        return Vec::new();
    };

    formats.iter().filter_map(parse_single_format).collect()
}

fn check_playability_status(payload: &Value) -> Result<(), FallbackReason> {
    let status = payload.pointer("/playabilityStatus/status").and_then(Value::as_str);
    if status == Some("OK") { Ok(()) } else { Err(FallbackReason::Playability) }
}

fn convert_format_to_stream(format: AudioFormat) -> Result<ResolvedStream, FallbackReason> {
    if format.ciphered {
        return Err(FallbackReason::Ciphered);
    }
    let Some(url) = format.url else {
        return Err(FallbackReason::NoFormats);
    };
    Ok(ResolvedStream { url, mime_type: format.mime_type, loudness_db: format.loudness_db })
}

/// Selects a playable audio stream matching the requested audio quality preference.
pub fn select_stream(
    payload: &Value,
    quality: AudioQuality,
) -> Result<ResolvedStream, FallbackReason> {
    check_playability_status(payload)?;
    let target_itag = select_itag(quality);
    let formats = parse_audio_formats(payload);
    let format =
        formats.into_iter().find(|fmt| fmt.itag == target_itag).ok_or(FallbackReason::NoFormats)?;
    convert_format_to_stream(format)
}

fn classify_request_error(err: reqwest::Error) -> FallbackReason {
    if err.is_timeout() { FallbackReason::Timeout } else { FallbackReason::Http }
}

fn execute_player_http(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    body: &Value,
) -> Result<Value, FallbackReason> {
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .header("X-YouTube-Client-Name", CLIENT_ID)
        .header("X-YouTube-Client-Version", CLIENT_VERSION)
        .header("Origin", ORIGIN)
        .header("User-Agent", USER_AGENT)
        .json(body)
        .send()
        .map_err(classify_request_error)?;

    if !response.status().is_success() {
        return Err(FallbackReason::Http);
    }

    response.json().map_err(|_| FallbackReason::InvalidPayload)
}

/// Resolves audio streams directly from YouTube InnerTube using the VISIONOS profile.
#[derive(Debug, Clone)]
pub struct InnertubeResolver {
    client: reqwest::blocking::Client,
    endpoint: String,
    quality: AudioQuality,
}

impl InnertubeResolver {
    /// Creates a new resolver configured with the standard player endpoint and timeout.
    pub fn new() -> Self {
        Self::with_endpoint(PLAYER_ENDPOINT)
    }

    /// Creates a new resolver targeting a custom endpoint.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, endpoint: endpoint.into(), quality: AudioQuality::Standard }
    }

    /// Returns the configured player endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Returns the current stream quality preference.
    pub fn quality(&self) -> AudioQuality {
        self.quality
    }

    /// Updates the stream quality preference.
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
    }

    fn fetch_stream_pipeline(&self, video_id: &str) -> Result<ResolvedStream, FallbackReason> {
        let trimmed_id = video_id.trim();
        if trimmed_id.is_empty() {
            return Err(FallbackReason::InvalidPayload);
        }

        let body = player_body(trimmed_id);
        let payload = execute_player_http(&self.client, &self.endpoint, &body)?;
        select_stream(&payload, self.quality)
    }

    /// Fetches and resolves a stream for the specified video ID.
    pub fn fetch_stream(&self, video_id: &str) -> Result<ResolvedStream, FallbackReason> {
        let result = self.fetch_stream_pipeline(video_id);
        if let Err(reason) = result {
            tracing::debug!(reason = reason.as_str(), "innertube stream resolution failed");
        }
        result
    }

    /// Fetches and resolves a stream honoring an explicit quality preference.
    pub fn fetch_stream_with(
        &self,
        video_id: &str,
        quality: AudioQuality,
    ) -> Result<ResolvedStream, FallbackReason> {
        let mut resolver = self.clone();
        resolver.set_quality(quality);
        resolver.fetch_stream(video_id)
    }
}

impl Default for InnertubeResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamResolver for InnertubeResolver {
    fn resolve(&self, video_id: &str) -> Option<String> {
        self.fetch_stream(video_id).ok().map(|stream| stream.url)
    }

    fn resolve_stream(&self, video_id: &str) -> Option<ResolvedStream> {
        self.fetch_stream(video_id).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn select_itag_maps_quality_to_aac_itags() {
        assert_eq!(select_itag(AudioQuality::Compact), ITAG_AAC_COMPACT);
        assert_eq!(select_itag(AudioQuality::Standard), ITAG_AAC_STANDARD);
        assert_eq!(select_itag(AudioQuality::High), ITAG_AAC_STANDARD);
    }

    #[test]
    fn standard_quality_selects_itag_140_with_loudness() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        let stream = select_stream(&fixture, AudioQuality::Standard)
            .expect("standard stream resolution succeeds");
        assert!(stream.url.contains("itag=140"));
        let mime = stream.mime_type.as_deref().expect("mime_type is present");
        assert!(mime.starts_with("audio/mp4"));
        let loudness = stream.loudness_db.expect("loudness_db is present");
        assert!((loudness - 0.9899998).abs() < 1e-4);
    }

    #[test]
    fn compact_quality_selects_itag_139() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        let stream = select_stream(&fixture, AudioQuality::Compact)
            .expect("compact stream resolution succeeds");
        assert!(stream.url.contains("itag=139"));
        let mime = stream.mime_type.as_deref().expect("mime_type is present");
        assert!(mime.starts_with("audio/mp4"));
    }

    #[test]
    fn high_quality_keeps_aac_and_never_selects_opus() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        let stream =
            select_stream(&fixture, AudioQuality::High).expect("high stream resolution succeeds");
        assert!(stream.url.contains("itag=140"));
        assert!(!stream.url.contains("itag=251"));
    }

    #[test]
    fn ciphered_format_falls_back() {
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        let formats = fixture
            .pointer_mut("/streamingData/adaptiveFormats")
            .and_then(Value::as_array_mut)
            .expect("adaptiveFormats is an array");
        for entry in formats.iter_mut() {
            if entry.get("itag").and_then(Value::as_u64) == Some(140) {
                *entry = serde_json::json!({
                    "itag": 140,
                    "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                    "signatureCipher": "s=mock_signature_cipher"
                });
            }
        }
        let err = select_stream(&fixture, AudioQuality::Standard)
            .expect_err("ciphered format must trigger fallback");
        assert_eq!(err, FallbackReason::Ciphered);
    }

    #[test]
    fn non_ok_playability_falls_back() {
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        fixture["playabilityStatus"]["status"] = Value::String("LOGIN_REQUIRED".to_owned());
        let err = select_stream(&fixture, AudioQuality::Standard)
            .expect_err("non-OK playability must trigger fallback");
        assert_eq!(err, FallbackReason::Playability);
    }

    #[test]
    fn payload_without_formats_falls_back() {
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        if let Some(obj) = fixture.as_object_mut() {
            obj.remove("streamingData");
        }
        let err = select_stream(&fixture, AudioQuality::Standard)
            .expect_err("missing streamingData must trigger fallback");
        assert_eq!(err, FallbackReason::NoFormats);
    }

    #[test]
    fn missing_loudness_db_yields_none() {
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/innertube_visionos_player.json"
        ))
        .expect("fixture parses");
        let formats = fixture
            .pointer_mut("/streamingData/adaptiveFormats")
            .and_then(Value::as_array_mut)
            .expect("adaptiveFormats is an array");
        for entry in formats.iter_mut() {
            if entry.get("itag").and_then(Value::as_u64) == Some(140) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.remove("loudnessDb");
                }
            }
        }
        let stream = select_stream(&fixture, AudioQuality::Standard)
            .expect("stream selects successfully without loudnessDb");
        assert!(stream.url.contains("itag=140"));
        assert_eq!(stream.loudness_db, None);
    }

    #[test]
    fn player_body_carries_visionos_client_context() {
        let body = player_body("xyz_123");
        let client = body.pointer("/context/client").expect("client context present");
        assert_eq!(client.get("clientName").and_then(Value::as_str), Some(CLIENT_NAME));
        assert_eq!(client.get("clientVersion").and_then(Value::as_str), Some(CLIENT_VERSION));
        assert_eq!(client.get("deviceMake").and_then(Value::as_str), Some(DEVICE_MAKE));
        assert_eq!(client.get("deviceModel").and_then(Value::as_str), Some(DEVICE_MODEL));
        assert_eq!(client.get("osName").and_then(Value::as_str), Some(OS_NAME));
        assert_eq!(client.get("osVersion").and_then(Value::as_str), Some(OS_VERSION));
        assert_eq!(client.get("userAgent").and_then(Value::as_str), Some(USER_AGENT));
        assert_eq!(client.get("hl").and_then(Value::as_str), Some("en"));
        assert_eq!(client.get("gl").and_then(Value::as_str), Some("US"));
        assert_eq!(body.get("videoId").and_then(Value::as_str), Some("xyz_123"));
        assert_eq!(body.get("contentCheckOk").and_then(Value::as_bool), Some(true));
        assert_eq!(body.get("racyCheckOk").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn blank_video_id_is_invalid_payload() {
        let resolver = InnertubeResolver::new();
        assert_eq!(resolver.fetch_stream(""), Err(FallbackReason::InvalidPayload));
        assert_eq!(resolver.fetch_stream("   "), Err(FallbackReason::InvalidPayload));
    }

    #[test]
    fn fallback_reason_labels_are_stable() {
        let cases = [
            (FallbackReason::Http, "http"),
            (FallbackReason::Timeout, "timeout"),
            (FallbackReason::Playability, "playability"),
            (FallbackReason::NoFormats, "no_formats"),
            (FallbackReason::Ciphered, "ciphered"),
            (FallbackReason::InvalidPayload, "invalid_payload"),
        ];
        for (reason, label) in cases {
            assert_eq!(reason.as_str(), label);
        }
    }

    #[test]
    fn parse_audio_formats_skips_invalid_or_missing_itag() {
        let payload = serde_json::json!({
            "streamingData": {
                "adaptiveFormats": [
                    {
                        "itag": "non-numeric",
                        "mimeType": "audio/mp4",
                        "url": "https://stream.local/1"
                    },
                    {
                        "mimeType": "audio/mp4",
                        "url": "https://stream.local/2"
                    },
                    {
                        "itag": 140,
                        "mimeType": "audio/mp4",
                        "url": "   "
                    }
                ]
            }
        });
        let formats = parse_audio_formats(&payload);
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].itag, 140);
        assert_eq!(formats[0].url, None);
    }

    #[test]
    fn unciphered_format_missing_url_returns_no_formats() {
        let payload = serde_json::json!({
            "playabilityStatus": {
                "status": "OK"
            },
            "streamingData": {
                "adaptiveFormats": [
                    {
                        "itag": 140,
                        "mimeType": "audio/mp4",
                        "url": ""
                    }
                ]
            }
        });
        assert_eq!(select_stream(&payload, AudioQuality::Standard), Err(FallbackReason::NoFormats));
    }

    #[test]
    fn resolver_accessors_and_defaults() {
        let mut resolver = InnertubeResolver::new();
        assert_eq!(resolver.endpoint(), PLAYER_ENDPOINT);
        assert_eq!(resolver.quality(), AudioQuality::Standard);
        resolver.set_quality(AudioQuality::Compact);
        assert_eq!(resolver.quality(), AudioQuality::Compact);

        let custom = InnertubeResolver::with_endpoint("https://custom.endpoint/player");
        assert_eq!(custom.endpoint(), "https://custom.endpoint/player");
    }

    #[test]
    fn stream_resolver_trait_delegates_to_fetch_stream() {
        let resolver = InnertubeResolver::new();
        assert_eq!(resolver.resolve("   "), None);
        assert_eq!(resolver.resolve_stream("   "), None);
    }

    #[test]
    fn fetch_stream_with_blank_id_reports_invalid_payload() {
        assert_eq!(
            InnertubeResolver::new().fetch_stream_with("  ", AudioQuality::High),
            Err(FallbackReason::InvalidPayload)
        );
    }
}
