//! Background download of a resolved track into the disk stream cache.

use std::sync::Arc;
use std::time::Duration;

use super::stream_disk_cache::StreamDiskCache;

pub(crate) fn download_stream_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("stream download returned HTTP {}", resp.status());
    }
    Ok(resp.bytes()?.to_vec())
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub(crate) fn cache_download_worker(cache: Arc<StreamDiskCache>, track_id: &str, url: &str) {
    match download_stream_bytes(url) {
        Ok(data) => {
            if let Err(err) = cache.put(track_id, &data) {
                tracing::warn!(track_id = %track_id, error = ?err, "failed to cache stream to disk");
            }
        }
        Err(err) => {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to download stream for caching");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_rejects_invalid_url() {
        assert!(download_stream_bytes("not a valid url").is_err());
    }
}
