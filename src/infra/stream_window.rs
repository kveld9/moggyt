//! Sliding window scheduling for the audio block pool.

use std::collections::BTreeSet;

/// Blocks kept ahead of the read position.
pub const STREAM_WINDOW_AHEAD_BLOCKS: u64 = 40;
/// Blocks kept behind the read position.
pub const STREAM_WINDOW_BEHIND_BLOCKS: u64 = 8;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
