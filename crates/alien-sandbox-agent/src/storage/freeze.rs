//! A brief freeze of the guest cgroup, so a commit copies a point-in-time state of the storage
//! directory (a SQLite database and its WAL from the same instant, git refs and objects from the
//! same instant). Never SIGSTOP: a stopped process is visible to its shell and parent, a frozen
//! cgroup is not.
//!
//! The thaw is unconditional: [`Frozen`] thaws when dropped, whatever the copy did, and a freeze
//! that does not settle within its bound is thawed before the error is returned.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The cgroup Alien's guest isolation puts every command and the image command in.
pub const GUEST_CGROUP: &str = "/sys/fs/cgroup/alien-guest";

/// How long the kernel may take to report the cgroup frozen.
pub const FREEZE_BOUND: Duration = Duration::from_secs(2);

/// A cgroup v2 directory that can be frozen.
#[derive(Debug, Clone)]
pub struct Freezer {
    cgroup: PathBuf,
}

/// The guest while frozen; thawed on drop.
#[derive(Debug)]
pub struct Frozen<'a> {
    freezer: &'a Freezer,
    thawed: bool,
}

impl Freezer {
    /// The guest cgroup, if this platform has one with a freezer.
    pub fn guest() -> Option<Self> {
        Self::at(Path::new(GUEST_CGROUP))
    }

    /// A cgroup at `path`, if it has `cgroup.freeze`.
    pub fn at(path: &Path) -> Option<Self> {
        path.join("cgroup.freeze").exists().then(|| Self {
            cgroup: path.to_path_buf(),
        })
    }

    fn write(&self, value: &str) -> io::Result<()> {
        std::fs::write(self.cgroup.join("cgroup.freeze"), value)
    }

    fn frozen(&self) -> io::Result<bool> {
        let events = std::fs::read_to_string(self.cgroup.join("cgroup.events"))?;
        Ok(events.lines().any(|line| line.trim() == "frozen 1"))
    }

    /// Freezes and waits until every process in the cgroup is frozen, or thaws and fails.
    pub fn freeze(&self, bound: Duration) -> io::Result<Frozen<'_>> {
        let guard = Frozen {
            freezer: self,
            thawed: false,
        };
        self.write("1")?;
        let deadline = Instant::now() + bound;
        loop {
            if self.frozen()? {
                return Ok(guard);
            }
            if Instant::now() >= deadline {
                // `guard` thaws as it drops.
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("the guest did not freeze within {bound:?}"),
                ));
            }
            std::thread::sleep(Duration::from_micros(200));
        }
    }
}

impl Frozen<'_> {
    /// Thaws now and reports whether the write worked.
    pub fn thaw(mut self) -> io::Result<()> {
        self.thawed = true;
        self.freezer.write("0")
    }
}

impl Drop for Frozen<'_> {
    fn drop(&mut self) {
        if !self.thawed {
            if let Err(error) = self.freezer.write("0") {
                tracing::error!(%error, "could not thaw the guest cgroup");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake cgroup directory: the kernel's files are plain files here.
    fn fake(frozen_reported: bool) -> (tempfile::TempDir, Freezer) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("cgroup.freeze"), "0").expect("write");
        std::fs::write(
            dir.path().join("cgroup.events"),
            if frozen_reported {
                "populated 1\nfrozen 1\n"
            } else {
                "populated 1\nfrozen 0\n"
            },
        )
        .expect("write");
        let freezer = Freezer::at(dir.path()).expect("has cgroup.freeze");
        (dir, freezer)
    }

    fn state(dir: &tempfile::TempDir) -> String {
        std::fs::read_to_string(dir.path().join("cgroup.freeze")).expect("read")
    }

    #[test]
    fn a_freeze_that_never_settles_is_thawed_before_the_error() {
        let (dir, freezer) = fake(false);
        let error = freezer
            .freeze(Duration::from_millis(50))
            .expect_err("never reported frozen");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(state(&dir), "0");
    }

    #[test]
    fn a_failing_copy_still_thaws() {
        let (dir, freezer) = fake(true);
        let copy = || -> io::Result<()> {
            let _frozen = freezer.freeze(FREEZE_BOUND)?;
            assert_eq!(state(&dir), "1");
            Err(io::Error::new(io::ErrorKind::Other, "copy failed"))
        };
        copy().expect_err("the copy fails");
        assert_eq!(state(&dir), "0");
    }

    #[test]
    fn a_panicking_copy_still_thaws() {
        let (dir, freezer) = fake(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _frozen = freezer.freeze(FREEZE_BOUND).expect("frozen");
            panic!("copy panicked");
        }));
        assert!(result.is_err());
        assert_eq!(state(&dir), "0");
    }

    #[test]
    fn no_freezer_without_cgroup_freeze() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(Freezer::at(dir.path()).is_none());
    }
}
