//! Artist-page section projection into Slint card rows.

use std::collections::HashMap;

use slint::{ModelRc, VecModel};

use crate::domain::{ArtistPage, DetailCard, DetailCardKind};
use crate::i18n::tr;
use crate::slint_shell::covers::resolve_row_cover_image;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{DetailCardData, DetailCardRow, DetailSectionData, MoggytWindow};

fn card_data(
    index: i32,
    card: &DetailCard,
    covers: &HashMap<String, slint::Image>,
) -> DetailCardData {
    DetailCardData {
        index,
        title: card.title.clone().into(),
        subtitle: card.subtitle.clone().into(),
        has_cover: card.artwork.is_some(),
        cover: resolve_row_cover_image(card.artwork.as_deref(), covers),
        circular: card.kind == DetailCardKind::Artist,
    }
}

fn spacer() -> DetailCardData {
    DetailCardData {
        index: -1,
        title: "".into(),
        subtitle: "".into(),
        has_cover: false,
        cover: slint::Image::default(),
        circular: false,
    }
}

fn row_with(cards: Vec<DetailCardData>) -> DetailCardRow {
    DetailCardRow { cards: ModelRc::new(VecModel::from(cards)) }
}

fn padded_row(mut cards: Vec<DetailCardData>, columns: usize) -> DetailCardRow {
    let padding = columns.saturating_sub(cards.len());
    cards.extend(std::iter::repeat_with(spacer).take(padding));
    row_with(cards)
}

/// Splits cards into fixed-width rows, padding the last row with invisible spacers.
fn chunk_cards(cards: Vec<DetailCardData>, columns: usize) -> Vec<DetailCardRow> {
    let cols = columns.max(1);
    let mut rows = Vec::new();
    let mut current = Vec::with_capacity(cols);
    for card in cards {
        current.push(card);
        if current.len() == cols {
            rows.push(row_with(std::mem::replace(&mut current, Vec::with_capacity(cols))));
        }
    }
    if !current.is_empty() {
        rows.push(padded_row(current, cols));
    }
    rows
}

/// Projects artist-page sections into card rows sized for the current window.
pub(crate) fn build_section_rows(
    page: &ArtistPage,
    covers: &HashMap<String, slint::Image>,
    columns: usize,
) -> Vec<DetailSectionData> {
    let mut flat_index = 0i32;
    page.sections
        .iter()
        .map(|section| {
            let cards: Vec<DetailCardData> = section
                .cards
                .iter()
                .map(|card| {
                    let card = card_data(flat_index, card, covers);
                    flat_index += 1;
                    card
                })
                .collect();
            DetailSectionData {
                title: section.title.clone().into(),
                rows: ModelRc::new(VecModel::from(chunk_cards(cards, columns))),
            }
        })
        .collect()
}

fn reset_extras(window: &MoggytWindow) {
    window.set_detail_is_artist(false);
    window.set_detail_songs_title("".into());
    window.set_detail_has_songs_more(false);
    window.set_detail_sections(ModelRc::new(VecModel::from(Vec::new())));
}

/// Synchronizes artist-page extras; resets them for album and playlist details.
pub(crate) fn sync_detail_extras(window: &MoggytWindow, state: &SlintAppState) {
    let Some(detail) = state.detail.as_ref() else {
        return reset_extras(window);
    };
    let Some(page) = detail.artist.as_ref() else {
        return reset_extras(window);
    };
    let songs_title = if page.songs_title.trim().is_empty() {
        tr(state.locale, "Popular songs")
    } else {
        page.songs_title.as_str()
    };
    window.set_detail_is_artist(true);
    window.set_detail_songs_title(songs_title.into());
    window.set_detail_has_songs_more(page.songs_more_id.is_some());
    window.set_detail_sections(ModelRc::new(VecModel::from(build_section_rows(
        page,
        &state.search_covers,
        state.explore_columns,
    ))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtistPage, DetailCard, DetailCardKind, DetailSection};
    use slint::Model;

    fn card(id: &str, kind: DetailCardKind) -> DetailCard {
        DetailCard::new(id, id, "2026", Some(format!("https://img.local/{id}.jpg")), kind)
    }

    fn page() -> ArtistPage {
        ArtistPage {
            songs_title: "Popular songs".to_owned(),
            songs_more_id: Some("VL1".to_owned()),
            sections: vec![
                DetailSection {
                    title: "Albums".to_owned(),
                    cards: vec![
                        card("a1", DetailCardKind::Album),
                        card("a2", DetailCardKind::Album),
                    ],
                },
                DetailSection {
                    title: "Fans".to_owned(),
                    cards: vec![card("ar1", DetailCardKind::Artist)],
                },
            ],
        }
    }

    #[test]
    fn build_section_rows_assigns_flat_indices_and_wraps() {
        let covers = HashMap::new();
        let sections = build_section_rows(&page(), &covers, 2);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title.as_str(), "Albums");
        assert_eq!(sections[0].rows.row_count(), 1);
        let row = sections[0].rows.row_data(0).expect("row");
        assert_eq!(row.cards.row_count(), 2);
        assert_eq!(row.cards.row_data(0).expect("card").index, 0);
        assert!(!row.cards.row_data(0).expect("card").circular);
        assert_eq!(row.cards.row_data(1).expect("card").index, 1);

        assert_eq!(sections[1].rows.row_count(), 1);
        let row = sections[1].rows.row_data(0).expect("row");
        assert_eq!(row.cards.row_count(), 2);
        assert_eq!(row.cards.row_data(0).expect("card").index, 2);
        assert!(row.cards.row_data(0).expect("card").circular);
        assert_eq!(row.cards.row_data(1).expect("spacer").index, -1);
    }

    #[test]
    fn build_section_rows_preserves_subtitles_and_cover_flags() {
        let covers = HashMap::new();
        let sections = build_section_rows(&page(), &covers, 4);
        let row = sections[0].rows.row_data(0).expect("row");
        let first = row.cards.row_data(0).expect("card");
        assert_eq!(first.title.as_str(), "a1");
        assert_eq!(first.subtitle.as_str(), "2026");
        assert!(first.has_cover);
    }
}
