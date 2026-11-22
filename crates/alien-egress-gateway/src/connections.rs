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
    policy::{check_addresses, check_link_addresses, PolicyExt, Target},
    runtime::LiveGateway,
    token::Claims,
};

pub const MAX_CONNECTIONS: usize = 2048;
pub const MAX_SOURCE_PENDING: usize = 32;
pub const MAX_IDENTITY_CONNECTIONS: usize = 128;
/// Idle limit of a connection that is not a declared link.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Lifetime of a connection that is not a declared link; limits resource retention by idle peers.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3600);
/// Idle limit of a declared link's connection.
pub const LINK_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DNS_CONCURRENCY: usize = 8;
type Identity = (String, String);

/// Idle and lifetime limits the sweep applies to open connections.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// Idle limit of a connection that is not a declared link.
    pub idle: Duration,
    /// Lifetime of a connection that is not a declared link.
    pub lifetime: Duration,
    /// Idle limit of a declared link's connection. A link carries long-lived connections
    /// (WebSockets, database and cache pools), so it has no lifetime limit: it stays open while
    /// the sandbox's token, the configuration and the policy keep it.
    pub link_idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            idle: IDLE_TIMEOUT,
            lifetime: CONNECTION_TIMEOUT,
            link_idle: LINK_IDLE_TIMEOUT,
        }
    }
}

#[derive(Default)]
pub struct Connections {
    pub timeouts: Timeouts,
    sources: Mutex<HashMap<IpAddr, usize>>,
    identities: Mutex<HashMap<Identity, Group>>,
    next_id: AtomicU64,
    pub active: AtomicU64,
    pub denials: [AtomicU64; 5],
    pub dns_latency: crate::metrics::Histogram,
    pub connect_latency: crate::metrics::Histogram,
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
    peer: Option<IpAddr>,
    /// Id of the declared link the session reached, if any.
    link: Option<String>,
    counters: Arc<Counters>,
    opened: tokio::time::Instant,
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
    /// A registry with these limits instead of the defaults.
    pub fn with_timeouts(timeouts: Timeouts) -> Self {
        Self {
            timeouts,
            ..Self::default()
        }
    }

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
                peer: None,
                link: None,
                counters,
                opened: tokio::time::Instant::now(),
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

    /// One sweep for every identity; no task or timer is retained for an expired token. A
    /// declared link's connection ends on its own idle limit or with the token, which is bounded
    /// by the sandbox's lifetime; any other connection also ends after `timeouts.lifetime`.
    pub fn expire_idle(&self) {
        let Ok(identities) = self.identities.lock() else {
            return;
        };
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        for session in identities
            .values()
            .flat_map(|group| group.sessions.values())
        {
            let limits = self.timeouts;
            let reason = if session.claims.exp <= now {
                Some("token expired")
            } else if session.link.is_some() {
                (session.counters.idle_for() >= limits.link_idle).then_some("idle timeout")
            } else if session.counters.idle_for() >= limits.idle {
                Some("idle timeout")
            } else if session.opened.elapsed() >= limits.lifetime {
                Some("connection lifetime exceeded")
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
                        // A linked session stays open only while the current configuration
                        // still declares the same link for its target and the policy keeps it.
                        let link = session.link.as_ref().and_then(|id| {
                            let gateway = gateway.as_ref()?;
                            let target = session.target.as_ref()?;
                            gateway
                                .config
                                .link(&identity.0, &target.host, target.port)
                                .filter(|link| &link.id == id)
                        });
                        let policy_allowed =
                            policy
                                .as_ref()
                                .is_some_and(|policy| match (&session.link, link) {
                                    (Some(_), Some(link)) => policy.allows_link(&link.id),
                                    (Some(_), None) => false,
                                    (None, _) => session
                                        .target
                                        .as_ref()
                                        .is_none_or(|target| policy.permits(target)),
                                });
                        let address_allowed = session.peer.is_none_or(|address| match link {
                            Some(link) => check_link_addresses(&[address], link).is_ok(),
                            None => gateway.as_ref().is_some_and(|gateway| {
                                session.link.is_none()
                                    && check_addresses(&[address], &gateway.config.denied_ranges)
                                        .is_ok()
                            }),
                        });
                        if !policy_allowed || !address_allowed {
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
impl IdentityPermit {
    /// Retain the actual checked peer for live address-range revocation, including named targets.
    pub fn record_peer(&self, address: IpAddr, link: Option<String>) -> Result<()> {
        let mut identities = self
            .registry
            .identities
            .lock()
            .map_err(|_| AlienError::new(unavailable("identity admission lock")))?;
        let session = identities
            .get_mut(&self.identity)
            .and_then(|group| group.sessions.get_mut(&self.id))
            .ok_or_else(|| AlienError::new(unavailable("connection registration missing")))?;
        session.peer = Some(address);
        session.link = link;
        Ok(())
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

#[cfg(test)]
mod timeout_tests {
    use std::{sync::Arc, time::Duration};

    use super::{Connections, Timeouts};
    use crate::{io::Counters, token::Claims};

    fn claims() -> Claims {
        let now = chrono::Utc::now().timestamp() as u64;
        Claims {
            iss: "alien-sandbox".into(),
            aud: "p/r".into(),
            sub: "s".into(),
            iat: now,
            exp: now + 600,
        }
    }

    /// A declared link's connection outlives the idle and lifetime limits of other
    /// connections and ends only on its own idle limit; the others keep theirs.
    #[tokio::test(start_paused = true)]
    async fn a_link_connection_outlives_the_other_limits_and_ends_on_its_own_idle_limit() {
        let registry = Arc::new(Connections::with_timeouts(Timeouts {
            idle: Duration::from_secs(2),
            lifetime: Duration::from_secs(5),
            link_idle: Duration::from_secs(60),
        }));
        let open = |link: Option<&str>| {
            let counters = Arc::new(Counters::default());
            let (permit, cancelled) = registry.register(claims(), None, counters.clone()).unwrap();
            permit
                .record_peer("10.0.0.5".parse().unwrap(), link.map(str::to_owned))
                .unwrap();
            (permit, cancelled, counters)
        };
        let (_link, link, link_counters) = open(Some("api"));
        let (_quiet, quiet, _) = open(None);
        let (_busy, busy, busy_counters) = open(None);

        // Quiet past the idle limit: only the unlinked quiet connection ends.
        tokio::time::advance(Duration::from_secs(3)).await;
        busy_counters.touch();
        registry.expire_idle();
        assert_eq!(*quiet.borrow(), Some("idle timeout"));
        assert_eq!(*link.borrow(), None);
        assert_eq!(*busy.borrow(), None);

        // Past the lifetime: the busy unlinked connection ends, the link stays.
        tokio::time::advance(Duration::from_secs(3)).await;
        busy_counters.touch();
        registry.expire_idle();
        assert_eq!(*busy.borrow(), Some("connection lifetime exceeded"));
        assert_eq!(*link.borrow(), None);

        // Activity keeps the link open well past both; quiet past its own limit ends it.
        tokio::time::advance(Duration::from_secs(50)).await;
        link_counters.touch();
        tokio::time::advance(Duration::from_secs(50)).await;
        registry.expire_idle();
        assert_eq!(*link.borrow(), None);
        tokio::time::advance(Duration::from_secs(11)).await;
        registry.expire_idle();
        assert_eq!(*link.borrow(), Some("idle timeout"));
    }

    #[test]
    fn default_limits() {
        let limits = Timeouts::default();
        assert_eq!(limits.idle, Duration::from_secs(120));
        assert_eq!(limits.lifetime, Duration::from_secs(3600));
        assert_eq!(limits.link_idle, Duration::from_secs(1800));
    }
}
