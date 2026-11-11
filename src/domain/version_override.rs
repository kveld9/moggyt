//! Persistent audio-version overrides for catalog tracks.
//!
//! Invariant: pure domain logic. No I/O, no clocks, no randomness. A single hop is
//! allowed per track: the replacement id must not itself be overridden.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Maximum number of stored overrides.
pub const MAX_VERSION_OVERRIDES: usize = 1000;

/// Errors returned when registering an override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOverrideError {
    /// One of the video ids is blank or contains whitespace.
    BlankId,
    /// The replacement id equals the original id.
    SameVideo,
    /// The replacement id is itself an override target key.
    ChainedOverride,
    /// The store already holds [`MAX_VERSION_OVERRIDES`] distinct originals.
    CapacityExceeded,
}

impl std::fmt::Display for VersionOverrideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankId => write!(f, "video id must be non-empty without whitespace"),
            Self::SameVideo => write!(f, "replacement video id must differ from the original"),
            Self::ChainedOverride => write!(f, "replacement video id is itself overridden"),
            Self::CapacityExceeded => {
                write!(f, "version override limit of {MAX_VERSION_OVERRIDES} reached")
            }
        }
    }
}

impl std::error::Error for VersionOverrideError {}

/// Map of original catalog video ids to alternative recording video ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionOverrides {
    entries: BTreeMap<String, String>,
}

impl VersionOverrides {
    /// Creates an empty override map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored overrides.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no override is stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the replacement video id for `video_id`, if one is stored.
    pub fn get(&self, video_id: &str) -> Option<&str> {
        self.entries.get(video_id).map(String::as_str)
    }

    /// Resolves the effective video id for playback, falling back to the input.
    pub fn resolve<'a>(&'a self, video_id: &'a str) -> &'a str {
        self.get(video_id).unwrap_or(video_id)
    }

    /// Registers an override, replacing any previous mapping for `original`.
    pub fn set(&mut self, original: &str, replacement: &str) -> Result<(), VersionOverrideError> {
        let original = sanitize_id(original).ok_or(VersionOverrideError::BlankId)?;
        let replacement = sanitize_id(replacement).ok_or(VersionOverrideError::BlankId)?;
        if original == replacement {
            return Err(VersionOverrideError::SameVideo);
        }
        if self.entries.contains_key(replacement) {
            return Err(VersionOverrideError::ChainedOverride);
        }
        if self.entries.len() >= MAX_VERSION_OVERRIDES && !self.entries.contains_key(original) {
            return Err(VersionOverrideError::CapacityExceeded);
        }
        self.entries.insert(original.to_owned(), replacement.to_owned());
        Ok(())
    }

    /// Removes the override for `video_id`, returning true when one existed.
    pub fn remove(&mut self, video_id: &str) -> bool {
        self.entries.remove(video_id).is_some()
    }
}

fn sanitize_id(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    Some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_resolve_round_trip() {
        let mut overrides = VersionOverrides::new();
        overrides.set("original", "replacement").expect("valid override");
        assert_eq!(overrides.get("original"), Some("replacement"));
        assert_eq!(overrides.resolve("original"), "replacement");
        assert_eq!(overrides.resolve("untouched"), "untouched");
        assert_eq!(overrides.len(), 1);
    }

    #[test]
    fn test_set_rejects_blank_ids_and_same_video() {
        let mut overrides = VersionOverrides::new();
        assert_eq!(overrides.set("  ", "b"), Err(VersionOverrideError::BlankId));
        assert_eq!(overrides.set("a", "b c"), Err(VersionOverrideError::BlankId));
        assert_eq!(overrides.set("a", "a"), Err(VersionOverrideError::SameVideo));
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_set_rejects_chained_override() {
        let mut overrides = VersionOverrides::new();
        overrides.set("a", "b").expect("first override");
        assert_eq!(overrides.set("c", "a"), Err(VersionOverrideError::ChainedOverride));
    }

    #[test]
    fn test_set_overwrites_existing_original() {
        let mut overrides = VersionOverrides::new();
        overrides.set("a", "b").expect("first");
        overrides.set("a", "c").expect("overwrite");
        assert_eq!(overrides.resolve("a"), "c");
        assert_eq!(overrides.len(), 1);
    }

    #[test]
    fn test_capacity_is_enforced() {
        let mut overrides = VersionOverrides::new();
        for index in 0..MAX_VERSION_OVERRIDES {
            overrides.set(&format!("id{index}"), &format!("alt{index}")).expect("within capacity");
        }
        assert_eq!(
            overrides.set("extra", "extra-alt"),
            Err(VersionOverrideError::CapacityExceeded)
        );
        overrides.set("id0", "other-alt").expect("overwrite allowed at capacity");
    }

    #[test]
    fn test_remove_and_serde_round_trip() {
        let mut overrides = VersionOverrides::new();
        overrides.set("a", "b").expect("override");
        let json = serde_json::to_string(&overrides).expect("serialize");
        assert_eq!(json, r#"{"a":"b"}"#);
        let parsed: VersionOverrides = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, overrides);
        assert!(overrides.remove("a"));
        assert!(!overrides.remove("a"));
    }
}
