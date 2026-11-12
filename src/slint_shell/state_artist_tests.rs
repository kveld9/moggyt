#[cfg(test)]
mod tests {
    use crate::domain::{
        ArtistPage, Detail, DetailCard, DetailCardKind, DetailKind, DetailSection,
    };
    use crate::slint_shell::test_support::{MockLibraryCatalog, test_wired, wrap_mock_catalog};
    use crate::slint_shell::view_phase::ViewPhase;

    #[test]
    fn artist_detail_with_only_sections_is_ready() {
        let mut state = test_wired();
        let detail = Detail::new(DetailKind::Artist, "Artist", "", Vec::new()).with_artist_page(
            ArtistPage {
                songs_title: "Popular songs".to_owned(),
                songs_more_id: None,
                sections: vec![DetailSection {
                    title: "Albums".to_owned(),
                    cards: vec![DetailCard::new(
                        "MPREb_1",
                        "Album",
                        "2026",
                        None,
                        DetailCardKind::Album,
                    )],
                }],
            },
        );
        state.catalog =
            wrap_mock_catalog(MockLibraryCatalog::default().with_detail(Ok(Some(detail))));
        state.open_detail("UC_artist", DetailKind::Artist);

        assert_eq!(state.detail_phase, ViewPhase::Ready);
        assert!(state.detail.and_then(|d| d.artist).is_some());
    }
}
