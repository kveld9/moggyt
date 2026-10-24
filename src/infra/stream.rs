//! Stream resolution port and deterministic fake (yt-dlp sidecar in production).

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
}
