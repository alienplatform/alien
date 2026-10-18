use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use alien_bindings::Kv;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{denied, unavailable, Result},
    policy::{canonical, digest, Policy, MAX_POLICY_BYTES},
};

const CHUNK_BYTES: usize = 16 * 1024;
const CACHE_ENTRIES: usize = 128;
type CacheKey = (String, String);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    policy: String,
    chunks: usize,
}

/// Every authorization reads the current policy reference. Only verified content is cached.
/// Store failures never fall back to a cached grant.
pub struct PolicyStore {
    kv: Arc<dyn Kv>,
    cache: Mutex<HashMap<CacheKey, (String, Arc<Policy>)>>,
}

impl PolicyStore {
    pub fn new(kv: Arc<dyn Kv>) -> Self {
        Self {
            kv,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Read the current policy; missing entries deny even when configuration has a default.
    pub async fn resolve(&self, aud: &str, sub: &str) -> Result<Arc<Policy>> {
        let entry = self
            .kv
            .get(&key(aud, sub))
            .await
            .context(unavailable("read policy record"))?
            .ok_or_else(|| denied("policy not installed"))?;
        let record: Record = serde_json::from_slice(&entry.value)
            .into_alien_error()
            .context(unavailable("decode policy record"))?;
        if !crate::token::valid_digest(&record.policy)
            || record.chunks == 0
            || record.chunks > MAX_POLICY_BYTES / CHUNK_BYTES
        {
            return Err(AlienError::new(unavailable(
                "invalid stored policy reference",
            )));
        }
        let cache_key = (aud.to_string(), sub.to_string());
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?;
            if let Some((hash, policy)) = cache.get(&cache_key) {
                if *hash == record.policy {
                    return Ok(policy.clone());
                }
            }
        }
        let mut bytes = Vec::new();
        for index in 0..record.chunks {
            let chunk = self
                .kv
                .get(&chunk_key(&key(aud, sub), &record.policy, index))
                .await
                .context(unavailable("read policy content"))?
                .ok_or_else(|| AlienError::new(unavailable("policy content missing")))?;
            if chunk.value.len() > CHUNK_BYTES || bytes.len() + chunk.value.len() > MAX_POLICY_BYTES
            {
                return Err(AlienError::new(unavailable("invalid stored policy size")));
            }
            bytes.extend(chunk.value);
        }
        if digest(&bytes) != record.policy {
            return Err(AlienError::new(unavailable(
                "stored policy digest mismatch",
            )));
        }
        let policy: Policy = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(unavailable("decode policy content"))?;
        if canonical(&policy)? != bytes {
            return Err(AlienError::new(unavailable(
                "stored policy is not canonical",
            )));
        }
        let policy = Arc::new(policy);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?;
        if cache.len() >= CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(cache_key, (record.policy, policy.clone()));
        Ok(policy)
    }

    /// Publish immutable content before replacing the policy reference. Concurrent writers
    /// cannot mix chunks; the last reference write wins. Repeated policies reuse their content.
    /// Distinct versions remain until sandbox deletion, avoiding a cleanup race with readers.
    pub async fn put(&self, aud: &str, sub: &str, policy: &Policy) -> Result<()> {
        let bytes = canonical(policy)?;
        let key = key(aud, sub);
        let chunks = bytes.len().div_ceil(CHUNK_BYTES);
        let hash = digest(&bytes);
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.kv
                .put(&chunk_key(&key, &hash, index), chunk.to_vec(), None)
                .await
                .context(unavailable("write policy content"))?;
        }
        let record = Record {
            policy: hash,
            chunks,
        };
        let value = serde_json::to_vec(&record)
            .into_alien_error()
            .context(unavailable("encode policy record"))?;
        self.kv
            .put(&key, value, None)
            .await
            .context(unavailable("publish policy record"))?;
        Ok(())
    }

    /// Remove authorization first, then every stored version. Retrying is safe.
    pub async fn delete(&self, aud: &str, sub: &str) -> Result<()> {
        let key = key(aud, sub);
        self.kv
            .delete(&key, None)
            .await
            .context(unavailable("delete policy record"))?;
        // The binding serializes termination with its writes. Collect before deleting so
        // provider continuation cursors are never used against a mutating scan.
        let mut cursor = None;
        let mut keys = Vec::new();
        loop {
            let page = self
                .kv
                .scan_prefix(&format!("{key}:chunk:"), Some(100), cursor)
                .await
                .context(unavailable("list policy content"))?;
            keys.extend(page.items.into_iter().map(|entry| entry.key));
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        for key in keys {
            self.kv
                .delete(&key, None)
                .await
                .context(unavailable("delete policy content"))?;
        }
        self.cache
            .lock()
            .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?
            .remove(&(aud.to_string(), sub.to_string()));
        Ok(())
    }
}

fn chunk_key(key: &str, hash: &str, index: usize) -> String {
    format!("{key}:chunk:{hash}:{index}")
}

fn key(aud: &str, sub: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((aud.len() as u64).to_be_bytes());
    hash.update(aud.as_bytes());
    hash.update(sub.as_bytes());
    format!("egress-policy:{}", URL_SAFE_NO_PAD.encode(hash.finalize()))
}
