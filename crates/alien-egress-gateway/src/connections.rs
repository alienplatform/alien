//! Bounded admission and live authorization, shared across configuration refreshes.
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use alien_error::AlienError;
use tokio::sync::{watch, Semaphore};

use crate::{
    error::{unavailable, ErrorData, Result},
    io::Counters,
    policy::{PolicyExt, Target},
    runtime::LiveGateway,
    token::Claims,
};

pub const MAX_CONNECTIONS: usize = 1024;
pub const MAX_SOURCE_PENDING: usize = 32;
pub const MAX_IDENTITY_CONNECTIONS: usize = 128;
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const DNS_CONCURRENCY: usize = 8;
type Identity = (String, String);

#[derive(Default)]
pub struct Connections {
    sources: Mutex<HashMap<IpAddr, usize>>,
    identities: Mutex<HashMap<Identity, Group>>,
    next_id: AtomicU64,
    pub active: AtomicU64,
    pub denials: [AtomicU64; 5],
    log_window: Mutex<Option<(tokio::time::Instant, u32)>>,
}
struct Group {
    dns: Arc<Semaphore>,
    sessions: HashMap<u64, Session>,
}
#[derive(Clone)]
struct Session {
    claims: Claims,
    target: Option<Target>,
    counters: Arc<Counters>,
    cancel: watch::Sender<Option<&'static str>>,
}
pub struct SourcePermit {
    registry: Arc<Connections>,
    address: IpAddr,
}
pub struct IdentityPermit {
    registry: Arc<Connections>,
    identity: Identity,
    id: u64,
    pub dns: Arc<Semaphore>,
}
pub fn capacity() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Capacity)
}

impl Connections {
    pub fn record_denial(&self, category: usize) -> bool {
        self.denials[category].fetch_add(1, Ordering::Relaxed);
        let Ok(mut window) = self.log_window.lock() else {
            return false;
        };
        let (start, count) = window.get_or_insert((tokio::time::Instant::now(), 0));
        if start.elapsed() >= Duration::from_secs(1) {
            *start = tokio::time::Instant::now();
            *count = 0;
        }
        *count = count.saturating_add(1);
        *count <= 20
    }

    pub fn source(self: &Arc<Self>, address: IpAddr) -> Result<SourcePermit> {
        let mut sources = self
            .sources
            .lock()
            .map_err(|_| AlienError::new(unavailable("source admission lock")))?;
        let count = sources.entry(address).or_default();
        if *count >= MAX_SOURCE_PENDING {
            return Err(capacity());
        }
        *count += 1;
        Ok(SourcePermit {
            registry: self.clone(),
            address,
        })
    }

    pub fn register(
        self: &Arc<Self>,
        claims: Claims,
        target: Option<Target>,
        counters: Arc<Counters>,
    ) -> Result<(IdentityPermit, watch::Receiver<Option<&'static str>>)> {
        let identity = (claims.aud.clone(), claims.sub.clone());
        let mut identities = self
            .identities
            .lock()
            .map_err(|_| AlienError::new(unavailable("identity admission lock")))?;
        let group = identities.entry(identity.clone()).or_insert_with(|| Group {
            dns: Arc::new(Semaphore::new(DNS_CONCURRENCY)),
            sessions: HashMap::new(),
        });
        if group.sessions.len() >= MAX_IDENTITY_CONNECTIONS {
            return Err(capacity());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (cancel, receiver) = watch::channel(None);
        group.sessions.insert(
            id,
            Session {
                claims,
                target,
                counters,
                cancel,
            },
        );
        self.active.fetch_add(1, Ordering::Relaxed);
        Ok((
            IdentityPermit {
                registry: self.clone(),
                identity,
                id,
                dns: group.dns.clone(),
            },
            receiver,
        ))
    }

    /// One sweep for every identity; no task or timer is retained for an expired token.
    pub fn expire_idle(&self) {
        let Ok(identities) = self.identities.lock() else {
            return;
        };
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        for session in identities
            .values()
            .flat_map(|group| group.sessions.values())
        {
            let reason = if session.claims.exp <= now {
                Some("token expired")
            } else if session.counters.idle_for() >= IDLE_TIMEOUT {
                Some("idle timeout")
            } else {
                None
            };
            if let Some(reason) = reason {
                session.cancel.send_replace(Some(reason));
            }
        }
    }

    pub async fn revalidate(&self, live: &LiveGateway) {
        let groups: Vec<_> = match self.identities.lock() {
            Ok(identities) => identities
                .iter()
                .map(|(identity, group)| {
                    (
                        identity.clone(),
                        group.sessions.values().cloned().collect::<Vec<_>>(),
                    )
                })
                .collect(),
            Err(_) => return,
        };
        let sweep = async {
            // Batch at most sixteen store reads at a time. Each identity is read once per sweep.
            let mut tasks = tokio::task::JoinSet::new();
            for (identity, sessions) in groups.clone() {
                let gateway = live.snapshot();
                tasks.spawn(async move {
                    let policy = match gateway.as_ref() {
                        Some(gateway) if gateway.config.audiences.contains(&identity.0) => {
                            tokio::time::timeout(
                                Duration::from_secs(5),
                                gateway.policies.resolve(&identity.0, &identity.1),
                            )
                            .await
                            .ok()
                            .and_then(Result::ok)
                        }
                        _ => None,
                    };
                    for session in sessions {
                        if !policy.as_ref().is_some_and(|policy| {
                            session
                                .target
                                .as_ref()
                                .is_none_or(|target| policy.permits(target))
                        }) {
                            session
                                .cancel
                                .send_replace(Some("policy revoked or unavailable"));
                        }
                    }
                });
                if tasks.len() >= 16 {
                    let _ = tasks.join_next().await;
                }
            }
            while tasks.join_next().await.is_some() {}
        };
        if tokio::time::timeout(Duration::from_secs(20), sweep)
            .await
            .is_err()
        {
            // An overloaded or stalled store cannot postpone revocation indefinitely.
            for (_, sessions) in groups {
                for session in sessions {
                    session.cancel.send_replace(Some("policy recheck timeout"));
                }
            }
        }
    }
}
impl Drop for SourcePermit {
    fn drop(&mut self) {
        if let Ok(mut sources) = self.registry.sources.lock() {
            if let Some(count) = sources.get_mut(&self.address) {
                *count -= 1;
                if *count == 0 {
                    sources.remove(&self.address);
                }
            }
        }
    }
}
impl Drop for IdentityPermit {
    fn drop(&mut self) {
        if let Ok(mut identities) = self.registry.identities.lock() {
            if let Some(group) = identities.get_mut(&self.identity) {
                group.sessions.remove(&self.id);
                if group.sessions.is_empty() {
                    identities.remove(&self.identity);
                }
            }
        }
        self.registry.active.fetch_sub(1, Ordering::Relaxed);
    }
}
