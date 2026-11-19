use std::sync::{Arc, Mutex};

use super::*;

use crate::domain::{PlaybackFailure, PlaybackPhase, Track};
use crate::infra::CatalogError;
use crate::slint_shell::platform::PresenceSnapshot;
use crate::slint_shell::playback_failure::PlaybackRecovery;
use crate::slint_shell::playback_worker::{PlayTrackResult, PlaybackWorkerCommand};
use crate::slint_shell::test_support::{EndedAudio, MockLibraryCatalog, test_wired};

#[test]
fn test_playback_controls_toggle_and_queue_navigation() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

    state.request_current_play();
    assert!(state.audio.lock().unwrap().is_playing());
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));

    state.toggle_play();
    assert!(!state.audio.lock().unwrap().is_playing());

    state.toggle_play();
    assert!(state.audio.lock().unwrap().is_playing());

    state.next_track();
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

    state.prev_track();
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
}

#[test]
fn test_seek_and_volume_controls() {
    let mut state = test_wired();
    state.set_volume(75);
    assert_eq!(state.audio.lock().unwrap().volume(), 75);
    state.seek_by(15);
    assert_eq!(state.audio.lock().unwrap().position_secs(), 15);
    state.seek_by(-50);
    assert_eq!(state.audio.lock().unwrap().position_secs(), 0);
}

#[test]
fn test_cycle_playback_speed_in_state() {
    let mut state = test_wired();
    assert_eq!(state.audio.lock().unwrap().speed(), 1.0);

    let next = state.cycle_playback_speed();
    assert_eq!(next, 1.25);
    assert_eq!(state.audio.lock().unwrap().speed(), 1.25);
    assert_eq!(state.settings_store.load().playback_speed, 1.25);

    let next2 = state.cycle_playback_speed();
    assert_eq!(next2, 1.5);
    assert_eq!(state.audio.lock().unwrap().speed(), 1.5);
}

#[test]
fn test_set_audio_quality_in_state() {
    let mut state = test_wired();
    assert_eq!(state.audio.lock().unwrap().quality(), AudioQuality::Standard);
    assert_eq!(state.settings_store.load().quality, AudioQuality::Standard);

    state.set_audio_quality(AudioQuality::High);
    assert_eq!(state.audio.lock().unwrap().quality(), AudioQuality::High);
    assert_eq!(state.settings_store.load().quality, AudioQuality::High);
}

#[test]
fn test_toggle_songs_only_flips_and_persists() {
    let mut state = test_wired();
    assert!(!state.songs_only);
    assert!(!state.settings_store.load().songs_only);

    state.toggle_songs_only();
    assert!(state.songs_only);
    assert!(state.settings_store.load().songs_only);

    state.toggle_songs_only();
    assert!(!state.songs_only);
    assert!(!state.settings_store.load().songs_only);
}

#[test]
fn test_toggle_autoplay_flips_and_persists() {
    let mut state = test_wired();
    assert!(!state.autoplay);
    assert!(!state.settings_store.load().autoplay_enabled);

    state.toggle_autoplay();
    assert!(state.autoplay);
    assert!(state.settings_store.load().autoplay_enabled);

    state.toggle_autoplay();
    assert!(!state.autoplay);
    assert!(!state.settings_store.load().autoplay_enabled);
}

#[test]
fn test_clear_queue_retains_playing_current_track_and_persists() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.request_current_play();
    assert!(state.audio.lock().unwrap().is_playing());
    state.gapless.mark_armed("t2");
    let phase_before = state.playback_phase;

    state.clear_queue();

    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    let audio = state.audio.lock().unwrap();
    assert!(audio.is_playing());
    assert_eq!(audio.last_requested(), Some("t1"));
    drop(audio);

    let stored = state.queue_store.load();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.current().map(|t| t.id.as_str()), Some("t1"));
    assert_eq!(state.playback_phase, phase_before);
    assert!(state.gapless.is_free());
}

#[test]
fn test_clear_queue_keeps_paused_current_track_paused() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.request_current_play();
    state.toggle_play();
    assert!(!state.audio.lock().unwrap().is_playing());
    assert_eq!(state.playback_phase, PlaybackPhase::Paused);

    state.clear_queue();

    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    let audio = state.audio.lock().unwrap();
    assert!(!audio.is_playing());
    assert_eq!(audio.last_requested(), Some("t1"));
    drop(audio);

    let stored = state.queue_store.load();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.current().map(|t| t.id.as_str()), Some("t1"));
    assert_eq!(state.playback_phase, PlaybackPhase::Paused);
}

#[test]
fn test_clear_queue_idle_tracks_without_request_clears_fully() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    assert_eq!(state.audio.lock().unwrap().last_requested(), None);

    state.clear_queue();

    assert!(state.queue.is_empty());
    assert_eq!(state.queue.cursor(), 0);
    assert!(!state.audio.lock().unwrap().is_playing());
    assert!(state.queue_store.load().is_empty());
    assert_eq!(state.playback_phase, PlaybackPhase::Empty);
}

#[test]
fn clear_queue_during_resolving_keeps_the_pending_track() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.play_track_at_index(0);
    state.queue.advance();
    state.set_playback_phase(PlaybackPhase::Resolving);

    state.clear_queue();

    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
    assert_eq!(state.queue.cursor(), 0);
    assert!(state.audio.lock().unwrap().is_playing());
    assert_eq!(state.playback_phase, PlaybackPhase::Resolving);
}

#[test]
fn test_continue_autoplay_if_ended_filters_video_recommendations_when_songs_only() {
    let mut state = test_wired();
    let catalog = MockLibraryCatalog::default().with_radio(vec![
        Track::new("vid-omv", "OMV Track", "Artist")
            .with_duration(180)
            .with_video_type(crate::domain::MusicVideoType::Omv),
        Track::new("song-atv", "ATV Track", "Artist")
            .with_duration(200)
            .with_video_type(crate::domain::MusicVideoType::Atv),
    ]);
    state.catalog = Arc::new(Mutex::new(Box::new(catalog)));
    state.autoplay = true;
    state.songs_only = true;
    let seed = Track::new("seed", "Seed", "Artist").with_duration(100);
    state.queue.push(seed);
    state.play_track_at_index_in_memory(0);
    state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("seed", 100))));
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue.tracks()[1].id, "song-atv");
}

#[test]
fn radio_failure_does_not_append_tracks() {
    let mut state = test_wired();
    let catalog = MockLibraryCatalog::default().with_radio_result(Err(CatalogError::Transport));
    state.catalog = Arc::new(Mutex::new(Box::new(catalog)));
    state.autoplay = true;
    let seed = Track::new("seed", "Seed", "Artist").with_duration(100);
    state.queue.push(seed);
    assert_eq!(state.queue.len(), 1);
    state.append_autoplay_radio("seed");
    assert_eq!(state.queue.len(), 1);
}

#[test]
fn test_continue_autoplay_crossfade_timing_and_advance() {
    let mut state = test_wired();
    state.crossfade_seconds = 5;

    state.queue.push(Track::new("cf-1", "Track 1", "Artist").with_duration(100));
    state.queue.push(Track::new("cf-2", "Track 2", "Artist").with_duration(120));

    state.play_track_at_index(0);
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-1"));

    // At 94s (remaining 6s > crossfade_seconds 5): should NOT crossfade yet
    state.audio.lock().unwrap().seek(94);
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-1"));

    // At 96s (remaining 4s <= crossfade_seconds 5): triggers crossfade advance to track 2
    state.audio.lock().unwrap().seek(96);
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 1);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-2"));
    assert_eq!(state.last_crossfaded_track, Some("cf-1".to_string()));

    // Repeated call should NOT re-trigger crossfade for cf-1
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 1);
}

#[test]
fn test_continue_autoplay_crossfade_disabled_when_zero() {
    let mut state = test_wired();
    state.crossfade_seconds = 0;

    state.queue.push(Track::new("no-cf-1", "Track 1", "Artist").with_duration(100));
    state.queue.push(Track::new("no-cf-2", "Track 2", "Artist").with_duration(120));

    state.play_track_at_index(0);
    // Seek to 98s (2s left): with crossfade_seconds == 0 the cursor must not advance;
    // the gapless hand-off only preloads the next track behind the active sink.
    state.audio.lock().unwrap().seek(98);
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("no-cf-1"));
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("no-cf-2"));
}

#[test]
fn request_current_play_uses_crossfade_when_playing_and_configured() {
    let mut state = test_wired();
    state.crossfade_seconds = 8;

    state.queue.push(Track::new("pq-1", "Track 1", "Artist").with_duration(100));
    state.queue.push(Track::new("pq-2", "Track 2", "Artist").with_duration(120));

    // First play: audio is not playing yet, so immediate play
    state.play_track_at_index(0);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("pq-1"));

    // Advance cursor in memory and play while audio is playing
    state.queue.advance();
    state.request_current_play();
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("pq-2"));
}

#[test]
fn test_set_crossfade_seconds_clamps_and_persists() {
    let mut state = test_wired();
    assert_eq!(state.crossfade_seconds, 0);

    state.set_crossfade_seconds(10);
    assert_eq!(state.crossfade_seconds, 10);
    assert_eq!(state.settings_store.load().crossfade_seconds, 10);

    state.set_crossfade_seconds(50);
    assert_eq!(state.crossfade_seconds, crate::infra::MAX_CROSSFADE_SECONDS);
    assert_eq!(state.settings_store.load().crossfade_seconds, crate::infra::MAX_CROSSFADE_SECONDS);
}

#[test]
fn test_continue_autoplay_advances_queue_when_crossfade_zero_and_ended() {
    let mut state = test_wired();
    state.crossfade_seconds = 0;

    state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

    state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("t1", 180))));
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));

    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 1);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t2"));
    assert!(state.audio.lock().unwrap().is_playing());
}

#[test]
fn test_continue_autoplay_no_advance_when_last_track_and_autoplay_disabled() {
    let mut state = test_wired();
    state.crossfade_seconds = 0;
    state.autoplay = false;

    let t1 = Track::new("t1", "Track 1", "Artist 1").with_duration(180);
    state.queue.push(t1);

    state.play_track_at_index(0);
    assert_eq!(state.queue.cursor(), 0);

    state.audio.lock().unwrap().pause();
    assert!(!state.audio.lock().unwrap().is_playing());

    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.queue.len(), 1);
    assert!(!state.audio.lock().unwrap().is_playing());
}

#[test]
fn pause_mid_queue_does_not_advance() {
    let mut state = test_wired();
    state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
    state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
    state.play_track_at_index(0);
    state.toggle_play();
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("a"));
    assert_eq!(state.playback_phase, PlaybackPhase::Paused);
}

#[test]
fn ended_advances_to_next() {
    let mut state = test_wired();
    state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
    state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
    state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 1);
    assert_eq!(state.audio.lock().unwrap().last_requested(), Some("b"));
}

#[test]
fn ended_last_track_autoplay_off_stays() {
    let mut state = test_wired();
    state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
    state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
    state.autoplay = false;
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.playback_phase, PlaybackPhase::Ended);
}

#[test]
fn restored_queue_startup_does_not_advance() {
    let mut state = test_wired();
    state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
    state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
    state.continue_autoplay_if_ended();
    assert_eq!(state.queue.cursor(), 0);
    assert_eq!(state.playback_phase, PlaybackPhase::Empty);
}

#[test]
fn generation_bumped_on_every_transition() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = test_wired();
    state.worker_tx = Some(tx);
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.next_track();
    let g1 = state.play_generation;
    state.prev_track();
    let g2 = state.play_generation;
    // state.drain_media_keys is not drivable without presence, so call next_track twice instead.
    state.next_track();
    let g3 = state.play_generation;
    state.next_track();
    let g4 = state.play_generation;
    let gens: Vec<_> = (0..4)
        .map(|_| match rx.recv().expect("cmd") {
            PlaybackWorkerCommand::Play(req) => req.generation,
            _ => panic!("expected Play"),
        })
        .collect();
    assert!(gens.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(gens, vec![g1, g2, g3, g4]);
}

#[test]
fn stale_play_result_is_discarded() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    let old = state.play_generation;
    let _ = state.begin_play_request().expect("request");
    assert!(!state.should_apply_play_result(old));
    assert!(state.should_apply_play_result(state.play_generation));
}

#[test]
fn toggle_play_restarts_current_track_when_sink_drained() {
    let mut state = test_wired();
    state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
    state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));

    state.toggle_play();

    let audio = state.audio.lock().unwrap();
    assert_eq!(audio.last_requested(), Some("a"));
    assert!(audio.is_playing());
    assert!(!audio.is_empty());
}

#[test]
fn play_track_at_index_in_memory_repeat_one_and_out_of_range() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.queue.set_repeat(crate::domain::RepeatMode::One);

    state.play_track_at_index_in_memory(1);
    assert_eq!(state.queue.cursor(), 1);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

    state.play_track_at_index_in_memory(99);
    assert_eq!(state.queue.cursor(), 1);
    assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
}

#[test]
fn play_library_songs_replaces_queue() {
    let mut state = test_wired();
    state.queue.push(Track::new("old", "Old", "Artist"));
    state.library_songs = vec![Track::new("s1", "S1", "A"), Track::new("s2", "S2", "A")];
    state.play_library_songs(1);
    let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["s1", "s2"]);
    assert_eq!(state.queue.cursor(), 1);
    state.play_library_songs(99);
    assert_eq!(state.queue.cursor(), 1);
}

#[test]
fn test_next_track_persists_queue_cursor() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.play_track_at_index_in_memory(0);
    state.persist_queue();
    assert_eq!(state.queue_store.load().cursor(), 0);

    state.next_track();

    assert_eq!(state.queue_store.load().cursor(), 1);
    assert_eq!(state.queue_store.load().current().map(|t| t.id.as_str()), Some("t2"));
}

#[test]
fn test_prev_track_persists_queue_cursor() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.play_track_at_index_in_memory(1);
    state.persist_queue();
    assert_eq!(state.queue_store.load().cursor(), 1);

    state.prev_track();

    assert_eq!(state.queue_store.load().cursor(), 0);
    assert_eq!(state.queue_store.load().current().map(|t| t.id.as_str()), Some("t1"));
}

#[test]
fn test_play_track_at_index_persists_target_cursor() {
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
    state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
    state.queue.push(Track::new("t3", "Track 3", "Artist 3"));

    state.play_track_at_index(2);

    let stored = state.queue_store.load();
    assert_eq!(stored.cursor(), 2);
    assert_eq!(stored.current().map(|t| t.id.as_str()), Some("t3"));
}

#[test]
fn test_play_library_songs_persists_replaced_queue() {
    let mut state = test_wired();
    state.library_songs =
        vec![Track::new("s1", "Song 1", "Artist"), Track::new("s2", "Song 2", "Artist")];

    state.play_library_songs(1);

    let stored = state.queue_store.load();
    let ids: Vec<&str> = stored.tracks().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["s1", "s2"]);
    assert_eq!(stored.cursor(), 1);
    assert_eq!(stored.current().map(|t| t.id.as_str()), Some("s2"));
}

#[test]
fn failed_play_completion_schedules_recovery_and_surfaces_error() {
    let Ok(window) = MoggytWindow::new() else {
        return;
    };
    let mut state = test_wired();
    state.queue.push(Track::new("t1", "Track 1", "Artist 1"));

    apply_play_track_completion(
        &window,
        &mut state,
        PlayTrackResult {
            generation: 1,
            track_id: "t1".into(),
            title: "Track 1".into(),
            artist: "Artist 1".into(),
            success: false,
            failure: Some(PlaybackFailure::Network),
            lyrics_lines: Vec::new(),
            cover_data: None,
            presence: PresenceSnapshot::new("Track 1", "Artist 1", false),
            gapless: false,
        },
    );

    assert!(!window.global::<PlayerUi>().get_is_playing());
    let error_text = window.global::<PlayerUi>().get_playback_error();
    assert!(!error_text.is_empty());
    assert!(error_text.contains("Network error"));
    assert!(matches!(state.playback_recovery, PlaybackRecovery::Scheduled { .. }));
}
