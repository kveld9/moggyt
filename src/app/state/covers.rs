//! Cover art fetching, caching, and texture decoding for [`AppState`].

use eframe::egui;

use crate::app::AppState;
use crate::infra::{
    AudioBackend, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog, CoverCache, LyricsSource,
    SessionStore, decode_cover, fetch_artwork,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Resolves the maximum pixel bound for decoded cover art based on current settings.
    pub fn cover_max_px(&self) -> u32 {
        if self.hi_res_covers { COVER_HIRES_MAX_PX } else { COVER_MAX_PX }
    }

    /// Decodes raw cover art bytes into a color image bounded by the active setting.
    pub fn decode_cover_image(&self, bytes: &[u8]) -> Option<egui::ColorImage> {
        let rgba = decode_cover(bytes, self.cover_max_px())?;
        let size = [rgba.width() as usize, rgba.height() as usize];
        Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
    }

    pub(crate) fn clear_cover_texture(&mut self) {
        self.cover_key = None;
        self.cover_texture = None;
    }

    fn current_artwork_url(&self) -> Option<String> {
        self.queue.current()?.artwork.clone()
    }

    fn load_cover_bytes(&mut self, url: &str) -> Option<Vec<u8>> {
        if let Some(cached) = self.covers.get(url) {
            return Some(cached);
        }
        match fetch_artwork(url, 8) {
            Ok(fetched) => {
                self.covers.put(url, fetched.clone());
                Some(fetched)
            }
            Err(err) => {
                tracing::warn!(url = %url, error = %err, "failed to fetch cover art");
                None
            }
        }
    }

    pub(crate) fn resolve_cover_bytes(&mut self) -> Option<(String, Vec<u8>)> {
        let Some(url) = self.current_artwork_url() else {
            self.clear_cover_texture();
            return None;
        };
        if self.cover_key.as_deref() == Some(&url) {
            return None;
        }
        let Some(bytes) = self.load_cover_bytes(&url) else {
            self.clear_cover_texture();
            return None;
        };
        Some((url, bytes))
    }

    fn store_cover_texture(&mut self, ctx: &egui::Context, url: String, bytes: &[u8]) {
        let Some(img) = self.decode_cover_image(bytes) else {
            self.clear_cover_texture();
            return;
        };
        let texture = ctx.load_texture("moggyt-cover", img, egui::TextureOptions::LINEAR);
        self.cover_key = Some(url.clone());
        self.cover_texture = Some((url, texture));
    }

    /// Fetches, caches, decodes, and loads cover art for the currently playing track.
    pub fn refresh_covers(&mut self, ctx: &egui::Context) {
        if let Some((url, bytes)) = self.resolve_cover_bytes() {
            self.store_cover_texture(ctx, url, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::Track;

    #[test]
    fn resolve_cover_bytes_empty_queue_clears_and_returns_none() {
        let mut state = wired();
        state.cover_key = Some("https://example.com/art.png".to_string());
        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key, None);
        assert!(state.cover_texture.is_none());
    }

    #[test]
    fn resolve_cover_bytes_track_without_artwork_clears_and_returns_none() {
        let mut state = wired();
        let track = Track::new("t1", "Song", "Artist");
        state.queue.push(track);
        state.cover_key = Some("https://example.com/art.png".to_string());
        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key, None);
        assert!(state.cover_texture.is_none());
    }

    #[test]
    fn resolve_cover_bytes_cache_hit_returns_url_and_bytes() {
        let mut state = wired();
        let url = "https://example.com/art.png";
        let track = Track::new("t1", "Song", "Artist").with_artwork(url);
        state.queue.push(track);
        let sample_bytes = vec![1, 2, 3, 4];
        state.covers.put(url, sample_bytes.clone());

        let resolved = state.resolve_cover_bytes();
        assert_eq!(resolved, Some((url.to_string(), sample_bytes)));
    }

    #[test]
    fn resolve_cover_bytes_deduplicates_when_already_matching_key() {
        let mut state = wired();
        let url = "https://example.com/art.png";
        let track = Track::new("t1", "Song", "Artist").with_artwork(url);
        state.queue.push(track);
        state.covers.put(url, vec![1, 2, 3]);
        state.cover_key = Some(url.to_string());

        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key.as_deref(), Some(url));
    }

    #[test]
    fn resolve_cover_bytes_advancing_track_resolves_new_artwork() {
        let mut state = wired();
        let url1 = "https://example.com/art1.png";
        let url2 = "https://example.com/art2.png";
        state.queue.push(Track::new("t1", "Song 1", "Artist").with_artwork(url1));
        state.queue.push(Track::new("t2", "Song 2", "Artist").with_artwork(url2));
        state.covers.put(url1, vec![10, 20]);
        state.covers.put(url2, vec![30, 40]);

        // First track
        assert_eq!(state.resolve_cover_bytes(), Some((url1.to_string(), vec![10, 20])));
        state.cover_key = Some(url1.to_string());

        // Advance to second track
        state.queue.advance();
        assert_eq!(state.resolve_cover_bytes(), Some((url2.to_string(), vec![30, 40])));
    }

    #[test]
    fn test_decode_cover_image_honors_hires_flag() {
        let mut state = wired();
        let img = image::RgbaImage::from_pixel(1200, 600, image::Rgba([10, 20, 30, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode test PNG");

        // With default (flag off), capped at COVER_MAX_PX (512)
        let decoded_default = state.decode_cover_image(&buf).expect("decode default");
        assert_eq!(decoded_default.size, [512, 256]);

        // With flag on, capped at COVER_HIRES_MAX_PX (1024)
        state.set_hi_res_covers(true);
        let decoded_hires = state.decode_cover_image(&buf).expect("decode hires");
        assert_eq!(decoded_hires.size, [1024, 512]);
    }
}
