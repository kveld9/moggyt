//! Blocking HTTP transport adapter for range-based audio streaming via reqwest.

use anyhow::Context;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};

use super::stream::{RangeResponse, RangeTransport};
use super::stream_reader::{
    DEFAULT_STREAM_CHUNK_SIZE, RangeReader, SyncReader, parse_content_range_total,
};

/// Real blocking HTTP transport for range-based streaming via reqwest.
pub struct ReqwestRangeTransport {
    client: reqwest::blocking::Client,
    url: String,
}

impl ReqwestRangeTransport {
    /// Creates a new transport for the specified URL, validating its format.
    pub fn new(url: impl Into<String>) -> anyhow::Result<Self> {
        let url = url.into();
        let _ = reqwest::Url::parse(&url)
            .with_context(|| format!("invalid audio stream url '{url}'"))?;
        let client = reqwest::blocking::Client::builder()
            .build()
            .context("failed to build reqwest client for audio streaming")?;
        Ok(Self { client, url })
    }

    fn handle_partial_response(
        &self,
        response: reqwest::blocking::Response,
    ) -> std::io::Result<RangeResponse> {
        let total_len = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|h| h.to_str().ok())
            .and_then(parse_content_range_total);

        let data = response.bytes().map_err(std::io::Error::other)?.to_vec();

        Ok(RangeResponse::Partial { data, total_len })
    }

    fn handle_response(
        &self,
        response: reqwest::blocking::Response,
    ) -> std::io::Result<RangeResponse> {
        match response.status() {
            StatusCode::PARTIAL_CONTENT => self.handle_partial_response(response),
            StatusCode::OK => Ok(RangeResponse::Full(Box::new(SyncReader::new(response)))),
            StatusCode::RANGE_NOT_SATISFIABLE => {
                let total_len = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|h| h.to_str().ok())
                    .and_then(parse_content_range_total);
                Ok(RangeResponse::Partial { data: Vec::new(), total_len })
            }
            _ => {
                let err = response.error_for_status().unwrap_err();
                Err(std::io::Error::other(err))
            }
        }
    }
}

impl RangeTransport for ReqwestRangeTransport {
    fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse> {
        let end = start.saturating_add(length as u64).saturating_sub(1);
        let response = self
            .client
            .get(&self.url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .map_err(std::io::Error::other)?;

        self.handle_response(response)
    }
}

/// Opens a streaming audio source from a remote URL.
pub fn open_audio_stream(url: &str) -> anyhow::Result<RangeReader<ReqwestRangeTransport>> {
    let transport = ReqwestRangeTransport::new(url)?;
    Ok(RangeReader::new(transport).with_chunk_size(DEFAULT_STREAM_CHUNK_SIZE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_url_without_scheme() {
        let result = ReqwestRangeTransport::new("example.invalid/audio");
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("invalid audio stream url 'example.invalid/audio'"));
    }

    #[test]
    fn new_accepts_absolute_https_url() {
        let transport = ReqwestRangeTransport::new("https://audio.example.com/stream.m4a");
        assert!(transport.is_ok());
        let transport = transport.unwrap();
        assert_eq!(transport.url, "https://audio.example.com/stream.m4a");
    }

    #[test]
    fn open_audio_stream_on_unparseable_url_fails_with_expected_message() {
        let result = open_audio_stream("invalid-url-without-scheme");
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("invalid audio stream url"));
    }
}
