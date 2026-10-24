//! Audio port and no-op stub (no sound yet).

/// Playback port: start streaming a catalog track by id.
pub trait AudioBackend {
    /// Requests playback of `track_id` (fire-and-forget intent).
    fn play(&mut self, track_id: &str);

    /// Last requested track id, if any (observability for UI/tests).
    fn last_requested(&self) -> Option<&str>;
}

/// Recording stub: captures intent without touching audio hardware.
#[derive(Debug, Default)]
pub struct AudioStub {
    last_requested: Option<String>,
}

impl AudioStub {
    /// Creates the stub backend.
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioBackend for AudioStub {
    fn play(&mut self, track_id: &str) {
        self.last_requested = Some(track_id.to_owned());
    }

    fn last_requested(&self) -> Option<&str> {
        self.last_requested.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_records_last_request() {
        let mut audio = AudioStub::new();
        assert_eq!(audio.last_requested(), None);
        audio.play("stub-1");
        assert_eq!(audio.last_requested(), Some("stub-1"));
    }
}
