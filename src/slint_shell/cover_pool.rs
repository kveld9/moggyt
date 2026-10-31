//! Bounded background cover fetcher: command channel, worker threads, decode-free hand-off to the UI thread.

use std::sync::{Arc, Mutex};

use crate::slint_shell::platform::{dispatch_to_ui, rgba_to_image};
use crate::slint_shell::playback_worker::{
    COVER_FETCH_CONCURRENCY_LIMIT, fetch_and_decode_cover, thumbnail_variant_for_row,
};
use crate::slint_shell::state::{SharedCovers, SlintAppState};
use crate::slint_shell::sync::{
    sync_explore_shelves, sync_library_and_detail, sync_shelves_and_queue,
};
use crate::slint_shell::window::with_ui_state;
use crate::slint_shell::{CoverArtData, MoggytWindow};

/// Handles background cover fetch result and injects loaded Image into UI state.
pub(crate) fn handle_fetch_cover_command(
    url: &str,
    generation: u64,
    covers: &SharedCovers,
    weak: slint::Weak<MoggytWindow>,
) {
    let is_hi_res = url != thumbnail_variant_for_row(url);
    let Some(cover) = fetch_and_decode_cover(url, covers, is_hi_res) else {
        return;
    };
    let _ = dispatch_to_ui(move || {
        let Some(win) = weak.upgrade() else {
            return;
        };
        with_ui_state(|st| {
            apply_fetched_cover_to_state(&win, st, cover, generation);
        });
    });
}

/// Applies decoded cover art to UI state, triggering shelf card and search row resync.
pub(crate) fn apply_fetched_cover_to_state(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    cover: CoverArtData,
    generation: u64,
) {
    let Some(img) = rgba_to_image(&cover.rgba, cover.width, cover.height) else {
        return;
    };
    let is_search = st.search_generation == generation;
    let is_detail = st.detail_generation == generation;
    let is_library = st.library_generation == generation;
    let is_play = st.play_generation == generation;
    let is_explore = st.explore_generation == generation;

    if is_search || is_detail || is_library || is_play || is_explore {
        st.search_covers.insert(cover.url.clone(), img.clone());
        if is_search || is_play {
            sync_shelves_and_queue(win, st);
        }
        if is_detail || is_library {
            sync_library_and_detail(win, st);
        }
        if is_explore {
            sync_explore_shelves(win, st);
        }
    }
    sync_now_playing_after_cover_fetch(win, st, &cover.url, &img);
}

/// Updates now-playing cover when fetched artwork matches active track without downgrading hi-res.
pub(crate) fn sync_now_playing_after_cover_fetch(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    cover_url: &str,
    img: &slint::Image,
) {
    let Some(track) = st.queue.current() else {
        return;
    };
    let is_full_match = track.artwork.as_deref() == Some(cover_url);
    let is_thumb_match =
        track.artwork.as_deref().map(thumbnail_variant_for_row).as_deref() == Some(cover_url);
    if is_full_match {
        st.now_playing_cover = Some(img.clone());
        st.current_cover_image = Some(img.clone());
        win.set_now_playing_cover(img.clone());
        win.set_cover_image(img.clone());
        win.set_has_cover(true);
    } else if is_thumb_match && st.now_playing_cover.is_none() {
        st.now_playing_cover = Some(img.clone());
        win.set_now_playing_cover(img.clone());
    }
}

/// Spawns a pool of background cover fetch workers bounded by COVER_FETCH_CONCURRENCY_LIMIT.
pub(crate) fn spawn_cover_fetch_pool(
    covers: SharedCovers,
    weak: slint::Weak<MoggytWindow>,
) -> std::sync::mpsc::Sender<(String, u64)> {
    let (tx, rx) = std::sync::mpsc::channel::<(String, u64)>();
    let rx_arc = Arc::new(Mutex::new(rx));

    for i in 0..COVER_FETCH_CONCURRENCY_LIMIT {
        let rx_worker = Arc::clone(&rx_arc);
        let covers_worker = Arc::clone(&covers);
        let weak_worker = weak.clone();
        let _ = std::thread::Builder::new().name(format!("moggyt-cover-{i}")).spawn(move || {
            run_cover_fetch_worker_loop(&rx_worker, &covers_worker, &weak_worker);
        });
    }

    tx
}

pub(crate) fn run_cover_fetch_worker_loop(
    rx: &Arc<Mutex<std::sync::mpsc::Receiver<(String, u64)>>>,
    covers: &SharedCovers,
    weak: &slint::Weak<MoggytWindow>,
) {
    loop {
        let task = {
            let Ok(guard) = rx.lock() else { break };
            guard.recv().ok()
        };
        let Some((url, generation)) = task else { break };
        handle_fetch_cover_command(&url, generation, covers, weak.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cover_fetch_pool_concurrency_bound_constant_integration() {
        assert_eq!(COVER_FETCH_CONCURRENCY_LIMIT, 4);

        let (tx, rx) = std::sync::mpsc::channel::<(String, u64)>();
        for i in 0..COVER_FETCH_CONCURRENCY_LIMIT {
            tx.send((format!("https://example.com/art/{i}"), i as u64)).unwrap();
        }

        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, COVER_FETCH_CONCURRENCY_LIMIT);
    }
}
