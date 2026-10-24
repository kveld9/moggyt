//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod queue;
mod search;

pub use queue::{PlaybackQueue, RepeatMode};
pub use search::{SearchQuery, Track};
