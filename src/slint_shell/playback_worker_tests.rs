use super::*;
use crate::domain::{MusicVideoType, SearchFilter, Track};
use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, StubScrobbler};
use crate::slint_shell::test_support::NoLastRequestedAudio;

fn dummy_play_req(track_id: &str) -> PlayTrackRequest {
    PlayTrackRequest {
        generation: 1,
        track_id: track_id.into(),
        title: "Song".into(),
        artist: "Artist".into(),
        artwork_url: None,
        duration_secs: Some(180),
        hi_res_covers: false,
        crossfade_seconds: 0,
        lyric_overrides: None,
        prefetch_lyrics: None,
        prefetch_stream_id: None,
        gapless: false,
    }
}

fn test_worker_adapters<A: AudioBackend>(
    audio: Arc<Mutex<A>>,
) -> PlaybackWorkerAdapters<A, LrclibStub, MemoryCoverCache> {
    let scrobbler = Arc::new(Mutex::new(Box::new(StubScrobbler::new()) as Box<dyn Scrobbler>));
    let lyrics = Arc::new(LrclibStub::new());
    let covers = Arc::new(Mutex::new(MemoryCoverCache::new()));
    PlaybackWorkerAdapters::new(audio, scrobbler, None, lyrics, covers)
}

#[test]
fn test_execute_play_track_in_worker_thread() {
    let audio = Arc::new(Mutex::new(AudioStub::new()));
    let adapters = test_worker_adapters(Arc::clone(&audio));
    let req = dummy_play_req("t1");

    let handle = std::thread::spawn(move || execute_play_track(req, &adapters));

    let res = handle.join().expect("thread join succeeds");
    assert_eq!(res.generation, 1);
    assert_eq!(res.track_id, "t1");
    assert!(res.success);
    assert_eq!(res.lyrics_lines.len(), 2);
    assert!(res.presence.is_playing);

    assert_eq!(audio.lock().unwrap().last_requested(), Some("t1"));
}

#[test]
fn gapless_request_appends_behind_the_active_track_and_reports_success() {
    let mut stub = AudioStub::new();
    assert_eq!(stub.play("current"), Ok(()));
    let audio = Arc::new(Mutex::new(stub));
    let adapters = test_worker_adapters(Arc::clone(&audio));
    let mut req = dummy_play_req("next");
    req.gapless = true;

    let res = execute_play_track(req, &adapters);

    assert!(res.success);
    assert!(res.gapless);
    let guard = audio.lock().unwrap();
    assert_eq!(guard.last_requested(), Some("next"));
    assert_eq!(guard.position_secs(), 0);
}

#[test]
fn gapless_request_without_active_sink_fails_without_starting() {
    let audio = Arc::new(Mutex::new(AudioStub::new()));
    let adapters = test_worker_adapters(Arc::clone(&audio));
    let mut req = dummy_play_req("next");
    req.gapless = true;

    let res = execute_play_track(req, &adapters);

    assert!(!res.success);
    assert_eq!(audio.lock().unwrap().last_requested(), None);
}

#[test]
fn failed_play_while_other_track_plays_reports_failure() {
    let mut stub = AudioStub::new();
    assert_eq!(stub.play("track-A"), Ok(()));
    stub.set_fail_plays(true);
    let audio = Arc::new(Mutex::new(stub));
    let adapters = test_worker_adapters(Arc::clone(&audio));
    let res = execute_play_track(dummy_play_req("track-B"), &adapters);
    assert!(!res.success);
    let guard = audio.lock().unwrap();
    assert_eq!(guard.last_requested(), Some("track-A"));
    assert!(guard.is_playing());
}

#[test]
fn success_requires_requested_id_match() {
    let audio = Arc::new(Mutex::new(NoLastRequestedAudio));
    let adapters = test_worker_adapters(audio);
    let res = execute_play_track(dummy_play_req("track-C"), &adapters);
    assert!(!res.success);
}

#[test]
fn test_execute_search_in_worker_thread() {
    let catalog = Arc::new(Mutex::new(CatalogStub::new()));
    let (cat_clone, q) = (Arc::clone(&catalog), SearchQuery::parse("rock").expect("valid query"));
    let q_clone = q.clone();
    let handle = std::thread::spawn(move || {
        execute_search(42, &q_clone, &cat_clone, Locale::English, false)
    });

    let (generation, query, shelves) = handle.join().expect("thread join succeeds");
    let shelves = shelves.expect("shelves ok");
    assert_eq!(generation, 42);
    assert_eq!(query, "rock");
    assert_eq!(shelves.len(), 1);
    assert_eq!(shelves[0].title, "Search Results");

    let cat_es = Arc::clone(&catalog);
    let handle_es =
        std::thread::spawn(move || execute_search(43, &q, &cat_es, Locale::Spanish, false));
    let (_, _, shelves_es) = handle_es.join().expect("thread join succeeds");
    let shelves_es = shelves_es.expect("shelves_es ok");
    assert_eq!(shelves_es[0].title, "Resultados de b\u{00fa}squeda");

    let q_albums =
        SearchQuery::parse("rock").expect("valid query").with_filter(SearchFilter::Albums);
    let (_, _, shelves_albums) = execute_search(44, &q_albums, &catalog, Locale::English, false);
    let shelves_albums = shelves_albums.expect("shelves_albums ok");
    assert_eq!(shelves_albums[0].items.len(), 1);
    assert_eq!(shelves_albums[0].items[0].kind(), SearchFilter::Albums);
}

#[derive(Debug, Default)]
struct VideoTrackSearchCatalog;

impl Catalog for VideoTrackSearchCatalog {
    fn search(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn search_filtered(&self, _: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        let t1 = Track::new("t1", "Video", "Artist").with_video_type(MusicVideoType::Omv);
        let t2 = Track::new("t2", "Song", "Artist").with_video_type(MusicVideoType::Atv);
        Ok(vec![SearchResult::Track(t1), SearchResult::Track(t2)])
    }
    fn search_by_lyrics(&self, s: &LyricsSnippet) -> Result<Vec<Track>, CatalogError> {
        Ok(vec![Track::new("lyric-hit", s.as_str(), "Artist")])
    }
    fn radio(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        Vec::new()
    }
    fn playlists(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
        Ok(Vec::new())
    }
    fn saved_albums(&self) -> Result<Vec<crate::domain::Album>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_artists(&self) -> Result<Vec<crate::domain::Artist>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_songs(&self) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_podcasts(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_profiles(&self) -> Result<Vec<crate::domain::Artist>, CatalogError> {
        Ok(Vec::new())
    }
    fn detail(&self, _: &str, _: DetailKind) -> Result<Option<Detail>, CatalogError> {
        Ok(None)
    }
}

#[test]
fn test_execute_search_routes_by_lyrics_flag() {
    let catalog = Arc::new(Mutex::new(VideoTrackSearchCatalog));
    let q = SearchQuery::parse("text").expect("query");
    let (_, _, plain) = execute_search(1, &q, &catalog, Locale::English, false);
    assert_eq!(plain.expect("plain")[0].items.len(), 2);

    let lyrics = q.with_lyrics(true);
    let (_, _, lyric_res) = execute_search(2, &lyrics, &catalog, Locale::English, false);
    let items = &lyric_res.expect("lyrics")[0].items;
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], crate::domain::SearchResult::Track(t) if t.id == "lyric-hit"));
}

#[test]
fn test_execute_search_with_songs_only_drops_video() {
    let catalog = Arc::new(Mutex::new(VideoTrackSearchCatalog));
    let q = SearchQuery::parse("query").expect("valid query");
    let (_, _, shelves) = execute_search(1, &q, &catalog, Locale::English, true);
    let shelves = shelves.expect("shelves ok");
    assert_eq!(shelves.len(), 1);
    assert_eq!(shelves[0].items.len(), 1);
    match &shelves[0].items[0] {
        crate::domain::SearchResult::Track(t) => assert_eq!(t.id, "t2"),
        _ => panic!("expected track result"),
    }
}

struct BadgeVideoSearchCatalog;

impl Catalog for BadgeVideoSearchCatalog {
    fn search(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn search_filtered(
        &self,
        _: &SearchQuery,
    ) -> Result<Vec<crate::domain::SearchResult>, CatalogError> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog_search_badges.json");
        let raw = std::fs::read_to_string(path).expect("fixture readable");
        let payload: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
        let hits = crate::infra::catalog_parse_search::parse_search_all(&payload);
        Ok(crate::infra::LiveCatalog::map_hits(hits, crate::domain::SearchFilter::All))
    }
    fn radio(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        Vec::new()
    }
    fn playlists(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
        Ok(Vec::new())
    }
    fn saved_albums(&self) -> Result<Vec<crate::domain::Album>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_artists(&self) -> Result<Vec<crate::domain::Artist>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_songs(&self) -> Result<Vec<crate::domain::Track>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_podcasts(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
        Ok(Vec::new())
    }
    fn library_profiles(&self) -> Result<Vec<crate::domain::Artist>, CatalogError> {
        Ok(Vec::new())
    }
    fn detail(&self, _: &str, _: DetailKind) -> Result<Option<Detail>, CatalogError> {
        Ok(None)
    }
}

#[test]
fn test_execute_search_with_songs_only_drops_video_classified_by_badge() {
    let catalog = Arc::new(Mutex::new(BadgeVideoSearchCatalog));
    let q = SearchQuery::parse("bachata").expect("valid query");
    let (_, _, unfiltered) = execute_search(1, &q, &catalog, Locale::English, false);
    let (_, _, filtered) = execute_search(1, &q, &catalog, Locale::English, true);
    let unfiltered = unfiltered.expect("unfiltered ok");
    let filtered = filtered.expect("filtered ok");

    let contains_video = |shelves: &[Shelf]| {
        shelves.iter().flat_map(|s| &s.items).any(|item| match item {
            crate::domain::SearchResult::Track(t) => t.id == "video-vid-2",
            _ => false,
        })
    };

    assert!(contains_video(&unfiltered), "unfiltered search includes badge-classified video");
    assert!(!contains_video(&filtered), "songs-only search drops badge-classified video");
}

#[test]
fn test_execute_detail_in_worker_thread() {
    let catalog = Arc::new(Mutex::new(CatalogStub::new()));
    let cat_clone = Arc::clone(&catalog);

    let handle =
        std::thread::spawn(move || execute_detail(7, "album-1", DetailKind::Album, &cat_clone));

    let (generation, detail) = handle.join().expect("thread join succeeds");
    let detail = detail.expect("detail ok");
    assert_eq!(generation, 7);
    assert!(detail.is_some());
    assert_eq!(detail.unwrap().title, "album-1 Title");
}

#[test]
fn test_worker_channel_dispatch_and_join() {
    let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<u64>();

    let handle = std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                PlaybackWorkerCommand::Play(req) => done_tx.send(req.generation).unwrap(),
                PlaybackWorkerCommand::FetchCover { generation, .. } => {
                    done_tx.send(generation).unwrap()
                }
                PlaybackWorkerCommand::Seek { position_secs } => {
                    done_tx.send(position_secs).unwrap()
                }
            }
        }
    });

    let mut play_req = dummy_play_req("test");
    play_req.generation = 99;
    tx.send(PlaybackWorkerCommand::Play(play_req)).unwrap();
    assert_eq!(done_rx.recv().unwrap(), 99);

    tx.send(PlaybackWorkerCommand::FetchCover {
        url: "https://artwork.local/art".into(),
        generation: 100,
    })
    .unwrap();
    assert_eq!(done_rx.recv().unwrap(), 100);

    tx.send(PlaybackWorkerCommand::Seek { position_secs: 42 }).unwrap();
    assert_eq!(done_rx.recv().unwrap(), 42);

    drop(tx);
    handle.join().expect("worker terminates on channel disconnect");
}

#[test]
fn test_execute_seek_in_worker_thread() {
    let audio = Arc::new(Mutex::new(AudioStub::new()));
    let _ = audio.lock().unwrap().play("track-1");
    execute_seek(35, &audio);
    assert_eq!(audio.lock().unwrap().position_secs(), 35);
}

#[test]
fn test_fetch_and_decode_cover_missing_returns_none() {
    let covers = Arc::new(Mutex::new(MemoryCoverCache::new()));
    let res = fetch_and_decode_cover("https://invalid.artwork.local/test", &covers, false);
    assert!(res.is_none());
}

#[test]
fn test_cover_fetch_concurrency_limit_constant() {
    assert_eq!(COVER_FETCH_CONCURRENCY_LIMIT, 4);
}

#[test]
fn test_thumbnail_variant_for_row_replaces_hqdefault_with_mqdefault() {
    assert_eq!(
        thumbnail_variant_for_row("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"),
        "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
    );
}

#[test]
fn test_thumbnail_variant_for_row_replaces_large_suffix() {
    assert_eq!(
        thumbnail_variant_for_row("https://lh3.googleusercontent.com/test-thumb-large"),
        "https://lh3.googleusercontent.com/test-thumb-small"
    );
    assert_eq!(
        thumbnail_variant_for_row("https://img.local/cover-large"),
        "https://img.local/cover-small"
    );
}

#[test]
fn test_thumbnail_variant_for_row_leaves_other_urls_untouched() {
    assert_eq!(
        thumbnail_variant_for_row("https://lh3.googleusercontent.com/artist-thumb"),
        "https://lh3.googleusercontent.com/artist-thumb"
    );
}

#[test]
fn test_search_results_sections_resolve_art_from_golden_fixture() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/catalog_search_badges.json");
    let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
    let payload: serde_json::Value = serde_json::from_str(&raw).expect("valid fixture json");

    let items = payload
        .pointer(
            "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents",
        )
        .and_then(serde_json::Value::as_array)
        .expect("fixture contents array");

    let mut section_art: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for item in items {
        let Some(renderer) = item.get("musicResponsiveListItemRenderer") else { continue };
        let Some(runs) = renderer
            .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let Some(badge) =
            runs.first().and_then(|r| r.get("text")).and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(thumbs) = renderer
            .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let Some(url) =
            thumbs.last().and_then(|t| t.get("url")).and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        section_art.insert(badge.trim().to_ascii_lowercase(), url.trim().to_string());
    }

    const B: &str = "https://lh3.googleusercontent.com";
    let cases = [
        ("song", format!("{B}/test-thumb-large"), format!("{B}/test-thumb-small")),
        ("video", format!("{B}/video-thumb-large"), format!("{B}/video-thumb-small")),
        ("artist", format!("{B}/artist-thumb"), format!("{B}/artist-thumb")),
        ("single", format!("{B}/single-thumb"), format!("{B}/single-thumb")),
        ("playlist", format!("{B}/playlist-thumb"), format!("{B}/playlist-thumb")),
    ];
    for (section, raw, small) in cases {
        let art = section_art.get(section).expect("section art");
        assert_eq!(art, &raw);
        assert_eq!(thumbnail_variant_for_row(art), small);
    }
}

#[test]
fn test_spawn_lyrics_prefetch_blank_or_none_returns_none() {
    let lyrics = Arc::new(LrclibStub::new());
    assert!(spawn_lyrics_prefetch(None, Arc::clone(&lyrics)).is_none());

    let blanks = [
        ("", "Title"),
        ("   ", "Title"),
        ("Artist", ""),
        ("Artist", "   "),
        ("", ""),
        ("  ", "  "),
    ];
    for (artist, title) in blanks {
        let prefetch =
            LyricsPrefetch { artist: artist.into(), title: title.into(), duration_secs: Some(120) };
        assert!(spawn_lyrics_prefetch(Some(prefetch), Arc::clone(&lyrics)).is_none());
    }
}

struct CountingPrefetchLyrics {
    calls: std::sync::atomic::AtomicUsize,
}

impl LyricsSource for CountingPrefetchLyrics {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        _duration_secs: Option<u64>,
    ) -> Option<crate::infra::SyncedLyrics> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(crate::infra::SyncedLyrics {
            lines: vec![LyricLine::new(0, format!("{title} by {artist}"))],
        })
    }
}

#[test]
fn test_spawn_lyrics_prefetch_warms_cache() {
    let fake = CountingPrefetchLyrics { calls: std::sync::atomic::AtomicUsize::new(0) };
    let cached = Arc::new(crate::infra::CachedLyrics::new(fake));

    let prefetch = LyricsPrefetch {
        artist: "Radiohead".into(),
        title: "Creep".into(),
        duration_secs: Some(236),
    };

    let handle = spawn_lyrics_prefetch(Some(prefetch), Arc::clone(&cached));
    let handle = handle.expect("prefetch thread spawned");
    handle.join().expect("prefetch thread join succeeds");

    assert_eq!(cached.inner().calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    let hit = cached.lookup("Radiohead", "Creep", Some(236));
    assert!(hit.is_some());
    assert_eq!(cached.inner().calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
