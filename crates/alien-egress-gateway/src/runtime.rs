//! Atomic configuration snapshots with a bounded last-good window.
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

#[cfg(feature = "aws")]
use alien_aws_clients::{
    secrets_manager::{GetSecretValueRequest, SecretsManagerApi, SecretsManagerClient},
    AwsCredentialProvider,
};
use alien_bindings::Bindings;
#[cfg(feature = "aws")]
use alien_client_config::ClientConfigExt;
use base64::Engine;
use sha2::Digest;
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
            let previous_secrets = self
                .current
                .read()
                .map_err(|_| AlienError::new(unavailable("config lock poisoned")))?
                .0
                .replication
                .secrets()
                .to_vec();
            let mut gateway = build_after(
                &encoded,
                bindings.clone(),
                self.accepted.clone(),
                self.denied.clone(),
                Some(&previous_secrets),
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
                    || previous.config.policy_memory_bytes != gateway.config.policy_memory_bytes
                    || previous.config.max_connections != gateway.config.max_connections
                {
                    return Err(invalid(
                        "replica discovery/memory-budget/connection-limit changes require a restart",
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
    build_after(encoded, bindings, accepted, denied, None).await
}

/// Builds a snapshot. A refresh passes the replica secrets the running snapshot accepts as
/// `previous`; boot passes `None`.
pub async fn build_after(
    encoded: &str,
    bindings: Arc<Bindings>,
    accepted: Arc<AtomicU64>,
    denied: Arc<AtomicU64>,
    previous: Option<&[[u8; 32]]>,
) -> Result<Gateway> {
    let config: Config = serde_json::from_str(encoded)
        .into_alien_error()
        .context(unavailable("parse gateway config"))?;
    let keys = config.verification_keys()?;
    credentials::validate_rules(&config.credentials)?;
    let embedded = config
        .replica_secret
        .as_deref()
        .map(decode_replica_secret)
        .transpose()?;
    let sourced = match &config.replica_secret_source {
        Some(source) => Some(sourced_replica_secret(
            &read_replica_secret(&bindings, source).await?,
        )?),
        None => None,
    };
    if config.peers.is_some() && embedded.is_none() && sourced.is_none() {
        return Err(invalid("peers requires replicaSecret or replicaSecretSource"));
    }
    let secrets = replica_secrets(previous, embedded, sourced)?;
    // The CA seed stays with the first secret this process knew, so issued CAs are stable
    // until restart.
    let seed = secrets.first().copied();
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
        secrets,
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

fn decode_replica_secret(secret: &str) -> Result<[u8; 32]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(secret.trim())
        .map_err(|_| invalid("invalid replicaSecret encoding"))?;
    bytes
        .try_into()
        .map_err(|_| invalid("replicaSecret must contain 32 bytes"))
}

/// A sourced secret is any generated string of at least 32 characters (CloudFormation and
/// Terraform generate printable passwords, not raw bytes); its SHA-256 is the 32-byte key.
fn sourced_replica_secret(secret: &str) -> Result<[u8; 32]> {
    if secret.len() < 32 {
        return Err(invalid("the replica secret source must hold at least 32 characters"));
    }
    Ok(sha2::Sha256::digest(secret.as_bytes()).into())
}

/// The replica secrets a snapshot accepts. A refresh keeps every secret the running snapshot
/// accepts, so a configuration that moves from an embedded secret to a source keeps verifying
/// (and signing for) peers that have not refreshed yet. The embedded secret never changes in
/// place, a replica without a secret does not gain one, and at most two secrets are held:
/// anything else needs a restart.
fn replica_secrets(
    previous: Option<&[[u8; 32]]>,
    embedded: Option<[u8; 32]>,
    sourced: Option<[u8; 32]>,
) -> Result<Vec<[u8; 32]>> {
    let mut secrets = previous.unwrap_or_default().to_vec();
    let refreshing = previous.is_some();
    let restart = || invalid("replica secret changes require a restart");
    for (secret, may_join) in [(embedded, false), (sourced, true)] {
        let Some(secret) = secret else {
            continue;
        };
        if secrets.contains(&secret) {
            continue;
        }
        if refreshing && (secrets.is_empty() || !may_join || secrets.len() >= 2) {
            return Err(restart());
        }
        secrets.push(secret);
    }
    Ok(secrets)
}

/// Reads the replica secret from its source with the gateway's own credentials.
async fn read_replica_secret(
    bindings: &Bindings,
    source: &crate::config::ReplicaSecretSource,
) -> Result<String> {
    match source {
        crate::config::ReplicaSecretSource::Vault { binding, key } => bindings
            .vault(binding)
            .await
            .context(unavailable("load replica secret vault"))?
            .get_secret(key)
            .await
            .context(unavailable("read replica secret")),
        crate::config::ReplicaSecretSource::AwsSecretsManager { secret_id } => {
            read_aws_secret(secret_id).await
        }
    }
}

#[cfg(feature = "aws")]
async fn read_aws_secret(secret_id: &str) -> Result<String> {
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let config = alien_core::ClientConfig::from_env(alien_core::Platform::Aws, &env)
        .await
        .context(unavailable("load AWS credentials for the replica secret"))?;
    let aws = config
        .aws_config()
        .cloned()
        .ok_or_else(|| invalid("replica secret source needs AWS credentials"))?;
    let credentials = AwsCredentialProvider::from_config(aws)
        .await
        .context(unavailable("load AWS credentials for the replica secret"))?;
    let client = SecretsManagerClient::new(reqwest::Client::new(), credentials);
    client
        .get_secret_value(GetSecretValueRequest {
            secret_id: secret_id.to_string(),
            version_id: None,
            version_stage: None,
        })
        .await
        .context(unavailable("read replica secret"))?
        .secret_string
        .ok_or_else(|| invalid("replica secret has no string value"))
}

#[cfg(not(feature = "aws"))]
async fn read_aws_secret(_secret_id: &str) -> Result<String> {
    Err(invalid("this gateway build cannot read AWS Secrets Manager"))
}

#[cfg(test)]
mod replica_secret_tests {
    use super::*;

    const OLD: [u8; 32] = [1; 32];
    const NEW: [u8; 32] = [2; 32];
    const OTHER: [u8; 32] = [3; 32];

    #[test]
    fn boot_accepts_what_the_configuration_names() {
        assert_eq!(replica_secrets(None, Some(OLD), None).unwrap(), [OLD]);
        assert_eq!(replica_secrets(None, None, Some(NEW)).unwrap(), [NEW]);
        assert_eq!(replica_secrets(None, Some(OLD), Some(NEW)).unwrap(), [OLD, NEW]);
        assert!(replica_secrets(None, None, None).unwrap().is_empty());
    }

    /// The one change a running gateway takes: from the embedded secret to a source. It keeps
    /// the embedded one, so peers that have not refreshed still verify it.
    #[test]
    fn a_refresh_moves_from_the_embedded_secret_to_a_source_keeping_both() {
        let moved = replica_secrets(Some(&[OLD]), None, Some(NEW)).unwrap();
        assert_eq!(moved, [OLD, NEW]);
        assert_eq!(replica_secrets(Some(&moved), None, Some(NEW)).unwrap(), [OLD, NEW]);
        assert_eq!(replica_secrets(Some(&[OLD]), Some(OLD), Some(NEW)).unwrap(), [OLD, NEW]);
    }

    #[test]
    fn every_other_change_needs_a_restart() {
        for (previous, embedded, sourced) in [
            (&[OLD][..], Some(NEW), None),
            (&[OLD, NEW][..], None, Some(OTHER)),
            (&[][..], Some(OLD), None),
            (&[][..], None, Some(NEW)),
        ] {
            let error = replica_secrets(Some(previous), embedded, sourced).unwrap_err();
            assert!(error.message.contains("require a restart"), "{error:?}");
        }
    }
}
