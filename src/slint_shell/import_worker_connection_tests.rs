#[cfg(test)]
mod tests {
    use super::super::*;

    struct FailingSearcher;
    impl TrackSearcher for FailingSearcher {
        fn search(&self, _: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
            Err(SearchFailure::Unavailable(anyhow::anyhow!("offline")))
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

    #[test]
    fn test_connection_lost_stops_pipeline_and_finishes_with_partial_results() {
        let p = playlist("sp_fail", vec![track("T1", "A"), track("T2", "A")]);
        let rx = spawn_import(
            Arc::new(FailingSearcher),
            p,
            Arc::new(AtomicBool::new(false)),
            WorkerConfig::default(),
        );
        let events: Vec<_> = rx.into_iter().collect();
        assert!(events.iter().any(|e| matches!(e, ImportProgressEvent::ConnectionLost { .. })));
        assert!(events.iter().any(|e| matches!(e, ImportProgressEvent::Finished { .. })));
        assert!(!events.iter().any(|e| matches!(e, ImportProgressEvent::Cancelled { .. })));
        let finish = events
            .iter()
            .find_map(|e| match e {
                ImportProgressEvent::Finished { playlist } => Some(playlist),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.track_count(), 0);
    }
}
