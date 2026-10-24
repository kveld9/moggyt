//! Media presence integration port and no-op stub (MPRIS/SMTC in production).

pub trait Presence {
    fn set_track(&mut self, artist: &str, title: &str);
    fn clear(&mut self);
}

#[derive(Debug, Default)]
pub struct NoopPresence;

impl NoopPresence {
    pub fn new() -> Self {
        Self
    }
}

impl Presence for NoopPresence {
    fn set_track(&mut self, _artist: &str, _title: &str) {}
    fn clear(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_presence_calls_do_not_panic() {
        let mut presence = NoopPresence::new();
        presence.set_track("Artist", "Title");
        presence.clear();
    }
}
