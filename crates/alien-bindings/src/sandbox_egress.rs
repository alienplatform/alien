//! Signed policy writes and identity-token delivery shared by sandbox providers.
use crate::sandbox_egress_token::{EgressAdminOperation, EgressTokenIssuer, EgressTokenRequest};
use crate::{
    error::{ErrorData, Result},
    Vault,
};
use alien_core::{bindings::SandboxEgressGateway, EgressPolicy};
use alien_error::AlienError;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

/// A token for the backend's authenticated agent transport. Debug never exposes it.
pub struct EgressDelivery {
    pub(crate) token: String,
}
impl fmt::Debug for EgressDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EgressDelivery").finish_non_exhaustive()
    }
}

/// Uses the gateway's policy store; callers do not maintain a second policy database.
pub struct SandboxEgressManager {
    config: SandboxEgressGateway,
    vault: Arc<dyn Vault>,
    gateway: String,
    admin_url: String,
    admin: reqwest::Client,
    writes: Mutex<HashMap<String, Weak<Mutex<()>>>>,
}
impl fmt::Debug for SandboxEgressManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxEgressManager")
            .field("audience", &self.config.audience)
            .finish_non_exhaustive()
    }
}
impl SandboxEgressManager {
    /// Resolve the gateway's proxy and admin ports from the linked Container.
    pub fn new(
        config: SandboxEgressGateway,
        vault: Arc<dyn Vault>,
        proxy_url: &str,
        admin_url: &str,
    ) -> Result<Self> {
        fn origin(value: &str) -> Result<reqwest::Url> {
            let url = reqwest::Url::parse(value)
                .map_err(|_| unavailable("invalid gateway Container internal URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !matches!(url.path(), "" | "/")
            {
                return Err(unavailable(
                    "gateway Container internal URL must be an HTTP(S) origin without credentials",
                ));
            }
            Ok(url)
        }
        let proxy = origin(proxy_url)?;
        let admin = origin(admin_url)?;
        let gateway = proxy[url::Position::BeforeHost..url::Position::AfterPort].to_string();
        Self::with_endpoints(config, vault, gateway, admin.to_string())
    }

    fn with_endpoints(
        config: SandboxEgressGateway,
        vault: Arc<dyn Vault>,
        gateway: String,
        admin_url: String,
    ) -> Result<Self> {
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
            gateway,
            admin_url,
            admin,
            writes: Mutex::new(HashMap::new()),
        })
    }
    /// Nonsecret launch metadata. AWS injects its own sandbox id alongside this payload.
    pub fn bootstrap_payload(&self) -> String {
        serde_json::json!({"gateway":self.gateway,"audience":self.config.audience}).to_string()
    }
    async fn issuer(&self) -> Result<EgressTokenIssuer> {
        let secret = self.vault.get_secret(&self.config.signing_key.key).await?;
        EgressTokenIssuer::from_vault_json(&secret, &self.config.kid)
    }
    /// Install policy before the provider delivers the sandbox's identity token.
    pub async fn create(
        &self,
        id: &str,
        policy: &EgressPolicy,
        expires_at: i64,
    ) -> Result<EgressDelivery> {
        let delivery = self.renew(id, expires_at).await?;
        self.update(id, policy).await?;
        Ok(delivery)
    }
    /// Update new connections without changing the token or the sandbox's lifecycle state.
    pub async fn update(&self, id: &str, policy: &EgressPolicy) -> Result<()> {
        policy
            .validate()
            .map_err(crate::providers::sandbox::egress::policy_error)?;
        self.write(id, Some(policy)).await
    }
    /// Removing a policy makes existing identity tokens unable to open new connections.
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.write(id, None).await
    }
    /// Renewal never reads or rewrites policy. The backend supplies its authoritative deadline.
    pub async fn renew(&self, id: &str, expires_at: i64) -> Result<EgressDelivery> {
        let token = self.issuer().await?.mint(
            EgressTokenRequest::builder()
                .audience(&self.config.audience)
                .sandbox_id(id)
                .now_unix(now_unix()?)
                .sandbox_expires_at(expires_at)
                .build(),
        )?;
        Ok(EgressDelivery { token })
    }
    async fn write(&self, id: &str, policy: Option<&EgressPolicy>) -> Result<()> {
        let lock = {
            let mut writes = self.writes.lock().await;
            writes.retain(|_, lock| lock.strong_count() > 0);
            let entry = writes.entry(id.to_string()).or_default();
            if let Some(lock) = entry.upgrade() {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                *entry = Arc::downgrade(&lock);
                lock
            }
        };
        let _guard = lock.lock().await;
        let issuer = self.issuer().await?;
        // Read the gateway's durable sequence so a restarted client cannot replay an old
        // version. A concurrent writer can win this race; never retry with a newer version,
        // since that could undo its restriction with our stale intent.
        let version = self
            .policy_version(id, &issuer)
            .await?
            .checked_add(1)
            .ok_or_else(|| unavailable("gateway policy version is exhausted"))?;
        let operation = match policy {
            Some(policy) => EgressAdminOperation::Put { policy, version },
            None => EgressAdminOperation::Delete { version },
        };
        let authorization = issuer.mint_admin(&self.config.audience, id, operation, now_unix()?)?;
        let mut body = serde_json::json!({"aud":self.config.audience,"sub":id,"version":version});
        if let Some(policy) = policy {
            body["policy"] = serde_json::from_slice(
                &policy
                    .canonical_json()
                    .map_err(crate::providers::sandbox::egress::policy_error)?,
            )
            .map_err(|_| unavailable("could not encode runtime policy"))?;
        }
        let method = if policy.is_some() {
            reqwest::Method::PUT
        } else {
            reqwest::Method::DELETE
        };
        let response = self
            .admin
            .request(
                method,
                format!("{}/v1/policies", self.admin_url.trim_end_matches('/')),
            )
            .bearer_auth(authorization)
            .json(&body)
            .send()
            .await
            .map_err(|_| unavailable("gateway policy request failed"))?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err(AlienError::new(ErrorData::SandboxEgressConflict {
                sandbox_id: id.into(),
            }));
        }
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(unavailable("gateway refused the policy request"));
        }
        Ok(())
    }

    async fn policy_version(&self, id: &str, issuer: &EgressTokenIssuer) -> Result<u64> {
        #[derive(Deserialize)]
        struct PolicyVersion {
            version: u64,
        }
        let authorization = issuer.mint_admin(
            &self.config.audience,
            id,
            EgressAdminOperation::ReadVersion,
            now_unix()?,
        )?;
        let mut response = self
            .admin
            .get(format!(
                "{}/v1/policies",
                self.admin_url.trim_end_matches('/')
            ))
            .query(&[("aud", self.config.audience.as_str()), ("sub", id)])
            .bearer_auth(authorization)
            .send()
            .await
            .map_err(|_| unavailable("gateway policy version request failed"))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(unavailable("gateway refused the policy version request"));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("gateway policy version response failed"))?
        {
            if body.len() + chunk.len() > 1024 {
                return Err(unavailable("gateway policy version response is too large"));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<PolicyVersion>(&body)
            .map(|value| value.version)
            .map_err(|_| unavailable("gateway returned an invalid policy version"))
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
        operation: "sandbox.egress".into(),
        reason: reason.into(),
    })
}
#[cfg(all(test, feature = "local"))]
#[path = "sandbox_egress_tests.rs"]
pub(crate) mod tests;
