//! Feature-based state implementation modules for [`crate::app::AppState`].

pub mod covers;
pub mod home_detail;
pub mod likes_backup;
pub mod lyrics;
pub mod meta;
pub mod playback;
pub mod presence;
pub mod queue;
pub mod scrobbling;
pub mod search;
pub mod session;
pub mod settings;
pub mod shortcuts;
pub mod skips;
pub mod sleep;
pub mod updates;

#[cfg(test)]
pub mod test_support;

pub(crate) use home_detail::item_detail_target;

/// Returns current UTC timestamp in seconds since UNIX epoch, or 0 on clock failure.
pub(crate) fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
