//! Command-line surface for headless Spotify and Exportify playlist imports.
//!
//! Provides CLI argument parsing, headless execution pipeline, text progress rendering,
//! and machine-readable JSON reporting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::domain::{MatchConfidence, SpotifyPlaylist, SpotifyTrack, parse_spotify_ref};
use crate::infra::{PlaylistStore, SpotifyEmbedClient, parse_exportify_csv};
use crate::slint_shell::import_worker::{
    ImportProgressEvent, TrackResolution, TrackSearcher, WorkerConfig, assemble_playlist,
    resolve_tracks,
};

/// Where an import request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSourceArg {
    Link(String),
    Csv(PathBuf),
}

/// Parsed command line for the import feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCli {
    pub source: ImportSourceArg,
    pub headless: bool,
    pub json: bool,
}

#[derive(Default)]
struct ParsedFlags {
    spotify: Option<String>,
    csv: Option<PathBuf>,
    headless: bool,
    json: bool,
}

fn take_flag_value(args: &[String], i: usize, flag: &str) -> anyhow::Result<(String, usize)> {
    let next_idx = i + 1;
    let val =
        args.get(next_idx).ok_or_else(|| anyhow::anyhow!("flag '{flag}' requires a value"))?;
    if val.starts_with("--") || val.trim().is_empty() {
        anyhow::bail!("flag '{flag}' requires a value");
    }
    Ok((val.clone(), next_idx + 1))
}

fn extract_flags(args: &[String]) -> anyhow::Result<ParsedFlags> {
    let mut flags = ParsedFlags::default();
    let mut i = 0;
    while i < args.len() {
        i = match args[i].as_str() {
            "--import-spotify" => {
                let (val, next) = take_flag_value(args, i, "--import-spotify")?;
                flags.spotify = Some(val);
                next
            }
            "--import-csv" => {
                let (val, next) = take_flag_value(args, i, "--import-csv")?;
                flags.csv = Some(PathBuf::from(val));
                next
            }
            "--headless" => {
                flags.headless = true;
                i + 1
            }
            "--json" => {
                flags.json = true;
                i + 1
            }
            _ => i + 1,
        };
    }
    Ok(flags)
}

fn validate_flags(flags: ParsedFlags) -> anyhow::Result<Option<ImportCli>> {
    if flags.spotify.is_some() && flags.csv.is_some() {
        anyhow::bail!("cannot specify both --import-spotify and --import-csv");
    }
    if flags.json && !flags.headless {
        anyhow::bail!("--json requires --headless");
    }
    let source = match (flags.spotify, flags.csv) {
        (Some(url), None) => ImportSourceArg::Link(url),
        (None, Some(path)) => ImportSourceArg::Csv(path),
        _ => return Ok(None),
    };
    Ok(Some(ImportCli { source, headless: flags.headless, json: flags.json }))
}

/// Parses import-related arguments; `Ok(None)` when no import was requested.
pub fn parse_import_args(args: &[String]) -> anyhow::Result<Option<ImportCli>> {
    let flags = extract_flags(args)?;
    validate_flags(flags)
}

/// One resolved track in the machine-readable report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportTrack {
    pub source_title: String,
    pub source_artist: String,
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub confidence: String,
    pub resolved: bool,
}

/// Machine-readable summary of a headless import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub playlist_id: String,
    pub playlist_title: String,
    pub total: usize,
    pub matched: usize,
    pub high: usize,
    pub review: usize,
    pub low: usize,
    pub unresolved: usize,
    pub tracks: Vec<ReportTrack>,
}

/// Serializes the report as pretty JSON.
pub fn report_to_json(report: &ImportReport) -> String {
    let mut json = serde_json::to_string_pretty(report).unwrap_or_default();
    json.push('\n');
    json
}

fn confidence_label_key(confidence: MatchConfidence) -> &'static str {
    match confidence {
        MatchConfidence::High => "High confidence",
        MatchConfidence::Medium => "Medium confidence",
        MatchConfidence::Low => "Low confidence",
    }
}

fn map_resolution_to_report_track(
    source_track: Option<&SpotifyTrack>,
    resolution: &TrackResolution,
) -> ReportTrack {
    match resolution {
        TrackResolution::Matched { track, .. } => ReportTrack {
            source_title: source_track
                .map(|t| t.title.clone())
                .unwrap_or_else(|| track.title.clone()),
            source_artist: source_track
                .map(|t| t.artist.clone())
                .unwrap_or_else(|| track.artist.clone()),
            video_id: track.video_id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            confidence: confidence_label_key(track.confidence).to_string(),
            resolved: true,
        },
        TrackResolution::Unresolved { title, artist, .. } => ReportTrack {
            source_title: source_track.map(|t| t.title.clone()).unwrap_or_else(|| title.clone()),
            source_artist: source_track.map(|t| t.artist.clone()).unwrap_or_else(|| artist.clone()),
            video_id: String::new(),
            title: source_track.map(|t| t.title.clone()).unwrap_or_else(|| title.clone()),
            artist: source_track.map(|t| t.artist.clone()).unwrap_or_else(|| artist.clone()),
            confidence: String::new(),
            resolved: false,
        },
    }
}

#[derive(Default)]
struct ReportTallies {
    matched: usize,
    high: usize,
    review: usize,
    low: usize,
    unresolved: usize,
}

fn tally_resolution(tallies: &mut ReportTallies, resolution: &TrackResolution) {
    match resolution {
        TrackResolution::Matched { track, .. } => {
            tallies.matched += 1;
            match track.confidence {
                MatchConfidence::High => tallies.high += 1,
                MatchConfidence::Medium => tallies.review += 1,
                MatchConfidence::Low => tallies.low += 1,
            }
        }
        TrackResolution::Unresolved { .. } => {
            tallies.unresolved += 1;
        }
    }
}

/// Builds the report from the source and the worker resolutions.
pub fn build_report(
    source: &SpotifyPlaylist,
    resolutions: &[TrackResolution],
    playlist_id: &str,
) -> ImportReport {
    let mut tallies = ReportTallies::default();
    let mut tracks = Vec::with_capacity(resolutions.len());

    for (i, res) in resolutions.iter().enumerate() {
        tally_resolution(&mut tallies, res);
        tracks.push(map_resolution_to_report_track(source.tracks.get(i), res));
    }

    ImportReport {
        playlist_id: playlist_id.to_string(),
        playlist_title: source.title.clone(),
        total: source.tracks.len(),
        matched: tallies.matched,
        high: tallies.high,
        review: tallies.review,
        low: tallies.low,
        unresolved: tallies.unresolved,
        tracks,
    }
}

/// Runs the text progress rendering used on stderr.
pub fn render_progress(event: &ImportProgressEvent) -> Option<String> {
    match event {
        ImportProgressEvent::Started { total } => Some(format!("import started: {total} tracks")),
        ImportProgressEvent::Progress { done, total, unresolved, .. } => {
            let matched = done.saturating_sub(*unresolved);
            Some(format!("{matched} / {total} matched"))
        }
        ImportProgressEvent::Finished { .. } => Some("import finished".to_string()),
        ImportProgressEvent::Cancelled { .. } => Some("import cancelled".to_string()),
        ImportProgressEvent::Failed { message } => Some(format!("import failed: {message}")),
        ImportProgressEvent::Track { .. } => None,
    }
}

fn load_link_source(url: &str) -> anyhow::Result<SpotifyPlaylist> {
    let spotify_ref = parse_spotify_ref(url)
        .ok_or_else(|| anyhow::anyhow!("invalid or unsupported Spotify URL or URI: '{url}'"))?;
    let client = SpotifyEmbedClient::new();
    client.fetch(&spotify_ref)
}

fn load_csv_source(path: &Path) -> anyhow::Result<SpotifyPlaylist> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read CSV file at '{}'", path.display()))?;
    let tracks = parse_exportify_csv(&content)
        .with_context(|| format!("failed to parse CSV file at '{}'", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("import")
        .to_string();
    Ok(SpotifyPlaylist {
        id: stem.clone(),
        title: stem,
        author: String::new(),
        artwork: None,
        tracks,
    })
}

fn load_source(source_arg: &ImportSourceArg) -> anyhow::Result<SpotifyPlaylist> {
    match source_arg {
        ImportSourceArg::Link(url) => load_link_source(url),
        ImportSourceArg::Csv(path) => load_csv_source(path),
    }
}

struct PipelineRunner<'a> {
    searcher: &'a dyn TrackSearcher,
    store: &'a dyn PlaylistStore,
    config: &'a WorkerConfig,
    sleeper: &'a (dyn Fn(Duration) + Sync),
}

impl<'a> PipelineRunner<'a> {
    fn execute(
        &self,
        source: &SpotifyPlaylist,
        progress: &mut dyn FnMut(&str),
    ) -> anyhow::Result<ImportReport> {
        let cancel = AtomicBool::new(false);
        let cache = Mutex::new(HashMap::new());
        let mut on_event = |event: ImportProgressEvent| {
            if let Some(msg) = render_progress(&event) {
                progress(&msg);
            }
        };

        let (resolutions, _, _) = resolve_tracks(
            self.searcher,
            source,
            &cancel,
            self.config,
            self.sleeper,
            &cache,
            &mut on_event,
        );

        let playlist = assemble_playlist(source, &resolutions);
        self.store.save(&playlist)?;
        Ok(build_report(source, &resolutions, &playlist.id))
    }
}

fn run_headless_with_runner(
    cli: &ImportCli,
    runner: &PipelineRunner<'_>,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<ImportReport> {
    match load_source(&cli.source).and_then(|source| runner.execute(&source, progress)) {
        Ok(report) => Ok(report),
        Err(err) => {
            if let Some(msg) =
                render_progress(&ImportProgressEvent::Failed { message: err.to_string() })
            {
                progress(&msg);
            }
            Err(err)
        }
    }
}

/// Headless pipeline: load the source, run the worker, persist, return the report.
pub fn run_headless(
    cli: &ImportCli,
    searcher: Arc<dyn TrackSearcher>,
    store: &dyn PlaylistStore,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<ImportReport> {
    let runner = PipelineRunner {
        searcher: searcher.as_ref(),
        store,
        config: &WorkerConfig::default(),
        sleeper: &std::thread::sleep,
    };
    run_headless_with_runner(cli, &runner, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchCandidate, MusicVideoType};
    use crate::infra::{MemoryPlaylistStore, SearchFailure};
    use crate::slint_shell::import_searcher::CandidateHit;

    #[test]
    fn test_parse_import_args_link_only() {
        let args = vec![
            "--import-spotify".to_string(),
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string(),
        ];
        let cli = parse_import_args(&args).expect("parses link").expect("some");
        assert_eq!(
            cli.source,
            ImportSourceArg::Link(
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string()
            )
        );
        assert!(!cli.headless);
        assert!(!cli.json);
    }

    #[test]
    fn test_parse_import_args_csv_only() {
        let args = vec!["--import-csv".to_string(), "my_playlist.csv".to_string()];
        let cli = parse_import_args(&args).expect("parses csv").expect("some");
        assert_eq!(cli.source, ImportSourceArg::Csv(PathBuf::from("my_playlist.csv")));
        assert!(!cli.headless);
        assert!(!cli.json);
    }

    #[test]
    fn test_parse_import_args_each_with_headless() {
        let spotify_args = vec![
            "--headless".to_string(),
            "--import-spotify".to_string(),
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string(),
        ];
        let cli_sp = parse_import_args(&spotify_args).unwrap().unwrap();
        assert!(cli_sp.headless);
        assert!(!cli_sp.json);

        let csv_args =
            vec!["--import-csv".to_string(), "tracks.csv".to_string(), "--headless".to_string()];
        let cli_csv = parse_import_args(&csv_args).unwrap().unwrap();
        assert!(cli_csv.headless);
        assert!(!cli_csv.json);
    }

    #[test]
    fn test_parse_import_args_with_headless_and_json() {
        let args = vec![
            "--headless".to_string(),
            "--json".to_string(),
            "--import-csv".to_string(),
            "tracks.csv".to_string(),
        ];
        let cli = parse_import_args(&args).unwrap().unwrap();
        assert!(cli.headless);
        assert!(cli.json);
    }

    #[test]
    fn test_parse_import_args_both_sources_errors() {
        let args = vec![
            "--import-spotify".to_string(),
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string(),
            "--import-csv".to_string(),
            "tracks.csv".to_string(),
        ];
        let err = parse_import_args(&args).expect_err("mutually exclusive flags must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("--import-spotify") && msg.contains("--import-csv"));
    }

    #[test]
    fn test_parse_import_args_missing_value_errors() {
        let args_sp = vec!["--import-spotify".to_string()];
        let err_sp = parse_import_args(&args_sp).expect_err("missing value for spotify link");
        assert!(format!("{err_sp:#}").contains("--import-spotify"));

        let args_csv = vec!["--import-csv".to_string()];
        let err_csv = parse_import_args(&args_csv).expect_err("missing value for csv");
        assert!(format!("{err_csv:#}").contains("--import-csv"));

        let args_next_flag = vec!["--import-spotify".to_string(), "--headless".to_string()];
        let err_next = parse_import_args(&args_next_flag).expect_err("flag cannot be value");
        assert!(format!("{err_next:#}").contains("--import-spotify"));
    }

    #[test]
    fn test_parse_import_args_json_without_headless_errors() {
        let args = vec![
            "--import-spotify".to_string(),
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string(),
            "--json".to_string(),
        ];
        let err = parse_import_args(&args).expect_err("--json without --headless must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("--json") && msg.contains("--headless"));

        let args_json_only = vec!["--json".to_string()];
        let err_only = parse_import_args(&args_json_only).expect_err("lone json must fail");
        assert!(format!("{err_only:#}").contains("--json"));
    }

    #[test]
    fn test_parse_import_args_no_import_flags_returns_none() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(parse_import_args(&empty).unwrap(), None);

        let unrelated = vec!["--verbose".to_string(), "moggyt:handoff_payload".to_string()];
        assert_eq!(parse_import_args(&unrelated).unwrap(), None);
    }

    #[test]
    fn test_parse_import_args_unrelated_flags_ignored() {
        let args = vec![
            "--verbose".to_string(),
            "--import-csv".to_string(),
            "custom.csv".to_string(),
            "--port".to_string(),
            "8080".to_string(),
        ];
        let cli = parse_import_args(&args).unwrap().unwrap();
        assert_eq!(cli.source, ImportSourceArg::Csv(PathBuf::from("custom.csv")));
        assert!(!cli.headless);
        assert!(!cli.json);
    }

    #[test]
    fn test_parse_import_args_flag_order_independence() {
        let a1 = vec![
            "--headless".to_string(),
            "--json".to_string(),
            "--import-spotify".to_string(),
            "https://open.spotify.com/album/37i9dQZF1DXcBWIGoYBM5M".to_string(),
        ];
        let a2 = vec![
            "--import-spotify".to_string(),
            "https://open.spotify.com/album/37i9dQZF1DXcBWIGoYBM5M".to_string(),
            "--json".to_string(),
            "--headless".to_string(),
        ];
        let c1 = parse_import_args(&a1).unwrap().unwrap();
        let c2 = parse_import_args(&a2).unwrap().unwrap();
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_render_progress_all_event_variants() {
        let started = ImportProgressEvent::Started { total: 42 };
        assert_eq!(render_progress(&started), Some("import started: 42 tracks".to_string()));

        let progress =
            ImportProgressEvent::Progress { done: 5, total: 50, high: 3, review: 0, unresolved: 2 };
        assert_eq!(render_progress(&progress), Some("3 / 50 matched".to_string()));

        let finished = ImportProgressEvent::Finished {
            playlist: crate::domain::LocalPlaylist::new("local:1", "Fin"),
        };
        assert_eq!(render_progress(&finished), Some("import finished".to_string()));

        let cancelled = ImportProgressEvent::Cancelled {
            playlist: crate::domain::LocalPlaylist::new("local:1", "Canc"),
        };
        assert_eq!(render_progress(&cancelled), Some("import cancelled".to_string()));

        let failed = ImportProgressEvent::Failed { message: "timeout".to_string() };
        assert_eq!(render_progress(&failed), Some("import failed: timeout".to_string()));

        let track = ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Unresolved {
                title: "T".to_string(),
                artist: "A".to_string(),
                reason: "no match".to_string(),
            },
        };
        assert_eq!(render_progress(&track), None);
    }

    #[test]
    fn test_build_report_mixed_resolutions() {
        let source = SpotifyPlaylist {
            id: "sp123".to_string(),
            title: "Road Trip".to_string(),
            author: "DJ".to_string(),
            artwork: None,
            tracks: vec![
                SpotifyTrack {
                    title: "Song High".to_string(),
                    artist: "Artist 1".to_string(),
                    album: None,
                    duration_ms: 180000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song Med".to_string(),
                    artist: "Artist 2".to_string(),
                    album: None,
                    duration_ms: 200000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song Low".to_string(),
                    artist: "Artist 3".to_string(),
                    album: None,
                    duration_ms: 210000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song Unres".to_string(),
                    artist: "Artist 4".to_string(),
                    album: None,
                    duration_ms: 220000,
                    isrc: None,
                },
            ],
        };

        let t1 = LocalTrack::new("v1", "Song High", "Artist 1", MatchConfidence::High);
        let t2 = LocalTrack::new("v2", "Song Med", "Artist 2", MatchConfidence::Medium);
        let t3 = LocalTrack::new("v3", "Song Low", "Artist 3", MatchConfidence::Low);

        let resolutions = vec![
            TrackResolution::Matched { track: t1.clone(), alternatives: vec![t1] },
            TrackResolution::Matched { track: t2.clone(), alternatives: vec![t2] },
            TrackResolution::Matched { track: t3.clone(), alternatives: vec![t3] },
            TrackResolution::Unresolved {
                title: "Song Unres".to_string(),
                artist: "Artist 4".to_string(),
                reason: "no suitable match found".to_string(),
            },
        ];

        let report = build_report(&source, &resolutions, "local:sp123");
        assert_eq!(report.playlist_id, "local:sp123");
        assert_eq!(report.playlist_title, "Road Trip");
        assert_eq!(report.total, 4);
        assert_eq!(report.matched, 3);
        assert_eq!(report.high, 1);
        assert_eq!(report.review, 1);
        assert_eq!(report.low, 1);
        assert_eq!(report.unresolved, 1);
        assert_eq!(report.tracks.len(), 4);

        assert_eq!(report.tracks[0].video_id, "v1");
        assert_eq!(report.tracks[0].confidence, "High confidence");
        assert!(report.tracks[0].resolved);

        assert_eq!(report.tracks[1].video_id, "v2");
        assert_eq!(report.tracks[1].confidence, "Medium confidence");
        assert!(report.tracks[1].resolved);

        assert_eq!(report.tracks[2].video_id, "v3");
        assert_eq!(report.tracks[2].confidence, "Low confidence");
        assert!(report.tracks[2].resolved);

        assert_eq!(report.tracks[3].video_id, "");
        assert_eq!(report.tracks[3].confidence, "");
        assert!(!report.tracks[3].resolved);
        assert_eq!(report.tracks[3].source_title, "Song Unres");
        assert_eq!(report.tracks[3].source_artist, "Artist 4");
        assert_eq!(report.tracks[3].title, "Song Unres");
        assert_eq!(report.tracks[3].artist, "Artist 4");
    }

    #[test]
    fn test_report_to_json_roundtrip_with_every_field() {
        let report = ImportReport {
            playlist_id: "local:roundtrip".to_string(),
            playlist_title: "Title".to_string(),
            total: 1,
            matched: 1,
            high: 1,
            review: 0,
            low: 0,
            unresolved: 0,
            tracks: vec![ReportTrack {
                source_title: "ST".to_string(),
                source_artist: "SA".to_string(),
                video_id: "VID".to_string(),
                title: "T".to_string(),
                artist: "A".to_string(),
                confidence: "High confidence".to_string(),
                resolved: true,
            }],
        };

        let json = report_to_json(&report);
        assert!(json.ends_with('\n'));
        let parsed: ImportReport = serde_json::from_str(&json).expect("deserializes JSON");
        assert_eq!(parsed, report);
    }

    struct FakeSearcher {
        hit: CandidateHit,
    }

    impl TrackSearcher for FakeSearcher {
        fn search(&self, _query: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
            Ok(vec![self.hit.clone()])
        }
    }

    #[test]
    fn test_run_headless_with_csv_saves_playlist_and_returns_report() {
        let temp_dir = std::env::temp_dir();
        let csv_path = temp_dir.join(format!("my_playlist_test_{}.csv", std::process::id()));
        let csv_content = "Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms),ISRC\n\
                           sp:1,Summer Song,Great Artist,Summer Album,180000,US1234567890\n";
        std::fs::write(&csv_path, csv_content).expect("writes temp CSV");

        let cli = ImportCli {
            source: ImportSourceArg::Csv(csv_path.clone()),
            headless: true,
            json: false,
        };

        let hit = CandidateHit::new(
            "yt_video_123",
            None,
            MatchCandidate::new("Summer Song", "Great Artist", Some(180), MusicVideoType::Atv),
        );
        let searcher = Arc::new(FakeSearcher { hit });
        let store = MemoryPlaylistStore::new();

        let mut progress_lines = Vec::new();
        let runner = PipelineRunner {
            searcher: searcher.as_ref(),
            store: &store,
            config: &WorkerConfig { interval: Duration::ZERO, ..Default::default() },
            sleeper: &|_| {},
        };
        let report = run_headless_with_runner(&cli, &runner, &mut |line| {
            progress_lines.push(line.to_string())
        })
        .expect("headless run succeeds");

        let _ = std::fs::remove_file(&csv_path);

        assert_eq!(report.matched, 1);
        assert_eq!(report.total, 1);
        assert!(report.playlist_id.starts_with("local:"));
        assert_eq!(report.playlist_title, format!("my_playlist_test_{}", std::process::id()));

        let stored = store.list();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, report.playlist_id);
        assert_eq!(stored[0].tracks.len(), 1);
        assert_eq!(stored[0].tracks[0].video_id, "yt_video_123");

        assert!(progress_lines.iter().any(|l| l.contains("matched")));
        assert!(progress_lines.iter().any(|l| l == "import finished"));
    }
}
