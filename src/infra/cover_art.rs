//! Artwork fetching and image decoding for cover art rendering.

use std::time::Duration;

use anyhow::Context;

/// Maximum width or height in pixels for decoded cover art textures.
///
/// Bounds GPU texture memory while preserving crisp visual quality on high-DPI displays.
/// Source images exceeding this bound are downscaled using a Lanczos3 filter.
pub const COVER_MAX_PX: u32 = 512;

/// Maximum width or height in pixels for high-resolution decoded cover art textures.
pub const COVER_HIRES_MAX_PX: u32 = 1024;

/// Fetches cover art bytes over HTTP with a blocking GET request and configurable timeout.
///
/// Fails loud with rich context on network, timeout, or HTTP error status codes.
/// Callers are expected to catch errors and soft-fail to cached or fallback artwork.
pub fn fetch_artwork(url: &str, timeout_secs: u64) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to build HTTP client for cover art fetch")?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to fetch cover art from {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error response fetching cover art from {url}"))?;
    let bytes = response
        .bytes()
        .with_context(|| format!("failed to read cover art response body from {url}"))?
        .to_vec();
    Ok(bytes)
}

/// Decodes raw image bytes into an [`image::RgbaImage`].
///
/// Downscales images exceeding `max_px` on either dimension to fit within `max_px` × `max_px`
/// while preserving the aspect ratio and using a high-quality Lanczos3 filter.
/// Returns `None` if the byte payload cannot be decoded as an image.
pub fn decode_cover(bytes: &[u8], max_px: u32) -> Option<image::RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = downscale_if_oversized(img, max_px);
    Some(img.to_rgba8())
}

fn downscale_if_oversized(img: image::DynamicImage, max_px: u32) -> image::DynamicImage {
    if img.width() > max_px || img.height() > max_px {
        img.resize(max_px, max_px, image::imageops::FilterType::Lanczos3)
    } else {
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png_bytes(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba(color));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode test PNG");
        buf
    }

    #[test]
    fn decode_cover_rejects_garbage_and_empty_bytes() {
        assert_eq!(decode_cover(b"", COVER_MAX_PX), None);
        assert_eq!(decode_cover(b"corrupted image header", COVER_MAX_PX), None);
    }

    #[test]
    fn decode_cover_decodes_valid_png_within_bounds() {
        let bytes = sample_png_bytes(8, 8, [255, 0, 0, 255]);
        let decoded = decode_cover(&bytes, COVER_MAX_PX).expect("decode valid PNG");
        assert_eq!(decoded.dimensions(), (8, 8));
        assert_eq!(decoded.as_raw().len(), 64 * 4);
        assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn decode_cover_downscales_oversized_images() {
        let bytes = sample_png_bytes(1024, 4, [0, 255, 0, 255]);
        let decoded = decode_cover(&bytes, 512).expect("decode oversized PNG");
        assert_eq!(decoded.dimensions(), (512, 2));
    }

    #[test]
    fn decode_cover_downscales_oversized_images_to_hires_bound() {
        let bytes = sample_png_bytes(2048, 1024, [0, 255, 0, 255]);
        let decoded_default = decode_cover(&bytes, COVER_MAX_PX).expect("decode default PNG");
        assert_eq!(decoded_default.dimensions(), (512, 256));

        let decoded_hires = decode_cover(&bytes, COVER_HIRES_MAX_PX).expect("decode hires PNG");
        assert_eq!(decoded_hires.dimensions(), (1024, 512));
    }

    #[test]
    fn decode_cover_decodes_valid_jpeg() {
        let img = image::RgbImage::from_pixel(16, 16, image::Rgb([0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .expect("encode test JPEG");
        let decoded = decode_cover(&buf, COVER_MAX_PX).expect("decode valid JPEG");
        assert_eq!(decoded.dimensions(), (16, 16));
    }
}
