//! Atomic configuration snapshots with a bounded last-good window.
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use alien_bindings::Bindings;
use base64::Engine;
use tokio::time::Instant;

/// A vault outage may delay key/range changes by at most ten minutes.
pub const MAX_CONFIG_AGE: Duration = Duration::from_secs(600);
use alien_error::{AlienError, Context, IntoAlienError};

use crate::{
    config::Config,
    credentials::{self, Credentials},
    error::{invalid, unavailable, Result},
    server::Gateway,
    store::PolicyStore,
};

/// One consistent config/key/policy-store snapshot per request.
pub struct LiveGateway {
    stopping: AtomicBool,
    current: RwLock<(Arc<Gateway>, Option<Instant>)>,
    accepted: Arc<AtomicU64>,
    denied: Arc<AtomicU64>,
    addresses: (SocketAddr, SocketAddr),
    connections: Arc<crate::connections::Connections>,
    policies: Arc<PolicyStore>,
}

impl LiveGateway {
    pub fn new(gateway: Gateway) -> Self {
        Self::with_deadline(gateway, Some(Instant::now()))
    }

    fn with_deadline(gateway: Gateway, loaded: Option<Instant>) -> Self {
        Self {
            stopping: AtomicBool::new(false),
            connections: gateway.connections.clone(),
            policies: gateway.policies.clone(),
            accepted: gateway.accepted.clone(),
            denied: gateway.denied.clone(),
            addresses: (gateway.config.listen, gateway.config.admin),
            current: RwLock::new((Arc::new(gateway), loaded)),
        }
    }

    /// Explicit local configuration has no refresh source or stale deadline.
    pub fn static_config(gateway: Gateway) -> Self {
        Self::with_deadline(gateway, None)
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    pub fn snapshot(&self) -> Option<Arc<Gateway>> {
        if self.stopping.load(Ordering::Acquire) {
            return None;
        }
        self.current.read().ok().and_then(|current| {
            let (gateway, loaded) = &*current;
            (loaded.is_none_or(|loaded| loaded.elapsed() < MAX_CONFIG_AGE)).then(|| gateway.clone())
        })
    }

    /// Poll only the rendered public config entry, never the signing-key entry.
    pub async fn refresh(&self, bindings: &Arc<Bindings>) -> Result<()> {
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let encoded = read_vault_config(bindings).await?;
            let mut gateway = build(
                &encoded,
                bindings.clone(),
                self.accepted.clone(),
                self.denied.clone(),
            )
            .await?;
            gateway.connections = self.connections.clone();
            if (gateway.config.listen, gateway.config.admin) != self.addresses {
                return Err(invalid(
                    "listener address changes require a container restart",
                ));
            }
            gateway.policies = self.policies.clone();
            {
                // Compare immutable settings even after the last-good serving window expired.
                let previous = self
                    .current
                    .read()
                    .map_err(|_| AlienError::new(unavailable("config lock poisoned")))?
                    .0
                    .clone();
                if previous.config.peers != gateway.config.peers
                    || previous.config.replica_secret != gateway.config.replica_secret
                    || previous.config.policy_memory_bytes != gateway.config.policy_memory_bytes
                    || previous.config.max_connections != gateway.config.max_connections
                {
                    return Err(invalid(
                        "replica discovery/key/memory-budget/connection-limit changes require a restart",
                    ));
                }
                if previous.config.credentials == gateway.config.credentials
                    && previous.config.upstream_ca_certificates
                        == gateway.config.upstream_ca_certificates
                {
                    gateway.credentials = previous.credentials.clone();
                }
            }
            Ok(Arc::new(gateway))
        })
        .await
        .into_alien_error()
        .context(unavailable("config refresh timeout"))
        .and_then(|value| value);
        let mut current = self
            .current
            .write()
            .map_err(|_| AlienError::new(unavailable("config lock poisoned")))?;
        match result {
            Ok(gateway) => {
                *current = (gateway, Some(Instant::now()));
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

pub async fn read_vault_config(bindings: &Bindings) -> Result<String> {
    bindings
        .vault("secrets")
        .await
        .context(unavailable("load config vault"))?
        .get_secret("sandbox-egress-gateway-config")
        .await
        .context(unavailable("read gateway config"))
}

pub async fn build(
    encoded: &str,
    bindings: Arc<Bindings>,
    accepted: Arc<AtomicU64>,
    denied: Arc<AtomicU64>,
) -> Result<Gateway> {
    let config: Config = serde_json::from_str(encoded)
        .into_alien_error()
        .context(unavailable("parse gateway config"))?;
    let keys = config.verification_keys()?;
    credentials::validate_rules(&config.credentials)?;
    let seed = config
        .replica_secret
        .as_ref()
        .map(|secret| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(secret)
                .map_err(|_| invalid("invalid replicaSecret encoding"))?;
            bytes
                .try_into()
                .map_err(|_| invalid("replicaSecret must contain 32 bytes"))
        })
        .transpose()?;
    if !config.credentials.is_empty() && seed.is_none() {
        return Err(invalid("credential rules require replicaSecret"));
    }
    let credentials = Credentials::new(
        bindings,
        &config.upstream_ca_certificates,
        &config.credentials,
        seed,
    )?;
    let mut resolver = hickory_resolver::TokioResolver::builder_tokio()
        .into_alien_error()
        .context(unavailable("read DNS configuration"))?;
    // Every connection validates fresh answers. Do not retain stale DNS cache grants.
    resolver.options_mut().cache_size = 0;
    let resolver = resolver
        .build()
        .into_alien_error()
        .context(unavailable("build DNS resolver"))?;
    let replication = Arc::new(crate::replication::Replication::new(
        config.peers.clone(),
        seed,
        config.admin.port(),
        resolver.clone(),
    )?);
    let policies = Arc::new(PolicyStore::new(
        !replication.enabled(),
        crate::memory::policy_budget(
            config.policy_memory_bytes,
            std::path::Path::new("/sys/fs/cgroup/memory.max"),
        )?,
    ));
    Ok(Gateway {
        replication,
        connections: Arc::new(crate::connections::Connections::default()),
        resolver,
        config,
        keys,
        policies,
        credentials: Arc::new(credentials),
        accepted,
        denied,
    })
}
