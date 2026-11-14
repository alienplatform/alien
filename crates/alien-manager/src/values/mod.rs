//! Deployment values held by the manager.
//!
//! Environment variables and stack inputs that a deployment's workloads read
//! are kept here, in the store of the manager that deploys them. Other
//! services (a dashboard, a control plane) hold only metadata: the slot, its
//! kind, when it changed, and a keyed digest of the value.
//!
//! A value lives in a [`ValueScope`] under a [`ValueSlot`]. Backends decide
//! where the bytes go: the manager's key-value binding (DynamoDB, Firestore,
//! Table Storage) or its encrypted SQLite file.

mod access;
mod backend;
mod cipher;
mod container_env;
mod digest_key;
mod durability;
mod generated;
mod hydrate;
mod limiter;
mod standalone;
mod state;
mod types;

pub use access::{ValueAccess, ValueAccessAuthorizer, ValueGrant, ValueMove};
pub use backend::{KvValueBackend, ValueBackend};
pub use cipher::{EnvelopeCipher, ValueCipher, DATA_KEY_CACHE_TTL};
pub use container_env::load_container_env;
pub use digest_key::{load_or_create_digest_key, DIGEST_KEY_NAME};
pub use durability::state_dir_is_ephemeral;
pub use generated::{with_generated_inputs, GeneratedInputs};
pub use hydrate::hydrate_held_values;
pub use limiter::VerifyLimiter;
pub use standalone::StandaloneValueAccess;
pub use state::HeldValues;
pub use types::{
    ScopeKind, SlotSelection, StoredValue, ValueKind, ValueMeta, ValueScope, ValueSlot, MAX_VALUE_BYTES,
};

use std::collections::HashMap;
use std::sync::Arc;

use alien_error::AlienError;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::{ErrorData, Result};

/// Reads and writes values in one backend.
#[derive(Clone)]
pub struct ValueStore {
    backend: Arc<dyn ValueBackend>,
    /// Set when the backend cannot keep a value across a restart: reads and
    /// deletes still work, writes are refused.
    refuse_writes: bool,
    /// One lock per setup-link or registration scope. A write to such a scope
    /// and a move out of it never overlap, so a value written while a move is
    /// running is never lost.
    scope_locks: Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

/// The current time at the precision a stored value keeps (milliseconds), so
/// the time returned by a write equals the time every later read returns.
fn now_in_stored_precision() -> chrono::DateTime<Utc> {
    Utc.timestamp_millis_opt(Utc::now().timestamp_millis())
        .single()
        .expect("the current time is a valid timestamp")
}

impl ValueStore {
    pub fn new(backend: Arc<dyn ValueBackend>) -> Self {
        Self {
            backend,
            refuse_writes: false,
            scope_locks: Arc::default(),
        }
    }

    /// Holds the scope's lock until dropped. Deployment scopes are never
    /// moved out of, so they take none.
    async fn lock_scope(&self, scope: &ValueScope) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        if scope.kind() == ScopeKind::Deployment {
            return None;
        }
        let lock = {
            let mut locks = self.scope_locks.lock().unwrap_or_else(|e| e.into_inner());
            // Forget locks nobody holds or waits for.
            locks.retain(|key, lock| key == scope.as_str() || Arc::strong_count(lock) > 1);
            locks.entry(scope.as_str().to_string()).or_default().clone()
        };
        Some(lock.lock_owned().await)
    }

    /// Refuses writes because the backend's storage is not durable. Values
    /// already stored stay readable and deletable.
    pub fn refusing_writes(mut self) -> Self {
        self.refuse_writes = true;
        self
    }

    /// Whether a write can succeed. A manager whose store does not offer
    /// this does not advertise the values capability.
    pub fn accepts_writes(&self) -> bool {
        !self.refuse_writes
    }

    /// Stores `value`, replacing any earlier value in the slot.
    pub async fn put(
        &self,
        scope: &ValueScope,
        slot: &ValueSlot,
        kind: ValueKind,
        value: String,
    ) -> Result<ValueMeta> {
        if self.refuse_writes {
            return Err(durability::ephemeral_state_error());
        }
        check_size(slot, &value)?;
        let _guard = self.lock_scope(scope).await;
        let stored = StoredValue {
            slot: slot.clone(),
            kind,
            value,
            updated_at: now_in_stored_precision(),
        };
        self.backend.put(scope, &stored).await?;
        stored.meta(scope)
    }

    /// Stores `value` only when the slot is empty. Returns the stored
    /// metadata when this call wrote it, `None` when the slot already held a
    /// value, which is left untouched.
    pub async fn put_if_absent(
        &self,
        scope: &ValueScope,
        slot: &ValueSlot,
        kind: ValueKind,
        value: String,
    ) -> Result<Option<ValueMeta>> {
        if self.refuse_writes {
            return Err(durability::ephemeral_state_error());
        }
        check_size(slot, &value)?;
        let _guard = self.lock_scope(scope).await;
        let stored = StoredValue {
            slot: slot.clone(),
            kind,
            value,
            updated_at: now_in_stored_precision(),
        };
        if !self.backend.put_if_absent(scope, &stored).await? {
            return Ok(None);
        }
        stored.meta(scope).map(Some)
    }

    /// Copies every value of `from` into `to`, leaving `from` untouched, and
    /// returns the metadata of what `to` holds for those slots. A slot that
    /// `to` already holds is kept, so a copy never overwrites anything written
    /// since. The copy is recorded under `claim_id`: presenting the same claim
    /// again changes nothing and returns what the first copy produced.
    pub async fn copy_scope(
        &self,
        from: &ValueScope,
        to: &ValueScope,
        claim_id: &str,
    ) -> Result<Vec<ValueMeta>> {
        self.transfer(from, to, claim_id, false).await
    }

    /// Moves every value of `from` into `to`, then empties `from`. Recorded
    /// under `claim_id` like [`Self::copy_scope`]. A move that stopped halfway
    /// is finished by presenting the claim again.
    pub async fn move_scope(
        &self,
        from: &ValueScope,
        to: &ValueScope,
        claim_id: &str,
    ) -> Result<Vec<ValueMeta>> {
        self.transfer(from, to, claim_id, true).await
    }

    async fn transfer(
        &self,
        from: &ValueScope,
        to: &ValueScope,
        claim_id: &str,
        remove_source: bool,
    ) -> Result<Vec<ValueMeta>> {
        if self.refuse_writes {
            return Err(durability::ephemeral_state_error());
        }
        // Writes to the source wait while this runs, so nothing is written
        // between the copy and the removal.
        let _guard = self.lock_scope(from).await;
        let key = transfer_key(from, to, claim_id);
        let record = ValueSlot::moved(&key);
        if self.backend.get(to, &record).await?.is_some() {
            // The copy finished. The completion record is written before any
            // source slot is removed, so a retry only finishes the removal
            // and reports what the destination holds for the recorded slots.
            let mut held = Vec::new();
            for (name, copied_version) in self.recorded_slots(to, &key).await? {
                let slot = ValueSlot::from_stored(&name)?;
                if remove_source {
                    // Remove only the version that was copied: an answer written
                    // to the source since (after the first attempt finished) is
                    // a newer one and stays.
                    if let Some(current) = self.backend.get(from, &slot).await? {
                        if current.updated_at == copied_version {
                            self.backend.delete(from, &slot).await?;
                        }
                    }
                }
                if let Some(stored) = self.get(to, &slot).await? {
                    held.push(stored.meta(to)?);
                }
            }
            return Ok(held);
        }
        let sources = self.list(from).await?;
        let mut copied = Vec::new();
        for stored in &sources {
            let stored = StoredValue {
                updated_at: now_in_stored_precision(),
                ..stored.clone()
            };
            self.backend.put_if_absent(to, &stored).await?;
            let held = self.get(to, &stored.slot).await?.ok_or_else(|| {
                ErrorData::internal(format!("Slot '{}' vanished during a copy", stored.slot))
            })?;
            copied.push(held.meta(to)?);
        }
        // One small marker per slot, then the completion record: no single
        // value grows with the number of slots.
        // The marker's own time is the version of the source value that was copied.
        for (meta, source) in copied.iter().zip(&sources) {
            self.backend
                .put(
                    to,
                    &StoredValue {
                        slot: ValueSlot::moved(&slot_marker_key(&key, meta.slot.as_str())),
                        kind: ValueKind::Plain,
                        value: meta.slot.as_str().to_string(),
                        updated_at: source.updated_at.clone(),
                    },
                )
                .await?;
        }
        self.backend
            .put(
                to,
                &StoredValue {
                    slot: record,
                    kind: ValueKind::Plain,
                    value: copied.len().to_string(),
                    updated_at: now_in_stored_precision(),
                },
            )
            .await?;
        if remove_source {
            for stored in &sources {
                self.backend.delete(from, &stored.slot).await?;
            }
        }
        Ok(copied)
    }

    /// The slot names a finished transfer recorded, one marker each, with the
    /// version (update time) of the source value that was copied.
    async fn recorded_slots(
        &self,
        to: &ValueScope,
        key: &str,
    ) -> Result<Vec<(String, DateTime<Utc>)>> {
        let prefix = ValueSlot::moved(key);
        let mut names: Vec<(String, DateTime<Utc>)> = self
            .backend
            .list_prefix(to, prefix.as_str())
            .await?
            .into_iter()
            .filter(|marker| marker.slot != prefix)
            .map(|marker| (marker.value, marker.updated_at))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(names)
    }

    /// Removes every value of the scope, including the records of served values.
    pub async fn delete_scope(&self, scope: &ValueScope) -> Result<()> {
        let _guard = self.lock_scope(scope).await;
        for stored in self.backend.list(scope).await? {
            self.backend.delete(scope, &stored.slot).await?;
        }
        Ok(())
    }

    /// Removes the slot. Removing an empty slot is not an error.
    pub async fn delete(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<()> {
        let _guard = self.lock_scope(scope).await;
        self.backend.delete(scope, slot).await
    }

    pub async fn get(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>> {
        self.backend.get(scope, slot).await
    }

    /// Every value in the scope, sorted by slot.
    pub async fn list(&self, scope: &ValueScope) -> Result<Vec<StoredValue>> {
        let mut values = self.backend.list(scope).await?;
        values.retain(|v| !v.slot.is_record());
        values.sort_by(|a, b| a.slot.cmp(&b.slot));
        Ok(values)
    }

    /// The scope's values whose slot starts with `slot_prefix`, sorted by slot.
    pub async fn list_prefix(
        &self,
        scope: &ValueScope,
        slot_prefix: &str,
    ) -> Result<Vec<StoredValue>> {
        let mut values = self.backend.list_prefix(scope, slot_prefix).await?;
        values.retain(|v| !v.slot.is_record());
        values.sort_by(|a, b| a.slot.cmp(&b.slot));
        Ok(values)
    }

    /// Remembers that the values with these `(variable name, digest)` pairs
    /// were served to a workload, a runner or an operator. Only the digest is
    /// kept, never the value. The history lets a later report that still
    /// carries a value that has since changed or been deleted be redacted.
    /// Nothing is written when the store refuses writes: such a store never
    /// received a value to serve.
    pub async fn record_served(
        &self,
        scope: &ValueScope,
        served: &[(String, String)],
    ) -> Result<()> {
        if self.refuse_writes || served.is_empty() {
            return Ok(());
        }
        let known = self.served(scope).await?;
        for (name, digest) in served {
            if known.iter().any(|(_, have)| have == digest) {
                continue;
            }
            let stored = StoredValue {
                slot: ValueSlot::served(digest),
                kind: ValueKind::Plain,
                value: name.clone(),
                updated_at: now_in_stored_precision(),
            };
            self.backend.put(scope, &stored).await?;
        }
        Ok(())
    }

    /// The `(variable name, digest)` pairs of every value served so far.
    pub async fn served(&self, scope: &ValueScope) -> Result<Vec<(String, String)>> {
        Ok(self
            .backend
            .list_prefix(scope, "served:")
            .await?
            .into_iter()
            .filter_map(|stored| {
                let digest = stored.slot.served_digest()?.to_string();
                Some((stored.value, digest))
            })
            .collect())
    }

    /// Whether `candidate` equals the stored value. A missing slot is no match.
    pub async fn matches(
        &self,
        scope: &ValueScope,
        slot: &ValueSlot,
        candidate: &str,
    ) -> Result<bool> {
        Ok(match self.get(scope, slot).await? {
            Some(stored) => constant_time_eq(stored.value.as_bytes(), candidate.as_bytes()),
            None => false,
        })
    }
}

/// Names one move: a hash of its source, destination and claim id.
fn transfer_key(from: &ValueScope, to: &ValueScope, claim_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in [from.as_str(), to.as_str(), claim_id] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Names the marker of one slot of a finished transfer.
fn slot_marker_key(transfer: &str, slot: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{transfer}{}", hex::encode(Sha256::digest(slot.as_bytes())))
}

fn check_size(slot: &ValueSlot, value: &str) -> Result<()> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(AlienError::new(ErrorData::ValueTooLarge {
            slot: slot.to_string(),
            bytes: value.len(),
            max_bytes: MAX_VALUE_BYTES,
        }));
    }
    Ok(())
}

/// Checks a value arriving over the API: within the size limit, and JSON text
/// for `input:` slots (the deployment reads those back as JSON).
pub(crate) fn check_incoming(slot: &ValueSlot, value: &str) -> Result<()> {
    check_size(slot, value)?;
    if slot.input_id().is_some() && serde_json::from_str::<serde_json::Value>(value).is_err() {
        return Err(ErrorData::bad_request(format!(
            "The value for '{slot}' must be JSON text"
        )));
    }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use alien_bindings::providers::kv::local::LocalKv;

    /// A sealed store over a local key-value file in `dir`, with a digest key set.
    pub async fn kv_store(dir: &std::path::Path) -> ValueStore {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let kv = LocalKv::single_process(dir.join("values-kv"))
            .await
            .unwrap();
        ValueStore::new(Arc::new(cipher::fixtures::sealed_backend(Arc::new(kv)).1))
    }
}

#[cfg(test)]
mod tests {
    use super::cipher::fixtures::sealed_backend;
    use super::*;
    use alien_bindings::providers::kv::local::LocalKv;

    pub(crate) async fn kv_store() -> (tempfile::TempDir, ValueStore) {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv"))
            .await
            .unwrap();
        let store = ValueStore::new(Arc::new(sealed_backend(Arc::new(kv)).1));
        (dir, store)
    }

    fn scope(s: &str) -> ValueScope {
        s.parse().unwrap()
    }

    fn slot(s: &str) -> ValueSlot {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn a_write_returns_the_time_that_reads_return() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, store) = kv_store().await;
        let dep = scope("dep:dep_1");

        let written = store
            .put(&dep, &slot("env:REGION"), ValueKind::Plain, "eu".into())
            .await
            .unwrap();

        let read = store.get(&dep, &slot("env:REGION")).await.unwrap().unwrap();
        assert_eq!(written.updated_at, read.updated_at);
        assert_eq!(
            written
                .updated_at
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            read.updated_at
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
        );
    }

    #[test]
    fn setup_link_and_registration_scopes_parse_and_do_not_name_a_deployment() {
        let link = scope("link:lnk_1");
        assert_eq!(link.kind(), ScopeKind::SetupLink);
        assert_eq!(link.id(), "lnk_1");
        assert_eq!(link.deployment_id(), None);
        assert_eq!(scope("reg:reg_1").kind(), ScopeKind::Registration);
        assert_eq!(scope("dep:dep_1").deployment_id(), Some("dep_1"));
        for bad in ["link:", "reg:a:b", "other:x", "lnk_1", "link:a/b"] {
            assert!(bad.parse::<ValueScope>().is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_move_carries_every_slot_to_the_deployment_and_empties_the_source() {
        let (_dir, store) = kv_store().await;
        let (link, dep) = (scope("link:lnk_1"), scope("dep:dep_1"));
        store
            .put(&link, &slot("input:key"), ValueKind::Secret, "\"s3\"".into())
            .await
            .unwrap();
        store
            .put(&link, &slot("env:A"), ValueKind::Plain, "a".into())
            .await
            .unwrap();
        // Another link's values are not touched.
        store
            .put(&scope("link:lnk_2"), &slot("env:A"), ValueKind::Plain, "other".into())
            .await
            .unwrap();

        let moved = store.move_scope(&link, &dep, "c").await.unwrap();
        let slots: Vec<_> = moved.iter().map(|m| m.slot.to_string()).collect();
        assert_eq!(slots, ["env:A", "input:key"]);
        assert!(store.list(&link).await.unwrap().is_empty());
        let held = store.list(&dep).await.unwrap();
        assert_eq!(held[1].value, "\"s3\"");
        // The digest is keyed by the new scope, and the move reports it.
        assert_eq!(moved[1].digest, held[1].digest(&dep).unwrap());
        assert_eq!(store.list(&scope("link:lnk_2")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_copy_keeps_the_source_and_serves_every_deployment_made_from_it() {
        let (_dir, store) = kv_store().await;
        let link = scope("link:lnk_1");
        store
            .put(&link, &slot("input:key"), ValueKind::Secret, "\"s3\"".into())
            .await
            .unwrap();
        for dep in ["dep:dep_1", "dep:dep_2"] {
            let dep = scope(dep);
            let copied = store.copy_scope(&link, &dep, &format!("c-{dep}")).await.unwrap();
            assert_eq!(copied.len(), 1);
            let held = store.list(&dep).await.unwrap();
            assert_eq!(held[0].value, "\"s3\"");
            // Each deployment's digest is keyed by its own scope.
            assert_eq!(copied[0].digest, held[0].digest(&dep).unwrap());
        }
        assert_eq!(store.list(&link).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn repeating_a_move_keeps_what_the_deployment_holds_since() {
        let (_dir, store) = kv_store().await;
        let (reg, dep) = (scope("reg:reg_1"), scope("dep:dep_1"));
        store
            .put(&reg, &slot("env:A"), ValueKind::Plain, "from-registration".into())
            .await
            .unwrap();
        store.move_scope(&reg, &dep, "c1").await.unwrap();
        store
            .put(&dep, &slot("env:A"), ValueKind::Plain, "edited".into())
            .await
            .unwrap();
        // A retry of the same claim after a lost response changes nothing.
        let again = store.move_scope(&reg, &dep, "c1").await.unwrap();
        assert_eq!(again.len(), 1);
        // A source slot that came back never overwrites the deployment's value.
        store
            .put(&reg, &slot("env:A"), ValueKind::Plain, "stale".into())
            .await
            .unwrap();
        store.move_scope(&reg, &dep, "c2").await.unwrap();
        assert_eq!(
            store.get(&dep, &slot("env:A")).await.unwrap().unwrap().value,
            "edited"
        );
        assert!(store.list(&reg).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_repeated_claim_neither_restores_deleted_slots_nor_adds_new_ones() {
        let (_dir, store) = kv_store().await;
        let (link, dep) = (scope("link:lnk_1"), scope("dep:dep_1"));
        store
            .put(&link, &slot("env:A"), ValueKind::Plain, "a".into())
            .await
            .unwrap();
        store.copy_scope(&link, &dep, "claim").await.unwrap();
        store.delete(&dep, &slot("env:A")).await.unwrap();
        store
            .put(&link, &slot("env:B"), ValueKind::Plain, "b".into())
            .await
            .unwrap();

        store.copy_scope(&link, &dep, "claim").await.unwrap();
        assert!(store.list(&dep).await.unwrap().is_empty());
        // The record is not a value: it is never listed or served.
        assert!(store.list_prefix(&dep, "moved:").await.unwrap().is_empty());
        // Another claim id copies again.
        store.copy_scope(&link, &dep, "other").await.unwrap();
        assert_eq!(store.list(&dep).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_repeated_move_keeps_an_answer_written_to_the_source_after_it_finished() {
        let (_dir, store) = kv_store().await;
        let (reg, dep) = (scope("reg:reg_1"), scope("dep:dep_1"));
        store
            .put(&reg, &slot("env:A"), ValueKind::Plain, "old".into())
            .await
            .unwrap();
        store.move_scope(&reg, &dep, "claim").await.unwrap();
        assert!(store.list(&reg).await.unwrap().is_empty());

        // A newer answer lands in the source, then the same claim is presented again.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        store
            .put(&reg, &slot("env:A"), ValueKind::Plain, "new".into())
            .await
            .unwrap();
        let held = store.move_scope(&reg, &dep, "claim").await.unwrap();
        assert_eq!(held.len(), 1);
        let kept = store.get(&reg, &slot("env:A")).await.unwrap().unwrap();
        assert_eq!(kept.value, "new");
        // The deployment keeps what the first move copied.
        let copied = store.get(&dep, &slot("env:A")).await.unwrap().unwrap();
        assert_eq!(copied.value, "old");
    }

    /// A backend that starts refusing deletes after `allowed` of them.
    struct CrashingDeletes {
        inner: super::backend::KvValueBackend,
        allowed: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl super::backend::ValueBackend for CrashingDeletes {
        async fn put(&self, s: &ValueScope, v: &StoredValue) -> Result<()> {
            self.inner.put(s, v).await
        }
        async fn put_if_absent(&self, s: &ValueScope, v: &StoredValue) -> Result<bool> {
            self.inner.put_if_absent(s, v).await
        }
        async fn get(&self, s: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>> {
            self.inner.get(s, slot).await
        }
        async fn delete(&self, s: &ValueScope, slot: &ValueSlot) -> Result<()> {
            use std::sync::atomic::Ordering;
            if self
                .allowed
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_err()
            {
                return Err(ErrorData::internal("crash".to_string()));
            }
            self.inner.delete(s, slot).await
        }
        async fn list(&self, s: &ValueScope) -> Result<Vec<StoredValue>> {
            self.inner.list(s).await
        }
        async fn list_prefix(&self, s: &ValueScope, p: &str) -> Result<Vec<StoredValue>> {
            self.inner.list_prefix(s, p).await
        }
    }

    async fn crashing_store(allowed: usize) -> (tempfile::TempDir, Arc<CrashingDeletes>, ValueStore) {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv")).await.unwrap();
        let backend = Arc::new(CrashingDeletes {
            inner: sealed_backend(Arc::new(kv)).1,
            allowed: std::sync::atomic::AtomicUsize::new(allowed),
        });
        let store = ValueStore::new(backend.clone());
        (dir, backend, store)
    }

    #[tokio::test]
    async fn a_move_that_crashed_while_removing_sources_is_finished_by_a_retry() {
        use std::sync::atomic::Ordering;
        let (_dir, backend, store) = crashing_store(1).await;
        let (reg, dep) = (scope("reg:reg_1"), scope("dep:dep_1"));
        for name in ["env:A", "env:B", "env:C"] {
            store
                .put(&reg, &slot(name), ValueKind::Plain, name.into())
                .await
                .unwrap();
        }
        // Dies after removing one source slot.
        assert!(store.move_scope(&reg, &dep, "claim").await.is_err());
        assert_eq!(store.list(&reg).await.unwrap().len(), 2);

        backend.allowed.store(100, Ordering::SeqCst);
        let held = store.move_scope(&reg, &dep, "claim").await.unwrap();
        let mut names: Vec<_> = held.iter().map(|m| m.slot.as_str().to_string()).collect();
        names.sort();
        assert_eq!(names, ["env:A", "env:B", "env:C"]);
        assert!(store.list(&reg).await.unwrap().is_empty());
        assert_eq!(store.list(&dep).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_move_of_many_slots_with_long_ids_keeps_every_record_small() {
        let (_dir, store) = kv_store().await;
        let (reg, dep) = (scope("reg:reg_1"), scope("dep:dep_1"));
        for i in 0..200 {
            let name = format!("input:{i:03}{}", "x".repeat(110));
            store
                .put(&reg, &slot(&name), ValueKind::Plain, "v".into())
                .await
                .unwrap();
        }
        let first = store.move_scope(&reg, &dep, "claim").await.unwrap();
        assert_eq!(first.len(), 200);
        let again = store.move_scope(&reg, &dep, "claim").await.unwrap();
        assert_eq!(again.len(), 200);
        // Every record kept in the destination fits a key-value limit.
        for record in store.backend.list_prefix(&dep, "moved:").await.unwrap() {
            assert!(record.value.len() < 1024, "{}", record.value.len());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_write_during_a_move_is_never_lost() {
        let (_dir, store) = kv_store().await;
        let (reg, dep) = (scope("reg:reg_1"), scope("dep:dep_1"));
        for i in 0..200 {
            store
                .put(&reg, &slot(&format!("env:V{i}")), ValueKind::Plain, "old".into())
                .await
                .unwrap();
        }
        let writer = {
            let store = store.clone();
            let reg = reg.clone();
            tokio::spawn(async move {
                for i in 0..200 {
                    store
                        .put(&reg, &slot(&format!("env:V{i}")), ValueKind::Plain, "new".into())
                        .await
                        .unwrap();
                    tokio::task::yield_now().await;
                }
            })
        };
        store.move_scope(&reg, &dep, "c").await.unwrap();
        writer.await.unwrap();
        // Every slot's newest value is in the source (written after the move)
        // or in the deployment (copied before the write): never in neither.
        for i in 0..200 {
            let name = slot(&format!("env:V{i}"));
            let newest = |v: Option<StoredValue>| v.map(|v| v.value);
            let in_reg = newest(store.get(&reg, &name).await.unwrap());
            let in_dep = newest(store.get(&dep, &name).await.unwrap());
            assert!(in_reg.is_some() || in_dep.is_some(), "slot {i} was lost");
            if in_reg.is_none() {
                // Moved out whole: the write came first and was carried over.
                assert_eq!(in_dep.as_deref(), Some("new"), "slot {i} lost its newer value");
            }
        }
    }

    #[tokio::test]
    async fn deleting_a_scope_removes_only_that_scope() {
        let (_dir, store) = kv_store().await;
        let (link, other) = (scope("link:lnk_1"), scope("link:lnk_10"));
        for s in [&link, &other] {
            store
                .put(s, &slot("env:A"), ValueKind::Secret, "x".into())
                .await
                .unwrap();
        }
        store.delete_scope(&link).await.unwrap();
        assert!(store.list(&link).await.unwrap().is_empty());
        assert_eq!(store.list(&other).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn put_get_list_delete_round_trip() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, store) = kv_store().await;
        let dep = scope("dep:dep_1");

        let plain = store
            .put(&dep, &slot("env:REGION"), ValueKind::Plain, "eu".into())
            .await
            .unwrap();
        let secret = store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, "s3cret".into())
            .await
            .unwrap();
        assert_eq!(plain.kind, ValueKind::Plain);
        assert_eq!(secret.kind, ValueKind::Secret);
        assert_eq!(secret.digest.len(), 64);
        assert_ne!(secret.digest, plain.digest);
        assert!(!secret.digest.contains("s3cret"));

        let all = store.list(&dep).await.unwrap();
        let slots: Vec<_> = all.iter().map(|v| v.slot.to_string()).collect();
        assert_eq!(slots, ["env:REGION", "env:TOKEN"]);
        assert_eq!(all[1].value, "s3cret");
        assert_eq!(all[1].kind, ValueKind::Secret);

        // Another scope sees nothing.
        assert!(store.list(&scope("dep:dep_2")).await.unwrap().is_empty());

        // Replacing a value changes the digest.
        let replaced = store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, "other".into())
            .await
            .unwrap();
        assert_ne!(replaced.digest, secret.digest);

        store.delete(&dep, &slot("env:TOKEN")).await.unwrap();
        store.delete(&dep, &slot("env:TOKEN")).await.unwrap();
        assert!(store.get(&dep, &slot("env:TOKEN")).await.unwrap().is_none());
        assert_eq!(store.list(&dep).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_scope_that_prefixes_another_does_not_see_its_values() {
        let (_dir, store) = kv_store().await;
        store
            .put(
                &scope("dep:dep_1"),
                &slot("env:A"),
                ValueKind::Plain,
                "1".into(),
            )
            .await
            .unwrap();
        store
            .put(
                &scope("dep:dep_10"),
                &slot("env:B"),
                ValueKind::Plain,
                "2".into(),
            )
            .await
            .unwrap();
        let first = store.list(&scope("dep:dep_1")).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].slot.to_string(), "env:A");
    }

    /// A scan prefix is `val:{scope}:`. Scope ids hold no ':', so no other
    /// scope's keys can start with it, whatever else their ids look like.
    #[tokio::test]
    async fn no_scope_scan_sees_another_scopes_container_values() {
        let (_dir, store) = kv_store().await;
        let scopes = [
            "dep:x",
            "dep:x.y",
            "dep:x-1",
            "dep:x_",
            "dep:x.cenv",
            "dep:xcenv.a",
        ];
        for (n, id) in scopes.iter().enumerate() {
            for slot_name in ["cenv.a.X", "cenv.a.b.X", "env:A", "input:a.b"] {
                store
                    .put(
                        &scope(id),
                        &slot(slot_name),
                        ValueKind::Secret,
                        format!("\"{n}\""),
                    )
                    .await
                    .unwrap();
            }
        }
        for (n, id) in scopes.iter().enumerate() {
            let own = store.list(&scope(id)).await.unwrap();
            let slots: Vec<_> = own.iter().map(|v| v.slot.to_string()).collect();
            assert_eq!(
                slots,
                ["cenv.a.X", "cenv.a.b.X", "env:A", "input:a.b"],
                "{id}"
            );
            assert!(own.iter().all(|v| v.value == format!("\"{n}\"")), "{id}");
        }
    }

    #[tokio::test]
    async fn the_size_limit_is_enforced_and_a_value_at_the_limit_fits() {
        let (_dir, store) = kv_store().await;
        let dep = scope("dep:dep_1");

        let at_limit = "x".repeat(MAX_VALUE_BYTES);
        store
            .put(&dep, &slot("env:BIG"), ValueKind::Secret, at_limit.clone())
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&dep, &slot("env:BIG"))
                .await
                .unwrap()
                .unwrap()
                .value,
            at_limit
        );

        let err = store
            .put(
                &dep,
                &slot("env:BIGGER"),
                ValueKind::Secret,
                "x".repeat(MAX_VALUE_BYTES + 1),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "VALUE_TOO_LARGE");
        assert_eq!(err.http_status_code, Some(413));
    }

    #[tokio::test]
    async fn a_value_with_quotes_and_multibyte_text_survives_at_the_limit() {
        let (_dir, store) = kv_store().await;
        let dep = scope("dep:dep_1");
        let tricky = "\"é\\\n".repeat(MAX_VALUE_BYTES / 6);
        assert!(tricky.len() <= MAX_VALUE_BYTES);
        store
            .put(&dep, &slot("env:T"), ValueKind::Plain, tricky.clone())
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&dep, &slot("env:T"))
                .await
                .unwrap()
                .unwrap()
                .value,
            tricky
        );
    }

    #[tokio::test]
    async fn the_same_value_has_a_different_digest_in_another_scope() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, store) = kv_store().await;
        let put = |dep: &'static str| {
            let store = store.clone();
            async move {
                store
                    .put(
                        &scope(dep),
                        &slot("env:TOKEN"),
                        ValueKind::Secret,
                        "same".into(),
                    )
                    .await
                    .unwrap()
                    .digest
            }
        };
        let (first, second) = (put("dep:dep_1").await, put("dep:dep_2").await);
        assert_ne!(first, second);
        assert_eq!(first, put("dep:dep_1").await);
    }

    #[tokio::test]
    async fn matches_compares_the_stored_value() {
        let (_dir, store) = kv_store().await;
        let dep = scope("dep:dep_1");
        store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, "right".into())
            .await
            .unwrap();
        assert!(store
            .matches(&dep, &slot("env:TOKEN"), "right")
            .await
            .unwrap());
        assert!(!store
            .matches(&dep, &slot("env:TOKEN"), "wrong")
            .await
            .unwrap());
        assert!(!store
            .matches(&dep, &slot("env:NONE"), "right")
            .await
            .unwrap());
    }

    /// Value keys must never carry the key-value store's expiry, or the
    /// store would delete a deployment's values on its own.
    #[tokio::test]
    async fn values_are_written_without_a_ttl() {
        use alien_bindings::traits::{Kv, PutOptions};
        use std::sync::Mutex;

        #[derive(Debug, Default)]
        struct Recording {
            inner: Mutex<Vec<Option<std::time::Duration>>>,
        }
        impl alien_bindings::traits::Binding for Recording {}
        #[async_trait::async_trait]
        impl Kv for Recording {
            async fn get(
                &self,
                _key: &str,
            ) -> alien_bindings::error::Result<Option<alien_bindings::traits::KvEntry>>
            {
                Ok(None)
            }
            async fn put(
                &self,
                _key: &str,
                _value: Vec<u8>,
                options: Option<PutOptions>,
            ) -> alien_bindings::error::Result<bool> {
                self.inner.lock().unwrap().push(options.and_then(|o| o.ttl));
                Ok(true)
            }
            async fn delete(
                &self,
                _key: &str,
                _if_version: Option<&str>,
            ) -> alien_bindings::error::Result<bool> {
                Ok(true)
            }
            async fn exists(&self, _key: &str) -> alien_bindings::error::Result<bool> {
                Ok(false)
            }
            async fn scan_prefix(
                &self,
                _prefix: &str,
                _limit: Option<usize>,
                _cursor: Option<String>,
            ) -> alien_bindings::error::Result<alien_bindings::traits::ScanResult> {
                Ok(alien_bindings::traits::ScanResult {
                    items: vec![],
                    next_cursor: None,
                })
            }
        }

        alien_deployment::set_ephemeral_secrets_digest_key();
        let recording = Arc::new(Recording::default());
        let store = ValueStore::new(Arc::new(sealed_backend(recording.clone()).1));
        store
            .put(
                &scope("dep:dep_1"),
                &slot("env:A"),
                ValueKind::Plain,
                "1".into(),
            )
            .await
            .unwrap();
        // One write for the wrapped data key, one for the value.
        assert_eq!(*recording.inner.lock().unwrap(), vec![None, None]);
    }
}
