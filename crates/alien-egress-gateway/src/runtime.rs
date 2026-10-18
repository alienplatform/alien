//! Atomic configuration snapshots with a bounded last-good window.
use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc, RwLock},
    time::Duration,
};

use alien_bindings::Bindings;
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
    current: RwLock<(Arc<Gateway>, Option<Instant>)>,
    accepted: Arc<AtomicU64>,
    denied: Arc<AtomicU64>,
    addresses: (SocketAddr, SocketAddr),
}

impl LiveGateway {
    pub fn new(gateway: Gateway) -> Self {
        Self::with_deadline(gateway, Some(Instant::now()))
    }

    fn with_deadline(gateway: Gateway, loaded: Option<Instant>) -> Self {
        Self {
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

    pub fn snapshot(&self) -> Option<Arc<Gateway>> {
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
            if (gateway.config.listen, gateway.config.admin) != self.addresses {
                return Err(invalid(
                    "listener address changes require a container restart",
                ));
            }
            if let Some(previous) = self.snapshot() {
                if previous.config.generation_store.binding
                    == gateway.config.generation_store.binding
                {
                    gateway.policies = previous.policies.clone();
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
    let kv = bindings
        .kv(&config.generation_store.binding)
        .await
        .context(unavailable("load policy binding"))?;
    let credentials = Credentials::new(kv.clone(), bindings, &config.upstream_ca_certificates)?;
    Ok(Gateway {
        config,
        keys,
        policies: Arc::new(PolicyStore::new(kv)),
        credentials,
        accepted,
        denied,
    })
}
