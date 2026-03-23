//! Single-instance file lock.
//!
//! Prevents multiple alien-agent instances from running concurrently on the
//! same machine (or against the same data directory). Uses `flock` on Unix
//! and `fs2::FileExt` on Windows.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// A held file lock that is released on drop.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Try to acquire an exclusive lock on `<data_dir>/agent.lock`.
    ///
    /// Returns `Ok(lock)` if we are the only running instance, or an error
    /// if another instance already holds the lock.
    pub fn acquire(data_dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;

        let path = data_dir.join("agent.lock");
        let file = File::create(&path)?;

        try_lock_exclusive(&file, &path)?;

        Ok(Self { _file: file, path })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // Lock is released when the file descriptor is closed.
        // Optionally remove the lock file.
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File, path: &Path) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "Another alien-agent instance is already running (lock: {})",
                    path.display()
                ),
            ))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to acquire lock {}: {}", path.display(), err),
            ))
        }
    }
}

#[cfg(windows)]
fn try_lock_exclusive(file: &File, path: &Path) -> std::io::Result<()> {
    use fs2::FileExt;

    file.try_lock_exclusive().map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock
            || e.raw_os_error() == Some(33) /* ERROR_LOCK_VIOLATION */
        {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "Another alien-agent instance is already running (lock: {})",
                    path.display()
                ),
            )
        } else {
            e
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_data_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "alien-agent-lock-test-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_acquire_lock_succeeds() {
        let dir = temp_data_dir();
        let lock = InstanceLock::acquire(&dir).expect("should acquire lock");
        assert!(dir.join("agent.lock").exists());
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_second_lock_fails() {
        let dir = temp_data_dir();
        let _lock1 = InstanceLock::acquire(&dir).expect("first lock should succeed");

        let result = InstanceLock::acquire(&dir);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        drop(_lock1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_lock_released_on_drop() {
        let dir = temp_data_dir();

        {
            let _lock = InstanceLock::acquire(&dir).expect("should acquire lock");
            // lock held here
        }
        // lock dropped — should be able to acquire again

        let _lock2 = InstanceLock::acquire(&dir).expect("should acquire lock after drop");
        drop(_lock2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_creates_data_dir() {
        let dir = temp_data_dir();
        assert!(!dir.exists());

        let lock = InstanceLock::acquire(&dir).expect("should create dir and acquire lock");
        assert!(dir.exists());

        drop(lock);
        let _ = fs::remove_dir_all(&dir);
    }
}
