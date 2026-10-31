//! Background matching worker for Spotify playlist imports.
//!
//! Searches YouTube Music candidates per track, scores via multi-factor heuristics,
//! caches duplicate queries, retries rate limits with backoff, paces network requests,
//! and streams progress events over channels with atomic cancellation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::domain::{
    LocalPlaylist, LocalTrack, MatchConfidence, SpotifyPlaylist, SpotifyTrack, clean_track_title,
    local_playlist_id,
};
pub use crate::infra::SearchFailure;

use super::import_searcher::{CandidateHit, ranked_alternatives};

/// Port for candidate search used by the import worker.
pub trait TrackSearcher: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<CandidateHit>, SearchFailure>;
}

/// Tunables for the import pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerConfig {
    pub interval: Duration,
    pub backoff_base: Duration,
    pub max_retries: u32,
    pub workers: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(200),
            backoff_base: Duration::from_millis(500),
            max_retries: 3,
            workers: 2,
        }
    }
}

/// How one source track ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackResolution {
    Matched { track: LocalTrack, alternatives: Vec<LocalTrack> },
    Unresolved { title: String, artist: String, reason: String },
}

/// Progress event streamed to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportProgressEvent {
    Started { total: usize },
    Track { index: usize, resolution: TrackResolution },
    Progress { done: usize, total: usize, high: usize, review: usize, unresolved: usize },
    Finished { playlist: LocalPlaylist },
    Cancelled { playlist: LocalPlaylist },
    Failed { message: String },
}

/// Final tallies for one import run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportStats {
    pub total: usize,
    pub matched: usize,
    pub high: usize,
    pub review: usize,
    pub low: usize,
    pub unresolved: usize,
    pub with_alternatives: usize,
}

/// Pure query built for one Spotify track.
pub fn search_query(track: &SpotifyTrack) -> String {
    format!("{} {}", clean_track_title(&track.title), track.artist)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Builds the local playlist from an ordered resolution list.
pub fn assemble_playlist(
    source: &SpotifyPlaylist,
    resolutions: &[TrackResolution],
) -> LocalPlaylist {
    let mut p = LocalPlaylist::new(&local_playlist_id(&source.id), &source.title)
        .with_author(&source.author);
    if let Some(ref art) = source.artwork {
        p = p.with_artwork(art);
    }
    p.tracks = resolutions
        .iter()
        .filter_map(|r| match r {
            TrackResolution::Matched { track, .. } => Some(track.clone()),
            TrackResolution::Unresolved { .. } => None,
        })
        .collect();
    p
}

struct WorkerEnv<'a> {
    searcher: &'a dyn TrackSearcher,
    cancel: &'a AtomicBool,
    config: &'a WorkerConfig,
    sleeper: &'a (dyn Fn(Duration) + Sync),
    cache: &'a Mutex<HashMap<String, Vec<CandidateHit>>>,
}

fn compute_backoff(base: Duration, attempt: u32, query: &str, interval: Duration) -> Duration {
    let mult = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let half_nanos = (base / 2).as_nanos();
    let jitter = if half_nanos > 0 {
        let seed = (query.len() as u128)
            .wrapping_mul(2654435761)
            .wrapping_add((attempt as u128).wrapping_mul(2246822519));
        Duration::from_nanos((seed % (half_nanos + 1)) as u64)
    } else {
        Duration::ZERO
    };
    base.saturating_mul(mult).saturating_add(jitter).max(interval)
}

fn execute_search_attempt(
    env: &WorkerEnv<'_>,
    query: &str,
    attempt: u32,
) -> Result<Vec<CandidateHit>, SearchFailure> {
    let res = env.searcher.search(query);
    let wait = match &res {
        Err(SearchFailure::RateLimited) if attempt < env.config.max_retries => {
            compute_backoff(env.config.backoff_base, attempt, query, env.config.interval)
        }
        _ => env.config.interval,
    };
    (env.sleeper)(wait);
    res
}

fn search_with_retries(
    env: &WorkerEnv<'_>,
    query: &str,
) -> Result<Vec<CandidateHit>, SearchFailure> {
    for attempt in 0..=env.config.max_retries {
        if env.cancel.load(Ordering::Relaxed) {
            return Err(SearchFailure::Unavailable(anyhow::anyhow!("cancelled")));
        }
        match execute_search_attempt(env, query, attempt) {
            Ok(hits) => return Ok(hits),
            Err(SearchFailure::Unavailable(err)) => return Err(SearchFailure::Unavailable(err)),
            Err(SearchFailure::RateLimited) => {}
        }
    }
    Err(SearchFailure::RateLimited)
}

fn hits_to_resolution(track: &SpotifyTrack, hits: &[CandidateHit]) -> TrackResolution {
    let alternatives = ranked_alternatives(track, hits);
    if let Some(first) = alternatives.first().cloned() {
        TrackResolution::Matched { track: first, alternatives }
    } else {
        TrackResolution::Unresolved {
            title: track.title.clone(),
            artist: track.artist.clone(),
            reason: "no suitable match found".into(),
        }
    }
}

fn failure_to_resolution(track: &SpotifyTrack, failure: SearchFailure) -> TrackResolution {
    TrackResolution::Unresolved {
        title: track.title.clone(),
        artist: track.artist.clone(),
        reason: failure.to_string(),
    }
}

fn resolve_one_track(env: &WorkerEnv<'_>, track: &SpotifyTrack) -> Option<TrackResolution> {
    if env.cancel.load(Ordering::Relaxed) {
        return None;
    }
    let query = search_query(track);
    if let Some(hits) = env.cache.lock().ok()?.get(&query).cloned() {
        return Some(hits_to_resolution(track, &hits));
    }
    match search_with_retries(env, &query) {
        Ok(hits) => {
            if let Ok(mut g) = env.cache.lock() {
                g.insert(query, hits.clone());
            }
            Some(hits_to_resolution(track, &hits))
        }
        Err(failure) => {
            (!env.cancel.load(Ordering::Relaxed)).then(|| failure_to_resolution(track, failure))
        }
    }
}

fn apply_resolution(
    res: &mut Vec<TrackResolution>,
    stats: &mut ImportStats,
    resolution: TrackResolution,
) {
    match &resolution {
        TrackResolution::Matched { track, alternatives } => {
            stats.matched += 1;
            if alternatives.len() >= 2 {
                stats.with_alternatives += 1;
            }
            match track.confidence {
                MatchConfidence::High => stats.high += 1,
                MatchConfidence::Medium => stats.review += 1,
                MatchConfidence::Low => stats.low += 1,
            }
        }
        TrackResolution::Unresolved { .. } => stats.unresolved += 1,
    }
    res.push(resolution);
}

fn emit_track_progress(
    on_event: &mut dyn FnMut(ImportProgressEvent),
    index: usize,
    res: TrackResolution,
    s: &ImportStats,
) {
    on_event(ImportProgressEvent::Track { index, resolution: res });
    on_event(ImportProgressEvent::Progress {
        done: s.matched + s.unresolved,
        total: s.total,
        high: s.high,
        review: s.review,
        unresolved: s.unresolved,
    });
}

fn finish_pipeline_event(
    source: &SpotifyPlaylist,
    resolutions: &[TrackResolution],
    cancelled: bool,
) -> ImportProgressEvent {
    let playlist = assemble_playlist(source, resolutions);
    if cancelled {
        ImportProgressEvent::Cancelled { playlist }
    } else {
        ImportProgressEvent::Finished { playlist }
    }
}

/// Pure sequential pipeline: search, score, retry, pace, cache. Returns one entry per source track.
#[allow(clippy::too_many_arguments)]
pub fn resolve_tracks(
    searcher: &dyn TrackSearcher,
    source: &SpotifyPlaylist,
    cancel: &AtomicBool,
    config: &WorkerConfig,
    sleeper: &(dyn Fn(Duration) + Sync),
    cache: &Mutex<HashMap<String, Vec<CandidateHit>>>,
    on_event: &mut dyn FnMut(ImportProgressEvent),
) -> (Vec<TrackResolution>, ImportStats, bool) {
    let env = WorkerEnv { searcher, cancel, config, sleeper, cache };
    let mut stats = ImportStats { total: source.tracks.len(), ..Default::default() };
    let mut resolutions = Vec::with_capacity(source.tracks.len());
    on_event(ImportProgressEvent::Started { total: stats.total });

    for (i, t) in source.tracks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(res) = resolve_one_track(&env, t) else {
            break;
        };
        apply_resolution(&mut resolutions, &mut stats, res.clone());
        emit_track_progress(on_event, i, res, &stats);
    }
    let cancelled = cancel.load(Ordering::Relaxed) || resolutions.len() < source.tracks.len();
    on_event(finish_pipeline_event(source, &resolutions, cancelled));
    (resolutions, stats, cancelled)
}

fn drain_and_emit(
    source: &SpotifyPlaylist,
    cancel: &AtomicBool,
    rx: &mpsc::Receiver<(usize, TrackResolution)>,
    tx: &mpsc::Sender<ImportProgressEvent>,
) -> (Vec<TrackResolution>, bool) {
    let total = source.tracks.len();
    let (mut res, mut stats, mut next) =
        (Vec::with_capacity(total), ImportStats { total, ..Default::default() }, 0);
    let mut pending: HashMap<usize, TrackResolution> = HashMap::new();
    while next < total {
        if cancel.load(Ordering::Relaxed) {
            return (res, true);
        }
        if let Some(r) = pending.remove(&next) {
            apply_resolution(&mut res, &mut stats, r.clone());
            emit_track_progress(&mut |ev| drop(tx.send(ev)), next, r, &stats);
            next += 1;
        } else {
            match rx.recv() {
                Ok((i, r)) => drop(pending.insert(i, r)),
                Err(_) => return (res, true),
            }
        }
    }
    (res, false)
}

fn worker_thread_loop(
    env: &WorkerEnv<'_>,
    tracks: &[SpotifyTrack],
    next: &AtomicUsize,
    tx: &mpsc::Sender<(usize, TrackResolution)>,
) {
    while !env.cancel.load(Ordering::Relaxed) {
        let idx = next.fetch_add(1, Ordering::SeqCst);
        if idx >= tracks.len() {
            break;
        }
        if let Some(res) = resolve_one_track(env, &tracks[idx]) {
            drop(tx.send((idx, res)));
        }
    }
}

fn run_pipeline(
    searcher: Arc<dyn TrackSearcher>,
    source: SpotifyPlaylist,
    cancel: Arc<AtomicBool>,
    config: WorkerConfig,
    tx: mpsc::Sender<ImportProgressEvent>,
) {
    if cancel.load(Ordering::Relaxed) {
        drop(tx.send(ImportProgressEvent::Cancelled { playlist: assemble_playlist(&source, &[]) }));
        return;
    }
    let total = source.tracks.len();
    drop(tx.send(ImportProgressEvent::Started { total }));
    if total == 0 {
        drop(tx.send(ImportProgressEvent::Finished { playlist: assemble_playlist(&source, &[]) }));
        return;
    }

    let (cache, tracks, next) = (
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(source.tracks.clone()),
        Arc::new(AtomicUsize::new(0)),
    );
    let (worker_tx, worker_rx) = mpsc::channel();
    let num_workers = config.workers.clamp(1, total);
    let handles: Vec<_> = (0..num_workers)
        .map(|_| {
            let (sr, ca, ch, tr, wtx, n) = (
                Arc::clone(&searcher),
                Arc::clone(&cancel),
                Arc::clone(&cache),
                Arc::clone(&tracks),
                worker_tx.clone(),
                Arc::clone(&next),
            );
            std::thread::spawn(move || {
                let env = WorkerEnv {
                    searcher: sr.as_ref(),
                    cancel: ca.as_ref(),
                    config: &config,
                    sleeper: &std::thread::sleep,
                    cache: ch.as_ref(),
                };
                worker_thread_loop(&env, &tr, &n, &wtx);
            })
        })
        .collect();
    drop(worker_tx);

    let (resolutions, was_cancelled) = drain_and_emit(&source, &cancel, &worker_rx, &tx);
    let is_cancelled = was_cancelled || cancel.load(Ordering::Relaxed);
    drop(tx.send(finish_pipeline_event(&source, &resolutions, is_cancelled)));
    for h in handles {
        drop(h.join());
    }
}

/// Spawns the real threaded runner and returns the event receiver.
pub fn spawn_import(
    searcher: Arc<dyn TrackSearcher>,
    source: SpotifyPlaylist,
    cancel: Arc<AtomicBool>,
    config: WorkerConfig,
) -> mpsc::Receiver<ImportProgressEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("spotify-import-runner".into())
        .spawn(move || run_pipeline(searcher, source, cancel, config, tx))
        .expect("spawn import runner thread");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MatchCandidate, MusicVideoType};

    type SearchResult = Result<Vec<CandidateHit>, SearchFailure>;

    #[derive(Default)]
    struct FakeSearcher {
        calls: AtomicUsize,
        responses: Mutex<HashMap<String, Vec<SearchResult>>>,
    }

    impl FakeSearcher {
        fn with_hit(self, q: &str, hit: CandidateHit) -> Self {
            self.with_responses(q, vec![Ok(vec![hit])])
        }

        fn with_responses(self, q: &str, res: Vec<SearchResult>) -> Self {
            self.responses.lock().unwrap().insert(q.to_string(), res);
            self
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl TrackSearcher for FakeSearcher {
        fn search(&self, query: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut g = self.responses.lock().unwrap();
            g.get_mut(query)
                .and_then(|v| (!v.is_empty()).then(|| v.remove(0)))
                .unwrap_or(Ok(Vec::new()))
        }
    }

    #[derive(Default)]
    struct FakeSleeper {
        durations: Mutex<Vec<Duration>>,
    }

    impl FakeSleeper {
        fn recorded(&self) -> Vec<Duration> {
            self.durations.lock().unwrap().clone()
        }
    }

    fn track(title: &str, artist: &str) -> SpotifyTrack {
        SpotifyTrack {
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_ms: 180_000,
            isrc: None,
        }
    }

    fn playlist(id: &str, tracks: Vec<SpotifyTrack>) -> SpotifyPlaylist {
        SpotifyPlaylist {
            id: id.into(),
            title: "Playlist".into(),
            author: "User".into(),
            artwork: None,
            tracks,
        }
    }

    fn hit(id: &str, q: &str, ch: &str, bias: MusicVideoType) -> CandidateHit {
        CandidateHit::new(
            id,
            Some(format!("https://example.com/{id}.jpg")),
            MatchCandidate::new(q, ch, Some(180), bias),
        )
    }

    fn run_worker(
        searcher: &dyn TrackSearcher,
        p: &SpotifyPlaylist,
        cfg: &WorkerConfig,
        sleeper: &FakeSleeper,
        on_event: &mut dyn FnMut(ImportProgressEvent),
    ) -> (Vec<TrackResolution>, ImportStats, bool) {
        let (cache, cancel) = (Mutex::new(HashMap::new()), AtomicBool::new(false));
        resolve_tracks(
            searcher,
            p,
            &cancel,
            cfg,
            &|d| sleeper.durations.lock().unwrap().push(d),
            &cache,
            on_event,
        )
    }

    fn test_resolve(
        searcher: &dyn TrackSearcher,
        p: &SpotifyPlaylist,
        cancel: &AtomicBool,
    ) -> (Vec<TrackResolution>, ImportStats, bool) {
        resolve_tracks(
            searcher,
            p,
            cancel,
            &WorkerConfig::default(),
            &|_| {},
            &Mutex::new(HashMap::new()),
            &mut |_| {},
        )
    }

    #[test]
    fn test_search_query_normalizes_and_collapses_whitespace() {
        let t = track("Song (2011 Remaster) [Deluxe]", "The  Artist");
        assert_eq!(search_query(&t), "Song The Artist");
    }

    #[test]
    fn test_happy_path_resolves_all_with_confidence_buckets() {
        let p = playlist(
            "sp_h",
            vec![
                track("Song High", "Artist One"),
                track("Song Review", "Artist Two"),
                track("Song Low", "Artist Three"),
            ],
        );
        let fake = FakeSearcher::default()
            .with_hit(
                "Song High Artist One",
                hit("v1", "Song High", "Artist - Topic", MusicVideoType::Atv),
            )
            .with_hit(
                "Song Review Artist Two",
                hit("v2", "Song Review", "Artist Two", MusicVideoType::Other),
            )
            .with_hit(
                "Song Low Artist Three",
                hit("v3", "Song Low Live", "Topic", MusicVideoType::Atv),
            );

        let (mut events, sleeper) = (Vec::new(), FakeSleeper::default());
        let (res, stats, cancelled) =
            run_worker(&fake, &p, &WorkerConfig::default(), &sleeper, &mut |ev| events.push(ev));

        assert!(!cancelled && res.len() == 3);
        assert_eq!(
            (stats.total, stats.matched, stats.high, stats.review, stats.low, stats.unresolved),
            (3, 3, 1, 1, 1, 0)
        );
        assert!(matches!(
            &res[0],
            TrackResolution::Matched { track, alternatives }
                if track.video_id == "v1" && alternatives[0] == *track
        ));

        let track_events =
            events.iter().filter(|e| matches!(e, ImportProgressEvent::Track { .. })).count();
        let prog_events =
            events.iter().filter(|e| matches!(e, ImportProgressEvent::Progress { .. })).count();
        assert_eq!((track_events, prog_events), (3, 3));
        assert!(events.iter().any(|e| matches!(e, ImportProgressEvent::Finished { .. })));
    }

    #[test]
    fn test_pacing_records_durations_at_least_interval() {
        let p = playlist("sp_p", vec![track("P", "A")]);
        let fake =
            FakeSearcher::default().with_hit("P A", hit("vp", "P", "Topic", MusicVideoType::Atv));
        let config = WorkerConfig { interval: Duration::from_millis(150), ..Default::default() };
        let sleeper = FakeSleeper::default();
        run_worker(&fake, &p, &config, &sleeper, &mut |_| {});
        assert!(sleeper.recorded().into_iter().all(|d| d >= config.interval));
    }

    #[test]
    fn test_cache_deduplicates_duplicate_queries() {
        let p = playlist("sp_d", vec![track("D", "A"), track("D", "A")]);
        let fake =
            FakeSearcher::default().with_hit("D A", hit("vd", "D", "Topic", MusicVideoType::Atv));
        let (res, _, _) =
            run_worker(&fake, &p, &WorkerConfig::default(), &FakeSleeper::default(), &mut |_| {});
        assert_eq!((fake.calls(), res.len(), &res[0]), (1, 2, &res[1]));
    }

    #[test]
    fn test_rate_limit_backoff_and_retry_success() {
        let p = playlist("sp_r", vec![track("R", "A")]);
        let responses = vec![
            Err(SearchFailure::RateLimited),
            Ok(vec![hit("vr", "R", "Topic", MusicVideoType::Atv)]),
        ];
        let fake = FakeSearcher::default().with_responses("R A", responses);
        let config = WorkerConfig {
            backoff_base: Duration::from_millis(300),
            interval: Duration::from_millis(50),
            max_retries: 2,
            workers: 1,
        };
        let sleeper = FakeSleeper::default();
        let (res, stats, _) = run_worker(&fake, &p, &config, &sleeper, &mut |_| {});
        let rec = sleeper.recorded();
        assert_eq!(stats.matched, 1);
        assert!(matches!(res[0], TrackResolution::Matched { .. }));
        assert!(rec.len() >= 2 && rec[0] >= config.backoff_base);
    }

    #[test]
    fn test_retry_exhaustion_emits_unresolved_rate_limited() {
        let p = playlist("sp_e", vec![track("E", "A")]);
        let errs = (0..4).map(|_| Err(SearchFailure::RateLimited)).collect();
        let fake = FakeSearcher::default().with_responses("E A", errs);
        let cfg = WorkerConfig { max_retries: 3, ..Default::default() };
        let (res, stats, _) = run_worker(&fake, &p, &cfg, &FakeSleeper::default(), &mut |_| {});
        assert_eq!((fake.calls(), stats.unresolved), (4, 1));
        assert!(matches!(
            &res[0],
            TrackResolution::Unresolved { reason, .. } if reason == "rate limited"
        ));
    }

    #[test]
    fn test_unavailable_error_does_not_retry() {
        let p = playlist("sp_u", vec![track("U", "A")]);
        let err = Err(SearchFailure::Unavailable(anyhow::anyhow!("network unreachable")));
        let fake = FakeSearcher::default().with_responses("U A", vec![err]);
        let (res, _, _) =
            run_worker(&fake, &p, &WorkerConfig::default(), &FakeSleeper::default(), &mut |_| {});
        assert_eq!(fake.calls(), 1);
        assert!(matches!(
            &res[0],
            TrackResolution::Unresolved { reason, .. } if reason.contains("network unreachable")
        ));
    }

    #[test]
    fn test_cancellation_before_first_track() {
        let p = playlist("sp_c", vec![track("Cancelled Track", "Cancelled Artist")]);
        let (res, _, cancelled) =
            test_resolve(&FakeSearcher::default(), &p, &AtomicBool::new(true));
        assert!(cancelled && res.is_empty());
    }

    #[test]
    fn test_cancellation_mid_run() {
        let p = playlist("sp_m", vec![track("T1", "A"), track("T2", "A")]);
        let cancel = Arc::new(AtomicBool::new(false));
        struct MidSearcher(Arc<AtomicBool>, AtomicUsize);
        impl TrackSearcher for MidSearcher {
            fn search(&self, q: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
                self.1.fetch_add(1, Ordering::SeqCst);
                self.0.store(true, Ordering::SeqCst);
                Ok(vec![hit("v1", q, "Topic", MusicVideoType::Atv)])
            }
        }
        let fake = MidSearcher(Arc::clone(&cancel), AtomicUsize::new(0));
        let (res, _, cancelled) = test_resolve(&fake, &p, &cancel);
        assert!(cancelled && res.len() == 1 && fake.1.load(Ordering::SeqCst) == 1);
    }

    #[test]
    fn test_assemble_playlist_order_author_artwork_prefix() {
        let (ta, tb) = (
            LocalTrack::new("vid_a", "Song A", "Artist A", MatchConfidence::High),
            LocalTrack::new("vid_b", "Song B", "Artist B", MatchConfidence::Medium),
        );
        let source = SpotifyPlaylist {
            id: "37i9dQZF1DXcBWIGoYBM5M".into(),
            title: "My Playlist".into(),
            author: "Spotify Curator".into(),
            artwork: Some("https://example.com/art.jpg".into()),
            tracks: Vec::new(),
        };
        let res = assemble_playlist(
            &source,
            &[
                TrackResolution::Matched { track: ta.clone(), alternatives: vec![ta.clone()] },
                TrackResolution::Unresolved {
                    title: "X".into(),
                    artist: "X".into(),
                    reason: "no match".into(),
                },
                TrackResolution::Matched { track: tb.clone(), alternatives: vec![tb.clone()] },
            ],
        );
        assert_eq!(res.id, "local:37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(res.title, "My Playlist");
        assert_eq!(res.author, "Spotify Curator");
        assert_eq!(res.artwork.as_deref(), Some("https://example.com/art.jpg"));
        assert_eq!(res.tracks, vec![ta, tb]);
    }

    #[test]
    fn test_with_alternatives_counter() {
        let p = playlist(
            "sp_alts",
            vec![
                track("Multi Hit", "Artist"),
                track("Single Hit", "Artist"),
                track("No Hit", "Artist"),
            ],
        );
        let hits = vec![
            hit("v1", "Multi Hit", "Artist", MusicVideoType::Atv),
            hit("v2", "Multi Hit", "Artist - Topic", MusicVideoType::Atv),
        ];
        let fake = FakeSearcher::default()
            .with_responses("Multi Hit Artist", vec![Ok(hits)])
            .with_hit("Single Hit Artist", hit("v3", "Single Hit", "Artist", MusicVideoType::Atv));

        let (res, stats, _) =
            run_worker(&fake, &p, &WorkerConfig::default(), &FakeSleeper::default(), &mut |_| {});
        assert_eq!(res.len(), 3);
        assert_eq!(stats.matched, 2);
        assert_eq!(stats.unresolved, 1);
        assert_eq!(stats.with_alternatives, 1);

        assert!(matches!(
            &res[0],
            TrackResolution::Matched { alternatives, .. } if alternatives.len() == 2
        ));
        assert!(matches!(
            &res[1],
            TrackResolution::Matched { alternatives, .. } if alternatives.len() == 1
        ));
    }

    #[derive(Default)]
    struct HandshakeFakeSearcher {
        in_flight: AtomicUsize,
        max_seen: AtomicUsize,
        ready: AtomicBool,
    }

    impl TrackSearcher for HandshakeFakeSearcher {
        fn search(&self, q: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
            let n = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(n, Ordering::SeqCst);
            if n >= 2 {
                self.ready.store(true, Ordering::SeqCst);
            } else {
                while !self.ready.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![hit(&format!("v_{q}"), q, "Topic", MusicVideoType::Atv)])
        }
    }

    #[test]
    fn test_spawn_import_concurrency_ordering_and_completion() {
        let fake = Arc::new(HandshakeFakeSearcher::default());
        let tracks = (0..6).map(|i| track(&format!("T{i}"), "A")).collect();
        let p = playlist("sp_conc", tracks);
        let config = WorkerConfig { interval: Duration::ZERO, workers: 2, ..Default::default() };
        let rx = spawn_import(fake.clone(), p, Arc::new(AtomicBool::new(false)), config);
        let events: Vec<_> = rx.into_iter().collect();

        assert!(matches!(events.first(), Some(ImportProgressEvent::Started { total: 6 })));
        assert!(matches!(
            events.last(),
            Some(ImportProgressEvent::Finished { playlist }) if playlist.track_count() == 6
        ));
        let indices: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ImportProgressEvent::Track { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(indices, (0..6).collect::<Vec<_>>());
        assert_eq!(fake.max_seen.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_spawn_import_pre_cancelled_yields_cancelled_event_and_zero_searches() {
        let searcher = Arc::new(FakeSearcher::default());
        let p = playlist("sp_pre", vec![track("Track", "Artist")]);
        let rx = spawn_import(
            searcher.clone(),
            p,
            Arc::new(AtomicBool::new(true)),
            WorkerConfig::default(),
        );
        let events: Vec<_> = rx.into_iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ImportProgressEvent::Cancelled { playlist } if playlist.track_count() == 0
        ));
        assert_eq!(searcher.calls(), 0);
    }
}
