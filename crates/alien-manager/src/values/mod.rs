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
mod hydrate;
mod limiter;
mod standalone;
mod types;

pub use access::{ValueAccess, ValueAccessAuthorizer, ValueGrant};
pub use backend::{KvValueBackend, ValueBackend};
pub use hydrate::hydrate_held_values;
pub use limiter::VerifyLimiter;
pub use standalone::StandaloneValueAccess;
pub use types::{
    SlotSelection, StoredValue, ValueKind, ValueMeta, ValueScope, ValueSlot, MAX_VALUE_BYTES,
};

use std::sync::Arc;

use alien_error::AlienError;
use chrono::Utc;

use crate::error::{ErrorData, Result};

/// Reads and writes values in one backend.
#[derive(Clone)]
pub struct ValueStore {
    backend: Arc<dyn ValueBackend>,
}

impl ValueStore {
    pub fn new(backend: Arc<dyn ValueBackend>) -> Self {
        Self { backend }
    }

    /// Stores `value`, replacing any earlier value in the slot.
    pub async fn put(
        &self,
        scope: &ValueScope,
        slot: &ValueSlot,
        kind: ValueKind,
        value: String,
    ) -> Result<ValueMeta> {
        check_size(slot, &value)?;
        let stored = StoredValue {
            slot: slot.clone(),
            kind,
            value,
            updated_at: Utc::now(),
        };
        self.backend.put(scope, &stored).await?;
        stored.meta()
    }

    /// Removes the slot. Removing an empty slot is not an error.
    pub async fn delete(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<()> {
        self.backend.delete(scope, slot).await
    }

    pub async fn get(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>> {
        self.backend.get(scope, slot).await
    }

    /// Every value in the scope, sorted by slot.
    pub async fn list(&self, scope: &ValueScope) -> Result<Vec<StoredValue>> {
        let mut values = self.backend.list(scope).await?;
        values.sort_by(|a, b| a.slot.cmp(&b.slot));
        Ok(values)
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
mod tests {
    use super::*;
    use alien_bindings::providers::kv::local::LocalKv;

    pub(crate) async fn kv_store() -> (tempfile::TempDir, ValueStore) {
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv"))
            .await
            .unwrap();
        let store = ValueStore::new(Arc::new(KvValueBackend::new(Arc::new(kv))));
        (dir, store)
    }

    fn scope(s: &str) -> ValueScope {
        s.parse().unwrap()
    }

    fn slot(s: &str) -> ValueSlot {
        s.parse().unwrap()
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
        let store = ValueStore::new(Arc::new(KvValueBackend::new(recording.clone())));
        store
            .put(
                &scope("dep:dep_1"),
                &slot("env:A"),
                ValueKind::Plain,
                "1".into(),
            )
            .await
            .unwrap();
        assert_eq!(*recording.inner.lock().unwrap(), vec![None]);
    }
}
