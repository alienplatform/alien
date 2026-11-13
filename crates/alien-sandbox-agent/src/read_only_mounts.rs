//! Protected bootstrap files on Kubernetes, where the pod supplies the protection.
//!
//! A pod's default seccomp profile blocks `unshare`, so the agent cannot build the private mount
//! namespace [`crate::protected_files::protect`] uses elsewhere. The Kubernetes backend instead
//! mounts the agent binary and `/opt/alien` read-only from a volume an init container filled
//! (see [`stage`]). The agent trusts neither the flag that selects this path nor the pod: it
//! reads its own mount table and refuses to start unless both paths are separate read-only mounts.

use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// First argument that switches the binary into [`stage`] (run by the init container).
pub use alien_core::sandbox_image::STAGE_PROTECTED_FILES_ARGUMENT as STAGE_ARGUMENT;
use alien_core::sandbox_image::{STAGED_AGENT_NAME as STAGED_BINARY, STAGED_METADATA_NAME as STAGED_METADATA};

/// Called while single-threaded, before the supervisor's capabilities are reduced.
pub fn require(binary: &Path, metadata: &Path) -> io::Result<()> {
    // Same-uid image root must not attach a debugger to the agent or reach its descriptors
    // through /proc: the same step the private-namespace path takes first.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let table = fs::read_to_string("/proc/self/mountinfo")?;
    check(&table, binary, metadata).map_err(|reason| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("protected files are not mounted read-only by the pod: {reason}"),
        )
    })
}

/// Pure check over a `mountinfo` table: each path is exactly a mount point with the read-only flag.
fn check(table: &str, binary: &Path, metadata: &Path) -> Result<(), String> {
    for path in [binary, metadata] {
        let mut found = None;
        // Later entries cover earlier ones at the same point; the last one is what is visible.
        for line in table.lines() {
            let mut fields = line.split(' ');
            let (Some(point), Some(options)) = (fields.nth(4), fields.next()) else {
                continue;
            };
            if unescape(point) == path.as_os_str().as_encoded_bytes() {
                found = Some(options.split(',').any(|option| option == "ro"));
            }
        }
        match found {
            None => return Err(format!("{} is not a separate mount", path.display())),
            Some(false) => return Err(format!("{} is mounted writable", path.display())),
            Some(true) => {}
        }
    }
    Ok(())
}

/// Decodes the `\040`-style octal escapes `mountinfo` uses for spaces and the like.
fn unescape(field: &str) -> Vec<u8> {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&field[index + 1..index + 4], 8) {
                out.push(value);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

/// Copies this binary and the contents of `metadata` into `destination`, for the pod to mount
/// read-only over the originals. Symlinks are refused rather than followed.
pub fn stage(destination: &Path, metadata: &Path) -> io::Result<()> {
    let binary = std::env::current_exe()?;
    fs::copy(&binary, destination.join(STAGED_BINARY))?;
    fs::set_permissions(
        destination.join(STAGED_BINARY),
        fs::Permissions::from_mode(0o755),
    )?;
    let target = destination.join(STAGED_METADATA);
    fs::create_dir_all(&target)?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    copy_tree(metadata, &target)
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    let entries: Vec<PathBuf> = match fs::read_dir(source) {
        Ok(entries) => entries.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?,
        // An image without the directory has nothing to carry over.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for path in entries {
        let destination = target.join(path.file_name().expect("directory entry has a name"));
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} must not be a symlink", path.display()),
            ));
        }
        if metadata.is_dir() {
            fs::create_dir(&destination)?;
            fs::set_permissions(&destination, metadata.permissions())?;
            copy_tree(&path, &destination)?;
        } else {
            fs::copy(&path, &destination)?;
            fs::set_permissions(&destination, metadata.permissions())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &str = "/usr/local/bin/alien-sandbox-agent";
    const META: &str = "/opt/alien";

    fn table(agent: &str, meta: &str) -> String {
        format!(
            "1 0 0:1 / / rw - overlay overlay rw\n\
             2 1 0:2 /agent {AGENT} {agent},relatime - ext4 /dev/vda rw\n\
             3 1 0:2 /opt {META} {meta},relatime - ext4 /dev/vda rw\n"
        )
    }

    #[test]
    fn both_paths_read_only_are_accepted() {
        assert!(check(&table("ro", "ro"), Path::new(AGENT), Path::new(META)).is_ok());
    }

    #[test]
    fn a_writable_mount_or_a_missing_mount_is_refused() {
        let agent = Path::new(AGENT);
        let meta = Path::new(META);
        assert!(check(&table("rw", "ro"), agent, meta).unwrap_err().contains("writable"));
        assert!(check(&table("ro", "rw"), agent, meta).unwrap_err().contains("writable"));
        let bare = "1 0 0:1 / / ro - overlay overlay ro\n";
        assert!(check(bare, agent, meta).unwrap_err().contains("not a separate mount"));
    }

    #[test]
    fn a_later_writable_mount_over_a_read_only_one_is_refused() {
        let mut stacked = table("ro", "ro");
        stacked.push_str(&format!("4 1 0:3 / {AGENT} rw - tmpfs tmpfs rw\n"));
        assert!(check(&stacked, Path::new(AGENT), Path::new(META)).is_err());
    }

    #[test]
    fn mountinfo_escapes_are_decoded() {
        assert_eq!(unescape("/a\\040b"), b"/a b");
    }

    #[test]
    fn staging_copies_the_binary_and_metadata_and_refuses_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let meta = temp.path().join("meta");
        let out = temp.path().join("out");
        fs::create_dir_all(meta.join("sub")).unwrap();
        fs::create_dir(&out).unwrap();
        fs::write(meta.join("image-command.json"), b"{}").unwrap();
        fs::set_permissions(meta.join("image-command.json"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(meta.join("sub/x"), b"x").unwrap();
        stage(&out, &meta).unwrap();
        assert!(out.join(STAGED_BINARY).is_file());
        let staged = out.join(STAGED_METADATA).join("image-command.json");
        assert_eq!(fs::read(&staged).unwrap(), b"{}");
        assert_eq!(fs::metadata(&staged).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(out.join("opt/sub/x")).unwrap(), b"x");
        std::os::unix::fs::symlink("/etc/passwd", meta.join("link")).unwrap();
        let out2 = temp.path().join("out2");
        fs::create_dir(&out2).unwrap();
        assert!(stage(&out2, &meta).is_err());
    }
}
