//! Byte-range streaming reader and synchronized read wrapper.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use super::stream::RangeTransport;
use super::stream_pool::{BlockPool, STREAM_BLOCK_BYTES};

/// Default chunk size for streaming range requests (256 KiB alias of `STREAM_BLOCK_BYTES`).
pub const DEFAULT_STREAM_CHUNK_SIZE: usize = STREAM_BLOCK_BYTES;

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
        let mut guard =
            self.0.lock().map_err(|_| std::io::Error::other("sync reader mutex poisoned"))?;
        guard.read(buf)
    }
}

/// Stream adapter wrapping a prefetching block pool for rodio Decoder consumption.
pub struct RangeReader<T: RangeTransport> {
    transport: T,
    chunk_size: usize,
    logical_pos: u64,
    pool: BlockPool<T>,
}

impl<T: RangeTransport> std::fmt::Debug for RangeReader<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeReader")
            .field("chunk_size", &self.chunk_size)
            .field("logical_pos", &self.logical_pos)
            .field("total_len", &self.pool.total_len())
            .finish()
    }
}

impl<T: RangeTransport> RangeReader<T> {
    /// Creates a new range reader wrapping the provided transport.
    pub fn new(transport: T) -> Self {
        let chunk_size = DEFAULT_STREAM_CHUNK_SIZE;
        let pool = BlockPool::new(transport.clone(), chunk_size);
        Self { transport, chunk_size, logical_pos: 0, pool }
    }

    /// Sets a custom chunk size (useful for fine-grained chunk testing).
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        let chunk_size = size.max(1);
        if chunk_size != self.chunk_size {
            self.chunk_size = chunk_size;
            self.pool = BlockPool::new(self.transport.clone(), chunk_size);
            self.logical_pos = 0;
        }
        self
    }

    /// Seeds the underlying block pool with prefetched audio bytes.
    pub fn with_prefetched(mut self, prefix: Vec<u8>) -> Self {
        self.pool.seed_prefetched(prefix);
        self
    }

    /// Returns the current logical read position in the stream.
    pub const fn position(&self) -> u64 {
        self.logical_pos
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
        let total = self.pool.total_len().ok_or_else(|| {
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
}

impl<T: RangeTransport> Read for RangeReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.pool.read(buf)?;
        self.logical_pos = self.pool.position();
        Ok(n)
    }
}

impl<T: RangeTransport> Seek for RangeReader<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target = self.compute_target(pos)?;
        self.pool.seek(target)?;
        self.logical_pos = self.pool.position();
        Ok(self.position())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::stream::RangeResponse;
    use super::*;

    #[derive(Clone)]
    struct FakeRangeTransport {
        payload: Vec<u8>,
        total_len: Option<u64>,
        ignore_range: bool,
        error_at_start: Option<u64>,
        requests: Arc<Mutex<Vec<(u64, usize)>>>,
    }

    impl FakeRangeTransport {
        fn new(payload: Vec<u8>) -> Self {
            let total = payload.len() as u64;
            Self {
                payload,
                total_len: Some(total),
                ignore_range: false,
                error_at_start: None,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn ignoring_range(payload: Vec<u8>) -> Self {
            Self {
                payload,
                total_len: None,
                ignore_range: true,
                error_at_start: None,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn failing_at(payload: Vec<u8>, error_start: u64) -> Self {
            let total = payload.len() as u64;
            Self {
                payload,
                total_len: Some(total),
                ignore_range: false,
                error_at_start: Some(error_start),
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl RangeTransport for FakeRangeTransport {
        fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse> {
            self.requests.lock().unwrap().push((start, length));
            if self.ignore_range {
                let cursor = std::io::Cursor::new(self.payload.clone());
                return Ok(RangeResponse::Full(Box::new(SyncReader::new(cursor))));
            }
            if let Some(err_start) = self.error_at_start {
                if start >= err_start {
                    return Err(std::io::Error::other("simulated transport failure"));
                }
            }
            let total = self.total_len.unwrap_or(self.payload.len() as u64);
            if start >= total {
                return Ok(RangeResponse::Partial { data: Vec::new(), total_len: self.total_len });
            }
            let start_idx = start as usize;
            let end_idx = (start_idx + length).min(self.payload.len());
            let data = self.payload[start_idx..end_idx].to_vec();
            Ok(RangeResponse::Partial { data, total_len: self.total_len })
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
    fn chunk_reader_reads_final_partial_block() {
        let payload = b"partial-final-block".to_vec();
        let transport = FakeRangeTransport::new(payload.clone());
        let mut reader = RangeReader::new(transport).with_chunk_size(8);

        let mut output = Vec::new();
        let mut buf = [0u8; 16];
        loop {
            let n = reader.read(&mut buf).expect("read succeeds across the final partial block");
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

        let pos = reader.position();
        assert_eq!(pos, 26);
    }

    #[test]
    fn chunk_reader_negative_seek_rejected() {
        let payload = b"abcdefghij".to_vec();
        let transport = FakeRangeTransport::new(payload);
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf).expect("reads 2 bytes");

        let err = reader.seek(SeekFrom::Current(-10)).expect_err("negative target rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn chunk_reader_empty_read_returns_zero() {
        let payload = b"hello".to_vec();
        let transport = FakeRangeTransport::new(payload);
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut empty_buf = [0u8; 0];
        let n = reader.read(&mut empty_buf).expect("empty read succeeds");
        assert_eq!(n, 0);
    }

    #[test]
    fn chunk_reader_sequential_backward_seek_unsupported() {
        let payload = b"sequential-seek-test".to_vec();
        let transport = FakeRangeTransport::ignoring_range(payload);
        let mut reader = RangeReader::new(transport).with_chunk_size(4);

        let mut buf = [0u8; 6];
        reader.read_exact(&mut buf).expect("reads 6 bytes");

        let err = reader
            .seek(SeekFrom::Start(2))
            .expect_err("backward seek in sequential stream unsupported");
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn seeded_reader_reads_prefix_without_transport_requests() {
        let payload = b"0123456789abcdef".to_vec();
        let transport = FakeRangeTransport::new(payload.clone());
        let requests = Arc::clone(&transport.requests);
        let prefix = payload[..16].to_vec();
        let mut reader = RangeReader::new(transport).with_chunk_size(8).with_prefetched(prefix);

        let mut output = Vec::new();
        let mut buf = [0u8; 4];
        for _ in 0..4 {
            let n = reader.read(&mut buf).expect("read succeeds from prefix");
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output, payload);
        assert_eq!(reader.position(), 16);

        let reqs = requests.lock().unwrap().clone();
        let prefix_reqs: Vec<_> = reqs.into_iter().filter(|(start, _)| *start < 16).collect();
        assert!(
            prefix_reqs.is_empty(),
            "seeded blocks must not trigger transport requests: {prefix_reqs:?}"
        );
    }
}
