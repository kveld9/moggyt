use super::*;
use slint::Model;

use crate::domain::{Album, Artist, Playlist, SearchResult, Track};
use crate::infra::CatalogStub;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::thumbnail_variant_for_row;
use crate::slint_shell::search::SearchRow;
use crate::slint_shell::sync::DEFAULT_EXPLORE_COLUMNS;
use crate::slint_shell::test_support::test_wired;

fn make_test_card(index: i32) -> ExploreCardData {
    ExploreCardData {
        index,
        title: format!("Title {index}").into(),
        subtitle: "".into(),
        has_cover: false,
        cover: slint::Image::default(),
        tile_color: slint::Color::default(),
    }
}

#[test]
fn test_explore_state_initialization_and_selection() {
    let mut state = test_wired();
    assert!(state.explore_shelves.is_empty());
    assert!(!state.explore_loaded);
    state.select_page(SlintPage::Explore);
    assert_eq!(state.page, SlintPage::Explore);
    assert!(state.explore_loaded);
    assert_eq!(state.explore_shelves.len(), 2);
    assert_eq!(state.explore_shelves[0].title, "Charts");
    assert_eq!(state.explore_shelves[1].title, "Moods & genres");
}

#[test]
fn test_execute_explore_uses_catalog_port_and_generation() {
    let cat = Arc::new(Mutex::new(CatalogStub::new()));
    let (generation, shelves) = execute_explore(42, &cat);
    assert_eq!(generation, 42);
    let shelves = shelves.expect("explore ok");
    assert_eq!(shelves.len(), 2);
    assert_eq!(shelves[0].title, "Charts");
    assert_eq!(shelves[1].title, "Moods & genres");
}

#[test]
fn test_execute_category_uses_catalog_port() {
    let cat = Arc::new(Mutex::new(CatalogStub::new()));
    let (g, s) = execute_category(99, &cat, "FEmusic_moods_and_genres_category", Some("calma"));
    assert_eq!(g, 99);
    let s = s.expect("category ok");
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].title, "Chill Playlists");
}

#[test]
fn explore_transport_error_sets_error_phase() {
    let mut state = test_wired();
    state.explore_loading = true;
    state.explore_shelves = vec![Shelf { title: "Previous Shelf".to_string(), items: Vec::new() }];

    apply_explore_outcome(&mut state, Err(CatalogError::Transport));

    assert!(!state.explore_loading);
    assert_eq!(state.explore_phase, ViewPhase::Error);
    assert_eq!(state.explore_shelves.len(), 1);
    assert_eq!(state.explore_shelves[0].title, "Previous Shelf");
}

#[test]
fn category_transport_error_sets_error_phase() {
    let mut state = test_wired();
    state.category_loading = true;
    state.category_shelves =
        vec![Shelf { title: "Previous Category".to_string(), items: Vec::new() }];

    apply_category_outcome(&mut state, Err(CatalogError::Transport));

    assert!(!state.category_loading);
    assert_eq!(state.category_phase, ViewPhase::Error);
    assert_eq!(state.category_shelves.len(), 1);
    assert_eq!(state.category_shelves[0].title, "Previous Category");
}

#[test]
fn test_explore_item_at_indexing() {
    let shelves = vec![
        Shelf {
            title: "Charts".to_owned(),
            items: vec![
                SearchResult::Track(Track::new("c1", "Track 1", "Artist 1")),
                SearchResult::Track(Track::new("c2", "Track 2", "Artist 2")),
            ],
        },
        Shelf {
            title: "New Releases".to_owned(),
            items: vec![SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None))],
        },
    ];
    assert_eq!(
        explore_item_at(&shelves, 0),
        Some(&SearchResult::Track(Track::new("c1", "Track 1", "Artist 1")))
    );
    assert_eq!(
        explore_item_at(&shelves, 1),
        Some(&SearchResult::Track(Track::new("c2", "Track 2", "Artist 2")))
    );
    assert_eq!(
        explore_item_at(&shelves, 2),
        Some(&SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)))
    );
    assert_eq!(explore_item_at(&shelves, 3), None);
}

#[test]
fn test_refresh_explore_covers_if_loaded_behavior() {
    let mut state = test_wired();
    let (tx, rx) = std::sync::mpsc::channel();

    // 1. Returns false and dispatches nothing when explore_loaded is false.
    assert!(!state.explore_loaded);
    assert!(!refresh_explore_covers_if_loaded(&state, &tx));
    assert!(rx.try_recv().is_err());

    // 2. Returns true and dispatches exactly two FetchCover commands when loaded with 2 artwork items.
    let url1 = "https://lh3.googleusercontent.com/track-large";
    let url2 = "https://lh3.googleusercontent.com/album-large";
    let item1 = SearchResult::Track(Track::new("t1", "Track 1", "Artist 1").with_artwork(url1));
    let item2 =
        SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", Some(url2.to_string())));
    let expected_url1 =
        thumbnail_variant_for_row(SearchRow::from(&item1).artwork_url.as_deref().unwrap());
    let expected_url2 =
        thumbnail_variant_for_row(SearchRow::from(&item2).artwork_url.as_deref().unwrap());

    state.explore_loaded = true;
    state.explore_generation = 42;
    state.explore_shelves = vec![
        Shelf { title: "Shelf 1".to_string(), items: vec![item1] },
        Shelf { title: "Shelf 2".to_string(), items: vec![item2] },
    ];

    assert!(refresh_explore_covers_if_loaded(&state, &tx));

    let mut received = Vec::new();
    while let Ok(cmd) = rx.try_recv() {
        if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
            received.push((url, generation));
        }
    }
    assert_eq!(received.len(), 2);
    assert_eq!(received[0], (expected_url1.clone(), 42));
    assert_eq!(received[1], (expected_url2.clone(), 42));

    // 3. Returns true but dispatches no command when both thumbnails are already in search_covers.
    state.search_covers.insert(expected_url1, slint::Image::default());
    state.search_covers.insert(expected_url2, slint::Image::default());

    assert!(refresh_explore_covers_if_loaded(&state, &tx));
    assert!(rx.try_recv().is_err());
}

#[test]
fn test_explore_columns_for_width() {
    // 1924px window minus the 200px sidebar.
    assert_eq!(explore_columns_for_width(1724), 10);
    // Same window with the 340px queue panel open: the grid loses two columns.
    assert_eq!(explore_columns_for_width(1384), 8);
    assert_eq!(explore_columns_for_width(740), 4);
    assert_eq!(explore_columns_for_width(600), 3);
    // Minimum window width (800px) with the queue panel open: one column, no clipping.
    assert_eq!(explore_columns_for_width(260), 1);
    assert_eq!(explore_columns_for_width(0), 1);
    assert_eq!(explore_columns_for_width(3800), 12);
}

#[test]
fn test_explore_columns_never_overflow_the_content_width() {
    for content_width in (200..=4000).step_by(17) {
        let columns = explore_columns_for_width(content_width) as i32;
        let row_width = columns * EXPLORE_CARD_WIDTH + (columns - 1) * EXPLORE_CARD_GAP;
        let available = (content_width - EXPLORE_CONTENT_PADDING).max(EXPLORE_CARD_WIDTH);
        assert!(
            row_width <= available,
            "{columns} columns ({row_width}px) overflow {available}px available at content \
             width {content_width}"
        );
    }
}

#[test]
fn test_apply_content_width_reports_threshold_crossings() {
    let mut state = test_wired();
    assert_eq!(state.explore_columns, DEFAULT_EXPLORE_COLUMNS);

    assert!(apply_content_width(&mut state, 1724));
    assert_eq!(state.explore_columns, 10);

    // Another width inside the same 10-column band must not re-chunk the models.
    assert!(!apply_content_width(&mut state, 1730));

    // Opening the queue panel crosses into the 8-column band.
    assert!(apply_content_width(&mut state, 1384));
    assert_eq!(state.explore_columns, 8);
}

#[test]
fn test_chunk_card_rows_custom_columns() {
    // 0 cards -> no rows
    let rows_0 = chunk_card_rows(Vec::new(), 4);
    assert_eq!(rows_0.len(), 0);

    // 3 cards at 4 columns -> one row [0, 1, 2, -1]
    let cards_3 = vec![make_test_card(0), make_test_card(1), make_test_card(2)];
    let rows_1 = chunk_card_rows(cards_3, 4);
    assert_eq!(rows_1.len(), 1);
    assert_eq!(rows_1[0].cards.row_count(), 4);
    let indices_1: Vec<i32> = (0..4).map(|i| rows_1[0].cards.row_data(i).unwrap().index).collect();
    assert_eq!(indices_1, vec![0, 1, 2, -1]);

    // 7 cards at 4 columns -> two rows, second row padded to 4
    let cards_7: Vec<_> = (0..7).map(make_test_card).collect();
    let rows_2 = chunk_card_rows(cards_7, 4);
    assert_eq!(rows_2.len(), 2);
    assert_eq!(rows_2[0].cards.row_count(), 4);
    assert_eq!(rows_2[1].cards.row_count(), 4);
    let row0_indices: Vec<i32> =
        (0..4).map(|i| rows_2[0].cards.row_data(i).unwrap().index).collect();
    let row1_indices: Vec<i32> =
        (0..4).map(|i| rows_2[1].cards.row_data(i).unwrap().index).collect();
    assert_eq!(row0_indices, vec![0, 1, 2, 3]);
    assert_eq!(row1_indices, vec![4, 5, 6, -1]);
}

#[test]
fn test_chunk_card_rows_padding_and_empty() {
    // 0 cards -> 0 rows
    let rows_0 = chunk_card_rows(Vec::new(), 5);
    assert_eq!(rows_0.len(), 0);

    // 3 cards -> 1 row of 5 with indexes [0, 1, 2, -1, -1]
    let cards_3 = vec![make_test_card(0), make_test_card(1), make_test_card(2)];
    let rows_1 = chunk_card_rows(cards_3, 5);
    assert_eq!(rows_1.len(), 1);
    assert_eq!(rows_1[0].cards.row_count(), 5);
    let indices_1: Vec<i32> = (0..5).map(|i| rows_1[0].cards.row_data(i).unwrap().index).collect();
    assert_eq!(indices_1, vec![0, 1, 2, -1, -1]);

    // 7 cards -> 2 rows of 5 with the second row padded
    let cards_7: Vec<_> = (0..7).map(make_test_card).collect();
    let rows_2 = chunk_card_rows(cards_7, 5);
    assert_eq!(rows_2.len(), 2);
    assert_eq!(rows_2[0].cards.row_count(), 5);
    assert_eq!(rows_2[1].cards.row_count(), 5);
    let row0_indices: Vec<i32> =
        (0..5).map(|i| rows_2[0].cards.row_data(i).unwrap().index).collect();
    let row1_indices: Vec<i32> =
        (0..5).map(|i| rows_2[1].cards.row_data(i).unwrap().index).collect();
    assert_eq!(row0_indices, vec![0, 1, 2, 3, 4]);
    assert_eq!(row1_indices, vec![5, 6, -1, -1, -1]);
}

#[test]
fn test_explore_card_shelves_keeps_global_index_across_shelves() {
    let shelves = vec![
        Shelf {
            title: "Shelf 1".to_string(),
            items: vec![
                SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                SearchResult::Track(Track::new("t2", "Track 2", "Artist 2")),
            ],
        },
        Shelf {
            title: "Shelf 2".to_string(),
            items: vec![SearchResult::Track(Track::new("t3", "Track 3", "Artist 3"))],
        },
    ];
    let covers = HashMap::new();
    let result = explore_card_shelves(&shelves, &covers, 5);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].title.as_str(), "Shelf 1");
    assert_eq!(result[1].title.as_str(), "Shelf 2");

    assert_eq!(result[0].rows.row_count(), 1);
    let row0 = result[0].rows.row_data(0).unwrap();
    assert_eq!(row0.cards.row_count(), 5);
    let row0_indices: Vec<i32> = (0..5).map(|i| row0.cards.row_data(i).unwrap().index).collect();
    assert_eq!(row0_indices, vec![0, 1, -1, -1, -1]);
    assert_eq!(row0.cards.row_data(0).unwrap().title.as_str(), "Track 1");
    assert_eq!(row0.cards.row_data(1).unwrap().title.as_str(), "Track 2");

    assert_eq!(result[1].rows.row_count(), 1);
    let row1 = result[1].rows.row_data(0).unwrap();
    assert_eq!(row1.cards.row_count(), 5);
    let row1_indices: Vec<i32> = (0..5).map(|i| row1.cards.row_data(i).unwrap().index).collect();
    assert_eq!(row1_indices, vec![2, -1, -1, -1, -1]);
    assert_eq!(row1.cards.row_data(0).unwrap().title.as_str(), "Track 3");
}

#[test]
fn test_explore_card_shelves_subtitle_mapping_through_search_row() {
    let shelves = vec![Shelf {
        title: "Diverse".to_string(),
        items: vec![
            SearchResult::Track(Track::new("t", "Title", "Artist")),
            SearchResult::Album(Album::new("a", "Album", "Artist", None)),
            SearchResult::Artist(Artist::new("UC1", "Name", None)),
            SearchResult::Playlist(Playlist::new("VL1", "Mix", 0, None)),
        ],
    }];
    let covers = HashMap::new();
    let result = explore_card_shelves(&shelves, &covers, 5);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rows.row_count(), 1);
    let row0 = result[0].rows.row_data(0).unwrap();
    let cards = &row0.cards;
    assert_eq!(cards.row_count(), 5);

    assert_eq!(cards.row_data(0).unwrap().subtitle.as_str(), "Artist");
    assert_eq!(cards.row_data(1).unwrap().subtitle.as_str(), "Artist");
    assert_eq!(cards.row_data(2).unwrap().subtitle.as_str(), "");
    assert_eq!(cards.row_data(3).unwrap().subtitle.as_str(), "");
    assert_eq!(cards.row_data(4).unwrap().index, -1);
}

#[test]
fn test_explore_card_shelves_has_cover_and_image_resolution() {
    let art_url = "https://example.test/art.jpg";
    let shelves = vec![Shelf {
        title: "Covers".to_string(),
        items: vec![
            SearchResult::Playlist(Playlist::new("VL1", "Calma", 0, None)),
            SearchResult::Album(Album::new("a", "Album", "Artist", None).with_artwork(art_url)),
        ],
    }];

    // Map is empty: has_cover false for playlist, true for album; album cover empty image
    let empty_covers = HashMap::new();
    let res_empty = explore_card_shelves(&shelves, &empty_covers, 5);
    let row_empty = res_empty[0].rows.row_data(0).unwrap();
    let pl_card = row_empty.cards.row_data(0).unwrap();
    let alb_card_empty = row_empty.cards.row_data(1).unwrap();

    assert!(!pl_card.has_cover);
    assert_eq!(pl_card.cover.size(), slint::Image::default().size());
    assert!(alb_card_empty.has_cover);
    assert_eq!(alb_card_empty.cover.size(), slint::Image::default().size());

    // Covers holds entry keyed by thumbnail_variant_for_row(url)
    let mut populated_covers = HashMap::new();
    let real_img = rgba_to_image(&[255, 0, 0, 255], 1, 1).expect("valid image");
    let thumb_url = thumbnail_variant_for_row(art_url);
    populated_covers.insert(thumb_url, real_img);

    let res_populated = explore_card_shelves(&shelves, &populated_covers, 5);
    let row_pop = res_populated[0].rows.row_data(0).unwrap();
    let alb_card_populated = row_pop.cards.row_data(1).unwrap();

    assert!(alb_card_populated.has_cover);
    assert!(alb_card_populated.cover.size().width > 0);
}

#[test]
fn test_tile_color_for_deterministic_and_within_palette() {
    let color1 = tile_color_for("Chill Vibes");
    let color2 = tile_color_for("Chill Vibes");
    assert_eq!(color1, color2);

    let titles = [
        "Rock",
        "Pop",
        "Jazz",
        "Classical",
        "Hip Hop",
        "Electronic",
        "Acoustic",
        "Metal",
        "Ambient",
        "Dance",
        "Country",
        "Blues",
        "Folk",
        "Latin",
    ];
    let palette_colors: Vec<slint::Color> =
        TILE_PALETTE.iter().map(|&[r, g, b]| slint::Color::from_rgb_u8(r, g, b)).collect();

    for title in titles {
        let color = tile_color_for(title);
        assert!(
            palette_colors.contains(&color),
            "tile_color_for({title:?}) returned color not in TILE_PALETTE"
        );
    }
}

#[test]
fn test_artless_item_keeps_has_cover_false_and_receives_tile_color() {
    let shelves = vec![Shelf {
        title: "Moods".to_string(),
        items: vec![SearchResult::Playlist(Playlist::new("PL1", "Energize", 0, None))],
    }];
    let covers = HashMap::new();
    let result = explore_card_shelves(&shelves, &covers, 5);

    assert_eq!(result.len(), 1);
    let row0 = result[0].rows.row_data(0).unwrap();
    let card0 = row0.cards.row_data(0).unwrap();

    assert_eq!(card0.title.as_str(), "Energize");
    assert!(!card0.has_cover);
    let expected_color = tile_color_for("Energize");
    assert_eq!(card0.tile_color, expected_color);

    // Verify trailing spacer cards
    let spacer = row0.cards.row_data(1).unwrap();
    assert_eq!(spacer.index, -1);
    assert!(!spacer.has_cover);
    assert_eq!(spacer.tile_color, slint::Color::default());
}
