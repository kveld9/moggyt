//! Stream resolution port and deterministic fake (yt-dlp sidecar in production).

use serde::{Deserialize, Serialize};

/// Audio stream quality preference: Compact (<=64k), Standard (default), High (>=192k).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioQuality {
    Compact,
    #[default]
    Standard,
    High,
}

/// Maps an [`AudioQuality`] preference to yt-dlp format selector (SDD Q-3).
pub const fn audio_quality_format(quality: AudioQuality) -> &'static str {
    match quality {
        AudioQuality::Compact => "bestaudio[abr<=64]/bestaudio/best",
        AudioQuality::Standard => "bestaudio/best",
        AudioQuality::High => "bestaudio[abr>=192]/bestaudio/best",
    }
}

pub trait StreamResolver {
    fn resolve(&self, video_id: &str) -> Option<String>;
}

#[derive(Debug, Default)]
pub struct StreamStub;

impl StreamStub {
    pub fn new() -> Self {
        Self
    }
}

impl StreamResolver for StreamStub {
    fn resolve(&self, video_id: &str) -> Option<String> {
        let trimmed = video_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(format!("https://stream.local/{trimmed}"))
    }
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
    fn audio_quality_format_mapping_table() {
        let cases = [
            (AudioQuality::Compact, "bestaudio[abr<=64]/bestaudio/best"),
            (AudioQuality::Standard, "bestaudio/best"),
            (AudioQuality::High, "bestaudio[abr>=192]/bestaudio/best"),
        ];
        for (quality, expected) in cases {
            assert_eq!(audio_quality_format(quality), expected);
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
