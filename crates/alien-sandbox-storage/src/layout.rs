use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Directory in the linked Storage that holds every sandbox's persistent storage.
pub const ROOT_PREFIX: &str = "sandbox-storage";
/// The object naming the current commit, relative to the key root.
pub const HEAD_OBJECT: &str = "HEAD";
/// Directory of chunk packs, relative to the key root.
pub const PACKS_DIR: &str = "packs";
/// Directory of manifests, relative to the key root.
pub const MANIFESTS_DIR: &str = "manifests";

/// Longest storage key accepted.
const MAX_KEY_LEN: usize = 128;

/// Refuses a storage key that is not one path segment of `[A-Za-z0-9._-]`, at most 128 bytes,
/// and not `.` or `..`. The key becomes part of an object path and of a presigned policy prefix,
/// so anything that could name another key's objects is refused.
pub fn validate_key(key: &str) -> Result<()> {
    let invalid = |reason: &str| {
        Err(AlienError::new(ErrorData::KeyInvalid {
            key: key.to_string(),
            reason: reason.to_string(),
        }))
    };
    if key.is_empty() || key.len() > MAX_KEY_LEN {
        return invalid("must be 1 to 128 characters");
    }
    if key == "." || key == ".." {
        return invalid("must not be '.' or '..'");
    }
    if !key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return invalid("may contain only letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

/// Refuses a path inside the tree that is empty, absolute, or has a `.`, `..` or empty
/// component. Every path in a manifest passes this, so a restore can never write outside the
/// directory it restores into.
pub fn validate_tree_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(AlienError::new(ErrorData::PathInvalid {
            path: path.to_string(),
            reason: "must be relative, with no '.', '..' or empty component".to_string(),
        }));
    }
    Ok(())
}

/// The object prefix of one key: `sandbox-storage/<sandbox id>/<key>/`.
pub fn key_root(sandbox_id: &str, key: &str) -> Result<String> {
    validate_key(key)?;
    Ok(format!("{ROOT_PREFIX}/{sandbox_id}/{key}/"))
}

/// A commit's sequence number and random id, which together name its pack and manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectName {
    /// Position in the key's history; HEAD's manifest has the highest live one.
    pub seq: u64,
    /// Random per commit, so two writers never produce the same object name.
    pub commit: String,
}

impl ObjectName {
    /// A fresh name for the commit after `seq`.
    pub fn next(seq: u64) -> Self {
        Self {
            seq: seq + 1,
            commit: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    /// `packs/<seq>-<commit>.tar`
    pub fn pack(&self) -> String {
        format!("{PACKS_DIR}/{:020}-{}.tar", self.seq, self.commit)
    }

    /// `manifests/<seq>-<commit>.json`
    pub fn manifest(&self) -> String {
        format!("{MANIFESTS_DIR}/{:020}-{}.json", self.seq, self.commit)
    }

    /// Reads a name back from a pack or manifest object, or `None` for anything else.
    pub fn parse(object: &str) -> Option<Self> {
        let file = object
            .strip_prefix(&format!("{PACKS_DIR}/"))
            .and_then(|f| f.strip_suffix(".tar"))
            .or_else(|| {
                object
                    .strip_prefix(&format!("{MANIFESTS_DIR}/"))
                    .and_then(|f| f.strip_suffix(".json"))
            })?;
        let (seq, commit) = file.split_once('-')?;
        Some(Self {
            seq: seq.parse().ok()?,
            commit: commit.to_string(),
        })
    }
}

/// What HEAD names: a commit and the SHA-256 of its manifest's bytes.
///
/// Every object but HEAD is written create-only, so a stale writer cannot overwrite one; the
/// hash makes that checkable by the reader too, so a manifest that is not the bytes HEAD's
/// writer published is refused rather than restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRef {
    /// The commit's sequence number and id, which name its pack and manifest.
    pub name: ObjectName,
    /// Hex SHA-256 of the manifest object's bytes.
    pub manifest_sha256: String,
}

/// The content of `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    /// The current commit, or `None` for a key taken over before its first commit.
    pub commit: Option<CommitRef>,
    /// The sandbox that last moved HEAD. A takeover changes it, which is what refuses the
    /// previous writer.
    pub writer: String,
    /// When HEAD last moved.
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_one_safe_segment() {
        for key in ["engine-42", "a", "A.b_c-1", &"k".repeat(128)] {
            validate_key(key).expect(key);
        }
        for key in [
            "",
            ".",
            "..",
            "a/b",
            "a b",
            "ä",
            "a\0",
            &"k".repeat(129),
            "a%2fb",
        ] {
            assert!(validate_key(key).is_err(), "{key:?}");
        }
    }

    #[test]
    fn tree_paths_stay_inside_the_root() {
        for path in ["a", "a/b", ".git/HEAD", "a..b"] {
            validate_tree_path(path).expect(path);
        }
        for path in ["", "/a", "a/../b", "..", "a//b", "a/", "./a", "a\0b"] {
            assert!(validate_tree_path(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn object_names_round_trip_and_sort_by_seq() {
        let name = ObjectName::next(9);
        assert_eq!(name.seq, 10);
        assert_eq!(ObjectName::parse(&name.pack()), Some(name.clone()));
        assert_eq!(ObjectName::parse(&name.manifest()), Some(name.clone()));
        assert_eq!(ObjectName::parse("HEAD"), None);
        assert_eq!(ObjectName::parse("packs/x.tar"), None);
        assert!(ObjectName::next(1).pack() < ObjectName::next(10).pack());
    }

    #[test]
    fn key_root_is_scoped_to_sandbox_and_key() {
        assert_eq!(
            key_root("agent", "engine-42").unwrap(),
            "sandbox-storage/agent/engine-42/"
        );
        assert!(key_root("agent", "../other").is_err());
    }
}
