//! Single-instance file lock.
//!
//! Prevents multiple alien-operator instances from running concurrently on the
//! same machine (or against the same data directory). Uses `flock` on Unix
//! and `fs2::FileExt` on Windows.

use std::fs::{self, File};
use std::path::Path;
use std::time::Duration;

use tokio::time::{sleep, Instant};
use tracing::info;

/// A held file lock that is released on drop.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Wait for a terminating Operator to release its state, up to `timeout`.
    pub async fn acquire_with_timeout(data_dir: &Path, timeout: Duration) -> std::io::Result<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            match Self::acquire(data_dir) {
                Ok(lock) => return Ok(lock),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(error);
                    }
                    info!(path = %data_dir.display(), "Waiting for the previous Operator to release its state lock");
                    sleep(remaining.min(Duration::from_secs(1))).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Try to acquire an exclusive lock on `<data_dir>/operator.lock`.
    ///
    /// Returns `Ok(lock)` if we are the only running instance, or an error
    /// if another instance already holds the lock.
    pub fn acquire(data_dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;

        let path = data_dir.join("operator.lock");
        let file = File::create(&path)?;

        try_lock_exclusive(&file, &path)?;

        Ok(Self { _file: file })
    }
}

// Keep the lock file: removing it lets a new process lock a different inode
// while another waiter still holds the original file. Closing the descriptor
// releases the lock without changing its identity.

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
                std::io::ErrorKind::WouldBlock,
                format!(
                    "Another operator instance is already running (lock: {})",
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
        if e.kind() == std::io::ErrorKind::WouldBlock || e.raw_os_error() == Some(33)
        /* ERROR_LOCK_VIOLATION */
        {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!(
                    "Another operator instance is already running (lock: {})",
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
            "alien-operator-lock-test-{}-{}",
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
        assert!(dir.join("operator.lock").exists());
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
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);

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
    #[tokio::test]
    async fn waits_for_previous_instance_to_exit() {
        let dir = tempfile::tempdir().unwrap();
        let held = InstanceLock::acquire(dir.path()).unwrap();
        let waiter =
            InstanceLock::acquire_with_timeout(dir.path(), std::time::Duration::from_secs(2));
        let release = async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            drop(held);
        };
        let (result, ()) = tokio::join!(waiter, release);
        let next = result.expect("replacement waits for previous instance");
        assert_eq!(
            InstanceLock::acquire(dir.path()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(next);
        InstanceLock::acquire(dir.path()).expect("lock available after replacement exits");
    }

    #[tokio::test]
    async fn contention_times_out_but_io_errors_do_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        let _held = InstanceLock::acquire(dir.path()).unwrap();
        let started = tokio::time::Instant::now();
        let timeout = std::time::Duration::from_millis(80);
        let error = InstanceLock::acquire_with_timeout(dir.path(), timeout)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert!(started.elapsed() >= timeout);
        let file = dir.path().join("file");
        std::fs::write(&file, "not a directory").unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            InstanceLock::acquire_with_timeout(&file, std::time::Duration::from_secs(30)),
        )
        .await;
        assert!(result.expect("I/O failure returns immediately").is_err());
    }
}
