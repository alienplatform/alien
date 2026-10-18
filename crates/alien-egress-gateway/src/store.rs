use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use alien_bindings::{traits::PutOptions, Kv, PutCondition};
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
    version: u64,
    policy: Option<String>,
    chunks: usize,
}

/// Every authorization reads the current policy reference. Only verified content is cached.
/// Store failures never fall back to a cached grant.
pub struct PolicyStore {
    kv: Arc<dyn Kv>,
    cache: Mutex<VecDeque<(CacheKey, String, Arc<Policy>)>>,
}

impl PolicyStore {
    pub fn new(kv: Arc<dyn Kv>) -> Self {
        Self {
            kv,
            cache: Mutex::new(VecDeque::new()),
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
        let hash = record.policy.ok_or_else(|| denied("policy deleted"))?;
        if !crate::token::valid_digest(&hash)
            || record.chunks == 0
            || record.chunks > MAX_POLICY_BYTES / CHUNK_BYTES
        {
            return Err(AlienError::new(unavailable(
                "invalid stored policy reference",
            )));
        }
        let cache_key = (aud.to_string(), sub.to_string());
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?;
            if let Some(index) = cache
                .iter()
                .position(|(key, cached_hash, _)| *key == cache_key && *cached_hash == hash)
            {
                let entry = cache
                    .remove(index)
                    .ok_or_else(|| AlienError::new(unavailable("policy cache entry missing")))?;
                let policy = entry.2.clone();
                cache.push_back(entry);
                return Ok(policy);
            }
        }
        let mut bytes = Vec::new();
        for index in 0..record.chunks {
            let chunk = self
                .kv
                .get(&chunk_key(&key(aud, sub), &hash, index))
                .await
                .context(unavailable("read policy content"))?
                .ok_or_else(|| AlienError::new(unavailable("policy content missing")))?;
            if chunk.value.len() > CHUNK_BYTES || bytes.len() + chunk.value.len() > MAX_POLICY_BYTES
            {
                return Err(AlienError::new(unavailable("invalid stored policy size")));
            }
            bytes.extend(chunk.value);
        }
        if digest(&bytes) != hash {
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
        cache.retain(|(key, _, _)| *key != cache_key);
        if cache.len() >= CACHE_ENTRIES {
            cache.pop_front();
        }
        cache.push_back((cache_key, hash, policy.clone()));
        Ok(policy)
    }

    /// Publish immutable content using a monotonic signed version and atomic reference CAS.
    pub async fn put(&self, aud: &str, sub: &str, version: u64, policy: &Policy) -> Result<()> {
        let bytes = canonical(policy)?;
        let key = key(aud, sub);
        let chunks = bytes.len().div_ceil(CHUNK_BYTES);
        let hash = digest(&bytes);
        let record = Record {
            version,
            policy: Some(hash.clone()),
            chunks,
        };
        // Refuse known stale requests before uploading content. CAS checks again after upload.
        self.check_version(&key, &record).await?;
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.kv
                .put(&chunk_key(&key, &hash, index), chunk.to_vec(), None)
                .await
                .context(unavailable("write policy content"))?;
        }
        self.publish(&key, &record).await
    }

    /// Return the durable sequence, including after deletion; missing identities start at zero.
    pub async fn version(&self, aud: &str, sub: &str) -> Result<u64> {
        match self
            .kv
            .get(&key(aud, sub))
            .await
            .context(unavailable("read policy version"))?
        {
            None => Ok(0),
            Some(entry) => Ok(decode_record(&entry.value)?.version),
        }
    }

    async fn check_version(&self, key: &str, next: &Record) -> Result<(PutCondition, bool)> {
        if next.version == 0 {
            return Err(conflict());
        }
        let Some(entry) = self
            .kv
            .get(key)
            .await
            .context(unavailable("read policy sequence"))?
        else {
            return Ok((PutCondition::Absent, false));
        };
        let current = decode_record(&entry.value)?;
        let same = current.version == next.version && current.policy == next.policy;
        if same {
            return Ok((PutCondition::Version(entry.version), true));
        }
        if next.version <= current.version {
            return Err(conflict());
        }
        Ok((PutCondition::Version(entry.version), false))
    }

    async fn publish(&self, key: &str, record: &Record) -> Result<()> {
        let value = serde_json::to_vec(record)
            .into_alien_error()
            .context(unavailable("encode policy record"))?;
        for _ in 0..8 {
            let (condition, same) = self.check_version(key, record).await?;
            if same
                || self
                    .kv
                    .put(
                        key,
                        value.clone(),
                        Some(PutOptions {
                            ttl: None,
                            condition,
                        }),
                    )
                    .await
                    .context(unavailable("publish policy record"))?
            {
                return Ok(());
            }
        }
        Err(conflict())
    }

    /// Retain a version tombstone. Immutable content also remains: deleting chunks would race
    /// readers and an explicitly recreated identity publishing the same content again.
    pub async fn delete(&self, aud: &str, sub: &str, version: u64) -> Result<()> {
        self.publish(
            &key(aud, sub),
            &Record {
                version,
                policy: None,
                chunks: 0,
            },
        )
        .await?;
        self.cache
            .lock()
            .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?
            .retain(|(key, _, _)| key != &(aud.to_string(), sub.to_string()));
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

fn decode_record(value: &[u8]) -> Result<Record> {
    serde_json::from_slice(value)
        .into_alien_error()
        .context(unavailable("decode policy record"))
}
fn conflict() -> AlienError<crate::error::ErrorData> {
    AlienError::new(crate::error::ErrorData::PolicyVersionConflict)
}
