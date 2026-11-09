//! Shared atomic replacement for disk-backed stores.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;

/// Maximum duration a temporary write file can linger before being swept.
pub const TEMP_FILE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn resolve_target_parts(target: &Path) -> anyhow::Result<(&Path, &str)> {
    let parent = target.parent().ok_or_else(|| {
        anyhow::anyhow!("target path '{}' has no parent directory", target.display())
    })?;
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("target path '{}' has no valid file name", target.display()))?;
    Ok((parent, file_name))
}

fn build_temp_path(parent: &Path, file_name: &str) -> PathBuf {
    let pid = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{file_name}.tmp.{pid}.{counter}"))
}

fn write_temp_file(tmp: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(tmp)
        .with_context(|| format!("failed to create temporary file {}", tmp.display()))?;
    file.write_all(data)
        .with_context(|| format!("failed to write temporary file {}", tmp.display()))?;
    file.sync_all().with_context(|| format!("failed to sync temporary file {}", tmp.display()))?;
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::File::open(dir)
        .with_context(|| format!("failed to open directory {}", dir.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync directory {}", dir.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Atomically writes data to `target` using a unique temporary file, fsync, and atomic rename.
pub fn write_atomic(target: &Path, data: &[u8]) -> anyhow::Result<()> {
    let (parent, file_name) = resolve_target_parts(target)?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory {}", parent.display()))?;

    let tmp = build_temp_path(parent, file_name);
    write_temp_file(&tmp, data)?;
    std::fs::rename(&tmp, target)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), target.display()))?;
    sync_dir(parent)?;
    Ok(())
}

fn is_temp_file_name(entry: &std::fs::DirEntry) -> bool {
    entry.file_name().to_str().is_some_and(|name| name.contains(".tmp."))
}

fn is_file_expired(
    entry: &std::fs::DirEntry,
    now: std::time::SystemTime,
    max_age: Duration,
) -> bool {
    let Ok(metadata) = entry.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    metadata
        .modified()
        .ok()
        .and_then(|mtime| now.duration_since(mtime).ok())
        .is_some_and(|age| age > max_age)
}

fn should_sweep_entry(
    entry: &std::fs::DirEntry,
    now: std::time::SystemTime,
    max_age: Duration,
) -> bool {
    is_temp_file_name(entry) && is_file_expired(entry, now, max_age)
}

fn sweep_entry(
    entry: &std::fs::DirEntry,
    now: std::time::SystemTime,
    max_age: Duration,
) -> std::io::Result<bool> {
    if !should_sweep_entry(entry, now, max_age) {
        return Ok(false);
    }
    match std::fs::remove_file(entry.path()) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Sweeps and removes temporary files in `dir` that exceed `max_age`.
pub fn sweep_stale_temp_files(dir: &Path, max_age: Duration) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };

    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        if sweep_entry(&entry, now, max_age)? {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = std::process::id();
            let count = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("moggyt-{name}-{id}-{count}"));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
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
    fn temp_file_removed_after_success() {
        let temp = TempDir::new("atomic-success");
        let target = temp.path().join("target.json");
        let data = b"{\"hello\": \"world\"}";

        write_atomic(&target, data).expect("write_atomic succeeds");

        let read_back = std::fs::read(&target).expect("read target file");
        assert_eq!(read_back, data);

        let mut tmp_count = 0;
        for entry in std::fs::read_dir(temp.path()).expect("read_dir") {
            let entry = entry.expect("dir entry");
            if entry.file_name().to_string_lossy().contains(".tmp.") {
                tmp_count += 1;
            }
        }
        assert_eq!(tmp_count, 0, "no temporary file should remain");
    }

    #[test]
    fn concurrent_writes_use_distinct_temp_names() {
        use std::sync::Arc;

        let temp = TempDir::new("atomic-concurrent");
        let target = Arc::new(temp.path().join("shared.json"));

        let mut handles = Vec::new();
        for i in 0..8 {
            let target_clone = Arc::clone(&target);
            handles.push(std::thread::spawn(move || {
                let payload = format!("{{\"thread\": {i}}}");
                write_atomic(&target_clone, payload.as_bytes())
            }));
        }

        for handle in handles {
            let result = handle.join().expect("thread join succeeded");
            assert!(result.is_ok(), "write_atomic should succeed");
        }

        assert!(target.exists(), "target file should exist");
        let mut tmp_count = 0;
        for entry in std::fs::read_dir(temp.path()).expect("read_dir") {
            let entry = entry.expect("dir entry");
            if entry.file_name().to_string_lossy().contains(".tmp.") {
                tmp_count += 1;
            }
        }
        assert_eq!(tmp_count, 0, "no temporary file should remain after concurrent writes");
    }

    #[test]
    fn sweep_removes_stale_temp_files() {
        let temp = TempDir::new("atomic-sweep");
        let dir = temp.path();

        let stale_path = dir.join("x.tmp.1.1");
        let fresh_path = dir.join("y.tmp.2.2");
        let keep_path = dir.join("keep.json");

        std::fs::write(&stale_path, b"stale").expect("write stale file");
        std::fs::write(&fresh_path, b"fresh").expect("write fresh file");
        std::fs::write(&keep_path, b"keep").expect("write keep file");

        let two_hours_ago = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(7200))
            .expect("subtract 2h");

        std::fs::File::options()
            .write(true)
            .open(&stale_path)
            .expect("open stale file for set_modified")
            .set_modified(two_hours_ago)
            .expect("set_modified on stale file");

        let removed = sweep_stale_temp_files(dir, TEMP_FILE_MAX_AGE).expect("sweep succeeds");
        assert_eq!(removed, 1);

        assert!(!stale_path.exists(), "stale temp file should be removed");
        assert!(fresh_path.exists(), "fresh temp file should remain");
        assert!(keep_path.exists(), "unrelated file should remain");
    }
}
