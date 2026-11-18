use std::collections::BTreeMap;
use std::io::Read;

use alien_error::AlienError;
use chrono::Utc;

use crate::error::{ErrorData, Result};
use crate::layout::{validate_tree_path, Head, ObjectName, HEAD_OBJECT};
use crate::manifest::{ChunkLocation, Entry, EntryKind, Manifest, MANIFEST_VERSION};
use crate::pack::{PackBuilder, CHUNK_SIZE};
use crate::store::{ObjectStore, ObjectVersion, Precondition, PutOutcome};

/// One entry as a walk of the storage directory found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Relative to the storage directory.
    pub path: String,
    /// What it is.
    pub kind: TreeEntryKind,
    /// Permission bits (`0o7777` at most).
    pub mode: u32,
    /// Owner uid.
    pub uid: u32,
    /// Owner gid.
    pub gid: u32,
    /// Modification time in nanoseconds since the epoch.
    pub mtime_ns: i64,
}

/// A change token that matches nothing, not even itself: the file is reread at every commit
/// until a walk can give it a token. A walk gives it to a file changed in the clock tick the
/// walk started in, because a second change in that tick, of the same size, would leave size,
/// ctime and mtime as they were and could never be told apart from no change.
pub const UNKNOWN_CHANGE: i64 = 0;

/// The kind of a [`TreeEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEntryKind {
    /// A directory.
    Dir,
    /// A regular file; `size` is what the walk saw, the content read decides the final size.
    File {
        /// Length when walked.
        size: u64,
        /// ctime in nanoseconds, or any token that changes whenever the content may have.
        /// [`UNKNOWN_CHANGE`] means the token cannot be trusted and the file is always reread.
        change_token: i64,
    },
    /// A symbolic link.
    Symlink {
        /// The target, as read with `readlink`.
        target: String,
    },
    /// Another name for a file earlier in the walk.
    HardLink {
        /// The earlier name.
        target: String,
    },
}

/// Opens a walked file's content. The agent's implementation opens without following
/// symlinks and copies SQLite databases through the online backup API.
pub trait FileSource {
    /// A reader over the file's current content.
    fn open(&mut self, path: &str) -> Result<Box<dyn Read + Send + '_>>;
}

/// What a commit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// HEAD now names this commit.
    Committed {
        /// The new commit.
        commit: ObjectName,
        /// Bytes uploaded in its pack.
        pack_bytes: u64,
    },
    /// The tree is the same as HEAD's; nothing was written.
    Unchanged,
}

/// The one writer of a key: builds commits from walks and publishes them with a conditional
/// put on HEAD.
/// Largest pack a commit builds before it uploads it and starts the next. Bounds the writer's
/// memory, each upload and each object a reader fetches.
pub const MAX_PACK_BYTES: usize = 64 << 20;

pub struct Committer {
    pack_cap: usize,
    writer: String,
    head_version: ObjectVersion,
    head: Option<Manifest>,
    max_size: Option<u64>,
    superseded: bool,
}

impl Committer {
    /// A committer that continues from a takeover: `head_version` is the HEAD version the takeover
    /// wrote, `head` the manifest it restored.
    pub fn new(
        writer: impl Into<String>,
        head_version: ObjectVersion,
        head: Option<Manifest>,
        max_size: Option<u64>,
    ) -> Self {
        Self {
            pack_cap: MAX_PACK_BYTES,
            writer: writer.into(),
            head_version,
            head,
            max_size,
            superseded: false,
        }
    }

    /// Closes packs at `cap` bytes instead of [`MAX_PACK_BYTES`] (tests).
    pub fn with_pack_cap(mut self, cap: usize) -> Self {
        self.pack_cap = cap.max(1);
        self
    }

    /// Whether another writer took the key over; every later commit is refused.
    pub fn is_superseded(&self) -> bool {
        self.superseded
    }

    /// The manifest HEAD names, as this writer last wrote or restored it.
    pub fn head(&self) -> Option<&Manifest> {
        self.head.as_ref()
    }

    /// Builds a commit from a walk and publishes it: the pack, then the manifest, then HEAD
    /// with `If-Match` on the version this writer last saw. A refused HEAD put means another
    /// sandbox took the key over; the commit is dropped whole and this writer stops.
    pub async fn commit(
        &mut self,
        store: &dyn ObjectStore,
        key: &str,
        mut walk: Vec<TreeEntry>,
        source: &mut (dyn FileSource + Send),
    ) -> Result<CommitOutcome> {
        if self.superseded {
            return Err(AlienError::new(ErrorData::Superseded {
                key: key.to_string(),
            }));
        }
        walk.sort_by(|a, b| a.path.cmp(&b.path));
        walk.dedup_by(|a, b| a.path == b.path);
        let walk = order_hard_links(walk);
        let walked_size: u64 = walk
            .iter()
            .map(|entry| match entry.kind {
                TreeEntryKind::File { size, .. } => size,
                _ => 0,
            })
            .sum();
        self.check_size(walked_size)?;

        let seq = self.head.as_ref().map_or(0, |m| m.seq) + 1;
        let previous = self.head.as_ref();
        let mut packs = Packs::new(seq, self.pack_cap);
        let mut chunks: BTreeMap<String, ChunkLocation> = BTreeMap::new();
        let mut entries = Vec::with_capacity(walk.len());
        for item in walk {
            validate_tree_path(&item.path)?;
            let kind = match item.kind {
                TreeEntryKind::Dir => EntryKind::Dir,
                TreeEntryKind::Symlink { target } => EntryKind::Symlink { target },
                TreeEntryKind::HardLink { target } => EntryKind::HardLink { target },
                TreeEntryKind::File { size, change_token } => {
                    let unchanged = previous.and_then(|m| m.entry(&item.path)).and_then(|old| {
                        match &old.kind {
                            EntryKind::File {
                                size: old_size,
                                chunks: old_chunks,
                                change_token: old_token,
                            } if *old_size == size
                                && *old_token == change_token
                                && change_token != UNKNOWN_CHANGE
                                && old.mtime_ns == item.mtime_ns =>
                            {
                                Some(old_chunks.clone())
                            }
                            _ => None,
                        }
                    });
                    let (size, file_chunks) = match unchanged {
                        Some(file_chunks) => {
                            for hash in &file_chunks {
                                let location = previous
                                    .and_then(|m| m.chunks.get(hash))
                                    .expect("a parsed manifest locates every chunk");
                                chunks.insert(hash.clone(), location.clone());
                            }
                            (size, file_chunks)
                        }
                        None => {
                            read_file(store, source, &item.path, previous, &mut packs, &mut chunks)
                                .await?
                        }
                    };
                    EntryKind::File {
                        size,
                        chunks: file_chunks,
                        change_token,
                    }
                }
            };
            entries.push(Entry {
                path: item.path,
                kind,
                mode: item.mode & 0o7777,
                uid: item.uid,
                gid: item.gid,
                mtime_ns: item.mtime_ns,
            });
        }
        // A file that grew while it was read is checked again on what was actually read.
        let mut manifest = Manifest {
            version: MANIFEST_VERSION,
            seq,
            writer: self.writer.clone(),
            entries,
            chunks,
        };
        self.check_size(manifest.size())?;
        if previous.is_some_and(|old| old.entries == manifest.entries) {
            return Ok(CommitOutcome::Unchanged);
        }
        packs.flush(store, &mut manifest.chunks).await?;
        let pack_bytes = packs.uploaded;

        let manifest_bytes = serde_json::to_vec(&manifest).expect("a manifest serializes");
        let name = ObjectName::of(seq, &manifest_bytes);
        create_named(store, &name.manifest(), manifest_bytes).await?;
        let head = Head {
            commit: Some(name.clone()),
            writer: self.writer.clone(),
            updated_at: Utc::now(),
        };
        let outcome = store
            .put_if(
                HEAD_OBJECT,
                serde_json::to_vec(&head).expect("HEAD serializes"),
                Precondition::Matches(self.head_version.clone()),
            )
            .await?;
        match outcome {
            PutOutcome::Written { version } => {
                self.head_version = version;
                self.head = Some(manifest);
                Ok(CommitOutcome::Committed {
                    commit: name,
                    pack_bytes,
                })
            }
            PutOutcome::Conflict => {
                self.superseded = true;
                Err(AlienError::new(ErrorData::Superseded {
                    key: key.to_string(),
                }))
            }
        }
    }

    fn check_size(&self, size: u64) -> Result<()> {
        match self.max_size {
            Some(max_size) if size > max_size => Err(AlienError::new(ErrorData::MaxSizeExceeded {
                size,
                max_size,
            })),
            _ => Ok(()),
        }
    }
}

/// Creates a content-addressed object. A name that already exists holds the same bytes (the
/// name is their hash), so finding it is success, not a conflict.
async fn create_named(store: &dyn ObjectStore, object: &str, bytes: Vec<u8>) -> Result<()> {
    store
        .put_if(object, bytes, crate::store::Precondition::Absent)
        .await
        .map(|_| ())
}

/// Makes the first name of every hard-link group, in path order, the one that carries the
/// content, so a manifest sorted by path always lists a link after its target. A link whose
/// target is not a walked file becomes nothing: the walk saw the name but not the file.
fn order_hard_links(walk: Vec<TreeEntry>) -> Vec<TreeEntry> {
    let files: BTreeMap<String, usize> = walk
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, TreeEntryKind::File { .. }))
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    // Each group, keyed by the walked file, gets its smallest member path.
    let mut primary: BTreeMap<String, String> = files
        .keys()
        .map(|path| (path.clone(), path.clone()))
        .collect();
    for entry in &walk {
        if let TreeEntryKind::HardLink { target } = &entry.kind {
            if let Some(first) = primary.get_mut(target) {
                if entry.path < *first {
                    *first = entry.path.clone();
                }
            }
        }
    }
    let mut out = Vec::with_capacity(walk.len());
    for entry in walk.iter() {
        let group = match &entry.kind {
            TreeEntryKind::File { .. } => Some(entry.path.as_str()),
            TreeEntryKind::HardLink { target } => {
                if !files.contains_key(target) {
                    continue;
                }
                Some(target.as_str())
            }
            _ => None,
        };
        let Some(group) = group else {
            out.push(entry.clone());
            continue;
        };
        let first = &primary[group];
        let file = &walk[files[group]];
        if entry.path == *first {
            // The content-carrying name takes the file's kind and metadata; a source opens it
            // by this name, which is the same inode.
            out.push(TreeEntry {
                path: entry.path.clone(),
                ..file.clone()
            });
        } else {
            out.push(TreeEntry {
                kind: TreeEntryKind::HardLink {
                    target: first.clone(),
                },
                ..entry.clone()
            });
        }
    }
    out
}

/// The packs one commit writes: chunks go into the open pack, which is uploaded and replaced
/// once it reaches the cap. Chunks in a pack not yet uploaded point at a placeholder name until
/// its hash, and so its name, is known.
struct Packs {
    seq: u64,
    cap: usize,
    open: PackBuilder,
    placeholder: ObjectName,
    closed: usize,
    uploaded: u64,
}

impl Packs {
    fn new(seq: u64, cap: usize) -> Self {
        Self {
            seq,
            cap,
            open: PackBuilder::new(),
            placeholder: Self::placeholder(seq, 0),
            closed: 0,
            uploaded: 0,
        }
    }

    fn placeholder(seq: u64, index: usize) -> ObjectName {
        ObjectName {
            seq,
            hash: format!("open-{index}"),
        }
    }

    /// Uploads the open pack, if it holds anything, and points its chunks at its real name.
    async fn flush(
        &mut self,
        store: &dyn ObjectStore,
        chunks: &mut BTreeMap<String, ChunkLocation>,
    ) -> Result<()> {
        if self.open.is_empty() {
            return Ok(());
        }
        let pack = std::mem::take(&mut self.open).finish();
        let name = ObjectName::of(self.seq, &pack);
        self.uploaded += pack.len() as u64;
        create_named(store, &name.pack(), pack).await?;
        for location in chunks.values_mut() {
            if location.pack == self.placeholder {
                location.pack = name.clone();
            }
        }
        self.closed += 1;
        self.placeholder = Self::placeholder(self.seq, self.closed);
        Ok(())
    }
}

/// Reads a file in fixed chunks, adding the ones no earlier pack holds, and uploads a pack
/// whenever it reaches the cap, so a large file never sits whole in memory.
async fn read_file(
    store: &dyn ObjectStore,
    source: &mut (dyn FileSource + Send),
    path: &str,
    previous: Option<&Manifest>,
    packs: &mut Packs,
    chunks: &mut BTreeMap<String, ChunkLocation>,
) -> Result<(u64, Vec<String>)> {
    let io = |e: std::io::Error| {
        AlienError::new(ErrorData::TreeIo {
            operation: "read".to_string(),
            path: path.to_string(),
            reason: e.to_string(),
        })
    };
    let mut reader = source.open(path)?;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut size = 0u64;
    let mut hashes = Vec::new();
    loop {
        let mut filled = 0;
        while filled < CHUNK_SIZE {
            let read = reader.read(&mut buffer[filled..]).map_err(io)?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled == 0 {
            break;
        }
        let bytes = &buffer[..filled];
        let hash = crate::pack::chunk_hash(bytes);
        if !chunks.contains_key(&hash) {
            let location = match previous.and_then(|m| m.chunks.get(&hash)) {
                Some(location) => location.clone(),
                None => {
                    let (_, offset, len) = packs.open.add(bytes);
                    ChunkLocation {
                        pack: packs.placeholder.clone(),
                        offset,
                        len,
                    }
                }
            };
            chunks.insert(hash.clone(), location);
            if packs.open.len() >= packs.cap {
                packs.flush(store, chunks).await?;
            }
        }
        hashes.push(hash);
        size += filled as u64;
        if filled < CHUNK_SIZE {
            break;
        }
    }
    Ok((size, hashes))
}
