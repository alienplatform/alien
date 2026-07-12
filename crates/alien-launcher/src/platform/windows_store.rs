//! Windows `VersionStore`: the on-disk version store with directory-junction
//! pointers (`junction` crate) instead of Unix symlinks.
//!
//! Unlike a Unix `rename(2)` over a symlink, a junction flip is **not atomic** —
//! Windows cannot rename over an existing directory, so a flip must
//! `create <name>.new` → `remove <name>` → `rename <name>.new` → `<name>`. A
//! crash can therefore leave residue, which `current()` reconciles on the next
//! start (the launcher is single-threaded, so this is only ever a *restart*
//! concern, never a live race):
//!
//! - `<name>.new` **and** `<name>` present (crashed after create, before remove)
//!   → finish the flip (remove old, rename new).
//! - `<name>` missing, `<name>.new` present (crashed mid-rename) → finish the
//!   rename.
//! - `current` missing with no `.new` → reconstruct from `probation.json` (the
//!   new version if probation is active, else `last-stable`) and log loudly.
//!
//! Everything non-pointer (markers, snapshots, gc candidates) comes from the
//! shared `store_common` helpers, identical to the Unix store. `gc` additionally
//! tolerates a locked binary (a just-swapped-away operator `.exe` still mapped):
//! it schedules the straggler for deletion on the next reboot rather than failing.
//!
//! Constructed by `main.rs`'s Windows `run_supervisor` (aliased
//! `ActiveVersionStore`) and driven through `core::run`.

use std::path::{Path, PathBuf};

use alien_core::self_update as protocol;
use alien_error::{AlienError, Context, IntoAlienError};
use tracing::warn;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT,
};

use crate::core::store_common;
use crate::core::traits::{FailureRecord, PendingMarker, ProbationMarker, Version, VersionStore};
use crate::error::{ErrorData, Result};

pub struct WindowsVersionStore {
    root: PathBuf,
}

impl WindowsVersionStore {
    /// Open a store rooted at the operator's data dir. Creates the layout
    /// directories that may be missing (idempotent). Identical to the Unix store.
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

    /// The absolute junction target for a version. Junctions (unlike symlinks)
    /// require an absolute target, so this is `<root>/versions/<v>`.
    fn version_target(&self, version: &Version) -> PathBuf {
        protocol::version_dir(&self.root, version)
    }

    /// Read a pointer junction → the version it names. Absent → None; a pointer
    /// that exists but does not resolve/parse is store corruption.
    fn read_pointer(&self, name: &str) -> Result<Option<Version>> {
        let path = self.pointer_path(name);
        let target = match junction::get_target(&path) {
            Ok(target) => target,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(e).into_alien_error().context(ErrorData::StoreCorrupt {
                    path: path.display().to_string(),
                    message: "failed to read the pointer junction".to_string(),
                });
            }
        };
        let version_str = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| corrupt_pointer(&path, "junction target has no version component"))?;
        Version::parse(version_str)
            .map(Some)
            .map_err(|e| corrupt_pointer(&path, &format!("unparseable version in target: {e}")))
    }

    /// Repoint `name` at `versions/<version>` via the non-atomic junction flip:
    /// clear any stale `.new`, create the new junction, remove the old pointer
    /// (Windows can't rename over it), then rename the new one into place.
    fn write_pointer(&self, name: &str, version: &Version) -> Result<()> {
        let path = self.pointer_path(name);
        let staged = self.pointer_path(&format!("{name}.new"));
        let target = self.version_target(version);

        remove_junction_if_present(&staged)?;
        junction::create(&target, &staged)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to create pointer temp '{}'", staged.display()),
            })?;
        remove_junction_if_present(&path)?;
        std::fs::rename(&staged, &path)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to commit pointer '{}'", path.display()),
            })
    }

    /// Finish an interrupted flip: if `<name>.new` survived a crash, complete it
    /// (remove any leftover `<name>`, rename `<name>.new` into place). Covers the
    /// "both present" and "mid-rename" residues; a no-op when there is no `.new`.
    fn reconcile_flip(&self, name: &str) -> Result<()> {
        let path = self.pointer_path(name);
        let staged = self.pointer_path(&format!("{name}.new"));
        if !exists_no_follow(&staged) {
            return Ok(());
        }
        warn!(
            pointer = name,
            "found a staged '.new' pointer from an interrupted flip; completing it"
        );
        remove_junction_if_present(&path)?;
        std::fs::rename(&staged, &path)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to complete an interrupted flip for '{}'", path.display()),
            })
    }

    /// `current` is gone with no `.new` residue: rebuild it from the protocol
    /// markers. An active probation means the new version was live; otherwise
    /// fall back to `last-stable`. A truly fresh store (never installed) has
    /// neither — that legitimately stays `None`.
    fn reconstruct_current(&self) -> Result<Option<Version>> {
        let (target, source) = match self.read_probation()? {
            Some(probation) => (probation.new, "probation.new"),
            None => match self.last_stable()? {
                Some(version) => (version, "last-stable"),
                None => return Ok(None),
            },
        };
        warn!(
            version = %target,
            source,
            "current pointer is missing; reconstructing it from the recovery marker"
        );
        self.write_pointer("current", &target)?;
        Ok(Some(target))
    }
}

fn corrupt_pointer(path: &Path, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreCorrupt {
        path: path.display().to_string(),
        message: message.to_string(),
    })
}

/// Presence check that does NOT follow the reparse point (so a junction counts
/// as present regardless of whether its target exists).
fn exists_no_follow(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Remove a junction (or plain empty dir) if present. `remove_dir` on a junction
/// deletes the reparse point itself, never the target's contents.
fn remove_junction_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).into_alien_error().context(ErrorData::Other {
            message: format!("failed to remove pointer junction '{}'", path.display()),
        }),
    }
}

impl VersionStore for WindowsVersionStore {
    fn stage_dir(&self, version: &Version) -> PathBuf {
        protocol::version_dir(&self.root, version)
    }

    fn current(&self) -> Result<Option<Version>> {
        self.reconcile_flip("current")?;
        match self.read_pointer("current")? {
            Some(version) => Ok(Some(version)),
            None => self.reconstruct_current(),
        }
    }

    fn last_stable(&self) -> Result<Option<Version>> {
        self.reconcile_flip("last-stable")?;
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
            let dir = self.stage_dir(&candidate);
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    // A version's binary can still be mapped — a just-swapped-away
                    // operator `.exe` — and Windows refuses to delete an in-use
                    // file. Never fail gc for that: schedule the straggler for
                    // deletion on the next reboot (best-effort) and move on.
                    warn!(
                        version = %candidate,
                        error = %e,
                        "gc could not remove a version dir (likely a locked binary); scheduling reboot-delete"
                    );
                    schedule_delete_on_reboot(&dir);
                }
            }
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
        let available = free_bytes(&self.root)?;
        store_common::check_space(required, available, "state snapshot")
    }
}

/// Free bytes available to the caller on the volume holding `path`.
fn free_bytes(path: &Path) -> Result<u64> {
    let wide = to_wide_null(path);
    let mut free_available: u64 = 0;
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 path; we pass a valid
    // out-param for the caller-available free bytes and NULL for the two totals
    // we don't need.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(AlienError::new(ErrorData::Other {
            message: format!("GetDiskFreeSpaceExW failed for '{}'", path.display()),
        }));
    }
    Ok(free_available)
}

/// Schedule `dir` and everything under it for deletion on the next reboot,
/// deepest entry first. Best-effort: `MOVEFILE_DELAY_UNTIL_REBOOT` needs admin,
/// so a scheduling failure is only logged — gc must not fail on a locked binary.
fn schedule_delete_on_reboot(dir: &Path) {
    let mut paths = Vec::new();
    collect_depth_first(dir, &mut paths);
    for path in &paths {
        if !move_file_delay_until_reboot(path) {
            warn!(
                path = %path.display(),
                "failed to schedule reboot-delete for a locked straggler (needs admin?)"
            );
        }
    }
}

/// Collect `dir` and every descendant, children before their parent directory,
/// so reboot-deletion is scheduled in an order the OS accepts. A locked file
/// still enumerates (listing a directory does not open its files).
fn collect_depth_first(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                collect_depth_first(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    out.push(dir.to_path_buf());
}

/// `MoveFileExW(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)` — mark `path` for
/// deletion at the next reboot. Returns whether the OS accepted the request.
fn move_file_delay_until_reboot(path: &Path) -> bool {
    let wide = to_wide_null(path);
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 path; a NULL destination
    // with DELAY_UNTIL_REBOOT schedules deletion rather than a move.
    let ok = unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    ok != 0
}

fn to_wide_null(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn store_with(dir: &Path, versions: &[&str]) -> WindowsVersionStore {
        let store = WindowsVersionStore::open(dir).unwrap();
        for ver in versions {
            let stage = store.stage_dir(&v(ver));
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("alien-operator"), format!("binary-{ver}")).unwrap();
        }
        store
    }

    /// The pointer is a real junction, and a single-threaded flip storm always
    /// leaves a readable, residue-free pointer. (The junction flip is not atomic,
    /// so — unlike the Unix Linux test — there is no concurrent-reader variant:
    /// the launcher reads and flips on one thread, and crash residue is covered
    /// by the reconciliation tests below.)
    #[test]
    fn pointer_flip_storm_is_always_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store.set_current(&v("1.0.0")).unwrap();
        assert!(
            exists_no_follow(&dir.path().join("current")),
            "the pointer must exist as a junction"
        );

        for i in 0..500 {
            let ver = if i % 2 == 0 { "2.0.0" } else { "1.0.0" };
            store.set_current(&v(ver)).unwrap();
            assert_eq!(store.current().unwrap(), Some(v(ver)));
        }
        assert!(
            !exists_no_follow(&dir.path().join("current.new")),
            "no staged residue after the storm"
        );
    }

    /// Residue (a): a crash left both `current` (old) and `current.new` (new).
    /// `current()` finishes the flip → the new version, no residue.
    #[test]
    fn reconciles_current_and_new_both_present() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        junction::create(&store.version_target(&v("1.0.0")), &dir.path().join("current")).unwrap();
        junction::create(&store.version_target(&v("2.0.0")), &dir.path().join("current.new"))
            .unwrap();

        assert_eq!(store.current().unwrap(), Some(v("2.0.0")));
        assert!(!exists_no_follow(&dir.path().join("current.new")));
    }

    /// Residue (b): a crash mid-rename left only `current.new`. `current()`
    /// finishes the rename → the new version, and `current` now exists.
    #[test]
    fn reconciles_current_missing_new_present() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        junction::create(&store.version_target(&v("2.0.0")), &dir.path().join("current.new"))
            .unwrap();

        assert_eq!(store.current().unwrap(), Some(v("2.0.0")));
        assert!(exists_no_follow(&dir.path().join("current")));
        assert!(!exists_no_follow(&dir.path().join("current.new")));
    }

    /// Residue (c) with probation: `current` is gone entirely. `current()`
    /// reconstructs it from `probation.new` and re-creates the junction.
    #[test]
    fn reconstructs_current_from_probation() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store
            .write_probation(&ProbationMarker {
                new: v("2.0.0"),
                old: v("1.0.0"),
                started_at: chrono::Utc::now(),
                attempt: 1,
            })
            .unwrap();

        assert_eq!(store.current().unwrap(), Some(v("2.0.0")));
        assert!(exists_no_follow(&dir.path().join("current")));
    }

    /// Residue (c) without probation: `current` gone, no probation → fall back to
    /// `last-stable`.
    #[test]
    fn reconstructs_current_from_last_stable() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store.set_last_stable(&v("1.0.0")).unwrap();

        assert_eq!(store.current().unwrap(), Some(v("1.0.0")));
        assert!(exists_no_follow(&dir.path().join("current")));
    }

    /// A fresh store (never installed) has no pointer and no recovery marker —
    /// `current()` is legitimately `None`, not an error.
    #[test]
    fn fresh_store_has_no_current() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &[]);
        assert_eq!(store.current().unwrap(), None);
    }

    /// gc removes exactly the unpointed versions.
    #[test]
    fn gc_preserves_pointer_targets() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "1.1.0", "1.2.0"]);
        store.set_current(&v("1.2.0")).unwrap();
        store.set_last_stable(&v("1.1.0")).unwrap();

        store.gc(&[]).unwrap();
        assert_eq!(store.list_versions().unwrap(), vec![v("1.1.0"), v("1.2.0")]);
    }

    /// gc must not fail when a candidate's binary is locked (an open handle with
    /// no delete-share): the version is left in place (scheduled for reboot-
    /// deletion — a no-op without admin), and gc still returns Ok.
    #[test]
    fn gc_tolerates_a_locked_binary() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;

        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &["1.0.0", "2.0.0"]);
        store.set_current(&v("2.0.0")).unwrap();

        let victim = store.stage_dir(&v("1.0.0")).join("alien-operator");
        // Open WITHOUT delete-share so remove_dir_all hits a sharing violation.
        let _locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&victim)
            .unwrap();

        store.gc(&[]).expect("gc must not fail on a locked binary");
        assert!(
            store.stage_dir(&v("1.0.0")).exists(),
            "the locked version dir remains (deferred to reboot), not force-deleted"
        );
    }

    /// The real disk has space for a tiny state dir — the preflight passes
    /// against genuine free-space numbers.
    #[test]
    fn free_space_preflight_passes_on_a_real_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path(), &[]);
        std::fs::write(dir.path().join("state/db"), b"tiny").unwrap();
        store
            .free_space_for_snapshot()
            .expect("a 4-byte state dir must fit on any disk running this test");
    }

    // -- the full Phase-0 state-machine suite against the REAL store -----------

    use crate::core::testing::{
        scenario_classification_rows, scenario_crash_injection_promote,
        scenario_crash_injection_rollback, scenario_happy_promote,
        scenario_rollback_on_probation_crash, scenario_rollback_restores_state, TestStoreOps,
    };

    impl TestStoreOps for WindowsVersionStore {
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

    fn windows(dir: &Path) -> WindowsVersionStore {
        WindowsVersionStore::open(dir).unwrap()
    }

    #[test]
    fn state_machine_happy_promote_on_real_store() {
        scenario_happy_promote(windows);
    }

    #[test]
    fn state_machine_rollback_restores_state_on_real_store() {
        scenario_rollback_restores_state(windows);
    }

    #[test]
    fn state_machine_rollback_on_probation_crash_on_real_store() {
        scenario_rollback_on_probation_crash(windows);
    }

    #[test]
    fn state_machine_classification_rows_on_real_store() {
        scenario_classification_rows(windows);
    }

    #[test]
    fn state_machine_crash_injection_promote_on_real_store() {
        scenario_crash_injection_promote(windows);
    }

    #[test]
    fn state_machine_crash_injection_rollback_on_real_store() {
        scenario_crash_injection_rollback(windows);
    }
}
