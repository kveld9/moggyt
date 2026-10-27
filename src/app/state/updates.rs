//! Desktop application update checking and status management for [`AppState`].

use crate::app::{AppState, UpdateStatus};
use crate::infra::{
    AudioBackend, Catalog, CoverCache, GithubUpdater, LyricsSource, SessionStore, Updater, is_newer,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Returns the current update check status.
    pub fn update_status(&self) -> &UpdateStatus {
        &self.update_status
    }

    /// Checks for updates against an injected updater implementation.
    pub fn check_with(&mut self, updater: &impl Updater) {
        let Some(info) = updater.latest() else {
            self.update_status = UpdateStatus::CheckFailed;
            return;
        };
        let current = GithubUpdater::current_version();
        if is_newer(current, &info.version) {
            self.update_status = UpdateStatus::Available { version: info.version, url: info.url };
        } else {
            self.update_status = UpdateStatus::Current;
        }
    }

    /// Initiates a manual update check using the configured updater.
    pub fn refresh_update(&mut self) {
        let updater = self.updater.clone();
        self.check_with(&updater);
    }

    /// Dismisses the update banner, treating the app as up-to-date for this session.
    pub fn dismiss_update(&mut self) {
        self.update_status = UpdateStatus::Current;
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{CustomReleaseUpdater, NoUpdates, wired};
    use super::*;
    use crate::infra::StaticUpdater;

    #[test]
    fn update_status_lifecycle_and_dismiss() {
        let mut state = wired();
        assert_eq!(*state.update_status(), UpdateStatus::Unknown);

        state.check_with(&StaticUpdater::new());
        assert_eq!(
            *state.update_status(),
            UpdateStatus::Available {
                version: "v9.9.9-test".into(),
                url: "https://github.com/example/moggyt/releases/tag/v9.9.9-test".into(),
            }
        );

        state.dismiss_update();
        assert_eq!(*state.update_status(), UpdateStatus::Current);
    }

    #[test]
    fn update_check_failure_and_up_to_date_cases() {
        let mut state = wired();

        state.check_with(&NoUpdates);
        assert_eq!(*state.update_status(), UpdateStatus::CheckFailed);

        state.check_with(&CustomReleaseUpdater("0.1.0".into()));
        assert_eq!(*state.update_status(), UpdateStatus::Current);

        state.check_with(&CustomReleaseUpdater("0.0.1".into()));
        assert_eq!(*state.update_status(), UpdateStatus::Current);
    }
}
