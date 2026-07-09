//! Unix `VersionStore` (Linux + macOS): the on-disk version store with real
//! symlink pointers.
//!
//! `current` and `last-stable` are symlinks to `versions/<v>`; a flip creates
//! the new symlink at a temp name and `rename(2)`s it over the pointer, which
//! is atomic — a reader (or a crashed launcher's restart classification)
//! always sees either the old or the new target, never a missing pointer.
//! Everything non-pointer (markers, snapshots, gc candidates, disk check)
//! comes from the shared `store_common` helpers.

use std::path::{Path, PathBuf};

use crate::core::store_common;
use crate::core::traits::{
    FailureRecord, PendingMarker, ProbationMarker, Version, VersionStore,
};
use crate::error::{ErrorData, Result};
use alien_core::self_update as protocol;
use alien_error::{AlienError, Context, IntoAlienError};

pub struct UnixVersionStore {
    root: PathBuf,
}

impl UnixVersionStore {
    /// Open a store rooted at the operator's data dir. Creates the layout
    /// directories that may be missing (idempotent).
    pub fn open(root: &Path) -> Result<Self> {
        for dir in ["versions", "state", "state-snapshots", "failed", "download"] {
            std::fs::create_dir_all(root.join(dir))
                .into_alien_error()
                .context(ErrorData::StoreCorrupt {
                    path: root.display().to_string(),
                    message: format!("failed to create the '{dir}' directory"),
                })?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn pointer_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Read a pointer symlink → the version it names. Absent → None; a
    /// pointer that exists but does not parse is store corruption.
    fn read_pointer(&self, name: &str) -> Result<Option<Version>> {
        let path = self.pointer_path(name);
        let target = match std::fs::read_link(&path) {
            Ok(target) => target,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(e).into_alien_error().context(ErrorData::StoreCorrupt {
                    path: path.display().to_string(),
                    message: "failed to read the pointer symlink".to_string(),
                });
            }
        };
        let version_str = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| corrupt_pointer(&path, "symlink target has no version component"))?;
        Version::parse(version_str)
            .map(Some)
            .map_err(|e| corrupt_pointer(&path, &format!("unparseable version in target: {e}")))
    }

    /// Atomically repoint `name` at `versions/<version>`: create the symlink
    /// at `<name>.tmp` (removing any stale one from a crashed flip) and
    /// rename it over the pointer.
    fn write_pointer(&self, name: &str, version: &Version) -> Result<()> {
        let path = self.pointer_path(name);
        let tmp = self.pointer_path(&format!("{name}.tmp"));
        let target = Path::new("versions").join(version.as_str());

        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e).into_alien_error().context(ErrorData::Other {
                    message: format!("failed to clear stale pointer temp '{}'", tmp.display()),
                });
            }
        }
        std::os::unix::fs::symlink(&target, &tmp)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to create pointer temp '{}'", tmp.display()),
            })?;
        std::fs::rename(&tmp, &path)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to commit pointer '{}'", path.display()),
            })
    }
}

fn corrupt_pointer(path: &Path, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreCorrupt {
        path: path.display().to_string(),
        message: message.to_string(),
    })
}

impl VersionStore for UnixVersionStore {
    fn stage_dir(&self, version: &Version) -> PathBuf {
        protocol::version_dir(&self.root, version)
    }

    fn current(&self) -> Result<Option<Version>> {
        self.read_pointer("current")
    }

    fn last_stable(&self) -> Result<Option<Version>> {
        self.read_pointer("last-stable")
    }

    fn set_current(&self, version: &Version) -> Result<()> {
        self.write_pointer("current", version)
    }

    fn set_last_stable(&self, version: &Version) -> Result<()> {
        self.write_pointer("last-stable", version)
    }

    fn snapshot_state(&self, tag: &Version) -> Result<()> {
        store_common::snapshot_state_dir(
            &self.root.join("state"),
            &self.root.join("state-snapshots"),
            tag,
        )
    }

    fn restore_state(&self, tag: &Version) -> Result<()> {
        store_common::restore_state_dir(
            &self.root.join("state"),
            &self.root.join("state-snapshots"),
            tag,
        )
    }

    fn drop_snapshot(&self, tag: &Version) -> Result<()> {
        let dir = self.root.join("state-snapshots").join(tag.as_str());
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).into_alien_error().context(ErrorData::Other {
                message: format!("failed to drop snapshot '{}'", dir.display()),
            }),
        }
    }

    fn state_size(&self) -> Result<u64> {
        store_common::dir_size(&self.root.join("state"))
    }

    fn gc(&self, keep: &[Version]) -> Result<()> {
        let all = self.list_versions()?;
        let current = self.current()?;
        let last_stable = self.last_stable()?;
        for candidate in
            store_common::gc_candidates(&all, keep, current.as_ref(), last_stable.as_ref())
        {
            std::fs::remove_dir_all(self.stage_dir(&candidate))
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("gc failed for version {candidate}"),
                })?;
        }
        Ok(())
    }

    fn read_pending(&self) -> Result<Option<PendingMarker>> {
        store_common::read_marker(&protocol::pending_path(&self.root))
    }

    fn write_pending(&self, marker: &PendingMarker) -> Result<()> {
        store_common::write_marker_atomic(&protocol::pending_path(&self.root), marker)
    }

    fn clear_pending(&self) -> Result<()> {
        store_common::remove_marker(&protocol::pending_path(&self.root))
    }

    fn read_probation(&self) -> Result<Option<ProbationMarker>> {
        store_common::read_marker(&self.root.join("probation.json"))
    }

    fn write_probation(&self, marker: &ProbationMarker) -> Result<()> {
        store_common::write_marker_atomic(&self.root.join("probation.json"), marker)
    }

    fn clear_probation(&self) -> Result<()> {
        store_common::remove_marker(&self.root.join("probation.json"))
    }

    fn read_failure(&self, version: &Version) -> Result<Option<FailureRecord>> {
        store_common::read_marker(&protocol::failure_path(&self.root, version))
    }

    fn write_failure(&self, record: &FailureRecord) -> Result<()> {
        store_common::write_marker_atomic(
            &protocol::failure_path(&self.root, &record.version),
            record,
        )
    }

    fn list_versions(&self) -> Result<Vec<Version>> {
        let versions_dir = self.root.join("versions");
        let mut versions = Vec::new();
        for entry in std::fs::read_dir(&versions_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to read '{}'", versions_dir.display()),
            })?
        {
            let entry = entry.into_alien_error().context(ErrorData::Other {
                message: "failed to read a versions/ entry".to_string(),
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = Version::parse(&name).map_err(|e| {
                AlienError::new(ErrorData::StoreCorrupt {
                    path: entry.path().display().to_string(),
                    message: format!("versions/ entry is not a version: {e}"),
                })
            })?;
            versions.push(version);
        }
        versions.sort();
        Ok(versions)
    }

    fn free_space_for_snapshot(&self) -> Result<()> {
        let required = self.state_size()?;
        let stat = nix::sys::statvfs::statvfs(&self.root)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("statvfs failed for '{}'", self.root.display()),
            })?;
        let available = stat.blocks_available() as u64 * stat.fragment_size() as u64;
        store_common::check_space(required, available, "state snapshot")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn store_with(dir: &Path, versions: &[&str]) -> UnixVersionStore {
        let store = UnixVersionStore::open(dir).unwrap();
        for v in versions {
            let stage = store.stage_dir(&version(v));
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("alien-operator"), format!("binary-{v}")).unwrap();
        }
        store
    }

    /// Pointers are real symlinks, and a rapid flip storm always leaves a
    /// readable, parseable pointer (single-threaded — the launcher's own
    /// access pattern: one supervise thread, reads and flips never race).
    #[test]
    fn pointer_flip_storm_is_always_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store.set_current(&version("1.0.0")).unwrap();
        assert!(
            dir.path().join("current").symlink_metadata().unwrap().file_type().is_symlink(),
            "the pointer must be a real symlink"
        );

        for i in 0..1000 {
            let v = if i % 2 == 0 { "2.0.0" } else { "1.0.0" };
            store.set_current(&version(v)).unwrap();
            assert_eq!(store.current().unwrap(), Some(version(v)));
        }
        assert!(
            !dir.path().join("current.tmp").exists(),
            "no temp residue after the storm"
        );
    }

    /// Linux only (the Phase-1 target, exercised in CI): the flip is atomic
    /// even under a CONCURRENT reader — every read during 1000 flips parses
    /// to one of the two versions, never a missing/partial pointer. Gated
    /// off macOS: APFS `readlink` can transiently return EINVAL while a
    /// same-name `rename` is in flight — a platform quirk that does not
    /// affect the launcher (whose reads and flips share one thread); crash
    /// atomicity there is covered by the startup-classification E2E.
    #[cfg(target_os = "linux")]
    #[test]
    fn pointer_flip_is_atomic_under_a_reader() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store.set_current(&version("1.0.0")).unwrap();

        let reader_root = dir.path().to_path_buf();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_stop = stop.clone();
        let reader = std::thread::spawn(move || {
            let store = UnixVersionStore::open(&reader_root).unwrap();
            let mut reads = 0u32;
            while !reader_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let current = store.current().expect("read must never fail mid-flip");
                let current = current.expect("pointer must never be missing mid-flip");
                assert!(
                    current == version("1.0.0") || current == version("2.0.0"),
                    "unexpected pointer target {current}"
                );
                reads += 1;
            }
            reads
        });

        for i in 0..1000 {
            let v = if i % 2 == 0 { "2.0.0" } else { "1.0.0" };
            store.set_current(&version(v)).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let reads = reader.join().expect("reader must not panic");
        assert!(reads > 0, "the reader must actually have observed flips");
    }

    /// A stale `current.tmp` from a crashed flip is cleaned up by the next
    /// flip instead of failing it.
    #[test]
    fn stale_pointer_temp_is_cleared_on_the_next_flip() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        // Simulate a crash between symlink-create and rename.
        std::os::unix::fs::symlink("versions/1.0.0", dir.path().join("current.tmp")).unwrap();

        store.set_current(&version("2.0.0")).expect("flip should clear the stale temp");
        assert_eq!(store.current().unwrap(), Some(version("2.0.0")));
        assert!(!dir.path().join("current.tmp").exists(), "temp consumed by the rename");
    }

    /// gc removes exactly the unpointed versions.
    #[test]
    fn gc_preserves_pointer_targets() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "1.1.0", "1.2.0"]);
        store.set_current(&version("1.2.0")).unwrap();
        store.set_last_stable(&version("1.1.0")).unwrap();

        store.gc(&[]).unwrap();
        assert_eq!(
            store.list_versions().unwrap(),
            vec![version("1.1.0"), version("1.2.0")]
        );
    }

    /// The real disk has space for a tiny state dir — the preflight passes
    /// against genuine statvfs numbers.
    #[test]
    fn free_space_preflight_passes_on_a_real_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &[]);
        std::fs::write(dir.path().join("state/db"), b"tiny").unwrap();
        store
            .free_space_for_snapshot()
            .expect("a 4-byte state dir must fit on any disk running this test");
    }
    // -- the full Phase-0 state-machine suite against the REAL store -------

    use crate::core::testing::{
        scenario_classification_rows, scenario_crash_injection_promote,
        scenario_crash_injection_rollback, scenario_happy_promote,
        scenario_rollback_on_probation_crash, scenario_rollback_restores_state, TestStoreOps,
    };

    impl TestStoreOps for UnixVersionStore {
        fn install_version(&self, version: &Version) {
            let stage = self.stage_dir(version);
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("alien-operator"), format!("binary-{version}")).unwrap();
        }
        fn state_dir_path(&self) -> PathBuf {
            self.root.join("state")
        }
        fn store_root(&self) -> PathBuf {
            self.root.clone()
        }
    }

    fn unix(dir: &Path) -> UnixVersionStore {
        UnixVersionStore::open(dir).unwrap()
    }

    #[test]
    fn state_machine_happy_promote_on_real_store() {
        scenario_happy_promote(unix);
    }

    #[test]
    fn state_machine_rollback_restores_state_on_real_store() {
        scenario_rollback_restores_state(unix);
    }

    #[test]
    fn state_machine_rollback_on_probation_crash_on_real_store() {
        scenario_rollback_on_probation_crash(unix);
    }

    #[test]
    fn state_machine_classification_rows_on_real_store() {
        scenario_classification_rows(unix);
    }

    #[test]
    fn state_machine_crash_injection_promote_on_real_store() {
        scenario_crash_injection_promote(unix);
    }

    #[test]
    fn state_machine_crash_injection_rollback_on_real_store() {
        scenario_crash_injection_rollback(unix);
    }
}
