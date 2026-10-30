//! Deterministic stream resolver stub (no network).

use super::stream::StreamResolver;

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
    fn resolve_maps_plain_id_to_stream_local_url() {
        let stub = StreamStub::new();
        assert_eq!(stub.resolve("track-123").as_deref(), Some("https://stream.local/track-123"));
    }

    #[test]
    fn resolve_trims_surrounding_whitespace() {
        let stub = StreamStub::new();
        assert_eq!(
            stub.resolve("  track-456  \n\t").as_deref(),
            Some("https://stream.local/track-456")
        );
    }

    #[test]
    fn resolve_returns_none_for_blank_or_whitespace_id() {
        let stub = StreamStub::new();
        assert_eq!(stub.resolve(""), None);
        assert_eq!(stub.resolve("   "), None);
        assert_eq!(stub.resolve("\t\n"), None);
    }
}
