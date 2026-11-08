//! Where values are kept.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_bindings::traits::Kv;
use alien_error::{AlienError, Context};
use async_trait::async_trait;

use super::{StoredValue, ValueCipher, ValueScope, ValueSlot};
use crate::error::{ErrorData, Result};

/// Storage for [`super::ValueStore`]. A write is visible to the next read.
#[async_trait]
pub trait ValueBackend: Send + Sync {
    async fn put(&self, scope: &ValueScope, value: &StoredValue) -> Result<()>;
    async fn get(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>>;
    /// Removing an empty slot succeeds.
    async fn delete(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<()>;
    async fn list(&self, scope: &ValueScope) -> Result<Vec<StoredValue>>;
    /// The scope's values whose slot starts with `slot_prefix`. A backend
    /// that can narrow its read overrides this.
    async fn list_prefix(&self, scope: &ValueScope, slot_prefix: &str) -> Result<Vec<StoredValue>> {
        let mut values = self.list(scope).await?;
        values.retain(|v| v.slot.as_str().starts_with(slot_prefix));
        Ok(values)
    }
}

/// Values in the manager's key-value binding, under `val:{scope}:{slot}`.
/// Records are written without a TTL, so the store never expires them. Every
/// record is sealed by the cipher before it is written: the key-value store
/// never holds a value in the clear.
pub struct KvValueBackend {
    kv: Arc<dyn Kv>,
    cipher: Arc<dyn ValueCipher>,
}

const PAGE_SIZE: usize = 100;

impl KvValueBackend {
    pub fn new(kv: Arc<dyn Kv>, cipher: Arc<dyn ValueCipher>) -> Self {
        Self { kv, cipher }
    }

    fn key(scope: &ValueScope, slot: &ValueSlot) -> String {
        format!("val:{scope}:{slot}")
    }

    fn kv_error(action: &str) -> ErrorData {
        ErrorData::InternalError {
            message: format!("Could not {action} in the key-value store"),
        }
    }
}

#[async_trait]
impl ValueBackend for KvValueBackend {
    async fn put(&self, scope: &ValueScope, value: &StoredValue) -> Result<()> {
        let sealed = self
            .cipher
            .seal(scope, &value.slot, &value.encode())
            .await?;
        self.kv
            .put(&Self::key(scope, &value.slot), sealed, None)
            .await
            .context(Self::kv_error("write a value"))?;
        Ok(())
    }

    async fn get(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>> {
        let Some(entry) = self
            .kv
            .get(&Self::key(scope, slot))
            .await
            .context(Self::kv_error("read a value"))?
        else {
            return Ok(None);
        };
        let record = self.cipher.open(scope, slot, &entry.value).await?;
        StoredValue::decode(slot.clone(), &record).map(Some)
    }

    async fn delete(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<()> {
        self.kv
            .delete(&Self::key(scope, slot), None)
            .await
            .context(Self::kv_error("delete a value"))?;
        Ok(())
    }

    async fn list(&self, scope: &ValueScope) -> Result<Vec<StoredValue>> {
        self.list_prefix(scope, "").await
    }

    async fn list_prefix(&self, scope: &ValueScope, slot_prefix: &str) -> Result<Vec<StoredValue>> {
        let scope_prefix = format!("val:{scope}:");
        let prefix = format!("{scope_prefix}{slot_prefix}");
        let mut found = BTreeMap::new();
        let mut cursor = None;
        loop {
            let page = self
                .kv
                .scan_prefix(&prefix, Some(PAGE_SIZE), cursor)
                .await
                .context(Self::kv_error("list values"))?;
            for entry in page.items {
                // Scans may repeat keys across pages.
                let Some(slot) = entry
                    .key
                    .strip_prefix(&prefix)
                    .map(|_| &entry.key[scope_prefix.len()..])
                else {
                    return Err(AlienError::new(ErrorData::InternalError {
                        message: format!(
                            "Key-value scan returned '{}' outside '{prefix}'",
                            entry.key
                        ),
                    }));
                };
                let slot: ValueSlot = slot.parse()?;
                let record = self.cipher.open(scope, &slot, &entry.value).await?;
                let stored = StoredValue::decode(slot.clone(), &record)?;
                found.insert(slot, stored);
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(found.into_values().collect())
    }
}
