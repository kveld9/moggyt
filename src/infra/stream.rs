//! Stream resolution port and range-based HTTP streaming adapter.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use anyhow::Context;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};

/// Audio stream quality preference: Compact (<=64k), Standard (default), High (>=192k).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioQuality {
    Compact,
    #[default]
    Standard,
    High,
}

/// Maps an [`AudioQuality`] preference to yt-dlp format selector (SDD Q-3).
pub const fn audio_quality_format(quality: AudioQuality) -> &'static str {
    match quality {
        AudioQuality::Compact => "bestaudio[abr<=64]/bestaudio/best",
        AudioQuality::Standard => "bestaudio/best",
        AudioQuality::High => "bestaudio[abr>=192]/bestaudio/best",
    }
}

pub trait StreamResolver {
    fn resolve(&self, video_id: &str) -> Option<String>;
}

#[derive(Debug, Default)]
pub struct StreamStub;

impl StreamStub {
    pub fn new() -> Self {
        Self
    }
}

impl StreamResolver for StreamStub {
    fn resolve(&self, video_id: &str) -> Option<String> {
        let trimmed = video_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(format!("https://stream.local/{trimmed}"))
    }
}

/// Default chunk size for streaming range requests (1 MiB).
pub const DEFAULT_STREAM_CHUNK_SIZE: usize = 1024 * 1024;

/// Parses total stream length from a `Content-Range` header value if present.
pub fn parse_content_range_total(header_val: &str) -> Option<u64> {
    let (_, total_part) = header_val.split_once('/')?;
    total_part.trim().parse::<u64>().ok()
}

/// Thread-safe wrapper converting an arbitrary `Read + Send` into `Read + Send + Sync`.
pub struct SyncReader<R: Read + Send>(Mutex<R>);

impl<R: Read + Send> SyncReader<R> {
    pub fn new(reader: R) -> Self {
        Self(Mutex::new(reader))
    }
}

impl<R: Read + Send> Read for SyncReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut guard = self.0.lock().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "sync reader mutex poisoned")
        })?;
        guard.read(buf)
    }
}

/// Response returned by a [`RangeTransport`].
pub enum RangeResponse {
    /// Server supports range requests or supplied the requested byte slice.
    Partial { data: Vec<u8>, total_len: Option<u64> },
    /// Server ignored the Range header and supplied the full stream body sequentially.
    Full(Box<dyn Read + Send + Sync>),
}

/// Abstraction for fetching audio byte ranges over a transport seam.
pub trait RangeTransport: Send + Sync + 'static {
    /// Fetches a byte range `[start, start + length - 1]`.
    fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse>;
}

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

        let data = response
            .bytes()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?
            .to_vec();

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
                Err(std::io::Error::new(std::io::ErrorKind::Other, err))
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
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;

        self.handle_response(response)
    }
}

/// Stream adapter fetching sequential HTTP Range chunks and feeding rodio Decoder.
pub struct RangeReader<T> {
    transport: T,
    chunk_size: usize,
    buffer: Vec<u8>,
    buffer_offset: usize,
    chunk_start: u64,
    logical_pos: u64,
    total_len: Option<u64>,
    sequential_stream: Option<Box<dyn Read + Send + Sync>>,
}

impl<T> std::fmt::Debug for RangeReader<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeReader")
            .field("chunk_size", &self.chunk_size)
            .field("buffer_len", &self.buffer.len())
            .field("buffer_offset", &self.buffer_offset)
            .field("chunk_start", &self.chunk_start)
            .field("logical_pos", &self.logical_pos)
            .field("total_len", &self.total_len)
            .field("is_sequential", &self.sequential_stream.is_some())
            .finish()
    }
}

impl<T: RangeTransport> RangeReader<T> {
    /// Creates a new range reader wrapping the provided transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            chunk_size: DEFAULT_STREAM_CHUNK_SIZE,
            buffer: Vec::new(),
            buffer_offset: 0,
            chunk_start: 0,
            logical_pos: 0,
            total_len: None,
            sequential_stream: None,
        }
    }

    /// Sets a custom chunk size (useful for fine-grained chunk testing).
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size.max(1);
        self
    }

    fn read_buffered(&mut self, buf: &mut [u8]) -> usize {
        let available = self.buffer.len().saturating_sub(self.buffer_offset);
        let to_copy = buf.len().min(available);
        buf[..to_copy]
            .copy_from_slice(&self.buffer[self.buffer_offset..self.buffer_offset + to_copy]);
        self.buffer_offset += to_copy;
        self.logical_pos += to_copy as u64;
        to_copy
    }

    fn read_sequential(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(ref mut stream) = self.sequential_stream else {
            return Ok(0);
        };
        let bytes_read = stream.read(buf)?;
        self.logical_pos += bytes_read as u64;
        Ok(bytes_read)
    }

    fn handle_range_response(&mut self, response: RangeResponse) -> bool {
        match response {
            RangeResponse::Full(stream) => {
                self.sequential_stream = Some(stream);
                true
            }
            RangeResponse::Partial { data, total_len } => {
                if let Some(total) = total_len {
                    self.total_len = Some(total);
                }
                let is_empty = data.is_empty();
                self.chunk_start = self.logical_pos;
                self.buffer = data;
                self.buffer_offset = 0;
                !is_empty
            }
        }
    }

    fn fetch_next_chunk(&mut self) -> std::io::Result<bool> {
        if let Some(total) = self.total_len {
            if self.logical_pos >= total {
                return Ok(false);
            }
        }
        let response = self.transport.fetch_range(self.logical_pos, self.chunk_size)?;
        Ok(self.handle_range_response(response))
    }

    fn read_ranged(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer_offset < self.buffer.len() {
            return Ok(self.read_buffered(buf));
        }
        if !self.fetch_next_chunk()? {
            return Ok(0);
        }
        if self.sequential_stream.is_some() {
            return self.read_sequential(buf);
        }
        Ok(self.read_buffered(buf))
    }

    fn compute_current_target(&self, delta: i64) -> std::io::Result<u64> {
        let current = self.logical_pos as i64;
        let target = current.checked_add(delta).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek position overflow")
        })?;
        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot seek before start",
            ));
        }
        Ok(target as u64)
    }

    fn compute_end_target(&self, delta: i64) -> std::io::Result<u64> {
        let total = self.total_len.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "cannot seek from end when stream length is unknown",
            )
        })? as i64;
        let target = total.checked_add(delta).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek position overflow")
        })?;
        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot seek before start",
            ));
        }
        Ok(target as u64)
    }

    fn compute_target(&self, pos: SeekFrom) -> std::io::Result<u64> {
        match pos {
            SeekFrom::Start(target) => Ok(target),
            SeekFrom::Current(delta) => self.compute_current_target(delta),
            SeekFrom::End(delta) => self.compute_end_target(delta),
        }
    }

    fn seek_sequential(&mut self, new_pos: u64) -> std::io::Result<u64> {
        let unconsumed = self.buffer.len().saturating_sub(self.buffer_offset) as u64;
        self.buffer.clear();
        self.buffer_offset = 0;

        if new_pos == self.logical_pos {
            return Ok(self.logical_pos);
        }
        if new_pos < self.logical_pos {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "cannot seek backwards in sequential fallback stream",
            ));
        }
        let total_skip = new_pos - self.logical_pos;
        let discard_from_stream = total_skip.saturating_sub(unconsumed);
        if discard_from_stream > 0 {
            let mut sink = std::io::sink();
            let Some(ref mut stream) = self.sequential_stream else {
                self.logical_pos = new_pos;
                return Ok(self.logical_pos);
            };
            let mut limited = Read::by_ref(stream).take(discard_from_stream);
            std::io::copy(&mut limited, &mut sink)?;
        }
        self.logical_pos = new_pos;
        Ok(new_pos)
    }

    fn seek_ranged(&mut self, new_pos: u64) -> std::io::Result<u64> {
        self.buffer.clear();
        self.buffer_offset = 0;
        self.chunk_start = new_pos;
        self.logical_pos = new_pos;
        Ok(new_pos)
    }
}

impl<T: RangeTransport> Read for RangeReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.sequential_stream.is_some() {
            return self.read_sequential(buf);
        }
        self.read_ranged(buf)
    }
}

impl<T: RangeTransport> Seek for RangeReader<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target = self.compute_target(pos)?;
        if self.sequential_stream.is_some() {
            self.seek_sequential(target)
        } else {
            self.seek_ranged(target)
        }
    }
}

/// Opens a streaming audio source from a remote URL.
pub fn open_audio_stream(url: &str) -> anyhow::Result<RangeReader<ReqwestRangeTransport>> {
    let transport = ReqwestRangeTransport::new(url)?;
    Ok(RangeReader::new(transport))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRangeTransport {
        payload: Vec<u8>,
        ignore_range: bool,
        error_at_start: Option<u64>,
        max_slice_len: Option<usize>,
    }

    impl FakeRangeTransport {
        fn new(payload: Vec<u8>) -> Self {
            Self { payload, ignore_range: false, error_at_start: None, max_slice_len: None }
        }

        fn ignoring_range(payload: Vec<u8>) -> Self {
            Self { payload, ignore_range: true, error_at_start: None, max_slice_len: None }
        }

        fn failing_at(payload: Vec<u8>, error_start: u64) -> Self {
            Self {
                payload,
                ignore_range: false,
                error_at_start: Some(error_start),
                max_slice_len: None,
            }
        }

        fn with_max_slice(mut self, max_slice: usize) -> Self {
            self.max_slice_len = Some(max_slice);
            self
        }
    }

    impl RangeTransport for FakeRangeTransport {
        fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse> {
            if self.ignore_range {
                let cursor = std::io::Cursor::new(self.payload.clone());
                return Ok(RangeResponse::Full(Box::new(SyncReader::new(cursor))));
            }
            if let Some(err_start) = self.error_at_start {
                if start >= err_start {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "simulated transport failure",
                    ));
                }
            }
            let total = self.payload.len() as u64;
            if start >= total {
                return Ok(RangeResponse::Partial { data: Vec::new(), total_len: Some(total) });
            }
            let start_idx = start as usize;
            let mut end_idx = (start_idx + length).min(self.payload.len());
            if let Some(max_slice) = self.max_slice_len {
                end_idx = end_idx.min(start_idx + max_slice);
            }
            let data = self.payload[start_idx..end_idx].to_vec();
            Ok(RangeResponse::Partial { data, total_len: Some(total) })
        }
    }

    #[test]
    fn resolver_resolves_non_blank_and_rejects_blank() {
        let resolver = StreamStub::new();
        for (id, exp) in [("v-1", Some("https://stream.local/v-1")), ("", None), ("   ", None)] {
            assert_eq!(resolver.resolve(id).as_deref(), exp);
        }
    }

    #[test]
    fn audio_quality_format_mapping_table() {
        let cases = [
            (AudioQuality::Compact, "bestaudio[abr<=64]/bestaudio/best"),
            (AudioQuality::Standard, "bestaudio/best"),
            (AudioQuality::High, "bestaudio[abr>=192]/bestaudio/best"),
        ];
        for (quality, expected) in cases {
            assert_eq!(audio_quality_format(quality), expected);
        }
    }

    #[test]
    fn audio_quality_default_is_standard() {
        assert_eq!(AudioQuality::default(), AudioQuality::Standard);
    }

    #[test]
    fn audio_quality_serde_roundtrip_snake_case() {
        for (quality, json_str) in [
            (AudioQuality::Compact, "\"compact\""),
            (AudioQuality::Standard, "\"standard\""),
            (AudioQuality::High, "\"high\""),
        ] {
            let serialized = serde_json::to_string(&quality).expect("serialization succeeds");
            assert_eq!(serialized, json_str);
            let deserialized: AudioQuality =
                serde_json::from_str(json_str).expect("deserialization succeeds");
            assert_eq!(deserialized, quality);
        }
    }

    #[test]
    fn content_range_parser_table() {
        let cases = [
            ("bytes 0-1048575/4234567", Some(4234567)),
            ("bytes 0-1048575/*", None),
            ("bytes */4234567", Some(4234567)),
            ("bytes 100-200/not-a-number", None),
            ("invalid format", None),
            ("", None),
        ];
        for (header, expected) in cases {
            assert_eq!(parse_content_range_total(header), expected);
        }
    }

    #[test]
    fn chunk_reader_traverses_chunk_boundaries() {
        let payload = b"0123456789".to_vec();
        let transport = FakeRangeTransport::new(payload);
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut output = Vec::new();
        let mut buf = [0u8; 3];
        loop {
            let n = reader.read(&mut buf).expect("read succeeds across chunks");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output, b"0123456789");
    }

    #[test]
    fn chunk_reader_handles_short_reads_and_final_chunks() {
        let payload = b"short-chunk-data".to_vec();
        let transport = FakeRangeTransport::new(payload.clone()).with_max_slice(3);
        let mut reader = RangeReader::new(transport).with_chunk_size(8);

        let mut output = Vec::new();
        let mut buf = [0u8; 16];
        loop {
            let n = reader.read(&mut buf).expect("read succeeds with short chunks");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output, payload);
    }

    #[test]
    fn chunk_reader_falls_back_on_range_ignoring_server() {
        let payload = b"full sequential payload ignored range".to_vec();
        let transport = FakeRangeTransport::ignoring_range(payload.clone());
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut output = Vec::new();
        let mut buf = [0u8; 8];
        loop {
            let n = reader.read(&mut buf).expect("sequential fallback read succeeds");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output, payload);
    }

    #[test]
    fn chunk_reader_propagates_transport_errors() {
        let payload = b"initial-data-will-fail-soon".to_vec();
        let transport = FakeRangeTransport::failing_at(payload, 4);
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut buf = [0u8; 4];
        let n1 = reader.read(&mut buf).expect("first chunk succeeds");
        assert_eq!(n1, 4);
        assert_eq!(&buf, b"init");

        let err = reader.read(&mut buf).expect_err("second chunk must propagate error");
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "simulated transport failure");
    }

    #[test]
    fn chunk_reader_seeking_coherence() {
        let payload = b"abcdefghijklmnopqrstuvwxyz".to_vec();
        let transport = FakeRangeTransport::new(payload);
        let mut reader = RangeReader::new(transport).with_chunk_size(8);

        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf).expect("reads first 2 bytes");
        assert_eq!(&buf, b"ab");

        reader.seek(SeekFrom::Start(4)).expect("seek start");
        reader.read_exact(&mut buf).expect("reads after seek");
        assert_eq!(&buf, b"ef");

        reader.seek(SeekFrom::Start(20)).expect("seek start across chunk");
        reader.read_exact(&mut buf).expect("reads after across-chunk seek");
        assert_eq!(&buf, b"uv");

        reader.seek(SeekFrom::Current(-5)).expect("seek current backwards");
        reader.read_exact(&mut buf).expect("reads after current seek");
        assert_eq!(&buf, b"rs");

        reader.seek(SeekFrom::End(-2)).expect("seek from end");
        reader.read_exact(&mut buf).expect("reads last 2 bytes");
        assert_eq!(&buf, b"yz");

        let pos = reader.seek(SeekFrom::Current(0)).expect("current pos query");
        assert_eq!(pos, 26);
    }
}
