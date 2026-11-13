#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;

    fn fixture_json(name: &str) -> Value {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture readable"))
            .expect("valid fixture")
    }

    fn check_track(t: &RawTrack, id: &str, title: &str, artist: &str) {
        assert_eq!((t.video_id.as_str(), &t.title[..], &t.artist[..]), (id, title, artist));
    }

    fn check_album(a: &RawAlbum, id: &str, title: &str, artist: &str) {
        assert_eq!((a.browse_id.as_str(), &a.title[..], &a.artist[..]), (id, title, artist));
    }

    fn check_artist(a: &RawArtist, id: &str, name: &str) {
        assert_eq!((a.browse_id.as_str(), a.name.as_str()), (id, name));
    }

    fn check_playlist(p: &RawPlaylist, id: &str, title: &str) {
        assert_eq!((p.browse_id.as_str(), p.title.as_str()), (id, title));
    }

    fn check_lh3(url: Option<&str>, path: &str) {
        assert_eq!(url, Some(format!("https://lh3.googleusercontent.com/{path}").as_str()));
    }

    #[test]
    fn parse_search_tracks_tolerates_empty_and_malformed() {
        assert!(parse_search_tracks(&serde_json::json!({})).is_empty());
        assert!(parse_search_tracks(&serde_json::json!({"contents": null})).is_empty());
        let tabs = serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": []}}});
        assert!(parse_search_tracks(&tabs).is_empty());
    }

    #[test]
    fn parse_search_tracks_golden_fixture() {
        let tracks = parse_search_tracks(&fixture_json("catalog_search.json"));
        assert_eq!(tracks.len(), 2);
        check_track(&tracks[0], "vid-1", "Midnight City", "M83");
        check_lh3(tracks[0].thumb_url.as_deref(), "test-thumb-large");
        check_track(&tracks[1], "vid-2", "Starboy", "The Weeknd");
        assert_eq!(tracks[1].thumb_url, None);
    }

    #[test]
    fn parse_search_tracks_songs_fixture_extracts_video_type() {
        let tracks = parse_search_tracks(&fixture_json("catalog_search_songs.json"));
        assert!(!tracks.is_empty());
        assert!(tracks.iter().all(|t| t.video_type == Some(crate::domain::MusicVideoType::Atv)));
        assert_eq!(tracks[0].duration_secs, Some(224));
        assert_eq!(tracks[2].duration_secs, Some(156));
    }

    #[test]
    fn test_parse_search_tracks_resolves_artist_past_type_and_duration() {
        let (resp, w) = (test_responsive_item, test_watch);
        let items = vec![
            resp(
                "Bohemian Rhapsody",
                &["Canci\u{f3}n", " \u{2022} ", "Queen"],
                w("vid-type-artist"),
            ),
            resp("Tear It Up", &["Cancion", " \u{2022} ", "3:25"], w("vid-type-only")),
            resp(
                "Loser",
                &["Tame Impala", " \u{2022} ", "Deadbeat", " \u{2022} ", "3:44"],
                w("vid-artist-first"),
            ),
            resp(
                "Bohemian Rhapsody (Live)",
                &["Canci\u{f3}n", " \u{2022} ", "Queen"],
                w("vid-accented"),
            ),
        ];
        let shelf = serde_json::json!({"musicShelfRenderer": {"title": {"runs": [{"text": "Canciones"}]}, "contents": items}});
        let tracks = parse_search_tracks(&tabbed_sections(vec![shelf]));
        assert_eq!(tracks.len(), 4);
        assert_eq!(tracks[0].artist, "Queen");
        assert_eq!(tracks[1].artist, "", "a row with only a type and a duration has no artist");
        assert_eq!(
            tracks[2].artist, "Tame Impala \u{2022} Deadbeat",
            "artist-first rows keep their artist"
        );
        assert_eq!(tracks[3].artist, "Queen", "accented Spanish type labels are stripped too");
    }

    #[test]
    fn search_duration_extracts_from_fixed_length_and_flex_columns() {
        let fixed_r = |t| serde_json::json!({"fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": t}]}}}]});
        let fixed_s = |t| serde_json::json!({"fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": t}}}]});
        let len_r = |t| serde_json::json!({"lengthText": {"runs": [{"text": t}]}});
        let len_s = |t| serde_json::json!({"lengthText": {"simpleText": t}});
        let flex_r = serde_json::json!({"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Artist"}, {"text": "3:44"}]}}}]});
        let cases = [
            (fixed_r("3:45"), Some(225)),
            (fixed_s("1:20"), Some(80)),
            (len_r("2:30"), Some(150)),
            (len_s("4:00"), Some(240)),
            (flex_r, Some(224)),
            (serde_json::json!({}), None),
        ];
        for (renderer, expected) in cases {
            assert_eq!(search_duration(&renderer), expected);
        }
    }

    fn tabbed_sections(sections: Vec<Value>) -> Value {
        serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": sections}}}}]}}})
    }

    fn test_responsive_item(title: &str, badge_runs: &[&str], endpoint: Value) -> Value {
        let runs: Vec<_> = badge_runs.iter().map(|t| serde_json::json!({ "text": t })).collect();
        serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": title }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": runs } } }
                ],
                "navigationEndpoint": endpoint
            }
        })
    }

    #[test]
    fn parse_search_tracks_skips_item_without_title() {
        let item = serde_json::json!({"musicResponsiveListItemRenderer": {
            "navigationEndpoint": { "watchEndpoint": { "videoId": "v1" } }
        }});
        let payload = tabbed_sections(vec![serde_json::json!({
            "musicShelfRenderer": { "contents": [item] }
        })]);
        assert!(parse_search_tracks(&payload).is_empty());
    }

    #[test]
    fn shelf_category_routing_table() {
        for title in ["Songs", "Top result", "Top songs", "Videos", "V\u{ed}deos", "Music videos"] {
            assert_eq!(shelf_category(title), Some(SearchFilter::Tracks));
        }
        assert_eq!(shelf_category("Albums"), Some(SearchFilter::Albums));
        assert_eq!(shelf_category("Artists"), Some(SearchFilter::Artists));
        for title in ["Playlists", "Community mixes", "Featured shows"] {
            assert_eq!(shelf_category(title), Some(SearchFilter::Playlists));
        }
        assert_eq!(shelf_category("Podcasts"), None);
        assert_eq!(shelf_category(""), None);
    }

    #[test]
    fn parse_search_all_golden_fixture_and_determinism() {
        let payload = fixture_json("catalog_shelves.json");
        let first = parse_search_all(&payload);
        assert_eq!(first, parse_search_all(&payload), "parsing must be deterministic");
        assert_eq!(
            (first.tracks.len(), first.albums.len(), first.artists.len(), first.playlists.len()),
            (1, 1, 1, 0)
        );
        check_track(&first.tracks[0], "solar-1", "Solaris", "Photay");
        assert_eq!(first.tracks[0].thumb_url, None);
        check_album(&first.albums[0], "MPREb_album_1", "Waking Season", "Photay");
        check_artist(&first.artists[0], "UC_photay_1", "Photay");
    }

    #[test]
    fn parse_search_filtered_albums_golden_fixture() {
        let hits = parse_search_all(&fixture_json("catalog_search_filtered_albums.json"));
        assert_eq!(hits.albums.len(), 1);
        check_album(&hits.albums[0], "MPREb_test_album_1", "Test Album Title", "Test Artist");
        check_lh3(hits.albums[0].thumb_url.as_deref(), "test-album-thumb");
        assert_eq!(hits.albums[0].artist_id.as_deref(), Some("UC_test_artist"));
        assert_eq!(hits.albums[0].release_type.as_deref(), Some("Album"));
    }

    fn test_endpoint(id: &str, page_type: &str) -> Value {
        serde_json::json!({ "browseEndpoint": { "browseId": id, "pageType": page_type } })
    }

    fn test_browse_item(title: &str, badge: &str, id: &str, pt: &str) -> Value {
        test_responsive_item(title, &[badge], test_endpoint(id, pt))
    }

    #[test]
    fn parse_search_spanish_badges_and_page_types() {
        let browse = test_browse_item;
        let artist =
            browse("Artista Destacado", "Artista", "UC_es_artist", "MUSIC_PAGE_TYPE_ARTIST");
        let playlist = browse(
            "Exitos Espanol",
            "Lista de reproduccion",
            "VL_es_playlist",
            "MUSIC_PAGE_TYPE_PLAYLIST",
        );
        let single =
            browse("Sencillo Nuevo", "Sencillo", "MPREb_es_single", "MUSIC_PAGE_TYPE_ALBUM");
        let profile =
            browse("Perfil Usuario", "Perfil", "UC_profile", "MUSIC_PAGE_TYPE_USER_CHANNEL");
        let payload = tabbed_sections(vec![serde_json::json!({
            "itemSectionRenderer": { "contents": [artist, playlist, single, profile] }
        })]);
        let hits = parse_search_all(&payload);
        assert_eq!(hits.artists[0].browse_id, "UC_es_artist");
        assert_eq!(hits.playlists[0].browse_id, "VL_es_playlist");
        assert_eq!(hits.albums[0].browse_id, "MPREb_es_single");
    }

    #[test]
    fn parse_search_all_ignores_unknown_shelves_and_tolerates_empty() {
        assert!(parse_search_all(&serde_json::json!({})).is_empty());
        let item =
            serde_json::json!({"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Item"}]}}});
        let shelf = serde_json::json!({"musicShelfRenderer": {
            "title": {"runs": [{"text": "Podcasts"}]}, "contents": [item]
        }});
        assert!(parse_search_all(&tabbed_sections(vec![shelf])).is_empty());
    }

    #[test]
    fn parse_search_live_shape_and_determinism() {
        let payload = fixture_json("catalog_live_shape.json");
        let tracks = parse_search_tracks(&payload);
        assert_eq!(tracks, parse_search_tracks(&payload));
        assert_eq!(tracks.len(), 2);
        check_track(&tracks[0], "live-bare-1", "Live Track Bare", "Live Artist Bare");
        check_track(&tracks[1], "live-shelf-1", "Live Track Shelved", "Live Artist Shelved");

        let all = parse_search_all(&payload);
        assert_eq!(all, parse_search_all(&payload));
        assert_eq!(all.tracks.len(), 2);
        assert!(all.albums.is_empty() && all.artists.is_empty() && all.playlists.is_empty());
    }

    #[test]
    fn parse_search_all_badges_golden_fixture() {
        let payload = fixture_json("catalog_search_badges.json");
        let first = parse_search_all(&payload);
        let second = parse_search_all(&payload);
        assert_eq!(first, second, "parsing must be deterministic");

        assert_eq!(first.tracks.len(), 3);
        assert_eq!(first.albums.len(), 1);
        assert_eq!(first.artists.len(), 1);
        assert_eq!(first.playlists.len(), 1);

        check_track(&first.tracks[0], "song-vid-1", "Bachata Rosa", "Juan Luis Guerra & 4.40");
        check_lh3(first.tracks[0].thumb_url.as_deref(), "test-thumb-large");
        check_track(
            &first.tracks[1],
            "video-vid-2",
            "Propuesta Indecente (Official Video)",
            "Romeo Santos",
        );
        check_lh3(first.tracks[1].thumb_url.as_deref(), "video-thumb-large");
        assert_eq!(first.tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));
        check_track(
            &first.tracks[2],
            "ep-vid-6",
            "Episode 42: Latin Music History",
            "Latin Beats Podcast",
        );
        check_lh3(first.tracks[2].thumb_url.as_deref(), "episode-thumb");
        check_album(&first.albums[0], "MPREb_single_1", "Eres Mia", "Romeo Santos");
        assert_eq!(first.albums[0].artist_id.as_deref(), Some("UC_romeo_1"));
        assert_eq!(first.albums[0].release_type.as_deref(), Some("Single"));
        check_lh3(first.albums[0].thumb_url.as_deref(), "single-thumb");
        check_artist(&first.artists[0], "UC_romeo_1", "Romeo Santos");
        check_lh3(first.artists[0].thumb_url.as_deref(), "artist-thumb");
        check_playlist(&first.playlists[0], "VL_playlist_1", "Bachata Essentials");
        check_lh3(first.playlists[0].thumb_url.as_deref(), "playlist-thumb");
    }

    fn test_track_item(title: &str, vid: &str, badge: Option<&str>) -> Value {
        let run_col = |t| serde_json::json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}});
        let mut cols = vec![run_col(title)];
        if let Some(b) = badge {
            cols.push(run_col(b));
        }
        serde_json::json!({"musicResponsiveListItemRenderer": {"flexColumns": cols, "playlistItemData": {"videoId": vid}}})
    }

    fn test_watch(vid: &str) -> Value {
        serde_json::json!({ "watchEndpoint": { "videoId": vid } })
    }

    #[test]
    fn parse_search_all_badges_skips_profile_and_podcast() {
        let (browse, resp) = (test_browse_item, test_responsive_item);
        let items = [
            browse("User", "Profile", "UC_1", "MUSIC_PAGE_TYPE_USER_CHANNEL"),
            browse("Pod", "Podcast", "MPSP_1", "MUSIC_PAGE_TYPE_PODCAST_SHOW"),
            resp("Unknown", &["MysteryKind"], Value::Null),
        ];
        let payload =
            tabbed_sections(vec![serde_json::json!({"itemSectionRenderer": {"contents": items}})]);
        assert!(parse_search_all(&payload).is_empty(), "profile and podcast must be skipped");
    }

    #[test]
    fn parse_search_all_badges_empty_artist_and_defaults() {
        let items = [
            test_track_item("Solo Track", "solo-vid-1", Some("Song")),
            test_track_item("No Badge Track", "no-badge-vid-2", None),
        ];
        let payload =
            tabbed_sections(vec![serde_json::json!({"itemSectionRenderer": {"contents": items}})]);
        let hits = parse_search_all(&payload);
        assert_eq!(hits.tracks.len(), 2);
        check_track(&hits.tracks[0], "solo-vid-1", "Solo Track", "");
        check_track(&hits.tracks[1], "no-badge-vid-2", "No Badge Track", "");
    }

    #[test]
    fn is_video_badge_classification() {
        for badge in VIDEO_BADGES {
            assert!(is_video_badge(badge), "expected video badge for {badge}");
        }
        for badge in ["Video", "V\u{ed}deo", "V\u{cd}DEO", "Music Video", "Videoclip"] {
            assert!(is_video_badge(badge), "expected video badge for {badge}");
        }
        for badge in ["song", "Song", "album", "artist", "playlist", "podcast", ""] {
            assert!(!is_video_badge(badge), "expected non-video badge for {badge}");
        }
    }

    #[test]
    fn parse_search_detects_video_badge_and_video_shelf() {
        assert!(
            is_video_shelf("Videos") && is_video_shelf("V\u{ed}deos") && !is_video_shelf("Songs")
        );
        let s_item = test_responsive_item("Shelf Video", &["Artist"], test_watch("v-shelf-1"));
        let b_item = test_responsive_item("Badge Video", &["Music video"], test_watch("v-badge-1"));
        let shelf = serde_json::json!({"musicShelfRenderer": {"title": {"runs": [{"text": "Videos"}]}, "contents": [s_item]}});
        let bare = serde_json::json!({"itemSectionRenderer": {"contents": [b_item]}});
        let hits = parse_search_all(&tabbed_sections(vec![shelf, bare]));
        assert_eq!(hits.tracks.len(), 2);
        assert_eq!(hits.tracks[0].video_id, "v-shelf-1");
        assert_eq!(hits.tracks[0].video_type, Some(crate::domain::MusicVideoType::Omv));
        assert_eq!(hits.tracks[1].video_id, "v-badge-1");
        assert_eq!(hits.tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));
    }

    #[test]
    fn parse_search_albums_keeps_artist_id() {
        let renderer = serde_json::json!({
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Un Verano Sin Ti" }
                ] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Album" },
                    { "text": " • " },
                    { "text": "Bad Bunny", "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_bunny",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                            }
                        }
                    } } },
                    { "text": " • " },
                    { "text": "2022" }
                ] } } }
            ],
            "navigationEndpoint": { "browseEndpoint": {
                "browseId": "MPREb_un_verano",
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": {
                        "pageType": "MUSIC_PAGE_TYPE_ALBUM"
                    }
                }
            } }
        });

        let mut hits = SearchHits::default();
        dispatch_bare(&renderer, &mut hits);
        assert_eq!(hits.albums.len(), 1);
        assert_eq!(hits.albums[0].browse_id, "MPREb_un_verano");
        assert_eq!(hits.albums[0].artist, "Bad Bunny");
        assert_eq!(hits.albums[0].artist_id.as_deref(), Some("UC_bunny"));
        assert_eq!(hits.albums[0].release_type.as_deref(), Some("Album"));
    }

    #[test]
    fn parse_search_drops_non_music_audio_hits() {
        let renderer = serde_json::json!({
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Vini - Aria" }
                ] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Single" },
                    { "text": " \u{2022} " },
                    { "text": "Dj Vini", "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_dj_vini",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                            }
                        }
                    } } }
                ] } } }
            ],
            "navigationEndpoint": { "browseEndpoint": {
                "browseId": "MPED_audio_1",
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": {
                        "pageType": "MUSIC_PAGE_TYPE_NON_MUSIC_AUDIO_TRACK_PAGE"
                    }
                }
            } }
        });

        let mut hits = SearchHits::default();
        dispatch_bare(&renderer, &mut hits);
        assert!(hits.albums.is_empty());
        assert!(hits.tracks.is_empty());
    }
}
