//! Cross-platform single-instance guard.
//!
//! Enforces a single running instance of moggyt across the operating system
//! using system-level primitives (named mutex on Windows, socket lock on Unix).
//!
//! Stale locks auto-release on process exit via OS kernel primitives, requiring
//! no manual lock files or filesystem cleanup.

use single_instance::SingleInstance;

/// Cross-platform guard keeping the application single-instance lock held.
///
/// Dropping this guard releases the system-level lock, allowing subsequent
/// attempts to acquire it. Stale locks automatically release on process exit
/// via operating system kernel primitives (named mutex on Windows, socket lock
/// on Unix), requiring no stale lock files or filesystem cleanup.
pub struct SingleInstanceGuard {
    _guard: SingleInstance,
}

impl SingleInstanceGuard {
    /// Attempts to acquire the system-wide single instance guard.
    #[must_use]
    pub fn try_acquire() -> Option<Self> {
        try_acquire()
    }
}

/// Returns the stable cross-platform instance lock identifier.
#[must_use]
pub fn instance_lock_name() -> &'static str {
    "moggyt-single-instance"
}

/// Attempts to acquire the system-wide single instance guard.
///
/// Returns `Some(SingleInstanceGuard)` on first acquisition, or `None` when
/// another live instance (or another guard in the same process) holds it.
#[must_use]
pub fn try_acquire() -> Option<SingleInstanceGuard> {
    let guard = SingleInstance::new(instance_lock_name()).ok()?;
    if !guard.is_single() {
        return None;
    }
    Some(SingleInstanceGuard { _guard: guard })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_name_matches_expected_identifier() {
        assert_eq!(instance_lock_name(), "moggyt-single-instance");
    }

    #[test]
    fn single_instance_guard_lifecycle_acquire_conflict_and_reacquire() {
        let first = try_acquire();
        assert!(first.is_some(), "first acquisition must succeed");

        let second = try_acquire();
        assert!(second.is_none(), "second acquisition while first guard is alive must fail");

        drop(first);

        let third = try_acquire();
        assert!(third.is_some(), "re-acquisition after dropping guard must succeed");
    }
}
