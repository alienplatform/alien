//! Atomic configuration snapshots. A failed refresh closes new proxy and admin requests.
use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc, RwLock},
    time::Duration,
};

use alien_bindings::Bindings;
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
    current: RwLock<Option<Arc<Gateway>>>,
    accepted: Arc<AtomicU64>,
    denied: Arc<AtomicU64>,
    addresses: (SocketAddr, SocketAddr),
}

impl LiveGateway {
    pub fn new(gateway: Gateway) -> Self {
        Self {
            accepted: gateway.accepted.clone(),
            denied: gateway.denied.clone(),
            addresses: (gateway.config.listen, gateway.config.admin),
            current: RwLock::new(Some(Arc::new(gateway))),
        }
    }

    pub fn snapshot(&self) -> Option<Arc<Gateway>> {
        self.current.read().ok().and_then(|value| value.clone())
    }

    /// Poll only the rendered public config entry, never the signing-key entry.
    pub async fn refresh(&self, bindings: &Arc<Bindings>) -> Result<()> {
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let encoded = read_vault_config(bindings).await?;
            let gateway = build(
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
                *current = Some(gateway);
                Ok(())
            }
            Err(error) => {
                *current = None;
                Err(error)
            }
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
        policies: PolicyStore::new(kv),
        credentials,
        accepted,
        denied,
    })
}
