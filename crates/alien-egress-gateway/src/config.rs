use std::{collections::HashMap, net::SocketAddr};

use alien_error::{Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_compact::PublicKey;
use ipnet::Ipv4Net;
use serde::Deserialize;

use crate::{
    error::{invalid, unavailable, Result},
    policy::Policy,
};

/// Deployment-owned gateway configuration loaded from the gateway vault.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    /// Proxy address, normally 0.0.0.0:3128.
    pub listen: SocketAddr,
    /// Separate management listener; restrict network access to management workloads.
    #[serde(default = "admin_listen")]
    pub admin: SocketAddr,
    /// Sandbox resource audiences accepted by this deployment.
    pub audiences: Vec<String>,
    /// Create defaults are applied by the binding, never when a policy record is missing.
    #[serde(default)]
    pub default_policies: HashMap<String, Policy>,
    /// Public verification keys. The gateway never receives the token signing key.
    pub keys: Vec<VerificationKey>,
    /// Additional deployment CIDRs. Built-in private ranges cannot be disabled.
    pub denied_ranges: Vec<Ipv4Net>,
    /// Replica discovery; absent means a single local gateway.
    #[serde(default)]
    pub peers: Option<Peers>,
    /// Explicit policy storage budget; otherwise half of the cgroup memory limit.
    #[serde(default)]
    pub policy_memory_bytes: Option<usize>,
    /// Global open proxy connections; rejects excess admission before allocating tunnel buffers.
    #[serde(default = "max_connections")]
    pub max_connections: usize,
    /// Base64 32-byte gateway-only key for replica authentication and scoped CAs. Older
    /// controllers embed it here; newer ones name a `replica_secret_source` instead.
    #[serde(default)]
    pub replica_secret: Option<String>,
    /// Where the gateway reads its replica secret with its own identity. The value never
    /// appears in this configuration, so the managing identity that publishes it never holds it.
    #[serde(default)]
    pub replica_secret_source: Option<ReplicaSecretSource>,
    /// Additional deployment-owned trust roots, base64 DER, for upstream TLS.
    #[serde(default)]
    pub upstream_ca_certificates: Vec<String>,
    /// Accepted only empty, as configurations written by earlier controllers carry it. Credential
    /// rules are per sandbox and set at runtime (`PUT /v1/credentials`), never in configuration.
    #[serde(default)]
    pub credentials: Vec<serde::de::IgnoredAny>,
    /// Services of the deployment each audience reaches by their exact internal hostname,
    /// declared in the stack. Every other private destination stays refused.
    #[serde(default)]
    pub links: HashMap<String, Vec<GatewayLink>>,
}

pub use alien_core::sandbox_links::GatewayLink;

/// A replica secret stored outside the configuration, readable only by the gateway.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ReplicaSecretSource {
    /// An AWS Secrets Manager secret holding the base64 32-byte value as its string.
    #[serde(rename_all = "camelCase")]
    AwsSecretsManager {
        /// Secret name or ARN.
        secret_id: String,
    },
    /// An entry of one of the gateway's vault bindings.
    #[serde(rename_all = "camelCase")]
    Vault {
        /// Vault binding name.
        binding: String,
        /// Secret name within the vault.
        key: String,
    },
    /// A file holding the value, such as a Kubernetes Secret the kubelet mounts into the pod.
    #[serde(rename_all = "camelCase")]
    File {
        /// Path of the file.
        path: String,
    },
}

/// One signing key accepted during key rotation.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationKey {
    /// Deployment key identifier.
    pub kid: String,
    /// Standard base64-encoded raw Ed25519 public key.
    pub public_key: String,
}

impl Config {
    /// The declared link a connection from `aud` to `host:port` names, if any. A literal IP
    /// never names one: link hosts are DNS names.
    pub fn link(&self, aud: &str, host: &str, port: u16) -> Option<&GatewayLink> {
        self.links
            .get(aud)?
            .iter()
            .find(|link| link.matches(host, port))
    }

    /// Declared link ids of one audience.
    pub fn link_ids(&self, aud: &str) -> Vec<String> {
        self.links
            .get(aud)
            .map(|links| links.iter().map(|link| link.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Decode public keys and reject duplicate key IDs.
    pub fn verification_keys(&self) -> Result<HashMap<String, PublicKey>> {
        if !self.credentials.is_empty() {
            return Err(invalid(
                "credentials in the gateway configuration are no longer read; sandbox credential rules are set at runtime",
            ));
        }
        if self.max_connections == 0 || self.max_connections > 65536 {
            return Err(invalid("maxConnections must be between 1 and 65536"));
        }
        if self.keys.is_empty() || self.audiences.is_empty() {
            return Err(invalid("keys and audiences are required"));
        }
        for policy in self.default_policies.values() {
            crate::policy::canonical(policy)?;
        }
        for (aud, links) in &self.links {
            if !self.audiences.contains(aud) {
                return Err(invalid("links name an unknown audience"));
            }
            if links.len() > alien_core::sandbox_links::MAX_SANDBOX_LINKS {
                return Err(invalid("too many links for one audience"));
            }
            for (index, link) in links.iter().enumerate() {
                link.validate().map_err(|reason| invalid(&reason))?;
                if links[..index]
                    .iter()
                    .any(|other| other.id == link.id || other.host == link.host)
                {
                    return Err(invalid("duplicate link id or host"));
                }
            }
        }
        let mut keys = HashMap::new();
        for key in &self.keys {
            let bytes = STANDARD
                .decode(&key.public_key)
                .into_alien_error()
                .context(unavailable("decode verification key"))?;
            let public = PublicKey::from_slice(&bytes)
                .into_alien_error()
                .context(unavailable("load verification key"))?;
            if key.kid.is_empty() || keys.insert(key.kid.clone(), public).is_some() {
                return Err(invalid("empty or duplicate key ID"));
            }
        }
        Ok(keys)
    }
}

fn admin_listen() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 3129))
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Peers {
    Dns(DnsPeers),
    Static(StaticPeers),
}
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsPeers {
    pub dns: String,
    pub port: u16,
}
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticPeers {
    pub r#static: Vec<String>,
}

fn max_connections() -> usize {
    crate::connections::MAX_CONNECTIONS
}
