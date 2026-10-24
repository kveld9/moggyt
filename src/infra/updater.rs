//! Application update check port and static fake (GitHub Releases in production).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
}

pub trait Updater {
    fn latest(&self) -> Option<ReleaseInfo>;
}

#[derive(Debug, Default)]
pub struct StaticUpdater;

impl StaticUpdater {
    pub fn new() -> Self {
        Self
    }
}

impl Updater for StaticUpdater {
    fn latest(&self) -> Option<ReleaseInfo> {
        Some(ReleaseInfo {
            version: "v9.9.9-test".to_owned(),
            url: "https://github.com/example/moggyt/releases/tag/v9.9.9-test".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_updater_returns_fixed_release_info() {
        let rel = StaticUpdater::new().latest().expect("release");
        assert_eq!(rel.version, "v9.9.9-test");
        assert_eq!(rel.url, "https://github.com/example/moggyt/releases/tag/v9.9.9-test");
    }
}
