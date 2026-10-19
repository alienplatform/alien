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
    expires: Option<u64>,
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
            .filter(|entry| {
                !entry
                    .record
                    .expired(chrono::Utc::now().timestamp_millis().max(0) as u64)
            })
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
            .filter(|entry| {
                !entry
                    .record
                    .expired(chrono::Utc::now().timestamp_millis().max(0) as u64)
            })
            .map(|entry| entry.record.claims.ver.clone()))
    }
    pub fn record(&self, aud: &str, sub: &str) -> Result<Option<Arc<Record>>> {
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .get(&(aud.to_string(), sub.to_string()))
            .filter(|entry| {
                !entry
                    .record
                    .expired(chrono::Utc::now().timestamp_millis().max(0) as u64)
            })
            .map(|entry| entry.record.clone()))
    }
    pub fn summary(&self) -> Result<Vec<Summary>> {
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .values()
            .filter(|entry| {
                !entry
                    .record
                    .expired(chrono::Utc::now().timestamp_millis().max(0) as u64)
            })
            .map(|entry| Summary {
                aud: entry.record.claims.aud.clone(),
                sub: entry.record.claims.sub.clone(),
                ver: entry.record.claims.ver.clone(),
            })
            .collect())
    }
    /// Replicate only configured audiences, including when older in-flight writes remain stored.
    pub fn newer_than(
        &self,
        versions: &[Summary],
        audiences: &[String],
    ) -> Result<Vec<Arc<Record>>> {
        let mut page_bytes = 0;
        let mut known = BTreeMap::new();
        for version in versions {
            let current = known
                .entry((version.aud.as_str(), version.sub.as_str()))
                .or_insert(&version.ver);
            if compare_versions(current, &version.ver).is_lt() {
                *current = &version.ver;
            }
        }
        Ok(self
            .state
            .read()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?
            .entries
            .values()
            // Filter before paging: a full page of retired records must not hide active records.
            .filter(|entry| audiences.contains(&entry.record.claims.aud))
            .filter(|entry| {
                !entry
                    .record
                    .expired(chrono::Utc::now().timestamp_millis().max(0) as u64)
            })
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
    /// All replicas use the signed timestamp, so catch-up never extends retention.
    pub fn expire_tombstones(&self) -> Result<()> {
        self.expire_at(chrono::Utc::now().timestamp_millis().max(0) as u64)
    }
    fn expire_at(&self, now: u64) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?;
        let mut removed = 0;
        state.entries.retain(|_, entry| {
            let keep = entry.expires.is_none_or(|expires| expires >= now);
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
        self.apply_at(
            record,
            only_missing,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        )
    }
    fn apply_at(&self, mut record: Record, only_missing: bool, now_ms: u64) -> Result<bool> {
        if record.expired(now_ms) {
            return Ok(false);
        }
        if let Some(policy) = &record.claims.policy {
            canonical(policy)?;
        }
        // HTTP bodies and snapshot strings can have different spare capacity. Retain
        // exactly the same allocations for the same record on every ingestion path.
        fn compact(value: &mut String) {
            *value = std::mem::take(value).into_boxed_str().into_string();
        }
        compact(&mut record.signed);
        compact(&mut record.claims.aud);
        compact(&mut record.claims.sub);
        compact(&mut record.claims.ver.1);
        if let Some(Policy::Allow { allow }) = &mut record.claims.policy {
            for target in allow.iter_mut() {
                compact(target);
            }
            *allow = std::mem::take(allow).into_boxed_slice().into_vec();
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
        let allocated_bytes = policy_bytes
            + record.signed.capacity()
            + record.claims.aud.capacity()
            + record.claims.sub.capacity()
            + record.claims.aud.len()
            + record.claims.sub.len()
            + record.claims.ver.1.capacity()
            + 512;
        // Release-load measurements show allocator size classes and fragmentation can
        // nearly double resident memory. Charge that overhead inside the policy budget,
        // leaving the other half of the container for connections and transient requests.
        let bytes = allocated_bytes.saturating_mul(2);
        let key = (record.claims.aud.clone(), record.claims.sub.clone());
        let mut state = self
            .state
            .write()
            .map_err(|_| AlienError::new(unavailable("policy state lock")))?;
        if state
            .entries
            .get(&key)
            .is_some_and(|entry| entry.expires.is_some_and(|expiry| now_ms > expiry))
        {
            if let Some(expired) = state.entries.remove(&key) {
                state.bytes -= expired.bytes;
            }
        }
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
                expires: record
                    .claims
                    .deleted
                    .then(|| record.claims.ver.0.saturating_add(TOMBSTONE_RETENTION_MS)),
                record: Arc::new(record),
                bytes,
            },
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Claims;

    #[tokio::test]
    async fn signed_expiry_is_identical_across_staggered_replicas_and_never_refreshes() {
        let issued = chrono::Utc::now().timestamp_millis() as u64;
        let deadline = issued + TOMBSTONE_RETENTION_MS;
        let record = Record {
            signed: "verified-by-caller".into(),
            claims: Claims {
                v: 1,
                aud: "p/r".into(),
                sub: "box".into(),
                ver: (issued, "0".repeat(32)),
                policy: None,
                deleted: true,
            },
        };
        let replicas = [
            PolicyStore::new(true, 1024 * 1024),
            PolicyStore::new(true, 1024 * 1024),
            PolicyStore::new(true, 1024 * 1024),
        ];
        for (replica, arrival) in replicas
            .iter()
            .zip([issued, issued + 3600_000, deadline - 1])
        {
            assert!(replica.apply_at(record.clone(), false, arrival).unwrap());
            assert_eq!(
                replica
                    .state
                    .read()
                    .unwrap()
                    .entries
                    .values()
                    .next()
                    .unwrap()
                    .expires,
                Some(deadline)
            );
            replica.expire_at(deadline).unwrap();
            assert_eq!(replica.state.read().unwrap().entries.len(), 1);
        }
        for replica in &replicas {
            assert!(!replica.apply_at(record.clone(), false, deadline).unwrap());
            replica.expire_at(deadline + 1).unwrap();
            assert!(replica.state.read().unwrap().entries.is_empty());
            assert!(!replica
                .apply_at(record.clone(), false, deadline + 2)
                .unwrap());
            assert!(replica.state.read().unwrap().entries.is_empty());
        }
        let mut live = record;
        live.claims.deleted = false;
        live.claims.policy = Some(Policy::Internet);
        replicas[0].apply_at(live, false, deadline + 2).unwrap();
        replicas[0].expire_at(deadline + 100 * 3600_000).unwrap();
        assert_eq!(
            *replicas[0].resolve("p/r", "box").await.unwrap(),
            Policy::Internet
        );
    }
}
