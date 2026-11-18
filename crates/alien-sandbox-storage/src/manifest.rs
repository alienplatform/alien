use std::collections::BTreeMap;

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::layout::{validate_tree_path, ObjectName};

/// Format version written in every manifest. A reader refuses a newer one rather than
/// restoring a tree it only partly understands.
pub const MANIFEST_VERSION: u32 = 1;

/// The whole tree at one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Format version, [`MANIFEST_VERSION`] when written by this crate.
    pub version: u32,
    /// This commit's name.
    pub commit: ObjectName,
    /// The commit this one was built on, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectName>,
    /// The sandbox that wrote it.
    pub writer: String,
    /// Every entry, sorted by path, parents before children.
    pub entries: Vec<Entry>,
    /// Where each chunk the entries use is stored. Only chunks some entry uses are listed, so
    /// the packs named here are exactly the ones this commit keeps alive.
    pub chunks: BTreeMap<String, ChunkLocation>,
}

/// One file, directory or symlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Relative to the storage directory.
    pub path: String,
    /// What it is.
    #[serde(flatten)]
    pub kind: EntryKind,
    /// Permission bits including setuid, setgid and sticky (`0o7777` at most).
    pub mode: u32,
    /// Owner uid.
    pub uid: u32,
    /// Owner gid.
    pub gid: u32,
    /// Modification time in nanoseconds since the epoch.
    pub mtime_ns: i64,
}

/// The kind of an [`Entry`] and what only that kind carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntryKind {
    /// A directory.
    Dir,
    /// A regular file.
    #[serde(rename_all = "camelCase")]
    File {
        /// Length in bytes.
        size: u64,
        /// Chunk hashes in order; their lengths add up to `size`.
        chunks: Vec<String>,
        /// A change token from the writer's file system (ctime in nanoseconds), compared with
        /// size and mtime to skip rereading an unchanged file. Never restored.
        #[serde(default)]
        change_token: i64,
    },
    /// A symbolic link, restored with this target and never followed.
    Symlink {
        /// The link target, as written.
        target: String,
    },
    /// Another name for a regular file earlier in the manifest.
    #[serde(rename_all = "camelCase")]
    HardLink {
        /// The path of the first name, which carries the content.
        target: String,
    },
}

/// Where a chunk's bytes are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkLocation {
    /// The commit whose pack holds it.
    pub pack: ObjectName,
    /// Byte offset of the chunk's data in the pack.
    pub offset: u64,
    /// Length of the chunk.
    pub len: u32,
}

impl Manifest {
    /// Parses and checks a manifest: known version, clean paths, every hard link pointing at an
    /// earlier file, every chunk located, and file sizes matching their chunks.
    pub fn parse(object: &str, bytes: &[u8]) -> Result<Self> {
        let corrupt = |reason: String| {
            AlienError::new(ErrorData::Corrupt {
                object: object.to_string(),
                reason,
            })
        };
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| corrupt(e.to_string()))?;
        if manifest.version > MANIFEST_VERSION {
            return Err(corrupt(format!(
                "format version {} is newer than this reader's {MANIFEST_VERSION}",
                manifest.version
            )));
        }
        let mut files = std::collections::HashSet::new();
        let mut previous: Option<&str> = None;
        for entry in &manifest.entries {
            validate_tree_path(&entry.path).map_err(|e| corrupt(e.message.clone()))?;
            if previous.is_some_and(|p| p >= entry.path.as_str()) {
                return Err(corrupt("entries are not sorted and unique".to_string()));
            }
            previous = Some(&entry.path);
            if entry.mode > 0o7777 {
                return Err(corrupt(format!(
                    "'{}' has mode {:o}",
                    entry.path, entry.mode
                )));
            }
            match &entry.kind {
                EntryKind::File { size, chunks, .. } => {
                    let mut total = 0u64;
                    for hash in chunks {
                        let location = manifest
                            .chunks
                            .get(hash)
                            .ok_or_else(|| corrupt(format!("chunk {hash} has no location")))?;
                        total += u64::from(location.len);
                    }
                    if total != *size {
                        return Err(corrupt(format!(
                            "'{}' is {size} bytes but its chunks hold {total}",
                            entry.path
                        )));
                    }
                    files.insert(entry.path.as_str());
                }
                EntryKind::HardLink { target } => {
                    if !files.contains(target.as_str()) {
                        return Err(corrupt(format!(
                            "'{}' links to '{target}', which is not an earlier file",
                            entry.path
                        )));
                    }
                }
                EntryKind::Dir | EntryKind::Symlink { .. } => {}
            }
        }
        Ok(manifest)
    }

    /// Total bytes of file content; hard links count once.
    pub fn size(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| match &entry.kind {
                EntryKind::File { size, .. } => *size,
                _ => 0,
            })
            .sum()
    }

    /// The entry at `path`.
    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Every pack this manifest reads from.
    pub fn packs(&self) -> std::collections::BTreeSet<&ObjectName> {
        self.chunks
            .values()
            .map(|location| &location.pack)
            .collect()
    }
}
