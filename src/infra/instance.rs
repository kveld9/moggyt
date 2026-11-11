//! Cross-platform single-instance guard.
//!
//! Enforces a single running instance of moggyt across the operating system
//! using system-level primitives (named mutex on Windows, socket lock on Unix).
//!
//! Stale locks auto-release on process exit via OS kernel primitives, requiring
//! no manual lock files or filesystem cleanup.

#[cfg(not(target_os = "linux"))]
use single_instance::SingleInstance;

/// Cross-platform guard keeping the application single-instance lock held.
///
/// Dropping this guard releases the system-level lock, allowing subsequent
/// attempts to acquire it. Stale locks automatically release on process exit
/// via operating system kernel primitives (named mutex on Windows, socket lock
/// on Unix), requiring no stale lock files or filesystem cleanup.
pub struct SingleInstanceGuard {
    /// OS handle held for the lifetime of the process; released when dropped.
    _inner: platform::Guard,
}

impl SingleInstanceGuard {
    /// Attempts to acquire the system-wide single instance guard.
    #[must_use]
    pub fn try_acquire() -> Option<Self> {
        try_acquire()
    }

    #[cfg(test)]
    fn try_acquire_named(name: &str) -> Option<Self> {
        platform::try_acquire(name).ok().flatten().map(|_inner| Self { _inner })
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
    platform::try_acquire(instance_lock_name())
        .ok()
        .flatten()
        .map(|_inner| SingleInstanceGuard { _inner })
}

#[cfg(target_os = "linux")]
mod platform {
    //! Abstract Unix socket bound close-on-exec.

    use std::io;
    use std::os::unix::io::RawFd;

    pub struct Guard {
        fd: RawFd,
    }

    pub fn try_acquire(name: &str) -> io::Result<Option<Guard>> {
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        match bind_abstract(fd, name) {
            Ok(()) => Ok(Some(Guard { fd })),
            Err(err) if err.raw_os_error() == Some(libc::EADDRINUSE) => {
                unsafe { libc::close(fd) };
                Ok(None)
            }
            Err(err) => {
                unsafe { libc::close(fd) };
                Err(err)
            }
        }
    }

    fn bind_abstract(fd: RawFd, name: &str) -> io::Result<()> {
        let bytes = name.as_bytes();
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        if bytes.len() + 1 > addr.sun_path.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "instance lock name too long"));
        }
        for (slot, byte) in addr.sun_path.iter_mut().skip(1).zip(bytes.iter()) {
            *slot = *byte as libc::c_char;
        }
        let offset = std::mem::size_of::<libc::sockaddr_un>() - addr.sun_path.len();
        debug_assert_eq!(offset, 2);
        let len = offset + 1 + bytes.len();
        let rc = unsafe {
            libc::bind(
                fd,
                &addr as *const libc::sockaddr_un as *const libc::sockaddr,
                len as libc::socklen_t,
            )
        };
        if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    impl Guard {
        #[cfg(test)]
        pub(super) fn raw_fd(&self) -> RawFd {
            self.fd
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    //! Wrapper over the `single-instance` crate for non-Linux platforms.

    use std::io;

    pub struct Guard {
        _inner: super::SingleInstance,
    }

    pub fn try_acquire(name: &str) -> io::Result<Option<Guard>> {
        match super::SingleInstance::new(name) {
            Ok(inner) if inner.is_single() => Ok(Some(Guard { _inner: inner })),
            Ok(_) => Ok(None),
            Err(err) => Err(io::Error::other(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::id;

    #[test]
    fn lock_name_matches_expected_identifier() {
        assert_eq!(instance_lock_name(), "moggyt-single-instance");
    }

    #[test]
    fn single_instance_guard_lifecycle_acquire_conflict_and_reacquire() {
        // Usamos un nombre de lock único por proceso de test para no colisionar
        // con otras instancias de moggyt (u otros procesos) que puedan tener el
        // lock del SO tomado bajo el nombre por defecto.
        let lock_name = format!("moggyt-single-instance-test-{}", id());

        let first = SingleInstanceGuard::try_acquire_named(&lock_name);
        assert!(first.is_some(), "first acquisition must succeed");

        let second = SingleInstanceGuard::try_acquire_named(&lock_name);
        assert!(second.is_none(), "second acquisition while first guard is alive must fail");

        drop(first);

        let third = SingleInstanceGuard::try_acquire_named(&lock_name);
        assert!(third.is_some(), "re-acquisition after dropping guard must succeed");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn instance_socket_is_close_on_exec() {
        let lock_name = format!("moggyt-single-instance-cloexec-{}", id());
        let guard =
            platform::try_acquire(&lock_name).expect("acquisition succeeds").expect("lock is free");
        let fd = guard.raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "fcntl must succeed");
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "the lock socket must not leak into children");
    }
}
