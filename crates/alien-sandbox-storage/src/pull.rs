//! The pull transport: the agent commits into a spool inside the sandbox, and the binding, which
//! holds the store's credentials, drains the spool over the agent's inbound channel and publishes
//! it. Used where the sandbox has no route to object storage (GCP Agent Platform under PSC).
//!
//! The agent side is [`Spool`]: an [`ObjectStore`] over an agent-only directory that the agent's
//! [`crate::Committer`] writes to exactly as it would write the real store. It remembers which
//! objects are not yet published and the HEAD the last commit wrote.
//!
//! The binding side is [`Puller`]: it asks the agent for what is pending, reads each object in
//! pages no larger than the channel's reply limit, checks every object against the hash in its
//! name, creates it in the store, then moves HEAD with a conditional put on the version its
//! takeover wrote. A refused HEAD put means another sandbox took the key over; the puller tells
//! the agent, whose spool then refuses every later commit, so its replicator stops.
//!
//! Intermediate commits the binding never saw are skipped, not lost: every manifest names the
//! whole tree and where each of its chunks lives, and the packs of skipped commits are still
//! pending, so publishing the latest HEAD with every pending object is a complete commit.
//!
//! For a restore the binding does the reverse with [`seed`]: it copies the manifest HEAD names
//! and the packs it reads from the store into the spool, and the agent restores from the spool.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use alien_error::AlienError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::layout::{Head, ObjectName, HEAD_OBJECT};
use crate::manifest::Manifest;
use crate::pack::chunk_hash;
use crate::store::{ObjectStore, ObjectVersion, Precondition, PutOutcome, StoredObject};

/// What the agent has committed and the binding has not yet published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// Identifies this snapshot of the spool; the binding acknowledges it.
    pub generation: u64,
    /// The HEAD bytes the agent's last commit wrote.
    pub head: String,
    /// Packs and manifests to create before HEAD moves, in the order they were written.
    pub objects: Vec<PendingObject>,
    /// When the oldest commit not yet published was made: how long writes have waited for the
    /// binding. Absent from an agent older than the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

/// One object waiting in the spool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingObject {
    /// Relative to the key root (`packs/…` or `manifests/…`).
    pub name: String,
    /// Its length in bytes.
    pub size: u64,
}

/// What the binding did with a [`Pending`] snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AckOutcome {
    /// HEAD moved to the snapshot's commit; its objects may be dropped from the spool.
    Published,
    /// Another sandbox owns the key; the spool refuses every later commit.
    Superseded,
}

/// A name the spool will hold: a pack or a manifest, never HEAD or a path.
fn spooled_name(object: &str) -> Result<ObjectName> {
    let name = ObjectName::parse(object).filter(|name| {
        name.hash.len() == 64
            && name
                .hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && (name.pack() == object || name.manifest() == object)
    });
    name.ok_or_else(|| {
        AlienError::new(ErrorData::StoreFailed {
            operation: "spool".to_string(),
            object: object.to_string(),
            reason: "only packs and manifests are spooled".to_string(),
        })
    })
}

fn spool_io(operation: &str, object: &str, error: std::io::Error) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreFailed {
        operation: operation.to_string(),
        object: object.to_string(),
        reason: error.to_string(),
    })
}

#[derive(Debug)]
struct SpoolState {
    /// HEAD's version as the agent's committer sees it.
    head_version: ObjectVersion,
    /// HEAD written by the last unpublished commit.
    head: Option<Vec<u8>>,
    /// Advances on every write; a pending object and HEAD carry the value when written.
    tick: u64,
    /// The tick of the last HEAD written.
    generation: u64,
    /// Unpublished objects, with the tick they were written at.
    pending: BTreeMap<String, u64>,
    /// When each unpublished HEAD was written, by its tick.
    commits: BTreeMap<u64, chrono::DateTime<chrono::Utc>>,
    /// Objects the binding copied in for a restore.
    seeded: BTreeSet<String>,
    superseded: bool,
}

/// The agent's side of the pull transport: an [`ObjectStore`] over an agent-only directory.
///
/// Objects are files named after the object with `/` replaced, written to a temporary name and
/// renamed, so a reader never sees half an object. Memory holds names and HEAD only.
#[derive(Debug)]
pub struct Spool {
    dir: PathBuf,
    state: Mutex<SpoolState>,
}

impl Spool {
    /// A spool in `dir` (created if missing, emptied of anything a previous agent left) whose
    /// HEAD is at `head_version`, the version the binding's takeover wrote. The committer that writes
    /// here is created with the same version.
    pub fn new(dir: impl Into<PathBuf>, head_version: ObjectVersion) -> Result<Self> {
        let dir = dir.into();
        let display = dir.display().to_string();
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| spool_io("clear", &display, e))?;
        }
        fs::create_dir_all(&dir).map_err(|e| spool_io("create", &display, e))?;
        Ok(Self {
            dir,
            state: Mutex::new(SpoolState {
                head_version,
                head: None,
                tick: 0,
                generation: 0,
                pending: BTreeMap::new(),
                commits: BTreeMap::new(),
                seeded: BTreeSet::new(),
                superseded: false,
            }),
        })
    }

    fn path(&self, object: &str) -> PathBuf {
        self.dir.join(object.replace('/', "%"))
    }

    fn partial(&self, object: &str) -> PathBuf {
        self.dir.join(format!("{}.part", object.replace('/', "%")))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, SpoolState> {
        self.state.lock().expect("not poisoned")
    }

    /// HEAD's version as a committer writing here must name it next. A spool's versions are its
    /// own; the binding's [`Puller`] holds the store's.
    pub fn head_version(&self) -> ObjectVersion {
        self.state().head_version.clone()
    }

    /// Whether the binding reported that another sandbox owns the key.
    pub fn is_superseded(&self) -> bool {
        self.state().superseded
    }

    /// The unpublished commit, or `None` when everything committed is published.
    pub fn pending(&self) -> Option<Pending> {
        let state = self.state();
        let head = state.head.as_ref()?;
        let mut objects: Vec<(u64, PendingObject)> = state
            .pending
            .iter()
            .filter(|(_, tick)| **tick < state.generation)
            .filter_map(|(name, tick)| {
                let size = fs::metadata(self.path(name)).ok()?.len();
                Some((
                    *tick,
                    PendingObject {
                        name: name.clone(),
                        size,
                    },
                ))
            })
            .collect();
        objects.sort_by_key(|(tick, _)| *tick);
        Some(Pending {
            generation: state.generation,
            head: String::from_utf8_lossy(head).into_owned(),
            objects: objects.into_iter().map(|(_, object)| object).collect(),
            since: state.commits.values().next().copied(),
        })
    }

    /// `len` bytes of a pending or seeded object from `offset`; fewer at its end.
    pub fn read(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        {
            let state = self.state();
            if !state.pending.contains_key(object) && !state.seeded.contains(object) {
                return Err(AlienError::new(ErrorData::StoreFailed {
                    operation: "read".to_string(),
                    object: object.to_string(),
                    reason: "not in the spool".to_string(),
                }));
            }
        }
        let mut file =
            fs::File::open(self.path(object)).map_err(|e| spool_io("read", object, e))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| spool_io("read", object, e))?;
        let mut bytes = Vec::new();
        file.take(len)
            .read_to_end(&mut bytes)
            .map_err(|e| spool_io("read", object, e))?;
        Ok(bytes)
    }

    /// Records what the binding did with the snapshot at `generation`.
    pub fn ack(&self, generation: u64, outcome: AckOutcome) -> Result<()> {
        let mut state = self.state();
        match outcome {
            AckOutcome::Superseded => {
                state.superseded = true;
                state.head = None;
                state.commits.clear();
            }
            AckOutcome::Published => {
                if generation > state.generation {
                    return Err(AlienError::new(ErrorData::StoreFailed {
                        operation: "ack".to_string(),
                        object: HEAD_OBJECT.to_string(),
                        reason: format!(
                            "generation {generation} was never pending (latest {})",
                            state.generation
                        ),
                    }));
                }
                let published: Vec<String> = state
                    .pending
                    .iter()
                    .filter(|(_, tick)| **tick < generation)
                    .map(|(name, _)| name.clone())
                    .collect();
                for name in published {
                    state.pending.remove(&name);
                    let _ = fs::remove_file(self.path(&name));
                }
                state.commits.retain(|tick, _| *tick > generation);
                if generation == state.generation {
                    state.head = None;
                }
            }
        }
        Ok(())
    }

    /// Writes one page of an object the binding copies in for a restore. Pages arrive in order
    /// (`offset` is the bytes already received); the last one makes the object readable.
    pub fn seed(&self, object: &str, offset: u64, bytes: &[u8], last: bool) -> Result<()> {
        spooled_name(object)?;
        let partial = self.partial(object);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&partial)
            .map_err(|e| spool_io("seed", object, e))?;
        let have = file
            .metadata()
            .map_err(|e| spool_io("seed", object, e))?
            .len();
        if have != offset {
            return Err(AlienError::new(ErrorData::StoreFailed {
                operation: "seed".to_string(),
                object: object.to_string(),
                reason: format!("page at {offset}, but {have} bytes are already here"),
            }));
        }
        file.write_all(bytes)
            .map_err(|e| spool_io("seed", object, e))?;
        if last {
            file.sync_all().map_err(|e| spool_io("seed", object, e))?;
            fs::rename(&partial, self.path(object)).map_err(|e| spool_io("seed", object, e))?;
            self.state().seeded.insert(object.to_string());
        }
        Ok(())
    }

    /// Drops what a restore was seeded with; the committer only needs the manifest it restored.
    pub fn clear_seeded(&self) {
        let seeded = std::mem::take(&mut self.state().seeded);
        let state = self.state();
        for name in seeded {
            if !state.pending.contains_key(&name) {
                let _ = fs::remove_file(self.path(&name));
            }
        }
    }

    fn write_new(&self, object: &str, bytes: &[u8]) -> Result<bool> {
        let path = self.path(object);
        if path.exists() {
            return Ok(false);
        }
        let partial = self.partial(object);
        let mut file = fs::File::create(&partial).map_err(|e| spool_io("put", object, e))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| spool_io("put", object, e))?;
        fs::rename(&partial, &path).map_err(|e| spool_io("put", object, e))?;
        Ok(true)
    }
}

#[async_trait]
impl ObjectStore for Spool {
    async fn get(&self, object: &str) -> Result<Option<StoredObject>> {
        if object == HEAD_OBJECT {
            let state = self.state();
            return Ok(state.head.as_ref().map(|bytes| StoredObject {
                bytes: bytes.clone(),
                version: state.head_version.clone(),
            }));
        }
        spooled_name(object)?;
        match fs::read(self.path(object)) {
            Ok(bytes) => Ok(Some(StoredObject {
                bytes,
                version: ObjectVersion::default(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(spool_io("get", object, e)),
        }
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        spooled_name(object)?;
        let mut file = fs::File::open(self.path(object)).map_err(|e| spool_io("get", object, e))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| spool_io("get", object, e))?;
        let mut bytes = Vec::new();
        file.take(len)
            .read_to_end(&mut bytes)
            .map_err(|e| spool_io("get", object, e))?;
        Ok(bytes)
    }

    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> Result<PutOutcome> {
        if object == HEAD_OBJECT {
            let mut state = self.state();
            let matches = matches!(&precondition, Precondition::Matches(version) if *version == state.head_version);
            if state.superseded || !matches {
                return Ok(PutOutcome::Conflict);
            }
            state.tick += 1;
            state.generation = state.tick;
            state.head_version = ObjectVersion::etag(format!("spool-{}", state.tick));
            state.head = Some(bytes);
            let tick = state.tick;
            state.commits.insert(tick, chrono::Utc::now());
            return Ok(PutOutcome::Written {
                version: state.head_version.clone(),
            });
        }
        spooled_name(object)?;
        if precondition != Precondition::Absent {
            return Ok(PutOutcome::Conflict);
        }
        if !self.write_new(object, &bytes)? {
            return Ok(PutOutcome::Conflict);
        }
        let mut state = self.state();
        state.tick += 1;
        let tick = state.tick;
        state.pending.insert(object.to_string(), tick);
        Ok(PutOutcome::Written {
            version: ObjectVersion::default(),
        })
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let state = self.state();
        Ok(state
            .pending
            .keys()
            .chain(state.seeded.iter())
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn delete(&self, object: &str) -> Result<()> {
        spooled_name(object)?;
        let mut state = self.state();
        state.pending.remove(object);
        state.seeded.remove(object);
        match fs::remove_file(self.path(object)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(spool_io("delete", object, e))
            }
            _ => Ok(()),
        }
    }
}

/// The agent as the binding reaches it: each call is one request over the inbound channel.
#[async_trait]
pub trait PullChannel: Send + Sync {
    /// The unpublished commit, or `None`.
    async fn pending(&self) -> Result<Option<Pending>>;
    /// Up to `len` bytes of a pending or seeded object from `offset`.
    async fn read(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>>;
    /// Reports what was done with a snapshot.
    async fn ack(&self, generation: u64, outcome: AckOutcome) -> Result<()>;
    /// Writes one page of an object for a restore.
    async fn seed(&self, object: &str, offset: u64, bytes: Vec<u8>, last: bool) -> Result<()>;
}

/// An in-process channel to a spool, for tests and for a binding that runs beside the agent.
#[async_trait]
impl PullChannel for Spool {
    async fn pending(&self) -> Result<Option<Pending>> {
        Ok(Spool::pending(self))
    }
    async fn read(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        Spool::read(self, object, offset, len)
    }
    async fn ack(&self, generation: u64, outcome: AckOutcome) -> Result<()> {
        Spool::ack(self, generation, outcome)
    }
    async fn seed(&self, object: &str, offset: u64, bytes: Vec<u8>, last: bool) -> Result<()> {
        Spool::seed(self, object, offset, &bytes, last)
    }
}

/// What one [`Puller::pull`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// Nothing was pending.
    Idle,
    /// HEAD now names the agent's latest commit.
    Published {
        /// The commit HEAD names (or `None` for a HEAD without a commit).
        commit: Option<ObjectName>,
        /// Bytes copied from the agent.
        bytes: u64,
    },
}

/// The binding's side of the pull transport for one key.
#[derive(Debug)]
pub struct Puller {
    writer: String,
    head_version: ObjectVersion,
    page: u64,
    superseded: bool,
}

impl Puller {
    /// A puller for the sandbox `writer`, which took the key over at `head_version`. `page` is the
    /// most raw bytes one read may return over the channel.
    pub fn new(writer: impl Into<String>, head_version: ObjectVersion, page: u64) -> Self {
        Self {
            writer: writer.into(),
            head_version,
            page: page.max(1),
            superseded: false,
        }
    }

    /// Publishes whatever the agent committed since the last pull.
    ///
    /// Every object is checked against the hash in its name before it is created, so a damaged
    /// or substituted page is refused rather than published. Objects are created only if absent;
    /// one that already exists holds the same bytes (its name is their hash). HEAD moves last,
    /// conditional on the version this puller last wrote, so a refused put means another sandbox
    /// owns the key; the agent is told, and this puller refuses every later pull.
    pub async fn pull(
        &mut self,
        channel: &dyn PullChannel,
        store: &dyn ObjectStore,
        key: &str,
    ) -> Result<PullOutcome> {
        if self.superseded {
            return Err(AlienError::new(ErrorData::Superseded {
                key: key.to_string(),
            }));
        }
        let Some(pending) = channel.pending().await? else {
            return Ok(PullOutcome::Idle);
        };
        let head: Head = serde_json::from_str(&pending.head).map_err(|e| {
            AlienError::new(ErrorData::Corrupt {
                object: HEAD_OBJECT.to_string(),
                reason: format!("the agent's HEAD does not parse: {e}"),
            })
        })?;
        if head.writer != self.writer {
            return Err(AlienError::new(ErrorData::Corrupt {
                object: HEAD_OBJECT.to_string(),
                reason: format!(
                    "the agent's HEAD names writer '{}', not this sandbox '{}'",
                    head.writer, self.writer
                ),
            }));
        }

        let mut copied = 0u64;
        for object in &pending.objects {
            let name = spooled_name(&object.name)?;
            let bytes = match self.read_object(channel, object).await {
                Ok(bytes) => bytes,
                // Another puller of this sandbox (a second binding instance) published this
                // snapshot meanwhile, and its ack dropped the objects from the spool: the commit
                // is in the store, nothing is left for this pull.
                Err(error) => match channel.pending().await {
                    Ok(now) if now.as_ref().map(|p| p.generation) != Some(pending.generation) => {
                        return Ok(PullOutcome::Idle);
                    }
                    _ => return Err(error),
                },
            };
            if chunk_hash(&bytes) != name.hash {
                return Err(AlienError::new(ErrorData::Corrupt {
                    object: object.name.clone(),
                    reason: "the bytes read from the agent do not match their name".to_string(),
                }));
            }
            copied += bytes.len() as u64;
            store
                .put_if(&object.name, bytes, Precondition::Absent)
                .await?;
        }
        if let Some(commit) = &head.commit {
            if store.get(&commit.manifest()).await?.is_none() {
                return Err(AlienError::new(ErrorData::Corrupt {
                    object: commit.manifest(),
                    reason: "the agent's HEAD names a manifest it did not send".to_string(),
                }));
            }
        }

        let ours = head.commit.as_ref().map(|commit| commit.seq);
        let bytes = pending.head.into_bytes();
        // A conflict means HEAD moved since this puller last wrote it. Another binding instance
        // pulling the same sandbox moves it too, with this sandbox as the writer: then the newer
        // of the two commits stays (a sandbox's commits only grow) and the sandbox carries on.
        // Only a HEAD naming another writer supersedes this sandbox. Bounded: each retry follows
        // a publish by this same sandbox.
        for _ in 0..4 {
            match store
                .put_if(
                    HEAD_OBJECT,
                    bytes.clone(),
                    Precondition::Matches(self.head_version.clone()),
                )
                .await?
            {
                PutOutcome::Written { version } => {
                    self.head_version = version;
                    channel
                        .ack(pending.generation, AckOutcome::Published)
                        .await?;
                    return Ok(PullOutcome::Published {
                        commit: head.commit,
                        bytes: copied,
                    });
                }
                PutOutcome::Conflict => match crate::read_head(store).await? {
                    Some((current, version)) if current.writer == self.writer => {
                        self.head_version = version;
                        if current.commit.as_ref().map(|commit| commit.seq) >= ours {
                            channel
                                .ack(pending.generation, AckOutcome::Published)
                                .await?;
                            return Ok(PullOutcome::Published {
                                commit: current.commit,
                                bytes: copied,
                            });
                        }
                    }
                    _ => {
                        self.superseded = true;
                        channel
                            .ack(pending.generation, AckOutcome::Superseded)
                            .await?;
                        return Err(AlienError::new(ErrorData::Superseded {
                            key: key.to_string(),
                        }));
                    }
                },
            }
        }
        Err(AlienError::new(ErrorData::StoreFailed {
            operation: "put".to_string(),
            object: HEAD_OBJECT.to_string(),
            reason: "HEAD kept moving under this sandbox's own commits; retried next pull"
                .to_string(),
        }))
    }

    async fn read_object(
        &self,
        channel: &dyn PullChannel,
        object: &PendingObject,
    ) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(object.size as usize);
        while (bytes.len() as u64) < object.size {
            let want = (object.size - bytes.len() as u64).min(self.page);
            let page = channel.read(&object.name, bytes.len() as u64, want).await?;
            if page.is_empty() {
                return Err(AlienError::new(ErrorData::Corrupt {
                    object: object.name.clone(),
                    reason: format!(
                        "the agent's copy ended at {} of {} bytes",
                        bytes.len(),
                        object.size
                    ),
                }));
            }
            bytes.extend_from_slice(&page);
        }
        Ok(bytes)
    }
}

/// Copies what a restore of `commit` reads from the store into the agent's spool, in pages of
/// at most `page` bytes: the manifest, then each pack its chunks live in, up to the end of the
/// last chunk the manifest reads from it (a restore reads nothing past that). Returns the
/// manifest, which the agent restores from the spool with [`crate::restore`].
pub async fn seed(
    channel: &dyn PullChannel,
    store: &dyn ObjectStore,
    commit: &ObjectName,
    page: u64,
) -> Result<Manifest> {
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
    let page = page.max(1);
    let mut offset = 0usize;
    for bytes in stored.bytes.chunks(page as usize) {
        let last = offset + bytes.len() == stored.bytes.len();
        channel
            .seed(&object, offset as u64, bytes.to_vec(), last)
            .await?;
        offset += bytes.len();
    }
    if stored.bytes.is_empty() {
        channel.seed(&object, 0, Vec::new(), true).await?;
    }

    let mut extents: BTreeMap<String, u64> = BTreeMap::new();
    for location in manifest.chunks.values() {
        let end = location.offset + u64::from(location.len);
        let extent = extents.entry(location.pack.pack()).or_default();
        *extent = (*extent).max(end);
    }
    for (pack, size) in extents {
        let mut offset = 0u64;
        loop {
            let len = (size - offset).min(page);
            let bytes = store.get_range(&pack, offset, len).await?;
            if bytes.len() as u64 != len {
                return Err(AlienError::new(ErrorData::Corrupt {
                    object: pack.clone(),
                    reason: format!("the store returned {} of {len} bytes", bytes.len()),
                }));
            }
            let last = offset + len >= size;
            channel.seed(&pack, offset, bytes, last).await?;
            offset += len;
            if last {
                break;
            }
        }
    }
    Ok(manifest)
}
