//! Bounded in-memory policy state. Replicas retain and verify the original signed mutations.
use crate::{
    error::{denied, unavailable, ErrorData, Result},
    policy::{canonical, Policy},
    record::{compare_versions, Record, Summary, Version, TOMBSTONE_RETENTION_MS},
};
use alien_error::AlienError;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

type Identity = (String, String);
const SNAPSHOT_PAGE_BYTES: usize = 8 * 1024 * 1024;

struct Stored {
    record: Arc<Record>,
    policy: Option<Arc<Policy>>,
    bytes: usize,
    expires: Option<tokio::time::Instant>,
}
#[derive(Default)]
struct State {
    entries: BTreeMap<Identity, Stored>,
    bytes: usize,
}

pub struct PolicyStore {
    state: RwLock<State>,
    ready: AtomicBool,
    max_bytes: usize,
}
impl PolicyStore {
    pub fn new(ready: bool, max_bytes: usize) -> Self {
        Self {
            state: RwLock::new(State::default()),
            ready: AtomicBool::new(ready),
            max_bytes,
        }
    }
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub async fn resolve(&self, aud: &str, sub: &str) -> Result<Arc<Policy>> {
        self.state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .get(&(aud.to_string(), sub.to_string()))
            .and_then(|entry| entry.policy.clone())
            .ok_or_else(|| denied("policy missing or deleted"))
    }
    pub async fn version(&self, aud: &str, sub: &str) -> Result<Option<Version>> {
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .get(&(aud.to_string(), sub.to_string()))
            .map(|entry| entry.record.claims.ver.clone()))
    }
    pub fn record(&self, aud: &str, sub: &str) -> Result<Option<Arc<Record>>> {
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .get(&(aud.to_string(), sub.to_string()))
            .map(|entry| entry.record.clone()))
    }
    pub fn summary(&self) -> Result<Vec<Summary>> {
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .values()
            .map(|entry| Summary {
                aud: entry.record.claims.aud.clone(),
                sub: entry.record.claims.sub.clone(),
                ver: entry.record.claims.ver.clone(),
            })
            .collect())
    }
    pub fn newer_than(&self, versions: &[Summary]) -> Result<Vec<Arc<Record>>> {
        let mut page_bytes = 0;
        let known: BTreeMap<_, _> = versions
            .iter()
            .map(|version| ((version.aud.as_str(), version.sub.as_str()), &version.ver))
            .collect();
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .values()
            .filter(|entry| {
                known
                    .get(&(
                        entry.record.claims.aud.as_str(),
                        entry.record.claims.sub.as_str(),
                    ))
                    .is_none_or(|ver| compare_versions(ver, &entry.record.claims.ver).is_lt())
            })
            .take_while(|entry| {
                let size = entry.record.signed.len() + 4;
                if page_bytes + size > SNAPSHOT_PAGE_BYTES {
                    return false;
                }
                page_bytes += size;
                true
            })
            .map(|entry| entry.record.clone())
            .collect())
    }
    /// Expiration only removes tombstones, at least 25 hours after this replica accepted them.
    pub fn expire_tombstones(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?;
        let now = tokio::time::Instant::now();
        let mut removed = 0;
        state.entries.retain(|_, entry| {
            let keep = entry.expires.is_none_or(|expires| expires > now);
            if !keep {
                removed += entry.bytes;
            }
            keep
        });
        state.bytes -= removed;
        Ok(())
    }

    /// Apply a verified record; equal and older versions are no-ops on every path.
    pub fn apply(&self, record: Record, only_missing: bool) -> Result<bool> {
        if let Some(policy) = &record.claims.policy {
            canonical(policy)?;
        }
        // Charge both decoded policy copies, their string/vector allocations, signed bytes,
        // identity keys, and fixed map/record overhead. Live entries are never evicted.
        let policy_bytes = match &record.claims.policy {
            Some(Policy::Allow { allow }) => {
                (allow.capacity() + allow.len()) * std::mem::size_of::<String>()
                    + allow
                        .iter()
                        .map(|target| target.capacity() + target.len())
                        .sum::<usize>()
            }
            _ => 0,
        };
        let bytes = policy_bytes
            + record.signed.capacity()
            + record.claims.aud.capacity()
            + record.claims.sub.capacity()
            + record.claims.aud.len()
            + record.claims.sub.len()
            + record.claims.ver.1.capacity()
            + 512;
        let key = (record.claims.aud.clone(), record.claims.sub.clone());
        let mut state = self
            .state
            .write()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?;
        let previous_bytes = if let Some(current) = state.entries.get(&key) {
            if only_missing
                || !crate::record::should_replace(
                    Some(&current.record.claims.ver),
                    &record.claims.ver,
                )
            {
                return Ok(false);
            }
            current.bytes
        } else {
            0
        };
        if state.bytes - previous_bytes + bytes > self.max_bytes {
            return Err(AlienError::new(ErrorData::StoreFull));
        }
        state.bytes = state.bytes - previous_bytes + bytes;
        state.entries.insert(
            key,
            Stored {
                policy: record.claims.policy.clone().map(Arc::new),
                expires: record.claims.deleted.then(|| {
                    tokio::time::Instant::now()
                        + std::time::Duration::from_millis(TOMBSTONE_RETENTION_MS)
                }),
                record: Arc::new(record),
                bytes,
            },
        );
        Ok(true)
    }
}
