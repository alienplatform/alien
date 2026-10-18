//! Durable policy generations and authenticated gateway fencing shared by sandbox providers.

use std::{
    fmt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use alien_core::{bindings::SandboxEgressGateway, SandboxEgress};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::providers::sandbox::egress::check_bound;
use crate::sandbox_egress_token::{EgressTokenIssuer, EgressTokenRequest};
use crate::{
    error::{ErrorData, Result},
    traits::PutOptions,
    Kv, PutCondition, Vault,
};

/// Reserved KV binding used by gateway replicas and sandbox management callers.
/// Deployment setup grants management read/write and points this at the existing generation store.
pub const STATE_BINDING: &str = "egress-state";

/// Durable state is shared across callers, process restarts and token renewals.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyRecord {
    generation: u64,
    policy_hash: String,
    policy_chunks: usize,
    expires_at: i64,
}

/// A fenced token ready for delivery through the backend's authenticated agent transport.
/// Debug deliberately excludes the bearer token.
pub struct EgressDelivery {
    pub(crate) token: String,
    pub(crate) generation: u64,
}

impl fmt::Debug for EgressDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EgressDelivery")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// One policy owner backed by the deployment's existing KV and management-only vault.
pub struct SandboxEgressManager {
    config: SandboxEgressGateway,
    vault: Arc<dyn Vault>,
    kv: Arc<dyn Kv>,
    admin: reqwest::Client,
}

impl fmt::Debug for SandboxEgressManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEgressManager")
            .field("audience", &self.config.audience)
            .finish_non_exhaustive()
    }
}

impl SandboxEgressManager {
    /// Resolve these bindings with the workload's management identity, never a guest identity.
    pub fn new(
        config: SandboxEgressGateway,
        vault: Arc<dyn Vault>,
        kv: Arc<dyn Kv>,
    ) -> Result<Self> {
        check_bound(&config.bound, Some(&config.bound))?;
        let url = reqwest::Url::parse(&config.admin)
            .map_err(|_| unavailable("invalid gateway admin URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(unavailable(
                "gateway admin URL must be an HTTP(S) origin without credentials",
            ));
        }
        let mut client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10));
        if let Some(pem) = &config.admin_ca_pem {
            let certificate = reqwest::Certificate::from_pem(pem.as_bytes())
                .map_err(|_| unavailable("invalid gateway admin CA certificate"))?;
            client = client.add_root_certificate(certificate);
        }
        let admin = client
            .build()
            .map_err(|_| unavailable("could not construct gateway admin client"))?;
        Ok(Self {
            config,
            vault,
            kv,
            admin,
        })
    }

    /// Nonsecret per-launch metadata. AWS injects the sandbox id alongside this payload.
    pub fn bootstrap_payload(&self) -> String {
        serde_json::json!({"gateway":self.config.gateway, "audience":self.config.audience})
            .to_string()
    }

    /// Declaration used by the provider for validation before it starts a sandbox.
    pub fn bound(&self) -> &SandboxEgress {
        &self.config.bound
    }

    /// Record the initial policy only once. A repeated id cannot reset its generation or lifetime.
    pub async fn create(
        &self,
        sandbox_id: &str,
        egress: &SandboxEgress,
        expires_at: i64,
    ) -> Result<EgressDelivery> {
        check_bound(egress, Some(self.bound()))?;
        if expires_at <= now_unix()? {
            return Err(unavailable("sandbox lifetime has expired"));
        }
        let (policy_hash, policy_chunks) = self.store_policy(egress).await?;
        let record = PolicyRecord {
            generation: 0,
            policy_hash,
            policy_chunks,
            expires_at,
        };
        self.write(sandbox_id, &record, PutCondition::Absent)
            .await?;
        self.deliver(sandbox_id, &record).await
    }

    /// Advance the policy using an atomic version precondition; concurrent writers cannot reset it.
    pub async fn update(&self, sandbox_id: &str, egress: &SandboxEgress) -> Result<EgressDelivery> {
        check_bound(egress, Some(self.bound()))?;
        let (mut record, version) = self.read(sandbox_id).await?;
        record.generation = record
            .generation
            .checked_add(1)
            .ok_or_else(|| unavailable("egress generation is exhausted"))?;
        (record.policy_hash, record.policy_chunks) = self.store_policy(egress).await?;
        self.write(sandbox_id, &record, PutCondition::Version(version))
            .await?;
        self.deliver(sandbox_id, &record).await
    }

    /// Reinstall the latest durable policy when contacting a sandbox, including after a restart.
    /// Renewal never changes either the generation or the sandbox's maximum lifetime.
    pub async fn renew(&self, sandbox_id: &str) -> Result<EgressDelivery> {
        let (record, _) = self.read(sandbox_id).await?;
        self.deliver(sandbox_id, &record).await
    }

    /// Detect a concurrent update that overtook delivery; the caller must not report stale success.
    pub async fn confirm(&self, sandbox_id: &str, generation: u64) -> Result<()> {
        if self.read(sandbox_id).await?.0.generation != generation {
            return Err(conflict());
        }
        Ok(())
    }

    async fn read(&self, sandbox_id: &str) -> Result<(PolicyRecord, String)> {
        let entry = self
            .kv
            .get(&self.key(sandbox_id))
            .await?
            .ok_or_else(|| unavailable("sandbox has no durable egress policy"))?;
        let record: PolicyRecord = serde_json::from_slice(&entry.value)
            .map_err(|_| unavailable("invalid stored sandbox egress policy"))?;
        if record.expires_at <= now_unix()? {
            return Err(unavailable("sandbox lifetime has expired"));
        }
        Ok((record, entry.version))
    }

    async fn write(
        &self,
        sandbox_id: &str,
        record: &PolicyRecord,
        condition: PutCondition,
    ) -> Result<()> {
        let value = serde_json::to_vec(record).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "encoding durable sandbox egress policy".to_string(),
            },
        )?;
        // No TTL: expiration must never turn a reused sandbox id into generation zero.
        if !self
            .kv
            .put(
                &self.key(sandbox_id),
                value,
                Some(PutOptions {
                    ttl: None,
                    condition,
                }),
            )
            .await?
        {
            return Err(conflict());
        }
        Ok(())
    }

    async fn deliver(&self, sandbox_id: &str, record: &PolicyRecord) -> Result<EgressDelivery> {
        let now = now_unix()?;
        let egress = self.load_policy(record).await?;
        let secret = self.vault.get_secret(&self.config.signing_key.key).await?;
        let issuer = EgressTokenIssuer::from_vault_json(&secret, &self.config.kid)?;
        let token = issuer.mint(
            EgressTokenRequest::builder()
                .audience(&self.config.audience)
                .sandbox_id(sandbox_id)
                .bound(self.bound())
                .egress(&egress)
                .generation(record.generation)
                .now_unix(now)
                .sandbox_expires_at(record.expires_at)
                .build(),
        )?;
        let authorization = issuer.mint_admin(
            &self.config.audience,
            sandbox_id,
            record.generation,
            &egress
                .policy_digest()
                .map_err(crate::providers::sandbox::egress::policy_error)?,
            now,
        )?;
        let response = self.admin.post(format!("{}/v1/generations", self.config.admin.trim_end_matches('/')))
            .bearer_auth(authorization).json(&serde_json::json!({"aud":self.config.audience, "sub": sandbox_id, "gen":record.generation,"policy":egress}))
            .send().await.map_err(|_| unavailable("gateway generation fence request failed"))?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err(conflict());
        }
        if !response.status().is_success() {
            return Err(unavailable("gateway refused the generation fence"));
        }
        self.confirm(sandbox_id, record.generation).await?;
        Ok(EgressDelivery {
            token,
            generation: record.generation,
        })
    }

    // A policy may contain up to 256 KiB of canonical JSON, exceeding KV's 24 KiB
    // value ceiling. Immutable content-addressed chunks keep the generation CAS in one row.
    // Losing writers can only leave reusable policy content, never a partially visible policy.
    async fn store_policy(&self, policy: &SandboxEgress) -> Result<(String, usize)> {
        let bytes = policy
            .canonical_json()
            .map_err(crate::providers::sandbox::egress::policy_error)?;
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
        let chunks = bytes.chunks(16 * 1024);
        let count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            self.kv
                .put(
                    &format!("sandbox-policy-content:{hash}:{index}"),
                    chunk.to_vec(),
                    Some(PutOptions {
                        ttl: None,
                        condition: PutCondition::Absent,
                    }),
                )
                .await?;
        }
        Ok((hash, count))
    }

    async fn load_policy(&self, record: &PolicyRecord) -> Result<SandboxEgress> {
        if record.policy_chunks == 0 || record.policy_chunks > 16 || record.policy_hash.len() != 43
        {
            return Err(unavailable("invalid policy content reference"));
        }
        let mut bytes = Vec::new();
        for index in 0..record.policy_chunks {
            let entry = self
                .kv
                .get(&format!(
                    "sandbox-policy-content:{}:{index}",
                    record.policy_hash
                ))
                .await?
                .ok_or_else(|| unavailable("stored policy content is missing"))?;
            bytes.extend(entry.value);
        }
        if URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes)) != record.policy_hash {
            return Err(unavailable(
                "stored policy content does not match its digest",
            ));
        }
        let policy =
            serde_json::from_slice(&bytes).map_err(|_| unavailable("invalid stored policy"))?;
        check_bound(&policy, Some(self.bound()))?;
        Ok(policy)
    }

    fn key(&self, sandbox_id: &str) -> String {
        let mut hash = Sha256::new();
        hash.update((self.config.audience.len() as u64).to_be_bytes());
        hash.update(self.config.audience.as_bytes());
        hash.update(sandbox_id.as_bytes());
        format!("sandbox-policy:{}", URL_SAFE_NO_PAD.encode(hash.finalize()))
    }
}

pub(crate) fn now_unix() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable("system clock predates Unix epoch"))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| unavailable("system clock is outside the supported range"))
}

fn unavailable(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxUnreachable {
        operation: "sandbox.egress".to_string(),
        reason: reason.to_string(),
    })
}
fn conflict() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressConflict)
}

#[cfg(all(test, feature = "local"))]
#[path = "sandbox_egress_tests.rs"]
pub(crate) mod tests;
