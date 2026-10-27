//! Pure domain: local metadata overrides for track display.
//!
//! Invariant: this module performs no I/O. It provides display-only overrides
//! for catalog track title and artist without altering canonical track identity.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::search::Track;

fn sanitize_field(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_owned())
}

/// Display-only metadata override for a catalog track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataOverride {
    /// Overridden title when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Overridden artist display name when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

impl MetadataOverride {
    /// Constructs a validated override; trims inputs, maps blank fields to `None`,
    /// and returns `Err` when both fields are blank.
    pub fn new(title: Option<&str>, artist: Option<&str>) -> anyhow::Result<Self> {
        let clean_title = sanitize_field(title);
        let clean_artist = sanitize_field(artist);
        if clean_title.is_none() && clean_artist.is_none() {
            anyhow::bail!("metadata override cannot have both title and artist blank");
        }
        Ok(Self { title: clean_title, artist: clean_artist })
    }

    /// Resolves display title and artist against `track`.
    ///
    /// Returns borrowed `Cow` from canonical track fields when unoverridden,
    /// or owned `Cow` when overridden.
    pub fn resolve<'a>(&self, track: &'a Track) -> (Cow<'a, str>, Cow<'a, str>) {
        let title = match &self.title {
            Some(t) => Cow::Owned(t.clone()),
            None => Cow::Borrowed(track.title.as_str()),
        };
        let artist = match &self.artist {
            Some(a) => Cow::Owned(a.clone()),
            None => Cow::Borrowed(track.artist.as_str()),
        };
        (title, artist)
    }

    /// Whether both title and artist overrides are absent.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.artist.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_blank_inputs_are_rejected() {
        assert!(MetadataOverride::new(None, None).is_err());
        assert!(MetadataOverride::new(Some(""), Some("")).is_err());
        assert!(MetadataOverride::new(Some("   "), Some("  \t\n  ")).is_err());
        assert!(MetadataOverride::new(None, Some("   ")).is_err());
        assert!(MetadataOverride::new(Some("   "), None).is_err());
    }

    #[test]
    fn blank_field_stays_canonical_when_resolving() {
        let track = Track::new("track-1", "Canonical Title", "Canonical Artist");

        let title_only =
            MetadataOverride::new(Some("Custom Title"), None).expect("valid title override");
        assert_eq!(title_only.title.as_deref(), Some("Custom Title"));
        assert_eq!(title_only.artist, None);
        let (resolved_title, resolved_artist) = title_only.resolve(&track);
        assert_eq!(resolved_title, "Custom Title");
        assert_eq!(resolved_artist, "Canonical Artist");

        let artist_only = MetadataOverride::new(Some("   "), Some("Custom Artist"))
            .expect("valid artist override");
        assert_eq!(artist_only.title, None);
        assert_eq!(artist_only.artist.as_deref(), Some("Custom Artist"));
        let (resolved_title, resolved_artist) = artist_only.resolve(&track);
        assert_eq!(resolved_title, "Canonical Title");
        assert_eq!(resolved_artist, "Custom Artist");
    }

    #[test]
    fn resolve_borrows_when_no_override_and_owns_when_overridden() {
        let track = Track::new("track-2", "Base Title", "Base Artist");

        let empty_override = MetadataOverride { title: None, artist: None };
        assert!(empty_override.is_empty());
        let (t, a) = empty_override.resolve(&track);
        assert!(matches!(t, Cow::Borrowed(_)));
        assert!(matches!(a, Cow::Borrowed(_)));
        assert_eq!(t, "Base Title");
        assert_eq!(a, "Base Artist");

        let title_override =
            MetadataOverride::new(Some("New Title"), None).expect("valid title override");
        assert!(!title_override.is_empty());
        let (t, a) = title_override.resolve(&track);
        assert!(matches!(t, Cow::Owned(_)));
        assert!(matches!(a, Cow::Borrowed(_)));

        let artist_override =
            MetadataOverride::new(None, Some("New Artist")).expect("valid artist override");
        let (t, a) = artist_override.resolve(&track);
        assert!(matches!(t, Cow::Borrowed(_)));
        assert!(matches!(a, Cow::Owned(_)));

        let both_override = MetadataOverride::new(Some("New Title"), Some("New Artist"))
            .expect("valid both override");
        let (t, a) = both_override.resolve(&track);
        assert!(matches!(t, Cow::Owned(_)));
        assert!(matches!(a, Cow::Owned(_)));
    }

    #[test]
    fn fields_are_trimmed_on_construction() {
        let over = MetadataOverride::new(Some("  Spaced Title  "), Some("  Spaced Artist  "))
            .expect("valid trimmed override");
        assert_eq!(over.title.as_deref(), Some("Spaced Title"));
        assert_eq!(over.artist.as_deref(), Some("Spaced Artist"));
    }
}
