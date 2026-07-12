//! Portable building blocks shared by every `VersionStore` implementation:
//! atomic JSON marker I/O, `state/` snapshot copy + restore, gc-candidate
//! computation, and the disk-space check.
//!
//! Everything here is platform-blind. The one inherently platform-specific
//! piece of the disk-space story — querying *available* bytes — is supplied
//! by the platform store; this module only computes the *required* bytes
//! (`dir_size`) and applies the check (`check_space`).

// Skeleton staging: outside of test cfg, only the validation helpers have a
// consumer (the state machine) until the platform VersionStores land.
#![allow(dead_code)]

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

use super::traits::Version;

// ---------------------------------------------------------------------------
// Atomic marker I/O
// ---------------------------------------------------------------------------

// The atomic write/read/remove primitives live in `alien_core::self_update`
// (they are the normative protocol both binaries share). These wrappers add
// the launcher's error semantics: parse failure = StoreCorrupt, loudly.

/// Atomically write a JSON marker (temp → fsync → rename; see
/// `alien_core::self_update::write_json_atomic` for the crash semantics).
pub fn write_marker_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    alien_core::self_update::write_json_atomic(path, value)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to write marker '{}'", path.display()),
        })
}

/// Read a JSON marker. Absent file → `Ok(None)`. A file that exists but does
/// not parse is genuine corruption (atomic writes rule out torn markers) and
/// fails loudly as `StoreCorrupt` — never silently treated as absent.
pub fn read_marker<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match alien_core::self_update::read_json(path) {
        Ok(value) => Ok(value),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            Err(e).into_alien_error().context(ErrorData::StoreCorrupt {
                path: path.display().to_string(),
                message: "marker exists but is not valid JSON for its schema".to_string(),
            })
        }
        Err(e) => Err(e).into_alien_error().context(ErrorData::Other {
            message: format!("failed to read marker '{}'", path.display()),
        }),
    }
}

/// Remove a marker. Idempotent — an absent marker is success, matching the
/// re-runnable promote/rollback step sequences.
pub fn remove_marker(path: &Path) -> Result<()> {
    alien_core::self_update::remove_json(path)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to remove marker '{}'", path.display()),
        })
}

// ---------------------------------------------------------------------------
// Snapshot copy + restore
// ---------------------------------------------------------------------------

/// Copy `state_dir` to `snapshots_dir/<tag>/` via a temp dir + rename, so a
/// crash never leaves a half-copied snapshot under the final name. An
/// existing snapshot for `tag` is replaced.
pub fn snapshot_state_dir(state_dir: &Path, snapshots_dir: &Path, tag: &Version) -> Result<()> {
    let final_dir = snapshots_dir.join(tag.as_str());
    let tmp_dir = snapshots_dir.join(format!(".tmp-{tag}"));

    remove_dir_if_exists(&tmp_dir)?;
    std::fs::create_dir_all(snapshots_dir)
        .into_alien_error()
        .context(ErrorData::SnapshotFailed {
            message: format!("failed to create '{}'", snapshots_dir.display()),
        })?;
    copy_dir_recursive(state_dir, &tmp_dir).context(ErrorData::SnapshotFailed {
        message: format!(
            "failed to copy '{}' to snapshot temp dir",
            state_dir.display()
        ),
    })?;
    remove_dir_if_exists(&final_dir)?;
    std::fs::rename(&tmp_dir, &final_dir)
        .into_alien_error()
        .context(ErrorData::SnapshotFailed {
            message: format!("failed to commit snapshot '{}'", final_dir.display()),
        })
}

/// Restore `state_dir` from `snapshots_dir/<tag>/`.
///
/// Re-runnable at every crash point (rollback steps are idempotent): copy the
/// snapshot to a temp dir first, then swap it into place. Order: stale temp
/// cleanup → copy → remove old state → rename.
pub fn restore_state_dir(state_dir: &Path, snapshots_dir: &Path, tag: &Version) -> Result<()> {
    let snapshot_dir = snapshots_dir.join(tag.as_str());
    if !snapshot_dir.is_dir() {
        return Err(AlienError::new(ErrorData::SnapshotFailed {
            message: format!(
                "snapshot '{}' does not exist — cannot restore state",
                snapshot_dir.display()
            ),
        }));
    }
    let tmp_dir = state_dir.with_file_name(format!(
        ".tmp-restore-{}",
        state_dir
            .file_name()
            .expect("state dir always has a name")
            .to_string_lossy()
    ));

    remove_dir_if_exists(&tmp_dir)?;
    copy_dir_recursive(&snapshot_dir, &tmp_dir).context(ErrorData::SnapshotFailed {
        message: format!(
            "failed to copy snapshot '{}' for restore",
            snapshot_dir.display()
        ),
    })?;
    remove_dir_if_exists(state_dir)?;
    std::fs::rename(&tmp_dir, state_dir)
        .into_alien_error()
        .context(ErrorData::SnapshotFailed {
            message: format!(
                "failed to swap restored state into '{}'",
                state_dir.display()
            ),
        })
}

/// Recursively copy a directory tree (regular files + dirs; the state dir
/// contains no symlinks).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to create '{}'", dst.display()),
        })?;
    for entry in std::fs::read_dir(src)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to read dir '{}'", src.display()),
        })?
    {
        let entry = entry.into_alien_error().context(ErrorData::Other {
            message: format!("failed to read dir entry under '{}'", src.display()),
        })?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let file_type = entry.file_type().into_alien_error().context(ErrorData::Other {
            message: format!("failed to stat '{}'", from.display()),
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!(
                        "failed to copy '{}' to '{}'",
                        from.display(),
                        to.display()
                    ),
                })?;
        }
    }
    Ok(())
}

fn remove_dir_if_exists(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).into_alien_error().context(ErrorData::Other {
            message: format!("failed to remove '{}'", dir.display()),
        }),
    }
}

// ---------------------------------------------------------------------------
// Staged-binary validation
// ---------------------------------------------------------------------------

/// SHA-256 of a file, lowercase hex. Streams in 64 KiB chunks so a large
/// binary never sits in memory.
pub fn file_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to open '{}' for hashing", path.display()),
        })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to read '{}' for hashing", path.display()),
        })?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Validate a staged binary against its `pending.json` claim: the file must
/// exist and its re-computed SHA-256 must match. `Ok(false)` = invalid stage
/// (partial download, tampering, wrong file) — the caller discards the
/// pending marker; errors are real I/O failures.
pub fn validate_staged_binary(binary_path: &Path, expected_sha256: &str) -> Result<bool> {
    if !binary_path.is_file() {
        return Ok(false);
    }
    Ok(file_sha256(binary_path)? == expected_sha256)
}

// ---------------------------------------------------------------------------
// GC candidates + disk space
// ---------------------------------------------------------------------------

/// The versions safe to delete: everything in `all` that is not in `keep`
/// and is not what `current` / `last-stable` point at. Pure function — the
/// never-delete-the-pointers rule is enforced here, once, for every platform
/// store.
pub fn gc_candidates(
    all: &[Version],
    keep: &[Version],
    current: Option<&Version>,
    last_stable: Option<&Version>,
) -> Vec<Version> {
    all.iter()
        .filter(|v| !keep.contains(v))
        .filter(|v| Some(*v) != current)
        .filter(|v| Some(*v) != last_stable)
        .cloned()
        .collect()
}

/// Total size in bytes of all regular files under `dir` — the "required
/// bytes" side of the snapshot preflight.
pub fn dir_size(dir: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("failed to read dir '{}'", dir.display()),
        })?
    {
        let entry = entry.into_alien_error().context(ErrorData::Other {
            message: format!("failed to read dir entry under '{}'", dir.display()),
        })?;
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path)?;
        } else {
            let meta = entry.metadata().into_alien_error().context(ErrorData::Other {
                message: format!("failed to stat '{}'", path.display()),
            })?;
            total += meta.len();
        }
    }
    Ok(total)
}

/// Apply the disk-space preflight: fail with `DiskSpace` unless
/// `available_bytes` covers `required_bytes` plus 20% headroom. The platform
/// store queries `available_bytes` (statvfs / GetDiskFreeSpaceEx — inherently
/// platform code); this check stays shared so the policy lives in one place.
pub fn check_space(required_bytes: u64, available_bytes: u64, what: &str) -> Result<()> {
    let with_headroom = required_bytes.saturating_add(required_bytes / 5);
    if available_bytes < with_headroom {
        return Err(AlienError::new(ErrorData::DiskSpace {
            required_bytes: with_headroom,
            available_bytes,
            message: format!("not enough free space for {what}"),
        }));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::traits::PendingMarker;

    fn version(s: &str) -> Version {
        Version::parse(s).expect("test version should parse")
    }

    fn marker(v: &str) -> PendingMarker {
        PendingMarker {
            version: version(v),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            staged_at: "2026-07-08T12:00:00Z".parse().unwrap(),
        }
    }

    /// Write → read round-trips, and no `.tmp` residue survives a successful
    /// commit.
    #[test]
    fn marker_write_is_atomic_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.json");

        write_marker_atomic(&path, &marker("1.4.0")).expect("write should succeed");
        let back: Option<PendingMarker> = read_marker(&path).expect("read should succeed");
        assert_eq!(back, Some(marker("1.4.0")));

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp residue: {leftovers:?}");

        // Overwrite is also atomic and clean.
        write_marker_atomic(&path, &marker("1.5.0")).expect("overwrite should succeed");
        let back: Option<PendingMarker> = read_marker(&path).unwrap();
        assert_eq!(back.unwrap().version, version("1.5.0"));
    }

    /// Absent marker reads as None; corrupt marker fails loudly (never a
    /// silent None).
    #[test]
    fn marker_read_absent_is_none_corrupt_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.json");

        let absent: Option<PendingMarker> = read_marker(&path).expect("absent is not an error");
        assert!(absent.is_none());

        std::fs::write(&path, b"{ definitely not a marker").unwrap();
        let err = read_marker::<PendingMarker>(&path).expect_err("corrupt marker must error");
        assert_eq!(err.code, "STORE_CORRUPT");

        // remove_marker is idempotent.
        remove_marker(&path).expect("remove existing");
        remove_marker(&path).expect("remove absent is still Ok");
        assert!(!path.exists());
    }

    /// Snapshot, mutate state (edit + add + delete), restore → byte-identical.
    #[test]
    fn snapshot_then_restore_is_byte_identical() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let snaps = root.path().join("state-snapshots");
        std::fs::create_dir_all(state.join("sub")).unwrap();
        std::fs::write(state.join("db.sqlite"), b"original-db-bytes").unwrap();
        std::fs::write(state.join("sub/token"), b"original-token").unwrap();

        let tag = version("1.3.5");
        snapshot_state_dir(&state, &snaps, &tag).expect("snapshot should succeed");

        // Mutate: edit one file, add one, delete one — simulating a migration.
        std::fs::write(state.join("db.sqlite"), b"MIGRATED-db-bytes-longer").unwrap();
        std::fs::write(state.join("new-table"), b"added-by-new-version").unwrap();
        std::fs::remove_file(state.join("sub/token")).unwrap();

        restore_state_dir(&state, &snaps, &tag).expect("restore should succeed");

        assert_eq!(
            std::fs::read(state.join("db.sqlite")).unwrap(),
            b"original-db-bytes"
        );
        assert_eq!(
            std::fs::read(state.join("sub/token")).unwrap(),
            b"original-token"
        );
        assert!(
            !state.join("new-table").exists(),
            "files added after the snapshot must not survive restore"
        );
        // No temp residue from either operation.
        assert!(!snaps.join(".tmp-1.3.5").exists());
        assert!(!root.path().join(".tmp-restore-state").exists());
    }

    /// Restoring a missing snapshot fails loudly.
    #[test]
    fn restore_missing_snapshot_is_error() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let err = restore_state_dir(&state, &root.path().join("state-snapshots"), &version("9.9.9"))
            .expect_err("missing snapshot must error");
        assert_eq!(err.code, "SNAPSHOT_FAILED");
    }

    /// gc candidates never include current / last-stable, regardless of `keep`.
    #[test]
    fn gc_candidates_never_include_pointers() {
        let all = [version("1.0.0"), version("1.1.0"), version("1.2.0"), version("1.3.0")];
        let current = version("1.3.0");
        let last_stable = version("1.2.0");

        let candidates = gc_candidates(&all, &[], Some(&current), Some(&last_stable));
        assert_eq!(candidates, vec![version("1.0.0"), version("1.1.0")]);

        // Even an explicit empty keep-list with pointers unset keeps nothing.
        let candidates = gc_candidates(&all, &[version("1.0.0")], Some(&current), Some(&last_stable));
        assert_eq!(candidates, vec![version("1.1.0")]);

        // No pointers at all (fresh store): everything not kept is a candidate.
        let candidates = gc_candidates(&all, &[version("1.1.0")], None, None);
        assert_eq!(
            candidates,
            vec![version("1.0.0"), version("1.2.0"), version("1.3.0")]
        );
    }

    /// The space check demands required + 20% headroom.
    #[test]
    fn check_space_enforces_headroom() {
        check_space(1000, 1200, "state snapshot").expect("exactly at headroom passes");
        let err = check_space(1000, 1199, "state snapshot").expect_err("below headroom fails");
        assert_eq!(err.code, "DISK_SPACE");

        check_space(0, 0, "empty state").expect("zero required always passes");
    }

    /// dir_size counts nested regular files.
    #[test]
    fn dir_size_counts_nested_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("a/b")).unwrap();
        std::fs::write(root.path().join("f1"), vec![0u8; 100]).unwrap();
        std::fs::write(root.path().join("a/f2"), vec![0u8; 50]).unwrap();
        std::fs::write(root.path().join("a/b/f3"), vec![0u8; 7]).unwrap();
        assert_eq!(dir_size(root.path()).unwrap(), 157);
    }
}
