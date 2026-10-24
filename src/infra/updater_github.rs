//! GitHub Releases updater adapter.

use reqwest::header::ACCEPT;
use serde_json::Value;

use crate::infra::{ReleaseInfo, Updater};

/// Intrinsic upstream repository identity (overridable via test seam constructor).
pub const UPDATE_OWNER_REPO: &str = "kveld9/moggyt";

/// Parses release info from a GitHub release payload value.
pub fn parse_release_payload(value: &Value) -> Option<ReleaseInfo> {
    let tag = non_blank_field(value.get("tag_name"))?;
    let url = non_blank_field(value.get("html_url"))?;
    Some(ReleaseInfo { version: tag.to_owned(), url: url.to_owned() })
}

fn non_blank_field(field: Option<&Value>) -> Option<&str> {
    let s = field?.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s)
}

/// Compares two semver strings (up to 3 numeric components, ignoring leading 'v'/'V').
pub fn is_newer(current: &str, latest: &str) -> bool {
    let Some(curr) = parse_semver_triple(current) else {
        return false;
    };
    let Some(late) = parse_semver_triple(latest) else {
        return false;
    };
    late > curr
}

fn parse_semver_triple(raw: &str) -> Option<[u64; 3]> {
    let s = raw.strip_prefix(['v', 'V']).unwrap_or(raw);
    let core = s.split(['-', '+']).next().unwrap_or(s);
    if core.is_empty() {
        return None;
    }
    let mut triple = [0u64; 3];
    for (i, part) in core.split('.').enumerate() {
        let num = part.parse::<u64>().ok()?;
        if i < 3 {
            triple[i] = num;
        }
    }
    Some(triple)
}

/// Formats the GitHub releases API URL for the specified repository.
pub fn latest_url(owner_repo: &str) -> String {
    format!("https://api.github.com/repos/{owner_repo}/releases/latest")
}

/// GitHub Releases updater adapter.
#[derive(Clone, Debug)]
pub struct GithubUpdater {
    client: reqwest::blocking::Client,
    owner_repo: String,
}

impl GithubUpdater {
    pub fn new() -> Self {
        Self::with_owner_repo(UPDATE_OWNER_REPO)
    }

    pub fn with_owner_repo(owner_repo: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(format!("moggyt/{}", Self::current_version()))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, owner_repo: owner_repo.into() }
    }

    pub fn current_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    #[must_use]
    pub fn owner_repo(&self) -> &str {
        &self.owner_repo
    }
}

impl Default for GithubUpdater {
    fn default() -> Self {
        Self::new()
    }
}

impl Updater for GithubUpdater {
    fn latest(&self) -> Option<ReleaseInfo> {
        let url = latest_url(&self.owner_repo);
        let response =
            self.client.get(&url).header(ACCEPT, "application/vnd.github+json").send().ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: Value = response.json().ok()?;
        parse_release_payload(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_release_payload_valid_and_blank_handling() {
        let valid: Value = serde_json::json!({
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/kveld9/moggyt/releases/tag/v0.2.0"
        });
        let parsed = parse_release_payload(&valid).expect("should parse valid release");
        assert_eq!(parsed.version, "v0.2.0");
        assert_eq!(parsed.url, "https://github.com/kveld9/moggyt/releases/tag/v0.2.0");

        let blanks = [
            serde_json::json!({ "tag_name": "", "html_url": "https://example.com" }),
            serde_json::json!({ "tag_name": "  ", "html_url": "https://example.com" }),
            serde_json::json!({ "html_url": "https://example.com" }),
            serde_json::json!({ "tag_name": "v0.2.0", "html_url": "" }),
            serde_json::json!({ "tag_name": "v0.2.0", "html_url": "   " }),
            serde_json::json!({ "tag_name": "v0.2.0" }),
            serde_json::json!({}),
        ];
        for payload in blanks {
            assert_eq!(parse_release_payload(&payload), None);
        }
    }

    #[test]
    fn is_newer_table() {
        let cases = [
            ("0.1.0", "v0.2.0", true),
            ("v1.0.0", "v1.0.0", false),
            ("1.2", "1.2.1", true),
            ("1.2.1", "1.2", false),
            ("2.0.0", "1.9.9", false),
            ("abc", "1.0.0", false),
            ("1.0.0", "abc", false),
            ("", "1.0.0", false),
            ("1.0.0", "", false),
            ("v", "1.0.0", false),
            ("0.1.0", "V0.2.0", true),
            ("0.1.0", "v9.9.9-test", true),
            ("v9.9.9-test", "v9.9.9-test", false),
        ];
        for (current, latest, expected) in cases {
            assert_eq!(
                is_newer(current, latest),
                expected,
                "is_newer('{current}', '{latest}') expected {expected}"
            );
        }
    }

    #[test]
    fn latest_url_and_updater_constructor_seams() {
        assert_eq!(
            latest_url("owner/repo"),
            "https://api.github.com/repos/owner/repo/releases/latest"
        );
        let default_updater = GithubUpdater::new();
        assert_eq!(default_updater.owner_repo(), UPDATE_OWNER_REPO);
        assert_eq!(GithubUpdater::current_version(), env!("CARGO_PKG_VERSION"));

        let custom = GithubUpdater::with_owner_repo("custom/repo");
        assert_eq!(custom.owner_repo(), "custom/repo");
    }
}
