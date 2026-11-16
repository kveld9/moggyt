//! Application version reported by the built binary.
//!
//! `Cargo.toml` keeps a static placeholder version, so the released version is resolved at
//! build time by `build.rs`: it injects `MOGGYT_VERSION` from the pushed tag, falls back to
//! the newest local `v*` tag, and finally to the crate version. Every reader goes through
//! [`app_version`] so the About panel, the update check, and the outbound user agents agree.

/// Returns the application version without the `v` prefix (for example `1.4.0`).
///
/// `build.rs` always injects `MOGGYT_VERSION`; the crate version is only a last resort for
/// unusual builds that bypass the build script.
pub fn app_version() -> &'static str {
    normalize(option_env!("MOGGYT_VERSION")).unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Trims the injected value and strips a leading `v`, rejecting blank results.
fn normalize(raw: Option<&'static str>) -> Option<&'static str> {
    let trimmed = raw?.trim();
    let bare = trimmed.strip_prefix('v').unwrap_or(trimmed);
    (!bare.is_empty()).then_some(bare)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_tag_prefix_and_rejects_blanks() {
        let cases = [
            (None, None),
            (Some(""), None),
            (Some("   "), None),
            (Some("v"), None),
            (Some("1.4.0"), Some("1.4.0")),
            (Some("v1.4.0"), Some("1.4.0")),
            (Some("  1.4.0  "), Some("1.4.0")),
            (Some("v1.4.0-rc.1"), Some("1.4.0-rc.1")),
            (Some("V1.4.0"), Some("V1.4.0")),
        ];
        for (raw, expected) in cases {
            assert_eq!(normalize(raw), expected, "normalize({raw:?})");
        }
    }

    #[test]
    fn app_version_is_never_blank_or_prefixed() {
        let version = app_version();
        assert!(!version.is_empty());
        assert!(!version.starts_with('v'));
        assert_eq!(version, normalize(Some(version)).expect("normalized version"));
    }
}
