use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;

/// An object and the version a conditional put compares against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// The object's bytes.
    pub bytes: Vec<u8>,
    /// The store's version tag (an S3 or GCS `ETag`, an Azure `ETag`).
    pub etag: String,
}

/// When a conditional put may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Precondition {
    /// Only if the object does not exist (`If-None-Match: *`).
    Absent,
    /// Only if the object's version is this one (`If-Match`).
    Matches(String),
}

/// What a conditional put did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    /// Written; the new version.
    Written {
        /// The new version tag.
        etag: String,
    },
    /// The precondition failed and nothing was written.
    Conflict,
}

/// One key's objects, addressed relative to its root (`HEAD`, `packs/…`, `manifests/…`).
///
/// A transport implements this; the layout and the one-writer rule are the crate's. A writer
/// that only commits needs `put`, `put_if` and the reads it restores from; listing and deletion
/// are for the binding that collects garbage and erases keys.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// The object, or `None` if it does not exist.
    async fn get(&self, object: &str) -> Result<Option<StoredObject>>;
    /// `len` bytes of an existing object from `offset`.
    async fn get_range(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>>;
    /// Writes an object unconditionally.
    async fn put(&self, object: &str, bytes: Vec<u8>) -> Result<()>;
    /// Writes an object only if the precondition holds.
    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> Result<PutOutcome>;
    /// Every object whose name starts with `prefix`.
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;
    /// Deletes an object; deleting a missing one is not an error.
    async fn delete(&self, object: &str) -> Result<()>;
}

/// An in-memory store with the same conditional-put semantics as object storage, for tests
/// and for anything that must behave like the store without one.
#[derive(Debug, Default)]
pub struct MemoryStore {
    objects: Mutex<BTreeMap<String, (Vec<u8>, u64)>>,
    version: Mutex<u64>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn next_version(&self) -> u64 {
        let mut version = self.version.lock().expect("not poisoned");
        *version += 1;
        *version
    }

    /// Every object name, for assertions.
    pub fn names(&self) -> Vec<String> {
        self.objects
            .lock()
            .expect("not poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl ObjectStore for MemoryStore {
    async fn get(&self, object: &str) -> Result<Option<StoredObject>> {
        Ok(self
            .objects
            .lock()
            .expect("not poisoned")
            .get(object)
            .map(|(bytes, version)| StoredObject {
                bytes: bytes.clone(),
                etag: version.to_string(),
            }))
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let objects = self.objects.lock().expect("not poisoned");
        let bytes = objects.get(object).map(|(bytes, _)| bytes).ok_or_else(|| {
            alien_error::AlienError::new(crate::ErrorData::StoreFailed {
                operation: "get".to_string(),
                object: object.to_string(),
                reason: "no such object".to_string(),
            })
        })?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = start.saturating_add(len as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn put(&self, object: &str, bytes: Vec<u8>) -> Result<()> {
        let version = self.next_version();
        self.objects
            .lock()
            .expect("not poisoned")
            .insert(object.to_string(), (bytes, version));
        Ok(())
    }

    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> Result<PutOutcome> {
        let version = self.next_version();
        let mut objects = self.objects.lock().expect("not poisoned");
        let current = objects.get(object).map(|(_, v)| v.to_string());
        let allowed = match (&precondition, &current) {
            (Precondition::Absent, None) => true,
            (Precondition::Matches(expected), Some(current)) => expected == current,
            _ => false,
        };
        if !allowed {
            return Ok(PutOutcome::Conflict);
        }
        objects.insert(object.to_string(), (bytes, version));
        Ok(PutOutcome::Written {
            etag: version.to_string(),
        })
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .objects
            .lock()
            .expect("not poisoned")
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn delete(&self, object: &str) -> Result<()> {
        self.objects.lock().expect("not poisoned").remove(object);
        Ok(())
    }
}
