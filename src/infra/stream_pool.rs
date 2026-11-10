//! Prefetching audio block pool with worker threads and sliding window.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::stream::{RangeResponse, RangeTransport};

/// Audio block size fetched per range request.
pub const STREAM_BLOCK_BYTES: usize = 256 * 1024;
/// Number of block fetch worker threads.
pub const STREAM_WORKERS: usize = 4;
/// Blocks kept ahead of the read position.
pub const STREAM_WINDOW_AHEAD_BLOCKS: u64 = 40;
/// Blocks kept behind the read position.
pub const STREAM_WINDOW_BEHIND_BLOCKS: u64 = 8;
/// Stream size above which blocks outside the sliding window are dropped.
pub const LARGE_STREAM_THRESHOLD_BYTES: u64 = 40 * 1024 * 1024;

/// Calculates the block index for a byte position given a block size.
pub fn block_index(position: u64, block_bytes: usize) -> u64 {
    position / (block_bytes.max(1) as u64)
}

/// Checks whether a block index falls within the sliding window around base_block.
pub fn block_is_in_window(block: u64, base_block: u64) -> bool {
    block.saturating_add(STREAM_WINDOW_BEHIND_BLOCKS) >= base_block
        && block <= base_block.saturating_add(STREAM_WINDOW_AHEAD_BLOCKS)
}

fn find_unfetched_forward(
    base_block: u64,
    total_blocks: u64,
    have: &BTreeSet<u64>,
    pending: &BTreeSet<u64>,
) -> Option<u64> {
    if base_block >= total_blocks {
        return None;
    }
    let end = base_block.saturating_add(STREAM_WINDOW_AHEAD_BLOCKS).min(total_blocks - 1);
    (base_block..=end).find(|b| !have.contains(b) && !pending.contains(b))
}

fn find_unfetched_backward(
    base_block: u64,
    total_blocks: u64,
    have: &BTreeSet<u64>,
    pending: &BTreeSet<u64>,
) -> Option<u64> {
    if base_block == 0 || total_blocks == 0 {
        return None;
    }
    let start = (base_block - 1).min(total_blocks - 1);
    let end = base_block.saturating_sub(STREAM_WINDOW_BEHIND_BLOCKS);
    if start < end {
        return None;
    }
    (end..=start).rev().find(|b| !have.contains(b) && !pending.contains(b))
}

/// Determines the next block index to fetch, preferring base_block, then forward, then backward.
pub fn next_block_to_fetch(
    base_block: u64,
    total_blocks: u64,
    have: &BTreeSet<u64>,
    pending: &BTreeSet<u64>,
) -> Option<u64> {
    find_unfetched_forward(base_block, total_blocks, have, pending)
        .or_else(|| find_unfetched_backward(base_block, total_blocks, have, pending))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PoolMode {
    Blocked,
    Sequential,
}

struct SharedState {
    mode: PoolMode,
    stopped: bool,
    cursor_pos: u64,
    total_len: Option<u64>,
    terminal_error: Option<String>,
    blocks: BTreeMap<u64, Vec<u8>>,
    have: BTreeSet<u64>,
    pending: BTreeSet<u64>,
    sequential_stream: Option<Box<dyn Read + Send + Sync>>,
}

struct PoolCoordination {
    state: Mutex<SharedState>,
    cvar: Condvar,
}

fn prune_blocks_outside_window(state: &mut SharedState, base_block: u64) {
    let exceeds_threshold = match state.total_len {
        Some(total) => total > LARGE_STREAM_THRESHOLD_BYTES,
        None => true,
    };
    if !exceeds_threshold {
        return;
    }
    state.blocks.retain(|&idx, _| block_is_in_window(idx, base_block));
    state.have.retain(|&idx| block_is_in_window(idx, base_block));
}

fn calculate_remaining_len(fetch_start: u64, block_bytes: usize, total_len: Option<u64>) -> usize {
    match total_len {
        Some(total) => {
            if fetch_start >= total {
                0
            } else {
                (block_bytes as u64).min(total - fetch_start) as usize
            }
        }
        None => block_bytes,
    }
}

fn acquire_next_block(coord: &PoolCoordination, block_bytes: usize) -> Option<(u64, u64, usize)> {
    let mut state = coord.state.lock().unwrap();
    loop {
        if state.stopped || state.terminal_error.is_some() || state.mode == PoolMode::Sequential {
            return None;
        }
        let base = block_index(state.cursor_pos, block_bytes);
        let total_blocks = match state.total_len {
            Some(total) => total.div_ceil(block_bytes as u64),
            None => u64::MAX,
        };
        if let Some(candidate) =
            next_block_to_fetch(base, total_blocks, &state.have, &state.pending)
        {
            let fetch_start = candidate * (block_bytes as u64);
            let remaining_len = calculate_remaining_len(fetch_start, block_bytes, state.total_len);
            if remaining_len > 0 {
                state.pending.insert(candidate);
                return Some((candidate, fetch_start, remaining_len));
            }
        }
        state = coord.cvar.wait(state).unwrap();
    }
}

fn store_partial_response(
    state: &mut SharedState,
    target_block: u64,
    block_bytes: usize,
    data: Vec<u8>,
    total_len: Option<u64>,
) {
    if state.total_len.is_none() {
        if total_len.is_some() {
            state.total_len = total_len;
        } else if data.len() < block_bytes {
            state.total_len = Some(target_block * (block_bytes as u64) + data.len() as u64);
        }
    }
    let current_base = block_index(state.cursor_pos, block_bytes);
    if block_is_in_window(target_block, current_base) {
        state.have.insert(target_block);
        state.blocks.insert(target_block, data);
        prune_blocks_outside_window(state, current_base);
    }
}

fn handle_fetch_result(
    coord: &PoolCoordination,
    target_block: u64,
    block_bytes: usize,
    fetch_res: std::io::Result<RangeResponse>,
) -> bool {
    let mut state = coord.state.lock().unwrap();
    state.pending.remove(&target_block);
    if state.stopped {
        return false;
    }
    match fetch_res {
        Err(e) => {
            if state.terminal_error.is_none() {
                state.terminal_error = Some(e.to_string());
            }
            coord.cvar.notify_all();
            false
        }
        Ok(RangeResponse::Full(stream)) => {
            if state.mode == PoolMode::Blocked {
                state.mode = PoolMode::Sequential;
                state.sequential_stream = Some(stream);
            }
            coord.cvar.notify_all();
            false
        }
        Ok(RangeResponse::Partial { data, total_len }) => {
            store_partial_response(&mut state, target_block, block_bytes, data, total_len);
            coord.cvar.notify_all();
            true
        }
    }
}

fn run_worker<T: RangeTransport>(
    mut transport: T,
    block_bytes: usize,
    coord: Arc<PoolCoordination>,
) {
    loop {
        let fetch_target = acquire_next_block(&coord, block_bytes);
        let Some((target_block, fetch_start, remaining_len)) = fetch_target else {
            return;
        };

        let fetch_res = transport.fetch_range(fetch_start, remaining_len);
        let should_continue = handle_fetch_result(&coord, target_block, block_bytes, fetch_res);
        if !should_continue {
            return;
        }
    }
}

fn read_sequential_stream(
    state: &mut SharedState,
    buf: &mut [u8],
    cursor_pos: &mut u64,
) -> std::io::Result<usize> {
    let Some(ref mut stream) = state.sequential_stream else {
        return Ok(0);
    };
    let bytes_read = stream.read(buf)?;
    *cursor_pos += bytes_read as u64;
    state.cursor_pos = *cursor_pos;
    Ok(bytes_read)
}

fn seek_sequential_stream(
    state: &mut SharedState,
    target_pos: u64,
    cursor_pos: &mut u64,
) -> std::io::Result<()> {
    if target_pos == *cursor_pos {
        return Ok(());
    }
    if target_pos < *cursor_pos {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "cannot seek backwards in sequential fallback stream",
        ));
    }
    let total_skip = target_pos - *cursor_pos;
    if let Some(ref mut stream) = state.sequential_stream {
        let mut sink = std::io::sink();
        let mut limited = Read::by_ref(stream).take(total_skip);
        std::io::copy(&mut limited, &mut sink)?;
    }
    *cursor_pos = target_pos;
    state.cursor_pos = target_pos;
    Ok(())
}

fn check_stopped_or_sequential(
    state: &mut SharedState,
    buf: &mut [u8],
    cursor_pos: &mut u64,
) -> Option<std::io::Result<usize>> {
    if state.stopped {
        return Some(Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "stream pool stopped",
        )));
    }
    if state.mode == PoolMode::Sequential {
        return Some(read_sequential_stream(state, buf, cursor_pos));
    }
    None
}

fn is_eof(state: &SharedState, cursor_pos: u64) -> bool {
    state.total_len.is_some_and(|total| cursor_pos >= total)
}

fn try_copy_needed_block(
    state: &mut SharedState,
    cursor_pos: &mut u64,
    block_bytes: usize,
    buf: &mut [u8],
) -> Option<std::io::Result<usize>> {
    let needed = block_index(*cursor_pos, block_bytes);
    let block_data = state.blocks.get(&needed)?;
    let block_start = needed * (block_bytes as u64);
    let offset = (*cursor_pos - block_start) as usize;
    if offset >= block_data.len() {
        return Some(Ok(0));
    }
    let available = block_data.len() - offset;
    let to_copy = buf.len().min(available);
    buf[..to_copy].copy_from_slice(&block_data[offset..offset + to_copy]);
    *cursor_pos += to_copy as u64;
    state.cursor_pos = *cursor_pos;
    Some(Ok(to_copy))
}

/// Prefetching block pool maintaining a resident sliding window of audio chunks.
pub struct BlockPool<T: RangeTransport> {
    block_bytes: usize,
    cursor_pos: u64,
    coord: Arc<PoolCoordination>,
    worker_handles: Vec<std::thread::JoinHandle<()>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: RangeTransport> std::fmt::Debug for BlockPool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockPool")
            .field("block_bytes", &self.block_bytes)
            .field("cursor_pos", &self.cursor_pos)
            .field("total_len", &self.total_len())
            .finish()
    }
}

impl<T: RangeTransport> BlockPool<T> {
    /// Spawns `STREAM_WORKERS` workers and prepares an empty pool.
    pub fn new(transport: T, block_bytes: usize) -> Self {
        let block_bytes = block_bytes.max(1);
        let coord = Arc::new(PoolCoordination {
            state: Mutex::new(SharedState {
                mode: PoolMode::Blocked,
                stopped: false,
                cursor_pos: 0,
                total_len: None,
                terminal_error: None,
                blocks: BTreeMap::new(),
                have: BTreeSet::new(),
                pending: BTreeSet::new(),
                sequential_stream: None,
            }),
            cvar: Condvar::new(),
        });

        let mut worker_handles = Vec::with_capacity(STREAM_WORKERS);
        for _ in 0..STREAM_WORKERS {
            let worker_transport = transport.clone();
            let worker_coord = Arc::clone(&coord);
            let handle = std::thread::spawn(move || {
                run_worker(worker_transport, block_bytes, worker_coord);
            });
            worker_handles.push(handle);
        }

        Self {
            block_bytes,
            cursor_pos: 0,
            coord,
            worker_handles,
            _marker: std::marker::PhantomData,
        }
    }

    /// Block size fetched per range request.
    pub const fn block_bytes(&self) -> usize {
        self.block_bytes
    }

    /// Total stream length once known.
    pub fn total_len(&self) -> Option<u64> {
        self.coord.state.lock().unwrap().total_len
    }

    /// Current byte position.
    pub const fn position(&self) -> u64 {
        self.cursor_pos
    }

    /// Reads up to `buf.len()` bytes at the current internal position, waiting for its block.
    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let coord = Arc::clone(&self.coord);
        let mut state = coord.state.lock().unwrap();
        if state.mode == PoolMode::Sequential {
            let mut cursor_pos = self.cursor_pos;
            let res = read_sequential_stream(&mut state, buf, &mut cursor_pos);
            self.cursor_pos = cursor_pos;
            return res;
        }
        self.read_blocked(state, buf)
    }

    fn read_blocked(
        &mut self,
        mut state: MutexGuard<'_, SharedState>,
        buf: &mut [u8],
    ) -> std::io::Result<usize> {
        loop {
            if let Some(res) = check_stopped_or_sequential(&mut state, buf, &mut self.cursor_pos) {
                return res;
            }
            if is_eof(&state, self.cursor_pos) {
                return Ok(0);
            }
            if let Some(res) =
                try_copy_needed_block(&mut state, &mut self.cursor_pos, self.block_bytes, buf)
            {
                self.coord.cvar.notify_all();
                return res;
            }
            if let Some(ref err) = state.terminal_error {
                return Err(std::io::Error::other(err.clone()));
            }
            self.coord.cvar.notify_all();
            state = self.coord.cvar.wait(state).unwrap();
        }
    }

    /// Repositions the read cursor; re-prioritizes block fetches at the new position.
    pub fn seek(&mut self, position: u64) -> std::io::Result<()> {
        let mut state = self.coord.state.lock().unwrap();
        if state.mode == PoolMode::Sequential {
            return seek_sequential_stream(&mut state, position, &mut self.cursor_pos);
        }
        self.cursor_pos = position;
        state.cursor_pos = position;
        let base = block_index(position, self.block_bytes);
        state.pending.retain(|&b| block_is_in_window(b, base));
        prune_blocks_outside_window(&mut state, base);
        self.coord.cvar.notify_all();
        Ok(())
    }
}

impl<T: RangeTransport> Read for BlockPool<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        BlockPool::read(self, buf)
    }
}

impl<T: RangeTransport> Drop for BlockPool<T> {
    fn drop(&mut self) {
        {
            let mut state = self.coord.state.lock().unwrap();
            state.stopped = true;
            self.coord.cvar.notify_all();
        }
        for handle in self.worker_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::stream_reader::SyncReader;

    #[derive(Clone)]
    struct FakePoolTransport {
        payload: Vec<u8>,
        total_len: Option<u64>,
        ignore_range: bool,
        error_at_start: Option<u64>,
        requests: Arc<Mutex<Vec<(u64, usize)>>>,
    }

    impl FakePoolTransport {
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

    impl RangeTransport for FakePoolTransport {
        fn fetch_range(&mut self, start: u64, length: usize) -> std::io::Result<RangeResponse> {
            self.requests.lock().unwrap().push((start, length));
            if self.ignore_range {
                let cursor = std::io::Cursor::new(self.payload.clone());
                return Ok(RangeResponse::Full(Box::new(SyncReader::new(cursor))));
            }
            if let Some(err_start) = self.error_at_start {
                if start >= err_start {
                    return Err(std::io::Error::other("simulated transport error"));
                }
            }
            let total = self.total_len.unwrap_or(self.payload.len() as u64);
            if start >= total {
                return Ok(RangeResponse::Partial { data: Vec::new(), total_len: Some(total) });
            }
            let start_idx = start as usize;
            let end_idx = (start_idx + length).min(self.payload.len());
            let data = self.payload[start_idx..end_idx].to_vec();
            Ok(RangeResponse::Partial { data, total_len: Some(total) })
        }
    }

    #[test]
    fn next_block_to_fetch_prefers_read_position() {
        let have = BTreeSet::new();
        let pending = BTreeSet::new();
        assert_eq!(next_block_to_fetch(0, 10, &have, &pending), Some(0));
        assert_eq!(next_block_to_fetch(5, 10, &have, &pending), Some(5));
    }

    #[test]
    fn next_block_to_fetch_walks_forward_then_behind() {
        let mut have = BTreeSet::new();
        let pending = BTreeSet::new();
        have.insert(5);
        assert_eq!(next_block_to_fetch(5, 100, &have, &pending), Some(6));

        for b in 5..=45 {
            have.insert(b);
        }
        assert_eq!(next_block_to_fetch(5, 100, &have, &pending), Some(4));
    }

    #[test]
    fn next_block_to_fetch_skips_have_and_pending() {
        let mut have = BTreeSet::new();
        let mut pending = BTreeSet::new();
        have.insert(0);
        pending.insert(1);
        assert_eq!(next_block_to_fetch(0, 10, &have, &pending), Some(2));
    }

    #[test]
    fn next_block_to_fetch_respects_total_blocks() {
        let mut have = BTreeSet::new();
        let pending = BTreeSet::new();
        for b in 0..5 {
            have.insert(b);
        }
        assert_eq!(next_block_to_fetch(0, 5, &have, &pending), None);
        assert_eq!(next_block_to_fetch(5, 5, &have, &pending), None);
        assert_eq!(next_block_to_fetch(0, 0, &have, &pending), None);
    }

    #[test]
    fn block_is_in_window_bounds_ahead_and_behind() {
        assert!(block_is_in_window(0, 0));
        assert!(block_is_in_window(40, 0));
        assert!(!block_is_in_window(41, 0));

        assert!(block_is_in_window(10, 10));
        assert!(block_is_in_window(2, 10));
        assert!(!block_is_in_window(1, 10));
        assert!(block_is_in_window(50, 10));
        assert!(!block_is_in_window(51, 10));
    }

    #[test]
    fn pool_reads_contiguous_bytes_across_blocks() {
        let payload: Vec<u8> = (0..48u8).collect();
        let transport = FakePoolTransport::new(payload.clone());
        let mut pool = BlockPool::new(transport, 16);

        let mut output = Vec::new();
        let mut buf = [0u8; 7];
        loop {
            let n = pool.read(&mut buf).expect("read succeeds across blocks");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output, payload);
        assert_eq!(pool.position(), 48);
        assert_eq!(pool.read(&mut buf).expect("read at EOF returns 0"), 0);
    }

    #[test]
    fn pool_records_total_len_from_first_partial_response() {
        let payload: Vec<u8> = (0..32u8).collect();
        let transport = FakePoolTransport::new(payload);
        let mut pool = BlockPool::new(transport, 16);

        let mut buf = [0u8; 1];
        let n = pool.read(&mut buf).expect("reads first byte");
        assert_eq!(n, 1);
        assert_eq!(pool.total_len(), Some(32));
    }

    #[test]
    fn pool_partial_last_block_is_bounded() {
        let payload: Vec<u8> = (0..38u8).collect();
        let transport = FakePoolTransport::new(payload);
        let requests = Arc::clone(&transport.requests);
        let mut pool = BlockPool::new(transport, 16);

        let mut output = Vec::new();
        let mut buf = [0u8; 16];
        loop {
            let n = pool.read(&mut buf).expect("read succeeds");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(output.len(), 38);

        let reqs = requests.lock().unwrap().clone();
        let last_block_req = reqs.iter().find(|(start, _)| *start == 32);
        assert!(last_block_req.is_some(), "must have requested block at start 32");
        assert_eq!(last_block_req.unwrap().1, 6, "last block request must be bounded to 6 bytes");
    }

    #[test]
    fn pool_propagates_transport_error() {
        let payload: Vec<u8> = (0..32u8).collect();
        let transport = FakePoolTransport::failing_at(payload, 16);
        let mut pool = BlockPool::new(transport, 16);

        let mut buf = [0u8; 8];
        let n1 = pool.read(&mut buf).expect("first read succeeds");
        assert_eq!(n1, 8);
        assert_eq!(&buf, &[0, 1, 2, 3, 4, 5, 6, 7]);

        let n2 = pool.read(&mut buf).expect("second read within block 0 succeeds");
        assert_eq!(n2, 8);
        assert_eq!(&buf, &[8, 9, 10, 11, 12, 13, 14, 15]);

        let err = pool.read(&mut buf).expect_err("read at block 1 must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "simulated transport error");
    }

    #[test]
    fn pool_sequential_fallback_when_range_is_ignored() {
        let payload = b"sequential fallback payload".to_vec();
        let transport = FakePoolTransport::ignoring_range(payload.clone());
        let mut pool = BlockPool::new(transport, 8);

        let mut buf = [0u8; 10];
        let n = pool.read(&mut buf).expect("forward read succeeds");
        assert_eq!(n, 10);
        assert_eq!(&buf, b"sequential");

        let err = pool.seek(5).expect_err("backward seek in sequential mode must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);

        pool.seek(15).expect("forward seek in sequential mode succeeds");
        let mut buf2 = [0u8; 8];
        let n2 = pool.read(&mut buf2).expect("read after forward seek succeeds");
        assert_eq!(n2, 8);
        assert_eq!(&buf2, &payload[15..23]);
    }

    #[test]
    fn pool_seek_forward_and_backward_reads_correct_bytes() {
        let payload: Vec<u8> = (0..64u8).collect();
        let transport = FakePoolTransport::new(payload);
        let mut pool = BlockPool::new(transport, 16);

        let mut buf = [0u8; 4];
        pool.read_exact(&mut buf).expect("initial read");
        assert_eq!(&buf, &[0, 1, 2, 3]);

        pool.seek(30).expect("seek forward");
        pool.read_exact(&mut buf).expect("read after seek forward");
        assert_eq!(&buf, &[30, 31, 32, 33]);

        pool.seek(10).expect("seek backward");
        pool.read_exact(&mut buf).expect("read after seek backward");
        assert_eq!(&buf, &[10, 11, 12, 13]);
    }

    #[test]
    fn pool_drop_stops_workers() {
        let payload: Vec<u8> = (0..64u8).collect();
        let transport = FakePoolTransport::new(payload);
        let requests = Arc::clone(&transport.requests);
        {
            let mut pool = BlockPool::new(transport, 16);
            let mut buf = [0u8; 4];
            let _ = pool.read(&mut buf);
        }
        let total_requests = requests.lock().unwrap().len();
        assert!(total_requests <= 44, "number of requests must be bounded");
    }
}
