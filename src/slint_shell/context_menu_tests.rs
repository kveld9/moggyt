#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::domain::{
        Album, Artist, ArtistPage, Detail, DetailCard, DetailCardKind, DetailKind, DetailSection,
        Playlist, SearchResult, Shelf, Track,
    };
    use crate::i18n::Locale;
    use crate::slint_shell::context_menu::{
        ContextAction, ContextSubject, ContextTarget, FollowUp, apply_action, menu_items,
        resolve_subject,
    };
    use crate::slint_shell::page::SlintPage;
    use crate::slint_shell::test_support::test_wired;
    use crate::slint_shell::{ContextIcon, ContextSurface};

    #[test]
    fn menu_items_follow_track_action_order() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let items = menu_items(Locale::English, &subject, false);
        assert_eq!(items.len(), 7);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].label, "Play");
        assert_eq!(items[0].icon, ContextIcon::Play);
        assert_eq!(items[1].index, 1);
        assert_eq!(items[1].label, "Play next");
        assert_eq!(items[1].icon, ContextIcon::Next);
        assert_eq!(items[2].index, 2);
        assert_eq!(items[2].label, "Add to queue");
        assert_eq!(items[2].icon, ContextIcon::Queue);
        assert_eq!(items[3].index, 3);
        assert_eq!(items[3].label, "Go to artist");
        assert_eq!(items[3].icon, ContextIcon::Artist);
        assert_eq!(items[4].index, 4);
        assert_eq!(items[4].label, "Go to album");
        assert_eq!(items[4].icon, ContextIcon::Album);
        assert_eq!(items[5].index, 5);
        assert_eq!(items[5].label, "Like");
        assert_eq!(items[5].icon, ContextIcon::Heart);
        assert_eq!(items[6].index, 6);
        assert_eq!(items[6].label, "Copy link");
        assert_eq!(items[6].icon, ContextIcon::Copy);
    }

    #[test]
    fn menu_items_are_localized() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let items = menu_items(Locale::Spanish, &subject, false);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Reproducir",
                "Reproducir a continuación",
                "Añadir a la cola",
                "Ir al artista",
                "Ir al álbum",
                "Me gusta",
                "Copiar enlace",
            ]
        );
    }

    #[test]
    fn menu_items_switch_like_label_when_liked() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let en_items = menu_items(Locale::English, &subject, true);
        assert_eq!(en_items.get(5).map(|item| item.label.as_str()), Some("Unlike"));
        assert_eq!(en_items.get(5).map(|item| item.icon), Some(ContextIcon::HeartFilled));

        let unliked_items = menu_items(Locale::English, &subject, false);
        assert_eq!(unliked_items.get(5).map(|item| item.icon), Some(ContextIcon::Heart));

        let es_items = menu_items(Locale::Spanish, &subject, true);
        assert_eq!(es_items.get(5).map(|item| item.label.as_str()), Some("Quitar me gusta"));
    }

    #[test]
    fn action_index_mapping_rejects_out_of_range() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        assert_eq!(subject.action_at(0), Some(ContextAction::PlayNow));
        assert_eq!(subject.action_at(1), Some(ContextAction::PlayNext));
        assert_eq!(subject.action_at(2), Some(ContextAction::AddToQueue));
        assert_eq!(subject.action_at(3), Some(ContextAction::GoToArtist));
        assert_eq!(subject.action_at(4), Some(ContextAction::GoToAlbum));
        assert_eq!(subject.action_at(5), Some(ContextAction::ToggleLike));
        assert_eq!(subject.action_at(6), Some(ContextAction::CopyLink));
        assert_eq!(subject.action_at(-1), None);
        assert_eq!(subject.action_at(7), None);
    }

    #[test]
    fn container_menu_offers_only_open() {
        let container = ContextSubject::Container { id: "a1".to_owned(), kind: DetailKind::Album };
        let en_items = menu_items(Locale::English, &container, false);
        assert_eq!(en_items.len(), 1);
        assert_eq!(en_items[0].index, 0);
        assert_eq!(en_items[0].label, "Open");
        assert_eq!(en_items[0].icon, ContextIcon::Open);

        let es_items = menu_items(Locale::Spanish, &container, false);
        assert_eq!(es_items.len(), 1);
        assert_eq!(es_items[0].index, 0);
        assert_eq!(es_items[0].label, "Abrir");
    }

    #[test]
    fn resolve_subject_reads_search_items() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        let album = Album::new("a1", "Album 1", "Artist 1", None);
        let mut shelf = Shelf::new("Results");
        shelf.items.push(SearchResult::Track(track.clone()));
        shelf.items.push(SearchResult::Album(album));
        state.home_shelves = vec![shelf];

        let target_track = ContextTarget { surface: ContextSurface::Search, index: 0 };
        assert_eq!(resolve_subject(&state, target_track), Some(ContextSubject::Track(track)));

        let target_album = ContextTarget { surface: ContextSurface::Search, index: 1 };
        assert_eq!(
            resolve_subject(&state, target_album),
            Some(ContextSubject::Container { id: "a1".to_owned(), kind: DetailKind::Album })
        );
    }

    #[test]
    fn resolve_subject_reads_detail_tracks() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        state.detail = Some(Detail::new(DetailKind::Album, "album", "artist", vec![track.clone()]));

        let target = ContextTarget { surface: ContextSurface::Detail, index: 0 };
        assert_eq!(resolve_subject(&state, target), Some(ContextSubject::Track(track)));
    }

    #[test]
    fn resolve_subject_reads_explore_and_category_shelves() {
        let mut state = test_wired();
        let artist = Artist::new("art1", "Artist 1", None);
        let playlist = Playlist::new("pl1", "Playlist 1", 10, None);

        let mut explore_shelf = Shelf::new("Explore");
        explore_shelf.items.push(SearchResult::Artist(artist));
        state.explore_shelves = vec![explore_shelf];

        let mut category_shelf = Shelf::new("Category");
        category_shelf.items.push(SearchResult::Playlist(playlist));
        state.category_shelves = vec![category_shelf];

        let target_explore = ContextTarget { surface: ContextSurface::Explore, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_explore),
            Some(ContextSubject::Container { id: "art1".to_owned(), kind: DetailKind::Artist })
        );

        let target_category = ContextTarget { surface: ContextSurface::Category, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_category),
            Some(ContextSubject::Container { id: "pl1".to_owned(), kind: DetailKind::Playlist })
        );
    }

    #[test]
    fn resolve_subject_reads_library_items() {
        let mut state = test_wired();
        state.library_playlists = vec![Playlist::new("pl1", "Playlist 1", 5, None)];
        state.library_albums = vec![Album::new("al1", "Album 1", "Artist 1", None)];

        let target_playlist = ContextTarget { surface: ContextSurface::LibraryPlaylist, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_playlist),
            Some(ContextSubject::Container { id: "pl1".to_owned(), kind: DetailKind::Playlist })
        );

        let target_album = ContextTarget { surface: ContextSurface::LibraryAlbum, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_album),
            Some(ContextSubject::Container { id: "al1".to_owned(), kind: DetailKind::Album })
        );
    }

    #[test]
    fn resolve_subject_reads_detail_section_cards() {
        let mut state = test_wired();
        let card_album =
            DetailCard::new("c-album", "Album Title", "Sub", None, DetailCardKind::Album);
        let card_artist =
            DetailCard::new("c-artist", "Artist Title", "Sub", None, DetailCardKind::Artist);

        let section_1 = DetailSection { title: "Albums".to_owned(), cards: vec![card_album] };
        let section_2 =
            DetailSection { title: "Related Artists".to_owned(), cards: vec![card_artist] };

        let artist_page = ArtistPage {
            songs_title: "Top Songs".to_owned(),
            songs_more_id: None,
            sections: vec![section_1, section_2],
        };

        let mut detail = Detail::new(DetailKind::Artist, "Artist Name", "Description", vec![]);
        detail = detail.with_artist_page(artist_page);
        state.detail = Some(detail);

        let target_0 = ContextTarget { surface: ContextSurface::DetailCard, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_0),
            Some(ContextSubject::Container { id: "c-album".to_owned(), kind: DetailKind::Album })
        );

        let target_1 = ContextTarget { surface: ContextSurface::DetailCard, index: 1 };
        assert_eq!(
            resolve_subject(&state, target_1),
            Some(ContextSubject::Container { id: "c-artist".to_owned(), kind: DetailKind::Artist })
        );

        let target_2 = ContextTarget { surface: ContextSurface::DetailCard, index: 2 };
        assert_eq!(resolve_subject(&state, target_2), None);
    }

    #[test]
    fn apply_action_play_next_inserts_after_cursor() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));
        state.queue.push(Track::new("b", "Title B", "Artist B"));
        state.queue.push(Track::new("c", "Title C", "Artist C"));
        state.queue.advance();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("b"));

        let track_x = Track::new("x", "Title X", "Artist X");
        let target = ContextTarget { surface: ContextSurface::Queue, index: 1 };
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNext, track_x);

        assert_eq!(follow_up, FollowUp::None);
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "x", "c"]);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("b"));
    }

    #[test]
    fn apply_action_add_to_queue_appends() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));

        let track_x = Track::new("x", "Title X", "Artist X");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::AddToQueue, track_x);

        assert_eq!(follow_up, FollowUp::None);
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "x"]);
    }

    #[test]
    fn apply_action_play_now_on_queue_surface_moves_cursor() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));
        state.queue.push(Track::new("b", "Title B", "Artist B"));
        assert_eq!(state.queue.cursor(), 0);

        let target = ContextTarget { surface: ContextSurface::Queue, index: 1 };
        let track_b = state.queue.tracks()[1].clone();
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNow, track_b);

        assert_eq!(follow_up, FollowUp::StartPlayback);
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.queue.len(), 2);
    }

    #[test]
    fn apply_action_play_now_on_detail_pushes_and_opens_now_playing() {
        let mut state = test_wired();
        assert_eq!(state.queue.len(), 0);

        let track = Track::new("t1", "Title 1", "Artist 1");
        let target = ContextTarget { surface: ContextSurface::Detail, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNow, track);

        assert_eq!(follow_up, FollowUp::StartPlayback);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.page, SlintPage::NowPlaying);
    }

    #[test]
    fn apply_action_toggle_like_updates_set_and_settings() {
        let mut state = test_wired();
        let track = Track::new("t1", "Title 1", "Artist 1");
        let target = ContextTarget { surface: ContextSurface::Queue, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::ToggleLike, track);

        assert_eq!(follow_up, FollowUp::None);
        assert!(state.liked_ids.contains("t1"));
        assert_eq!(state.settings_store.load().liked_ids, vec!["t1"]);
    }

    #[test]
    fn apply_action_copy_link_returns_the_watch_url() {
        let mut state = test_wired();
        let track = Track::new("abc-123_XYZ", "Title", "Artist");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::CopyLink, track);

        assert_eq!(
            follow_up,
            FollowUp::CopyLink("https://music.youtube.com/watch?v=abc-123_XYZ".to_owned())
        );
        assert_eq!(state.queue.len(), 0);
    }

    #[test]
    fn apply_action_copy_link_skips_invalid_identifiers() {
        let mut state = test_wired();
        let track = Track::new("bad id", "Title", "Artist");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::CopyLink, track);

        assert_eq!(follow_up, FollowUp::None);
    }

    #[test]
    fn track_without_artist_omits_navigation_actions() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", " "));
        let items = menu_items(Locale::English, &subject, false);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Play", "Play next", "Add to queue", "Like", "Copy link"]);
    }

    #[test]
    fn apply_action_targets_artist_and_album_queries() {
        let mut state = test_wired();
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let with_album = Track::new("t1", "Title", "Artist").with_album("Record");
        assert_eq!(
            apply_action(&mut state, target, ContextAction::GoToArtist, with_album.clone()),
            FollowUp::GoToArtist("Artist".to_owned())
        );
        assert_eq!(
            apply_action(&mut state, target, ContextAction::GoToAlbum, with_album),
            FollowUp::GoToAlbum("Artist Record".to_owned())
        );

        let without_album = Track::new("t2", "Title", "Artist");
        assert_eq!(
            apply_action(&mut state, target, ContextAction::GoToAlbum, without_album),
            FollowUp::GoToAlbum("Artist Title".to_owned())
        );
    }
}
