//! Shared Spotify playlist and album link ingestion dispatcher.
//!
//! Evaluates incoming Spotify web links and URIs, choosing between the official
//! Spotify Web API (Tier 3 client credentials) and embed page scraping based on
//! whether complete client credentials are available.

use crate::domain::{SpotifyPlaylist, SpotifyRef, parse_spotify_ref};
use crate::infra::spotify_api::SpotifyApiClient;
use crate::infra::spotify_credentials::SpotifyCredentials;
use crate::infra::spotify_embed::SpotifyEmbedClient;

trait SpotifyFetcher {
    fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist>;
}

impl SpotifyFetcher for SpotifyApiClient {
    fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist> {
        self.fetch(source)
    }
}

impl SpotifyFetcher for SpotifyEmbedClient {
    fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist> {
        self.fetch(source)
    }
}

fn fetch_selected(
    source: &SpotifyRef,
    credentials: Option<&SpotifyCredentials>,
    api: &dyn SpotifyFetcher,
    embed: &dyn SpotifyFetcher,
) -> anyhow::Result<SpotifyPlaylist> {
    if credentials.is_some_and(SpotifyCredentials::is_complete) {
        api.fetch(source)
    } else {
        embed.fetch(source)
    }
}

/// Loads a Spotify playlist or album from a link or URI using available credentials.
///
/// If complete credentials exist, fetches via [`SpotifyApiClient`].
/// Otherwise, falls back to embed page extraction via [`SpotifyEmbedClient`].
pub fn load_spotify_link(
    link: &str,
    credentials: Option<&SpotifyCredentials>,
) -> anyhow::Result<SpotifyPlaylist> {
    let spotify_ref = parse_spotify_ref(link)
        .ok_or_else(|| anyhow::anyhow!("invalid or unsupported Spotify URL or URI: '{link}'"))?;
    let embed = SpotifyEmbedClient::new();
    let api_creds = credentials.cloned().unwrap_or_else(|| SpotifyCredentials::new("", ""));
    let api = SpotifyApiClient::new(api_creds);
    fetch_selected(&spotify_ref, credentials, &api, &embed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeFetcher {
        name: &'static str,
        called: Cell<bool>,
    }

    impl FakeFetcher {
        fn new(name: &'static str) -> Self {
            Self { name, called: Cell::new(false) }
        }
    }

    impl SpotifyFetcher for FakeFetcher {
        fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist> {
            self.called.set(true);
            let id = match source {
                SpotifyRef::Playlist(id) | SpotifyRef::Album(id) => id.clone(),
            };
            Ok(SpotifyPlaylist {
                id,
                title: format!("Title from {}", self.name),
                author: "Author".to_string(),
                artwork: None,
                tracks: Vec::new(),
            })
        }
    }

    #[test]
    fn test_invalid_link_errors() {
        let err = load_spotify_link("not-a-valid-spotify-link", None).expect_err("must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("not-a-valid-spotify-link"), "error must name invalid link: {msg}");
    }

    #[test]
    fn test_complete_credentials_select_api_fetcher() {
        let creds = SpotifyCredentials::new("valid_id", "valid_secret");
        let source = SpotifyRef::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string());
        let api = FakeFetcher::new("api");
        let embed = FakeFetcher::new("embed");

        let result = fetch_selected(&source, Some(&creds), &api, &embed).expect("fetch succeeds");
        assert!(api.called.get());
        assert!(!embed.called.get());
        assert_eq!(result.title, "Title from api");
    }

    #[test]
    fn test_absent_credentials_select_embed_fetcher() {
        let source = SpotifyRef::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string());
        let api = FakeFetcher::new("api");
        let embed = FakeFetcher::new("embed");

        let result = fetch_selected(&source, None, &api, &embed).expect("fetch succeeds");
        assert!(!api.called.get());
        assert!(embed.called.get());
        assert_eq!(result.title, "Title from embed");
    }

    #[test]
    fn test_incomplete_credentials_select_embed_fetcher() {
        let source = SpotifyRef::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string());
        let incomplete_cases = [
            SpotifyCredentials::new("id_only", "   "),
            SpotifyCredentials::new("   ", "secret_only"),
            SpotifyCredentials::new("", ""),
        ];

        for creds in incomplete_cases {
            let api = FakeFetcher::new("api");
            let embed = FakeFetcher::new("embed");
            let result =
                fetch_selected(&source, Some(&creds), &api, &embed).expect("fetch succeeds");
            assert!(!api.called.get(), "api fetcher should not be called for incomplete creds");
            assert!(embed.called.get(), "embed fetcher must be called for incomplete creds");
            assert_eq!(result.title, "Title from embed");
        }
    }
}
