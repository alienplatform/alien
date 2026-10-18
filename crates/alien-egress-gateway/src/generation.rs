use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use alien_bindings::{traits::PutOptions, Kv, PutCondition};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{denied, unavailable, ErrorData, Result},
    policy::{canonical, digest, Policy, MAX_POLICY_BYTES},
};

const CHUNK_BYTES: usize = 16 * 1024;
const CACHE_ENTRIES: usize = 128;
type CacheKey = (String, String, u64);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    gen: u64,
    policy: String,
    chunks: usize,
}

/// Exact generation and policy in the deployment KV. Every authorization reads the metadata;
/// only immutable policy content is cached. Store failures never use stale metadata.
pub struct GenerationFence {
    kv: Arc<dyn Kv>,
    cache: Mutex<HashMap<CacheKey, (String, Arc<Policy>)>>,
}

impl GenerationFence {
    pub fn new(kv: Arc<dyn Kv>) -> Self {
        Self {
            kv,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve precisely the installed generation and digest. Future tokens fail like stale ones.
    pub async fn resolve(
        &self,
        aud: &str,
        sub: &str,
        generation: u64,
        policy_digest: &str,
    ) -> Result<Arc<Policy>> {
        let entry = self
            .kv
            .get(&key(aud, sub))
            .await
            .context(unavailable("read policy generation"))?
            .ok_or_else(|| denied("policy not installed"))?;
        let record: Record = serde_json::from_slice(&entry.value)
            .into_alien_error()
            .context(unavailable("decode policy generation"))?;
        if generation != record.gen || policy_digest != record.policy {
            return Err(denied("policy generation or digest mismatch"));
        }
        if !crate::token::valid_digest(&record.policy)
            || record.chunks == 0
            || record.chunks > MAX_POLICY_BYTES / CHUNK_BYTES
        {
            return Err(AlienError::new(unavailable(
                "invalid stored policy reference",
            )));
        }
        let cache_key = (aud.to_string(), sub.to_string(), generation);
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| AlienError::new(unavailable("policy cache poisoned")))?;
            if let Some((hash, policy)) = cache.get(&cache_key) {
                if hash == policy_digest {
                    return Ok(policy.clone());
                }
            }
        }
        let mut bytes = Vec::new();
        for index in 0..record.chunks {
            let chunk = self
                .kv
                .get(&chunk_key(&record.policy, index))
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

    /// Publish immutable chunks before atomically advancing their metadata reference. Equal-gen
    /// requests are idempotent only for the same policy; no acknowledged update can change it.
    pub async fn advance(
        &self,
        aud: &str,
        sub: &str,
        generation: u64,
        policy: &Policy,
        policy_digest: &str,
    ) -> Result<()> {
        let bytes = canonical(policy)?;
        if digest(&bytes) != policy_digest {
            return Err(denied("signed policy digest mismatch"));
        }
        let key = key(aud, sub);
        let current = self
            .kv
            .get(&key)
            .await
            .context(unavailable("read policy generation"))?;
        let condition = match current {
            Some(entry) => {
                let record: Record = serde_json::from_slice(&entry.value)
                    .into_alien_error()
                    .context(unavailable("decode policy generation"))?;
                if record.gen > generation
                    || (record.gen == generation && record.policy != policy_digest)
                {
                    return Err(AlienError::new(ErrorData::GenerationSuperseded));
                }
                if record.gen == generation {
                    return Ok(());
                }
                PutCondition::Version(entry.version)
            }
            None => PutCondition::Absent,
        };
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            // Existing content is immutable; its digest is checked when loaded.
            self.kv
                .put(
                    &chunk_key(policy_digest, index),
                    chunk.to_vec(),
                    Some(PutOptions {
                        ttl: None,
                        condition: PutCondition::Absent,
                    }),
                )
                .await
                .context(unavailable("write policy content"))?;
        }
        let record = Record {
            gen: generation,
            policy: policy_digest.to_string(),
            chunks: bytes.len().div_ceil(CHUNK_BYTES),
        };
        let value = serde_json::to_vec(&record)
            .into_alien_error()
            .context(unavailable("encode policy generation"))?;
        if !self
            .kv
            .put(
                &key,
                value,
                Some(PutOptions {
                    ttl: None,
                    condition,
                }),
            )
            .await
            .context(unavailable("advance policy generation"))?
        {
            return Err(AlienError::new(unavailable(
                "generation changed concurrently; repeat update",
            )));
        }
        Ok(())
    }
}

fn chunk_key(digest: &str, index: usize) -> String {
    format!("egress-policy:{digest}:{index}")
}

fn key(aud: &str, sub: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((aud.len() as u64).to_be_bytes());
    hash.update(aud.as_bytes());
    hash.update(sub.as_bytes());
    format!(
        "egress-generation:{}",
        URL_SAFE_NO_PAD.encode(hash.finalize())
    )
}
