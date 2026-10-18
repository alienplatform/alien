use std::sync::Arc;

use alien_bindings::{traits::PutOptions, Kv, PutCondition};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

use crate::error::{denied, unavailable, Result};

/// Monotonic generation fence in the deployment's existing KV store.
/// No cache: every authorization reads the shared store and fails closed on errors.
pub struct GenerationFence {
    kv: Arc<dyn Kv>,
}

impl GenerationFence {
    /// Use a KV binding shared by every gateway replica.
    pub fn new(kv: Arc<dyn Kv>) -> Self {
        Self { kv }
    }

    /// Reject a generation that has been superseded by an authenticated update.
    pub async fn check(&self, aud: &str, sub: &str, generation: u64) -> Result<()> {
        let entry = self
            .kv
            .get(&key(aud, sub))
            .await
            .context(unavailable("read generation fence"))?;
        if let Some(entry) = entry {
            let highest = decode(&entry.value)?;
            if generation < highest {
                return Err(denied("stale generation"));
            }
        }
        Ok(())
    }

    /// Advance the fence atomically. Concurrent updates never lower the value.
    /// A lost compare-and-swap returns a retryable error to the admin caller.
    pub async fn advance(&self, aud: &str, sub: &str, generation: u64) -> Result<()> {
        let key = key(aud, sub);
        let current = self
            .kv
            .get(&key)
            .await
            .context(unavailable("read generation fence"))?;
        let condition = match current {
            Some(entry) => {
                if decode(&entry.value)? >= generation {
                    return Ok(());
                }
                PutCondition::Version(entry.version)
            }
            None => PutCondition::Absent,
        };
        let written = self
            .kv
            .put(
                &key,
                generation.to_be_bytes().to_vec(),
                Some(PutOptions {
                    ttl: None,
                    condition,
                }),
            )
            .await
            .context(unavailable("write generation fence"))?;
        if !written {
            return Err(AlienError::new(unavailable(
                "generation changed concurrently; repeat update",
            )));
        }
        Ok(())
    }
}

fn decode(value: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = value
        .try_into()
        .into_alien_error()
        .context(unavailable("invalid stored generation"))?;
    Ok(u64::from_be_bytes(bytes))
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
