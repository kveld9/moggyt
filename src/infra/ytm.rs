//! Catalog port and deterministic stub (no network).

use crate::domain::Track;

/// Read-only catalog port: search by free text.
pub trait Catalog {
    /// Returns catalog hits for `query` (empty when nothing matches).
    fn search(&self, query: &str) -> Vec<Track>;
}

/// Fixed-data stub: keeps UI and tests deterministic until the real API lands.
#[derive(Debug, Default)]
pub struct CatalogStub;

impl CatalogStub {
    /// Creates the stub catalog.
    pub fn new() -> Self {
        Self
    }

    fn fixed_hits(query: &str) -> Vec<Track> {
        vec![
            Track::new("stub-1", query, "Stub Artist"),
            Track::new("stub-2", "B-side", "Stub Artist"),
        ]
    }
}

impl Catalog for CatalogStub {
    fn search(&self, query: &str) -> Vec<Track> {
        Self::fixed_hits(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_deterministic_hits() {
        let catalog = CatalogStub::new();
        let hits = catalog.search("midnight");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "midnight");
    }
}
