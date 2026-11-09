//! Persistence health and quarantine helpers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;

/// Health of a disk-backed store after the last load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoreHealth {
    #[default]
    Ok,
    /// The payload was corrupt and has been renamed aside.
    Quarantined,
    /// The payload declares a schema newer than this binary supports.
    Incompatible,
}

/// Cloneable flag shared between a store and its readers.
#[derive(Debug, Clone, Default)]
pub struct HealthFlag(Arc<AtomicU8>);

impl HealthFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> StoreHealth {
        match self.0.load(Ordering::SeqCst) {
            1 => StoreHealth::Quarantined,
            2 => StoreHealth::Incompatible,
            _ => StoreHealth::Ok,
        }
    }

    pub fn set(&self, health: StoreHealth) {
        let val = match health {
            StoreHealth::Ok => 0,
            StoreHealth::Quarantined => 1,
            StoreHealth::Incompatible => 2,
        };
        self.0.store(val, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.set(StoreHealth::Ok);
    }

    pub fn is_quarantined(&self) -> bool {
        self.get() == StoreHealth::Quarantined
    }
}

fn unix_now_secs() -> anyhow::Result<u64> {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).context("system time is before UNIX epoch")?;
    Ok(duration.as_secs())
}

pub fn quarantine(path: &Path) -> anyhow::Result<PathBuf> {
    let stamp = unix_now_secs()?;
    quarantine_with_stamp(path, stamp)
}

pub(crate) fn quarantine_with_stamp(path: &Path, mut stamp: u64) -> anyhow::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name =
        path.file_name().and_then(|n| n.to_str()).context("target path has no valid file name")?;

    let mut candidate = parent.join(format!("{file_name}.corrupt.{stamp}"));
    while candidate.exists() {
        stamp = stamp.saturating_add(1);
        candidate = parent.join(format!("{file_name}.corrupt.{stamp}"));
    }

    std::fs::rename(path, &candidate).with_context(|| {
        format!(
            "failed to quarantine corrupt file from {} to {}",
            path.display(),
            candidate.display()
        )
    })?;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "moggyt-sh-{name}-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn quarantine_renames_and_returns_backup() {
        let dir = TempDir::new("rename");
        let file = dir.path().join("store.json");
        let payload = b"corrupt bytes payload";
        std::fs::write(&file, payload).expect("write initial file");

        let backup = quarantine(&file).expect("quarantine succeeds");
        assert!(!file.exists(), "original file must be gone");
        assert!(backup.exists(), "backup file must exist");
        assert_eq!(std::fs::read(&backup).expect("read backup"), payload);
        assert!(
            backup.to_string_lossy().contains(".corrupt."),
            "backup name must contain .corrupt."
        );
    }

    #[test]
    fn quarantine_never_overwrites_existing_backup() {
        let dir = TempDir::new("no-overwrite");
        let file = dir.path().join("store.json");
        let payload1 = b"first payload";
        std::fs::write(&file, payload1).expect("write first file");

        let backup1 = quarantine_with_stamp(&file, 1000).expect("first quarantine");
        assert_eq!(
            backup1.file_name().and_then(|n| n.to_str()).unwrap(),
            "store.json.corrupt.1000"
        );

        let payload2 = b"second payload";
        std::fs::write(&file, payload2).expect("write second file");

        let backup2 = quarantine_with_stamp(&file, 1000).expect("second quarantine");
        assert_eq!(
            backup2.file_name().and_then(|n| n.to_str()).unwrap(),
            "store.json.corrupt.1001"
        );

        assert_ne!(backup1, backup2);
        assert_eq!(std::fs::read(&backup1).expect("read backup1"), payload1);
        assert_eq!(std::fs::read(&backup2).expect("read backup2"), payload2);
    }

    #[test]
    fn health_flag_transitions() {
        let flag = HealthFlag::new();
        assert_eq!(flag.get(), StoreHealth::Ok);
        assert!(!flag.is_quarantined());

        flag.set(StoreHealth::Quarantined);
        assert_eq!(flag.get(), StoreHealth::Quarantined);
        assert!(flag.is_quarantined());

        flag.set(StoreHealth::Incompatible);
        assert_eq!(flag.get(), StoreHealth::Incompatible);
        assert!(!flag.is_quarantined());

        flag.clear();
        assert_eq!(flag.get(), StoreHealth::Ok);
        assert!(!flag.is_quarantined());
    }
}
