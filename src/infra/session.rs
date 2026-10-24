//! Session storage port and in-memory fake (OS keyring backend in production).

/// Persistence port for authenticated session cookies (blank saves clear stored session).
pub trait SessionStore {
    fn save(&mut self, cookies: &str);
    fn load(&self) -> Option<String>;
    fn clear(&mut self);
}

#[derive(Debug, Default)]
pub struct MemorySessionStore {
    cookies: Option<String>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for MemorySessionStore {
    fn save(&mut self, cookies: &str) {
        if cookies.trim().is_empty() {
            self.cookies = None;
            return;
        }
        self.cookies = Some(cookies.to_owned());
    }

    fn load(&self) -> Option<String> {
        self.cookies.clone()
    }

    fn clear(&mut self) {
        self.cookies = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_store_lifecycle_and_blank_handling() {
        let mut store = MemorySessionStore::new();
        assert_eq!(store.load(), None);
        store.save("SID=abc");
        assert_eq!(store.load().as_deref(), Some("SID=abc"));
        store.clear();
        assert_eq!(store.load(), None);

        for blank in ["", "   ", "\t\n"] {
            store.save("active");
            store.save(blank);
            assert_eq!(store.load(), None, "blank save clears stored session");
        }
    }
}
