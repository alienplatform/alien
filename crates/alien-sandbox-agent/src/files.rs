//! File transfer in and out of a sandbox.
//!
//! Every path goes through [`crate::paths::resolve_within_root`] first — there is no other way
//! to reach the filesystem from here, which is the property that makes the escape rules
//! enforceable rather than merely documented.

use std::io::{Read, Write};
use std::path::Path;

use crate::confine;
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

/// Largest single file the agent will move in either direction.
///
/// Bounded because the caller is on the other side of a network and the sandbox is not trusted
/// to be honest about size — an unbounded read is a memory exhaustion the workload can trigger.
pub const MAX_TRANSFER_BYTES: u64 = 32 * 1024 * 1024;

/// Reads a file out of the sandbox.
pub async fn read(root: &Path, requested: &str) -> Result<Vec<u8>> {
    let root = root.to_path_buf();
    let requested = requested.to_string();

    tokio::task::spawn_blocking(move || {
        let mut file = match confine::open_read(&root, &requested) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AlienError::new(ErrorData::PathNotFound { path: requested }))
            }
            Err(error) => return Err(refused_or_failed(error, &requested, "opening the file")),
        };

        // Refused rather than read: a directory yields a confusing OS error, and a FIFO or device
        // node is not something a caller can have meant by "read this file".
        let metadata = file
            .metadata()
            .into_alien_error()
            .context(failed("read", &requested, "inspecting the opened file"))?;
        if !metadata.is_file() {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: format!("'{requested}' is not a regular file"),
            }));
        }

        // Bounded on the descriptor rather than trusting the size read a moment earlier: the file
        // can grow between a stat and a read, and the caller is across a network.
        let mut contents = Vec::new();
        let read = (&mut file)
            .take(MAX_TRANSFER_BYTES + 1)
            .read_to_end(&mut contents)
            .into_alien_error()
            .context(failed("read", &requested, "reading the file contents"))?;

        if read as u64 > MAX_TRANSFER_BYTES {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: format!(
                    "'{requested}' is over the {MAX_TRANSFER_BYTES} byte transfer limit"
                ),
            }));
        }

        Ok(contents)
    })
    .await
    .into_alien_error()
    .context(failed("read", "", "waiting for the filesystem"))?
}

/// Writes a file into the sandbox, creating parent directories as needed.
pub async fn write(root: &Path, requested: &str, contents: &[u8]) -> Result<()> {
    if contents.len() as u64 > MAX_TRANSFER_BYTES {
        return Err(AlienError::new(ErrorData::RequestInvalid {
            reason: format!(
                "{} bytes exceeds the {MAX_TRANSFER_BYTES} byte transfer limit",
                contents.len()
            ),
        }));
    }

    let root = root.to_path_buf();
    let requested = requested.to_string();
    let contents = contents.to_vec();

    tokio::task::spawn_blocking(move || {
        if let Some(parent) = parent_of(&requested) {
            confine::create_dir_all(&root, &parent).map_err(|error| {
                refused_or_failed(error, &requested, "making room for the file being written")
            })?;
        }

        let mut file = confine::open_write(&root, &requested)
            .map_err(|error| refused_or_failed(error, &requested, "opening the file for writing"))?;

        file.write_all(&contents)
            .into_alien_error()
            .context(failed("write", &requested, "writing the file contents"))
    })
    .await
    .into_alien_error()
    .context(failed("write", "", "waiting for the filesystem"))?
}

/// The directory part of a caller's path, if it names one.
fn parent_of(requested: &str) -> Option<String> {
    let trimmed = requested.trim_end_matches('/');
    let cut = trimmed.rfind('/')?;
    let parent = &trimmed[..cut];
    (!parent.trim_matches('/').is_empty()).then(|| parent.to_string())
}

/// Creates a directory inside the sandbox.
pub async fn mkdir(root: &Path, requested: &str) -> Result<()> {
    let root = root.to_path_buf();
    let requested = requested.to_string();

    tokio::task::spawn_blocking(move || {
        confine::create_dir_all(&root, &requested)
            .map_err(|error| refused_or_failed(error, &requested, "creating the directory"))
    })
    .await
    .into_alien_error()
    .context(failed("mkdir", "", "waiting for the filesystem"))?
}

/// The kernel refuses an escape with `EXDEV`, and a symlink or `..` in the path with `ELOOP` or
/// `EXDEV` depending on which rule caught it. Those are the caller's mistake, not ours.
fn refused_or_failed(error: std::io::Error, requested: &str, purpose: &str) -> AlienError<ErrorData> {
    let raw = error.raw_os_error();
    if matches!(raw, Some(libc::EXDEV) | Some(libc::ELOOP)) {
        return AlienError::new(ErrorData::PathRefused {
            path: requested.to_string(),
            reason: "path leaves the session root".to_string(),
        });
    }

    Err::<(), std::io::Error>(error)
        .into_alien_error()
        .context(failed("open", requested, purpose))
        .expect_err("constructed from an error")
}

/// `purpose` says what the step was for. The template already says it failed, and the OS-level
/// cause is preserved as the error's source.
fn failed(operation: &str, path: &str, purpose: &str) -> ErrorData {
    ErrorData::OperationFailed {
        operation: format!("{operation} '{path}'"),
        reason: purpose.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn root() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonical root");
        (dir, root)
    }

    #[tokio::test]
    async fn a_file_round_trips() {
        let (_dir, root) = root();

        write(&root, "/work/main.py", b"print(1)")
            .await
            .expect("writes, creating parents");
        let read_back = read(&root, "/work/main.py").await.expect("reads");

        assert_eq!(read_back, b"print(1)");
    }

    #[tokio::test]
    async fn binary_content_survives_the_round_trip() {
        let (_dir, root) = root();
        let bytes: Vec<u8> = (0u8..=255).collect();

        write(&root, "/work/blob.bin", &bytes).await.expect("writes");
        assert_eq!(read(&root, "/work/blob.bin").await.expect("reads"), bytes);
    }

    /// Every entry point goes through the path resolver, so the escape rules hold for files as
    /// well as exec. This is the test that would catch someone adding a path that bypasses it.
    #[tokio::test]
    async fn traversal_is_refused_on_every_operation() {
        let (_dir, root) = root();

        read(&root, "/../etc/passwd").await.expect_err("read must refuse traversal");
        write(&root, "/../evil.txt", b"x")
            .await
            .expect_err("write must refuse traversal");
        mkdir(&root, "/../evil").await.expect_err("mkdir must refuse traversal");
    }

    #[tokio::test]
    async fn a_symlink_out_of_the_root_is_refused_on_read() {
        let (_dir, root) = root();
        let outside = TempDir::new().expect("outside");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, b"not yours").expect("secret");
        std::fs::create_dir_all(root.join("work")).expect("work");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, root.join("work/link")).expect("symlink");

        read(&root, "/work/link")
            .await
            .expect_err("a symlink out of the root must not be readable");
    }

    /// A directory read yields a confusing OS error, and by this point symlinks are resolved,
    /// so anything that is not a regular file is worth naming explicitly.
    #[tokio::test]
    async fn reading_a_directory_is_refused_with_a_clear_reason() {
        let (_dir, root) = root();
        mkdir(&root, "/work").await.expect("mkdir");

        let error = read(&root, "/work").await.expect_err("a directory is not a file");
        assert!(error.to_string().contains("not a regular file"));
    }

    /// The caller is across a network and the sandbox is not trusted to be honest about size;
    /// an unbounded read is memory exhaustion the workload can trigger at will.
    #[tokio::test]
    async fn an_oversized_write_is_refused_before_touching_the_disk() {
        let (_dir, root) = root();
        let too_big = vec![0u8; (MAX_TRANSFER_BYTES + 1) as usize];

        let error = write(&root, "/work/big.bin", &too_big)
            .await
            .expect_err("over the limit");
        assert!(error.to_string().contains("transfer limit"));

        assert!(
            !root.join("work/big.bin").exists(),
            "nothing should have been written"
        );
    }

    #[tokio::test]
    async fn mkdir_is_idempotent() {
        let (_dir, root) = root();

        mkdir(&root, "/work/build").await.expect("creates");
        mkdir(&root, "/work/build")
            .await
            .expect("creating an existing directory is not a failure");
    }
}
