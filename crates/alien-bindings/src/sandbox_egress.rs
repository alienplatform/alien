//! Signed policy writes and identity-token delivery shared by sandbox providers.
use crate::sandbox_egress_token::{EgressTokenIssuer, EgressTokenRequest};
use crate::{
    error::{ErrorData, Result},
    Vault,
};
use alien_core::{bindings::SandboxEgressGateway, EgressPolicy};
use alien_error::AlienError;
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
    admin_url: reqwest::Url,
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
        let admin_url = reqwest::Url::parse(&admin_url)
            .map_err(|_| unavailable("invalid gateway admin URL"))?;
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
    /// Proxy authority, without credentials, for native proxy environments and network rules.
    pub(crate) fn gateway_authority(&self) -> &str {
        &self.gateway
    }
    /// Admin authority exposed by the same Container link; requests still require admin tokens.
    pub(crate) fn gateway_admin_authority(&self) -> &str {
        &self.admin_url[url::Position::BeforeHost..url::Position::AfterPort]
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
        let authorization =
            self.issuer()
                .await?
                .mint_admin(&self.config.audience, id, policy, now_unix()?)?;
        let mut body = serde_json::json!({"aud":self.config.audience,"sub":id});
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
                format!(
                    "{}/v1/policies",
                    self.admin_url.as_str().trim_end_matches('/')
                ),
            )
            .bearer_auth(authorization)
            .json(&body)
            .send()
            .await
            .map_err(|_| unavailable("gateway policy request failed"))?;
        if !response.status().is_success() {
            return Err(unavailable("gateway refused the policy request"));
        }
        Ok(())
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
