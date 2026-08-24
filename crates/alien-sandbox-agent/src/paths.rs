//! Path safety for file operations inside a sandbox.
//!
//! The protocol's rule is specific and the ordering is the whole point: paths are
//! **resolved and re-checked against the root after resolution, not before**. Checking a path
//! for `..` and then opening it is the classic symlink escape — `/work/link` contains no
//! traversal and can still point at `/etc/shadow`.

use std::path::{Component, Path, PathBuf};

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Resolves a caller-supplied path against the session root, refusing anything that escapes.
///
/// `root` must already exist and be canonical. The returned path is canonical and guaranteed
/// to sit under it.
pub fn resolve_within_root(root: &Path, requested: &str) -> Result<PathBuf> {
    if requested.is_empty() {
        return Err(refused(requested, "path is empty"));
    }

    // Lexical rejection first — cheap, and it catches the obvious cases before any filesystem
    // work. It is not sufficient on its own, which is what the post-resolution check is for.
    let candidate = Path::new(requested);
    for component in candidate.components() {
        match component {
            Component::ParentDir => return Err(refused(requested, "path traverses upward")),
            Component::Prefix(_) => {
                return Err(refused(requested, "path carries a filesystem prefix"))
            }
            _ => {}
        }
    }

    // An absolute path is interpreted relative to the session root rather than the real
    // filesystem root, so `/work/x` means `<root>/work/x`. Treating it as host-absolute would
    // let a caller name any file the agent can read.
    let relative = candidate.strip_prefix("/").unwrap_or(candidate);
    let joined = root.join(relative);

    // Resolve the deepest existing ancestor, because the target itself may not exist yet on a
    // write. Whatever does exist is canonicalised, which is what collapses symlinks.
    let (existing, remainder) = deepest_existing(&joined);
    let canonical_existing = existing
        .canonicalize()
        .map_err(|error| refused(requested, &format!("path could not be resolved: {error}")))?;

    if !canonical_existing.starts_with(root) {
        // The lexical check passed and this still escaped, which means a symlink. This is the
        // check that actually holds.
        return Err(refused(requested, "path escapes the session root"));
    }

    // Same guard as above: joining an empty remainder would append a separator, and a path
    // ending in "/" makes the OS refuse a regular file with "Not a directory".
    if remainder.as_os_str().is_empty() {
        return Ok(canonical_existing);
    }

    Ok(canonical_existing.join(remainder))
}

/// Splits a path into its deepest existing ancestor and the not-yet-existing tail.
fn deepest_existing(path: &Path) -> (PathBuf, PathBuf) {
    let mut existing = path.to_path_buf();
    let mut remainder = PathBuf::new();

    // `symlink_metadata`, not `exists`: `exists` follows links, so a *dangling* symlink reads as
    // absent, is folded into `remainder`, and gets re-appended below without ever being
    // canonicalised. Anything that can plant a link in the root then writes through it.
    while existing.symlink_metadata().is_err() {
        let Some(parent) = existing.parent() else {
            break;
        };
        let Some(name) = existing.file_name() else {
            break;
        };

        // Guarded because `Path::join` with an empty path appends a separator, which would
        // make the resolved path end in "/" and the OS treat a file target as a directory.
        remainder = if remainder.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            Path::new(name).join(&remainder)
        };
        existing = parent.to_path_buf();
    }

    (existing, remainder)
}

fn refused(path: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::PathRefused {
        path: path.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn root() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonical root");
        fs::create_dir_all(root.join("work")).expect("work dir");
        (dir, root)
    }

    #[test]
    fn a_path_inside_the_root_resolves() {
        let (_dir, root) = root();
        fs::write(root.join("work/main.py"), b"x").expect("file");

        let resolved = resolve_within_root(&root, "/work/main.py").expect("inside the root");
        assert!(resolved.starts_with(&root));
        assert!(resolved.ends_with("work/main.py"));
    }

    #[test]
    fn a_path_that_does_not_exist_yet_resolves_for_writing() {
        let (_dir, root) = root();

        let resolved = resolve_within_root(&root, "/work/new/deep.txt").expect("write target");
        assert!(resolved.starts_with(&root));
        assert!(resolved.ends_with("work/new/deep.txt"));

        // Compared as a string, not with ends_with: Path comparison ignores a trailing
        // separator, and a trailing separator is exactly what made the OS refuse the write.
        assert!(
            !resolved.to_string_lossy().ends_with('/'),
            "a file target must not resolve with a trailing separator: {resolved:?}"
        );
    }

    #[test]
    fn upward_traversal_is_refused() {
        let (_dir, root) = root();

        for path in [
            "/work/../../etc/passwd",
            "../escape",
            "/work/../..",
            "a/../../b",
        ] {
            match resolve_within_root(&root, path) {
                Ok(resolved) => panic!(
                    "'{path}' resolved to {} instead of being refused",
                    resolved.display()
                ),
                Err(error) => assert!(
                    error.to_string().contains("traverses upward"),
                    "'{path}' must be refused for traversal, got: {error}"
                ),
            }
        }
    }

    /// The reason the check happens *after* resolution. This path contains no `..` and passes
    /// every lexical test; only canonicalisation reveals where it actually points.
    #[test]
    fn a_symlink_pointing_outside_the_root_is_refused() {
        let (_dir, root) = root();
        let outside = TempDir::new().expect("outside dir");
        let secret = outside.path().join("secret.txt");
        fs::write(&secret, b"not yours").expect("secret");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, root.join("work/link")).expect("symlink");

        let error = resolve_within_root(&root, "/work/link")
            .expect_err("a symlink out of the root must be refused");
        assert!(
            error.to_string().contains("escapes the session root"),
            "the refusal must name the actual reason: {error}"
        );
    }

    /// A symlinked *directory* is the same attack one level up: the traversal happens inside
    /// the resolved parent, so a check on the leaf alone would miss it.
    #[test]
    fn a_symlinked_parent_directory_is_refused() {
        let (_dir, root) = root();
        let outside = TempDir::new().expect("outside dir");
        fs::write(outside.path().join("secret.txt"), b"not yours").expect("secret");

        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), root.join("work/escape")).expect("symlink");

        resolve_within_root(&root, "/work/escape/secret.txt")
            .expect_err("a symlinked parent directory must be refused");
    }

    /// A link whose target does not exist yet. It is the dangerous case precisely because it
    /// looks absent: resolving it as a not-yet-created file would hand back a path that writes
    /// through the link, outside the root, the moment the OS follows it.
    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_out_of_the_root_is_refused() {
        let (_dir, root) = root();
        let outside = TempDir::new().expect("outside dir");
        let target = outside.path().join("planted.txt");
        assert!(
            !target.exists(),
            "the target must not exist for this to be a dangling link"
        );

        std::os::unix::fs::symlink(&target, root.join("work/evil")).expect("symlink");

        let error = resolve_within_root(&root, "/work/evil")
            .expect_err("a dangling symlink out of the root must be refused");
        assert!(
            !target.exists(),
            "resolving must not create the link target at {}",
            target.display()
        );
        assert!(
            error.to_string().contains("could not be resolved")
                || error.to_string().contains("escapes the session root"),
            "the refusal must name the resolution failure: {error}"
        );
    }

    /// Hard links are deliberately not refused here. A link is a second name for an inode, so
    /// this resolver cannot tell one from the file itself, and refusing multiply-linked files
    /// breaks the ones build tooling makes on purpose — package stores, `cp -al`, virtualenvs.
    #[test]
    fn a_hard_linked_file_inside_the_root_still_resolves() {
        let (_dir, root) = root();
        fs::write(root.join("work/a.txt"), b"x").expect("file");
        fs::hard_link(root.join("work/a.txt"), root.join("work/b.txt")).expect("hard link");

        resolve_within_root(&root, "/work/b.txt").expect("a hard-linked file is still a file");
    }

    #[test]
    fn an_empty_path_is_refused() {
        let (_dir, root) = root();
        resolve_within_root(&root, "").expect_err("an empty path names nothing");
    }

    /// An absolute path is relative to the session root, not the host filesystem. Treating it
    /// as host-absolute would let a caller name any file the agent can read.
    #[test]
    fn an_absolute_path_is_interpreted_against_the_session_root() {
        let (_dir, root) = root();
        fs::write(root.join("work/main.py"), b"x").expect("file");

        let with_slash = resolve_within_root(&root, "/work/main.py").expect("resolves");
        let without = resolve_within_root(&root, "work/main.py").expect("resolves");
        assert_eq!(with_slash, without);

        // Existing files take the other deepest_existing branch, which must also not append a separator.
        assert!(!with_slash.to_string_lossy().ends_with('/'));
    }
}
