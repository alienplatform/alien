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

/// Deployment-owned gateway configuration. Secret values are never stored here.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    /// Proxy address, normally 0.0.0.0:3128.
    pub listen: SocketAddr,
    /// Separate management listener; restrict network access to management workloads.
    #[serde(default = "admin_listen")]
    pub admin_listen: SocketAddr,
    /// Immutable declaration bounds keyed by audience.
    pub bounds: HashMap<String, Policy>,
    /// Public verification keys. The gateway never receives the token signing key.
    pub keys: Vec<VerificationKey>,
    /// Additional deployment CIDRs. Built-in private ranges cannot be disabled.
    pub denied_ranges: Vec<Ipv4Net>,
    /// Name of the deployment KV binding shared by every replica.
    #[serde(default = "generation_binding")]
    pub generation_binding: String,
    /// Additional deployment-owned trust roots, base64 DER, for upstream TLS.
    #[serde(default)]
    pub upstream_ca_certificates: Vec<String>,
    /// Opt-in credential rules; no rule means TLS is spliced after SNI validation.
    #[serde(default)]
    pub credentials: Vec<CredentialRule>,
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

/// Header injection scope. Matching is exact host/method and path-prefix at a slash boundary.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialRule {
    /// Audience permitted to use this credential.
    pub aud: String,
    /// Exact host, never a wildcard.
    pub host: String,
    /// HTTPS port.
    pub port: u16,
    /// Exact uppercase HTTP method.
    pub method: String,
    /// Absolute path or subtree, e.g. /v1/messages or /v1/.
    pub path: String,
    /// Header to replace in matching requests.
    pub header: String,
    /// Vault binding name.
    pub vault_binding: String,
    /// Vault secret name.
    pub secret: String,
    /// Optional value prefix, e.g. "Bearer ".
    #[serde(default)]
    pub value_prefix: String,
}

impl Config {
    /// Decode public keys and reject duplicate key IDs.
    pub fn verification_keys(&self) -> Result<HashMap<String, PublicKey>> {
        if self.keys.is_empty() || self.bounds.is_empty() || self.generation_binding.is_empty() {
            return Err(invalid("keys, bounds and generationBinding are required"));
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
fn generation_binding() -> String {
    "egress-state".into()
}
