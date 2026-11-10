//! SponsorBlock community skip segment adapter.
//!
//! Queries SponsorBlock skip segments using a SHA-256 k-anonymity privacy model:
//! only the first 4 lowercase hex characters (the first 2 digest bytes) of the
//! video id hash leave the machine. The server returns segments matching that prefix,
//! and candidate segments are filtered locally by the exact video id.
//!
//! A duration guard rejects segments submitted for a different upload when the
//! video duration in the response deviates from the local track duration by more
//! than `SPONSORBLOCK_DURATION_TOLERANCE_SECS`.

use sha2::{Digest, Sha256};

use crate::domain::{SkipRanges, SkipSegment};

/// Base URL for the public SponsorBlock skipSegments endpoint.
pub const SPONSORBLOCK_API_BASE: &str = "https://sponsor.ajay.app/api/skipSegments";

/// Percent-encoded categories requested from SponsorBlock: music_offtopic, sponsor, selfpromo, interaction.
pub const SPONSORBLOCK_CATEGORIES: &str =
    "[%22music_offtopic%22,%22sponsor%22,%22selfpromo%22,%22interaction%22]";

/// Maximum allowed difference in seconds between local track duration and segment video duration.
pub const SPONSORBLOCK_DURATION_TOLERANCE_SECS: f64 = 3.0;

/// Network timeout in seconds for SponsorBlock API requests.
pub const SPONSORBLOCK_TIMEOUT_SECS: u64 = 10;

/// User agent string sent with SponsorBlock HTTP requests.
pub const SPONSORBLOCK_USER_AGENT: &str = "moggyt";

/// Returns the first 4 lowercase hex characters of the SHA-256 digest of `video_id`.
pub fn hash_prefix(video_id: &str) -> String {
    let hash = Sha256::digest(video_id.as_bytes());
    format!("{:02x}{:02x}", hash[0], hash[1])
}

/// Returns the full skipSegments URL for a given SHA-256 prefix.
pub fn skip_segments_url(prefix: &str) -> String {
    format!("{SPONSORBLOCK_API_BASE}/{prefix}?categories={SPONSORBLOCK_CATEGORIES}&actionType=skip")
}

fn duration_matches(video_duration: f64, duration_secs: Option<u32>) -> bool {
    let Some(duration) = duration_secs else {
        return true;
    };
    if video_duration <= 0.0 {
        return true;
    }
    (video_duration - f64::from(duration)).abs() < SPONSORBLOCK_DURATION_TOLERANCE_SECS
}

fn to_valid_segment(s: f64, e: f64) -> Option<SkipSegment> {
    if !s.is_finite() || !e.is_finite() {
        return None;
    }
    if e <= s {
        return None;
    }
    let start_f = s.floor().max(0.0);
    let end_f = e.ceil();
    if end_f <= start_f || end_f > f64::from(u32::MAX) {
        return None;
    }
    SkipSegment::new(start_f as u32, end_f as u32).ok()
}

fn parse_one_segment(segment: &serde_json::Value) -> Option<SkipSegment> {
    let arr = segment.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    let s = arr[0].as_f64()?;
    let e = arr[1].as_f64()?;
    to_valid_segment(s, e)
}

fn action_is_skip(segment: &serde_json::Value) -> bool {
    let Some(action) = segment.get("actionType").and_then(|v| v.as_str()) else {
        return true;
    };
    action.eq_ignore_ascii_case("skip")
}

fn collect_segment_elem(
    elem: &serde_json::Value,
    duration_secs: Option<u32>,
    out: &mut SkipRanges,
) {
    if !action_is_skip(elem) {
        return;
    }
    let duration = elem.get("videoDuration").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if !duration_matches(duration, duration_secs) {
        return;
    }
    let raw = elem.get("segment").unwrap_or(elem);
    let Some(seg) = parse_one_segment(raw) else {
        return;
    };
    out.insert(seg);
}

fn collect_entry(
    entry: &serde_json::Value,
    video_id: &str,
    duration_secs: Option<u32>,
    out: &mut SkipRanges,
) {
    if entry.get("videoID").and_then(|v| v.as_str()) != Some(video_id) {
        return;
    }
    let Some(segments) = entry.get("segments").and_then(|v| v.as_array()) else {
        return;
    };
    for elem in segments {
        collect_segment_elem(elem, duration_secs, out);
    }
}

/// Parses a SponsorBlock JSON response array, extracting skip segments for `video_id`.
pub fn parse_response(
    json: &serde_json::Value,
    video_id: &str,
    duration_secs: Option<u32>,
) -> anyhow::Result<SkipRanges> {
    let Some(entries) = json.as_array() else {
        anyhow::bail!("expected sponsorblock payload to be an array");
    };
    let mut ranges = SkipRanges::new();
    for entry in entries {
        collect_entry(entry, video_id, duration_secs, &mut ranges);
    }
    Ok(ranges)
}

/// Port for fetching playback skip ranges from a remote segment provider.
pub trait SkipProvider: Send + Sync {
    /// Fetches skip ranges for a video id, optionally verifying video duration.
    fn fetch_ranges(
        &self,
        video_id: &str,
        duration_secs: Option<u32>,
    ) -> anyhow::Result<SkipRanges>;
}

/// SponsorBlock client implementing k-anonymity skip segment queries.
pub struct SponsorBlockClient {
    client: reqwest::blocking::Client,
    base: String,
}

impl SponsorBlockClient {
    /// Creates a new SponsorBlock client targeting the official API.
    pub fn new() -> anyhow::Result<Self> {
        Self::with_base(SPONSORBLOCK_API_BASE)
    }

    /// Creates a new SponsorBlock client targeting a custom base URL.
    pub fn with_base(base: impl Into<String>) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(SPONSORBLOCK_USER_AGENT)
            .timeout(std::time::Duration::from_secs(SPONSORBLOCK_TIMEOUT_SECS))
            .build()?;
        Ok(Self { client, base: base.into() })
    }

    /// Fetches skip ranges for `video_id`, returning empty ranges on HTTP 404.
    pub fn fetch_ranges(
        &self,
        video_id: &str,
        duration_secs: Option<u32>,
    ) -> anyhow::Result<SkipRanges> {
        let prefix = hash_prefix(video_id);
        let url =
            format!("{}/{prefix}?categories={SPONSORBLOCK_CATEGORIES}&actionType=skip", self.base);
        let response = self.client.get(&url).send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(SkipRanges::new());
        }
        if !response.status().is_success() {
            anyhow::bail!("sponsorblock request failed with status {}", response.status());
        }
        let json: serde_json::Value = response.json()?;
        parse_response(&json, video_id, duration_secs)
    }
}

impl SkipProvider for SponsorBlockClient {
    fn fetch_ranges(
        &self,
        video_id: &str,
        duration_secs: Option<u32>,
    ) -> anyhow::Result<SkipRanges> {
        self.fetch_ranges(video_id, duration_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_prefix_known_vectors() {
        assert_eq!(hash_prefix("dQw4w9WgXcQ"), "5f6b");
        assert_eq!(hash_prefix("aBcDeFgHiJk"), "43f5");
    }

    #[test]
    fn skip_segments_url_shape() {
        assert_eq!(
            skip_segments_url("5f6b"),
            "https://sponsor.ajay.app/api/skipSegments/5f6b?categories=[%22music_offtopic%22,%22sponsor%22,%22selfpromo%22,%22interaction%22]&actionType=skip"
        );
    }

    #[test]
    fn parse_matches_requested_video_only() {
        let payload = serde_json::json!([
            {
                "videoID": "other_video",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [10.0, 20.0],
                        "videoDuration": 100.0
                    }
                ]
            },
            {
                "videoID": "target_video",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [30.0, 40.0],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "target_video", Some(100)).expect("valid parse");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.segments()[0], SkipSegment::new(30, 40).unwrap());
    }

    #[test]
    fn parse_merges_and_sorts_segments() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [20.0, 30.0],
                        "videoDuration": 100.0
                    },
                    {
                        "actionType": "skip",
                        "segment": [5.0, 15.0],
                        "videoDuration": 100.0
                    },
                    {
                        "actionType": "skip",
                        "segment": [25.0, 35.0],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges.segments()[0], SkipSegment::new(5, 15).unwrap());
        assert_eq!(ranges.segments()[1], SkipSegment::new(20, 35).unwrap());
    }

    #[test]
    fn parse_rounds_fractional_bounds() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [12.4, 20.6],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.segments()[0], SkipSegment::new(12, 21).unwrap());
    }

    #[test]
    fn parse_rejects_duration_mismatch() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [10.0, 20.0],
                        "videoDuration": 200.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(180)).expect("valid parse");
        assert!(ranges.is_empty());
    }

    #[test]
    fn parse_accepts_unknown_duration() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [10.0, 20.0],
                        "videoDuration": 200.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", None).expect("valid parse");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.segments()[0], SkipSegment::new(10, 20).unwrap());
    }

    #[test]
    fn parse_skips_non_skip_action_type() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "mute",
                        "segment": [10.0, 20.0],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert!(ranges.is_empty());
    }

    #[test]
    fn parse_skips_invalid_bounds() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [30.0, 20.0],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert!(ranges.is_empty());
    }

    #[test]
    fn parse_skips_reversed_fractional_bounds() {
        let payload = serde_json::json!([
            {
                "videoID": "vid",
                "segments": [
                    {
                        "actionType": "skip",
                        "segment": [12.4, 12.1],
                        "videoDuration": 100.0
                    }
                ]
            }
        ]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert!(ranges.is_empty());
    }

    #[test]
    fn parse_rejects_non_array_payload() {
        let payload = serde_json::json!({ "error": "not found" });
        assert!(parse_response(&payload, "vid", None).is_err());
    }

    #[test]
    fn parse_empty_array_is_empty_ranges() {
        let payload = serde_json::json!([]);
        let ranges = parse_response(&payload, "vid", Some(100)).expect("valid parse");
        assert!(ranges.is_empty());
    }

    #[test]
    fn client_construction_without_network() {
        let default_client = SponsorBlockClient::new();
        assert!(default_client.is_ok());
        let custom_client = SponsorBlockClient::with_base("http://127.0.0.1:9");
        assert!(custom_client.is_ok());
    }
}
