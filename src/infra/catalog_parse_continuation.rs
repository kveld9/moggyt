//! Continuation-page parser for paginated detail track lists.

use serde_json::Value;

use super::catalog_parse_detail::{build_detail_track, detail_sections, parse_detail_all};
use crate::domain::{Detail, DetailKind, Track};

/// Returns the token that requests the next page of a detail track list, when present.
pub fn continuation_token(value: &Value) -> Option<String> {
    detail_sections(value)?.iter().find_map(shelf_continuation_token)
}

fn shelf_continuation_token(section: &Value) -> Option<String> {
    let shelf =
        section.get("musicPlaylistShelfRenderer").or_else(|| section.get("musicShelfRenderer"))?;
    shelf.get("contents")?.as_array()?.iter().rev().find_map(item_continuation_token)
}

fn item_continuation_token(item: &Value) -> Option<String> {
    let token = item
        .pointer("/continuationItemRenderer/continuationEndpoint/continuationCommand/token")?
        .as_str()?
        .trim();
    if token.is_empty() { None } else { Some(token.to_owned()) }
}

/// Collects the tracks and the next token carried by a continuation response.
pub fn continuation_page(value: &Value) -> (Vec<Track>, Option<String>) {
    let mut tracks = Vec::new();
    let mut next = None;
    for items in append_continuation_items(value) {
        for item in items {
            if let Some(token) = item_continuation_token(item) {
                next = Some(token);
                continue;
            }
            let renderer = item.get("musicResponsiveListItemRenderer").unwrap_or(item);
            if let Some(track) = build_detail_track(renderer) {
                tracks.push(track);
            }
        }
    }
    (tracks, next)
}

fn append_continuation_items(value: &Value) -> Vec<&[Value]> {
    let Some(actions) = value.get("onResponseReceivedActions").and_then(Value::as_array) else {
        return Vec::new();
    };
    actions
        .iter()
        .filter_map(|action| {
            action
                .get("appendContinuationItemsAction")
                .and_then(|append| append.get("continuationItems"))
                .and_then(Value::as_array)
                .map(Vec::as_slice)
        })
        .collect()
}

/// Parses a detail view and follows its track-list continuation pages up to `max_pages`.
///
/// `fetch` returns the raw continuation payload for a token. Returning `None` stops the
/// loop and keeps the pages collected so far, so a failed page degrades to a shorter list.
pub fn parse_paginated_detail<F>(
    payload: &Value,
    kind: DetailKind,
    max_pages: usize,
    mut fetch: F,
) -> Option<Detail>
where
    F: FnMut(&str) -> Option<Value>,
{
    let mut detail = parse_detail_all(payload, kind)?;
    let mut pages = 1;
    let mut token = continuation_token(payload);
    while pages < max_pages {
        let Some(current) = token.take() else {
            break;
        };
        let Some(page) = fetch(&current) else {
            break;
        };
        let (tracks, next) = continuation_page(&page);
        if tracks.is_empty() {
            break;
        }
        detail.tracks.extend(tracks);
        pages += 1;
        token = next;
    }
    Some(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song_item(video_id: &str, title: &str) -> Value {
        json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": title }] }
                    }},
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Artist" }] }
                    }}
                ],
                "playlistItemData": { "videoId": video_id }
            }
        })
    }

    fn continuation_item(token: &str) -> Value {
        json!({
            "continuationItemRenderer": {
                "continuationEndpoint": {
                    "continuationCommand": { "token": token }
                }
            }
        })
    }

    fn first_page(items: Vec<Value>) -> Value {
        json!({
            "header": { "musicResponsiveHeaderRenderer": {
                "title": { "runs": [{ "text": "Top songs" }] }
            }},
            "contents": { "twoColumnBrowseResultsRenderer": { "secondaryContents": {
                "sectionListRenderer": { "contents": [{
                    "musicPlaylistShelfRenderer": { "contents": items }
                }]}
            }}}
        })
    }

    fn continuation_page_payload(items: Vec<Value>) -> Value {
        json!({
            "onResponseReceivedActions": [{
                "appendContinuationItemsAction": { "continuationItems": items }
            }]
        })
    }

    #[test]
    fn continuation_token_reads_the_track_list_item() {
        let payload = first_page(vec![song_item("v1", "One"), continuation_item("PAGE2")]);
        assert_eq!(continuation_token(&payload).as_deref(), Some("PAGE2"));
        assert_eq!(continuation_token(&first_page(vec![song_item("v1", "One")])), None);
    }

    #[test]
    fn continuation_page_collects_tracks_and_the_next_token() {
        let payload = continuation_page_payload(vec![
            song_item("v2", "Two"),
            song_item("v3", "Three"),
            continuation_item("PAGE3"),
        ]);

        let (tracks, next) = continuation_page(&payload);

        let ids: Vec<&str> = tracks.iter().map(|track| track.id.as_str()).collect();
        assert_eq!(ids, vec!["v2", "v3"]);
        assert_eq!(next.as_deref(), Some("PAGE3"));
    }

    #[test]
    fn parse_paginated_detail_follows_pages_within_the_limit() {
        let first = first_page(vec![song_item("v1", "One"), continuation_item("PAGE2")]);
        let mut fetched = Vec::new();

        let detail = parse_paginated_detail(&first, DetailKind::Playlist, 2, |token| {
            fetched.push(token.to_owned());
            Some(continuation_page_payload(vec![song_item("v2", "Two")]))
        })
        .expect("detail");

        let ids: Vec<&str> = detail.tracks.iter().map(|track| track.id.as_str()).collect();
        assert_eq!(ids, vec!["v1", "v2"]);
        assert_eq!(fetched, vec!["PAGE2"]);
    }

    #[test]
    fn parse_paginated_detail_keeps_collected_pages_when_a_fetch_fails() {
        let first = first_page(vec![song_item("v1", "One"), continuation_item("PAGE2")]);

        let detail =
            parse_paginated_detail(&first, DetailKind::Playlist, 3, |_| None).expect("detail");

        let ids: Vec<&str> = detail.tracks.iter().map(|track| track.id.as_str()).collect();
        assert_eq!(ids, vec!["v1"]);
    }

    #[test]
    fn parse_paginated_detail_stops_at_the_page_cap() {
        let first = first_page(vec![song_item("v1", "One"), continuation_item("PAGE2")]);
        let mut calls = 0;

        let detail = parse_paginated_detail(&first, DetailKind::Playlist, 1, |_| {
            calls += 1;
            Some(continuation_page_payload(vec![song_item("v2", "Two")]))
        })
        .expect("detail");

        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(calls, 0);
    }
}
