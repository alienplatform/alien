use alien_error::AlienError;
use chrono::Utc;

use crate::error::{ErrorData, Result};
use crate::layout::{Head, ObjectName, HEAD_OBJECT};
use crate::manifest::{Entry, EntryKind, Manifest};
use crate::pack::chunk_hash;
use crate::store::{ObjectStore, ObjectVersion, Precondition, PutOutcome};

/// Largest single ranged read a restore issues; adjacent chunks of one pack are merged up to it.
const MAX_RANGE: u64 = 8 * 1024 * 1024;

/// HEAD and its version, or `None` for a key that was never written.
pub async fn read_head(store: &dyn ObjectStore) -> Result<Option<(Head, ObjectVersion)>> {
    let Some(object) = store.get(HEAD_OBJECT).await? else {
        return Ok(None);
    };
    let head = serde_json::from_slice(&object.bytes).map_err(|e| {
        AlienError::new(ErrorData::Corrupt {
            object: HEAD_OBJECT.to_string(),
            reason: e.to_string(),
        })
    })?;
    Ok(Some((head, object.version)))
}

/// The manifest HEAD names, checked against the hash in its name and then parsed.
pub async fn read_manifest(store: &dyn ObjectStore, commit: &ObjectName) -> Result<Manifest> {
    let object = commit.manifest();
    let stored = store.get(&object).await?.ok_or_else(|| {
        AlienError::new(ErrorData::Corrupt {
            object: object.clone(),
            reason: "HEAD names a manifest that does not exist".to_string(),
        })
    })?;
    if chunk_hash(&stored.bytes) != commit.hash {
        return Err(AlienError::new(ErrorData::Corrupt {
            object,
            reason: "the manifest is not the one HEAD names".to_string(),
        }));
    }
    let manifest = Manifest::parse(&object, &stored.bytes)?;
    if manifest.seq != commit.seq {
        return Err(AlienError::new(ErrorData::Corrupt {
            object,
            reason: "the manifest names a different commit".to_string(),
        }));
    }
    Ok(manifest)
}

/// The result of taking a key over.
#[derive(Debug, Clone)]
pub struct Takeover {
    /// HEAD's version after the takeover; the new writer's commits match on it.
    pub head_version: ObjectVersion,
    /// The tree to restore, or `None` for a new key.
    pub manifest: Option<Manifest>,
}

/// Makes `writer` the key's only writer: rewrites HEAD naming the same commit and the new
/// writer, conditional on the version just read. From then on the previous writer's commits
/// fail their `If-Match` and it stops.
pub async fn takeover(store: &dyn ObjectStore, writer: &str) -> Result<Takeover> {
    // Two sandboxes taking the same key over at once: one put wins, the other rereads and
    // takes over from the winner, so the later create is the writer. Bounded so a store that
    // never agrees surfaces as an error instead of a loop.
    for _ in 0..8 {
        let current = read_head(store).await?;
        let (commit, precondition) = match &current {
            Some((head, version)) => (head.commit.clone(), Precondition::Matches(version.clone())),
            None => (None, Precondition::Absent),
        };
        let manifest = match &commit {
            Some(commit) => Some(read_manifest(store, commit).await?),
            None => None,
        };
        let head = Head {
            commit,
            writer: writer.to_string(),
            updated_at: Utc::now(),
        };
        match store
            .put_if(
                HEAD_OBJECT,
                serde_json::to_vec(&head).expect("HEAD serializes"),
                precondition,
            )
            .await?
        {
            PutOutcome::Written { version } => {
                return Ok(Takeover {
                    head_version: version,
                    manifest,
                })
            }
            PutOutcome::Conflict => continue,
        }
    }
    Err(AlienError::new(ErrorData::StoreFailed {
        operation: "put".to_string(),
        object: HEAD_OBJECT.to_string(),
        reason: "HEAD kept changing during takeover".to_string(),
    }))
}

/// Receives a restored tree. The agent's implementation writes as root with `openat` and
/// `O_NOFOLLOW` under the storage directory; Local writes a host directory.
pub trait TreeSink {
    /// Creates a directory. Its mode and mtime are applied in [`TreeSink::finish`], after its
    /// children are written.
    fn dir(&mut self, entry: &Entry) -> Result<()>;
    /// Starts a regular file.
    fn begin_file(&mut self, entry: &Entry) -> Result<()>;
    /// Appends to the file begun last.
    fn append(&mut self, bytes: &[u8]) -> Result<()>;
    /// Finishes the file begun last: owner, mode and mtime.
    fn end_file(&mut self, entry: &Entry) -> Result<()>;
    /// Creates a symlink without following anything.
    fn symlink(&mut self, entry: &Entry, target: &str) -> Result<()>;
    /// Creates another name for an earlier file.
    fn hard_link(&mut self, entry: &Entry, target: &str) -> Result<()>;
    /// Applies directory owners, modes and mtimes, deepest first.
    fn finish(&mut self, dirs: &[&Entry]) -> Result<()>;
}

/// Writes a manifest's tree into a sink, reading chunks with ranged gets merged per pack.
/// Every chunk is checked against its hash, so a damaged pack fails the restore instead of
/// starting a sandbox on wrong bytes.
pub async fn restore(
    store: &dyn ObjectStore,
    manifest: &Manifest,
    sink: &mut dyn TreeSink,
) -> Result<()> {
    let mut dirs = Vec::new();
    for entry in &manifest.entries {
        match &entry.kind {
            EntryKind::Dir => {
                sink.dir(entry)?;
                dirs.push(entry);
            }
            EntryKind::Symlink { target } => sink.symlink(entry, target)?,
            EntryKind::HardLink { target } => sink.hard_link(entry, target)?,
            EntryKind::File { chunks, .. } => {
                sink.begin_file(entry)?;
                let mut index = 0;
                while index < chunks.len() {
                    let (bytes, count) = fetch_run(store, manifest, &chunks[index..]).await?;
                    sink.append(&bytes)?;
                    index += count;
                }
                sink.end_file(entry)?;
            }
        }
    }
    dirs.reverse();
    sink.finish(&dirs)
}

/// Reads the longest run of chunks at the front of `chunks` that sit back to back in one pack,
/// up to [`MAX_RANGE`], and returns their bytes and how many chunks that was.
async fn fetch_run(
    store: &dyn ObjectStore,
    manifest: &Manifest,
    chunks: &[String],
) -> Result<(Vec<u8>, usize)> {
    let first = &manifest.chunks[&chunks[0]];
    // Tar puts one 512-byte header and the padding to a block between consecutive members.
    let mut end = first.offset + u64::from(first.len);
    let mut count = 1;
    for hash in &chunks[1..] {
        let next = &manifest.chunks[hash];
        let padded_end = end.div_ceil(512) * 512 + 512;
        if next.pack != first.pack
            || next.offset != padded_end
            || next.offset + u64::from(next.len) - first.offset > MAX_RANGE
        {
            break;
        }
        end = next.offset + u64::from(next.len);
        count += 1;
    }
    let object = first.pack.pack();
    let range = store
        .get_range(&object, first.offset, end - first.offset)
        .await?;
    let mut bytes = Vec::with_capacity(range.len());
    for hash in &chunks[..count] {
        let location = &manifest.chunks[hash];
        let start = (location.offset - first.offset) as usize;
        let chunk = range
            .get(start..start + location.len as usize)
            .ok_or_else(|| corrupt_chunk(&object, hash))?;
        if crate::pack::chunk_hash(chunk) != *hash {
            return Err(corrupt_chunk(&object, hash));
        }
        bytes.extend_from_slice(chunk);
    }
    Ok((bytes, count))
}

fn corrupt_chunk(object: &str, hash: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Corrupt {
        object: object.to_string(),
        reason: format!("chunk {hash} does not match its hash"),
    })
}

/// One file of a key's current tree, read without a sandbox. A hard link reads its target.
pub async fn read_file(store: &dyn ObjectStore, key: &str, path: &str) -> Result<Vec<u8>> {
    let not_found = || {
        AlienError::new(ErrorData::FileNotFound {
            key: key.to_string(),
            path: path.to_string(),
        })
    };
    let Some((
        Head {
            commit: Some(commit),
            ..
        },
        _,
    )) = read_head(store).await?
    else {
        return Err(not_found());
    };
    let manifest = read_manifest(store, &commit).await?;
    let mut entry = manifest.entry(path).ok_or_else(not_found)?;
    if let EntryKind::HardLink { target } = &entry.kind {
        entry = manifest.entry(target).ok_or_else(not_found)?;
    }
    let EntryKind::File { chunks, .. } = &entry.kind else {
        return Err(not_found());
    };
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < chunks.len() {
        let (run, count) = fetch_run(store, &manifest, &chunks[index..]).await?;
        bytes.extend_from_slice(&run);
        index += count;
    }
    Ok(bytes)
}
