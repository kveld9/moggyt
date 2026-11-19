//! Preview session control: start, cancel and expire a 20-second track audition.
//!
//! The backend call resolves and decodes a stream, so it never runs on the UI thread: the start
//! is a one-shot thread and `poll_preview` adopts its result on the next tick.

use std::sync::mpsc::TryRecvError;

use crate::domain::{PreviewState, Track};
use crate::i18n::{Locale, tr};
use crate::infra::AudioError;
use crate::slint_shell::ContextIcon;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Starts a preview of `track`, or cancels it when that track is already previewing.
    pub fn toggle_preview(&mut self, track: &Track) {
        let same_track = self.preview.track_id() == Some(track.id.as_str());
        self.cancel_preview();
        if same_track {
            return;
        }
        let resolved = self.version_overrides.resolve(&track.id).to_owned();
        let audio = std::sync::Arc::clone(&self.audio);
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let spawned = std::thread::Builder::new()
            .name("moggyt-preview".to_string())
            .spawn(move || {
                let started = audio
                    .lock()
                    .map_err(|_| AudioError::Unavailable)
                    .and_then(|mut backend| backend.preview(&resolved))
                    .is_ok();
                let _ = tx.send(started);
            })
            .is_ok();
        if !spawned {
            let locale = self.locale;
            self.toasts.push(tr(locale, "Preview unavailable"), ContextIcon::Close);
            return;
        }
        self.preview_rx = Some(rx);
        self.preview =
            PreviewState::Starting { track_id: track.id.clone(), title: track.title.clone() };
    }

    /// Stops any running preview and restores the active track's volume.
    pub fn cancel_preview(&mut self) {
        if !matches!(self.preview, PreviewState::Idle) {
            if let Ok(mut audio) = self.audio.try_lock() {
                audio.stop_preview();
            }
        }
        self.preview_rx = None;
        self.preview = PreviewState::Idle;
    }

    /// Adopts a finished preview start and expires a preview whose sink drained.
    ///
    /// An idle state still reconciles with the backend, so a preview whose cancel could
    /// not take the audio lock is stopped on a later tick and the ducked volume is restored.
    pub fn poll_preview(&mut self) {
        match &self.preview {
            PreviewState::Idle => {
                if self.audio.try_lock().is_ok_and(|audio| audio.is_previewing()) {
                    if let Ok(mut audio) = self.audio.try_lock() {
                        audio.stop_preview();
                    }
                }
            }
            PreviewState::Starting { title, .. } => {
                let title = title.clone();
                self.poll_starting_preview(&title);
            }
            PreviewState::Playing { .. } => {
                self.poll_playing_preview();
            }
        }
    }

    fn poll_starting_preview(&mut self, title: &str) {
        let Some(rx) = &self.preview_rx else {
            self.preview = PreviewState::Idle;
            return;
        };
        let locale: Locale = self.locale;
        match rx.try_recv() {
            Ok(true) => {
                let track_id = self.preview.track_id().unwrap_or_default().to_string();
                self.preview_rx = None;
                self.preview = PreviewState::Playing { track_id };
                let msg = format!("{}: {title}", tr(locale, "Previewing"));
                self.toasts.push(msg, ContextIcon::Play);
            }
            Ok(false) | Err(TryRecvError::Disconnected) => {
                self.preview_rx = None;
                self.preview = PreviewState::Idle;
                self.toasts.push(tr(locale, "Preview unavailable"), ContextIcon::Close);
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn poll_playing_preview(&mut self) {
        let is_previewing =
            self.audio.try_lock().map(|audio| audio.is_previewing()).unwrap_or(true);
        if !is_previewing {
            self.cancel_preview();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::domain::{PREVIEW_SECS, PreviewState};
    use crate::infra::AudioStub;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn poll_preview_adopts_a_successful_start() {
        assert_eq!(PREVIEW_SECS, 20);
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(true).expect("send success");

        state.preview =
            PreviewState::Starting { track_id: "t1".to_string(), title: "Track One".to_string() };
        state.preview_rx = Some(rx);

        state.poll_preview();

        assert_eq!(state.preview, PreviewState::Playing { track_id: "t1".to_string() });
        assert!(state.preview_rx.is_none());
        assert_eq!(state.toasts.visible().len(), 1);
        assert_eq!(state.toasts.visible()[0].text, "Previewing: Track One");
        assert_eq!(state.toasts.visible()[0].icon, ContextIcon::Play);
    }

    #[test]
    fn poll_preview_reports_a_failed_start() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(false).expect("send failure");

        state.preview =
            PreviewState::Starting { track_id: "t1".to_string(), title: "Track One".to_string() };
        state.preview_rx = Some(rx);

        state.poll_preview();

        assert_eq!(state.preview, PreviewState::Idle);
        assert!(state.preview_rx.is_none());
        assert_eq!(state.toasts.visible().len(), 1);
        assert_eq!(state.toasts.visible()[0].text, "Preview unavailable");
        assert_eq!(state.toasts.visible()[0].icon, ContextIcon::Close);
    }

    #[test]
    fn starting_preview_moves_to_starting_and_opens_a_receiver() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track One", "Artist One");

        state.toggle_preview(&track);

        assert!(matches!(state.preview, PreviewState::Starting { .. }));
        assert!(state.preview_rx.is_some());

        let mut settled = false;
        for _ in 0..10_000 {
            state.poll_preview();
            if matches!(state.preview, PreviewState::Playing { .. }) {
                settled = true;
                break;
            }
            std::thread::yield_now();
        }
        if !settled {
            panic!("preview failed to settle into Playing state within loop limit");
        }

        assert_eq!(state.preview, PreviewState::Playing { track_id: "t1".to_string() });
        assert_eq!(state.toasts.visible().len(), 1);
        assert_eq!(state.toasts.visible()[0].text, "Previewing: Track One");
        assert_eq!(state.toasts.visible()[0].icon, ContextIcon::Play);
    }

    #[test]
    fn toggling_the_same_track_cancels() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track One", "Artist One");

        state.toggle_preview(&track);

        let mut settled = false;
        for _ in 0..10_000 {
            state.poll_preview();
            if state.preview.is_playing() {
                settled = true;
                break;
            }
            std::thread::yield_now();
        }
        if !settled {
            panic!("preview failed to settle into Playing state within loop limit");
        }

        state.toggle_preview(&track);
        assert_eq!(state.preview, PreviewState::Idle);
        assert!(!state.audio.lock().unwrap().is_previewing());
    }

    #[test]
    fn poll_preview_clears_a_drained_sink() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track One", "Artist One");

        state.toggle_preview(&track);

        let mut settled = false;
        for _ in 0..10_000 {
            state.poll_preview();
            if state.preview.is_playing() {
                settled = true;
                break;
            }
            std::thread::yield_now();
        }
        if !settled {
            panic!("preview failed to settle into Playing state within loop limit");
        }

        state.audio.lock().unwrap().stop_preview();
        state.poll_preview();
        assert_eq!(state.preview, PreviewState::Idle);
    }

    #[test]
    fn cancel_preview_never_blocks() {
        let mut state = test_wired();
        state.preview = PreviewState::Playing { track_id: "t1".to_string() };

        state.cancel_preview();

        assert_eq!(state.preview, PreviewState::Idle);
        assert!(state.preview_rx.is_none());
    }

    #[test]
    fn poll_preview_reconciles_idle_state_with_lingering_backend_preview() {
        let mut state = test_wired();
        let mut stub = AudioStub::new();
        stub.set_previewing(true);
        state.audio = Arc::new(Mutex::new(Box::new(stub)));
        state.preview = PreviewState::Idle;

        state.poll_preview();

        assert!(!state.audio.lock().unwrap().is_previewing());
        assert_eq!(state.preview, PreviewState::Idle);
    }
}
