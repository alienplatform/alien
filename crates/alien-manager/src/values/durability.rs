//! Whether the manager's state directory survives a restart.
//!
//! Without a values key, values live in the manager's SQLite file inside the
//! state directory. In a container that directory is usually part of the
//! image's writable layer and disappears with the container, so the manager
//! refuses to take values there rather than lose them later.
//!
//! The signal: the root filesystem is an overlay (how container runtimes build
//! the writable layer) and the state directory is on that same filesystem. A
//! mounted volume has its own device and passes. A bind mount of a node
//! directory, such as a Kubernetes `emptyDir`, has its own device too, so it
//! also passes even though it is not durable. That case is not detected.

use std::path::Path;

use alien_error::{AlienError, Context, IntoAlienError};

use crate::error::{ErrorData, Result};

const MOUNTINFO: &str = "/proc/self/mountinfo";

/// Whether `state_dir` is on the container's own writable layer.
pub fn state_dir_is_ephemeral(state_dir: &Path) -> Result<bool> {
    let mountinfo = match std::fs::read_to_string(MOUNTINFO) {
        Ok(text) => text,
        // No mount table (not Linux): there is no overlay to detect.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(e)
                .into_alien_error()
                .context(ErrorData::ServerInitFailed {
                    reason: format!("Failed to read {MOUNTINFO}"),
                })
        }
    };
    is_ephemeral(&mountinfo, state_dir, Path::new("/"))
}

fn is_ephemeral(mountinfo: &str, state_dir: &Path, root: &Path) -> Result<bool> {
    if !root_is_overlay(mountinfo) {
        return Ok(false);
    }
    Ok(device_of(state_dir)? == device_of(root)?)
}

#[cfg(unix)]
fn device_of(path: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path)
        .map(|m| m.dev())
        .into_alien_error()
        .context(ErrorData::ServerInitFailed {
            reason: format!("Failed to stat {}", path.display()),
        })
}

#[cfg(not(unix))]
fn device_of(_path: &Path) -> Result<u64> {
    Ok(0)
}

/// Whether the mount at `/` in a `/proc/self/mountinfo` table is an overlay.
/// A later line for the same mount point hides an earlier one.
fn root_is_overlay(mountinfo: &str) -> bool {
    let mut root_fstype = None;
    for line in mountinfo.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        if before.split_whitespace().nth(4) != Some("/") {
            continue;
        }
        root_fstype = after.split_whitespace().next();
    }
    matches!(root_fstype, Some("overlay" | "aufs"))
}

/// The error a write gets when the state directory is not durable.
pub(super) fn ephemeral_state_error() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValuesUnavailable {
        reason: "the state directory is not on a persistent volume and no values key is \
                 configured, so a stored value would be lost when the process restarts. Mount a \
                 persistent volume at the state directory or configure a values key"
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCKER_ROOT: &str = "\
1120 1060 0:67 / / rw,relatime master:1 - overlay overlay rw,lowerdir=/a,upperdir=/b,workdir=/c
1121 1120 0:70 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
1122 1120 259:1 /data /data rw,relatime - ext4 /dev/nvme1n1 rw
";

    #[test]
    fn an_overlay_root_is_found_among_other_mounts() {
        assert!(root_is_overlay(DOCKER_ROOT));
    }

    #[test]
    fn a_disk_root_is_not_an_overlay() {
        let table = "\
26 1 254:1 / / rw,relatime shared:1 - ext4 /dev/vda1 rw
27 26 0:6 / /dev rw,nosuid - devtmpfs udev rw
";
        assert!(!root_is_overlay(table));
    }

    #[test]
    fn a_mount_named_overlay_under_another_path_is_not_the_root() {
        let table = "\
26 1 254:1 / / rw,relatime - ext4 /dev/vda1 rw
30 26 0:40 / /var/lib/x rw - overlay overlay rw
";
        assert!(!root_is_overlay(table));
    }

    #[test]
    fn a_later_root_mount_hides_an_earlier_one() {
        let table = "\
1 0 0:1 / / rw - rootfs rootfs rw
2 1 0:67 / / rw - overlay overlay rw
";
        assert!(root_is_overlay(table));
    }

    #[test]
    fn an_empty_table_has_no_overlay_root() {
        assert!(!root_is_overlay(""));
    }

    #[test]
    fn a_state_dir_on_the_overlay_device_is_ephemeral() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_ephemeral(DOCKER_ROOT, dir.path(), dir.path()).unwrap());
    }

    #[test]
    fn a_state_dir_on_its_own_device_is_durable() {
        // /proc is a separate filesystem from the directory below it.
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_ephemeral(DOCKER_ROOT, Path::new("/proc"), dir.path()).unwrap());
    }

    #[test]
    fn a_disk_root_is_never_ephemeral_whatever_the_device() {
        let table = "26 1 254:1 / / rw - ext4 /dev/vda1 rw\n";
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_ephemeral(table, dir.path(), dir.path()).unwrap());
    }

    #[test]
    fn a_missing_state_dir_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = is_ephemeral(DOCKER_ROOT, Path::new("/does/not/exist"), dir.path()).unwrap_err();
        assert_eq!(err.code, "SERVER_INIT_FAILED");
    }
}
