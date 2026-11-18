use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

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

/// The object prefix of one key in the sandbox's own store: `<key>/`.
pub fn key_root(key: &str) -> Result<String> {
    validate_key(key)?;
    Ok(format!("{key}/"))
}

/// A pack or manifest: its key's sequence number and the SHA-256 of its bytes.
///
/// Content-addressed so that no write can change what a name means. Object storage cannot make
/// every upload create-only (an S3 POST policy upload ignores `If-None-Match`), so two writers
/// using the same name must be writing the same bytes; a reader checks the hash anyway.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectName {
    /// Position in the key's history; HEAD's manifest has the highest live one.
    pub seq: u64,
    /// Hex SHA-256 of the object's bytes.
    pub hash: String,
}

impl ObjectName {
    /// The name of `bytes` at `seq`.
    pub fn of(seq: u64, bytes: &[u8]) -> Self {
        Self {
            seq,
            hash: crate::pack::chunk_hash(bytes),
        }
    }

    /// `packs/<seq>-<sha256>.tar`
    pub fn pack(&self) -> String {
        format!("{PACKS_DIR}/{:020}-{}.tar", self.seq, self.hash)
    }

    /// `manifests/<seq>-<sha256>.json`
    pub fn manifest(&self) -> String {
        format!("{MANIFESTS_DIR}/{:020}-{}.json", self.seq, self.hash)
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
        let (seq, hash) = file.split_once('-')?;
        Some(Self {
            seq: seq.parse().ok()?,
            hash: hash.to_string(),
        })
    }
}

/// The content of `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    /// The current commit's manifest, or `None` for a key taken over before its first commit.
    /// Its name carries the manifest's hash, so HEAD pins the exact bytes.
    pub commit: Option<ObjectName>,
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
        let name = ObjectName::of(10, b"bytes");
        assert_eq!(name.seq, 10);
        assert_eq!(name.hash.len(), 64);
        assert_eq!(ObjectName::parse(&name.pack()), Some(name.clone()));
        assert_eq!(ObjectName::parse(&name.manifest()), Some(name.clone()));
        assert_eq!(ObjectName::parse("HEAD"), None);
        assert_eq!(ObjectName::parse("packs/x.tar"), None);
        assert!(ObjectName::of(1, b"z").pack() < ObjectName::of(10, b"a").pack());
    }

    #[test]
    fn key_root_is_one_validated_segment() {
        assert_eq!(key_root("engine-42").unwrap(), "engine-42/");
        assert!(key_root("../other").is_err());
    }
}
