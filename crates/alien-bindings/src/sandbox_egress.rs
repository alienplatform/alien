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
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, LazyLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
// One writer identity and logical clock per process, shared by reopened bindings.
// A process-wide clock is also strictly increasing for every individual sandbox,
// without retaining a map of all sandbox identities ever used.
static WRITER_ID: LazyLock<String> = LazyLock::new(|| {
    ed25519_compact::Seed::generate()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
});
static LAST_MILLIS: AtomicU64 = AtomicU64::new(0);

/// A token for the backend's authenticated agent transport. Debug never exposes it.
pub struct EgressDelivery {
    pub(crate) token: String,
    pub(crate) policy: String,
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
        let token = self.mint_identity(id, expires_at).await?;
        let policy = self.write(id, Some(policy)).await?;
        Ok(EgressDelivery { token, policy })
    }
    /// Update new connections without changing the token or the sandbox's lifecycle state.
    pub async fn update(&self, id: &str, policy: &EgressPolicy) -> Result<()> {
        policy
            .validate()
            .map_err(crate::providers::sandbox::egress::policy_error)?;
        self.write(id, Some(policy)).await.map(|_| ())
    }
    /// Removing a policy makes existing identity tokens unable to open new connections.
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.write(id, None).await.map(|_| ())
    }
    /// Resume delivers the latest gateway record without rewriting it.
    pub async fn renew(&self, id: &str, expires_at: i64) -> Result<EgressDelivery> {
        let token = self.mint_identity(id, expires_at).await?;
        let policy = self.policy_record(id).await?;
        Ok(EgressDelivery { token, policy })
    }
    /// Azure needs identity before create; this never reads or publishes policy.
    pub async fn mint_identity(&self, id: &str, expires_at: i64) -> Result<String> {
        let token = self.issuer().await?.mint(
            EgressTokenRequest::builder()
                .audience(&self.config.audience)
                .sandbox_id(id)
                .now_unix(now_unix()?)
                .sandbox_expires_at(expires_at)
                .build(),
        )?;
        Ok(token)
    }
    async fn write(&self, id: &str, policy: Option<&EgressPolicy>) -> Result<String> {
        let issuer = self.issuer().await?;
        let millis = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| unavailable("system clock predates Unix epoch"))?
                .as_millis(),
        )
        .map_err(|_| unavailable("system clock is outside the supported range"))?;
        let previous = LAST_MILLIS
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
                last.checked_add(1).map(|next| next.max(millis))
            })
            .map_err(|_| unavailable("policy clock is exhausted"))?;
        let version = millis.max(previous + 1);
        let record =
            issuer.mint_record(&self.config.audience, id, (version, &WRITER_ID), policy)?;
        let body = serde_json::json!({"record":record});
        // Partial fan-out may already have applied this write. Retry the exact record;
        // allocating a newer version here could overwrite another writer's decision.
        for attempt in 0..3 {
            let operation = if policy.is_some() {
                EgressAdminOperation::Put
            } else {
                EgressAdminOperation::Delete
            };
            let authorization =
                issuer.mint_admin(&self.config.audience, id, operation, now_unix()?)?;
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
                .await;
            match response {
                Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {
                    return Ok(record)
                }
                Ok(response) => {
                    let transient = response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE;
                    let error = gateway_error(response).await;
                    if !transient
                        || error.code != "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE"
                        || attempt == 2
                    {
                        return Err(error);
                    }
                }
                Err(_) if attempt == 2 => {
                    return Err(unavailable(
                        "gateway policy request failed after three attempts",
                    ))
                }
                Err(_) => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(100 * (attempt + 1))).await;
        }
        unreachable!("every final attempt returns")
    }

    async fn policy_record(&self, id: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct PolicyRecord {
            record: String,
        }
        let issuer = self.issuer().await?;
        let authorization = issuer.mint_admin(
            &self.config.audience,
            id,
            EgressAdminOperation::Read,
            now_unix()?,
        )?;
        let response = self
            .admin
            .get(format!(
                "{}/v1/policies/record",
                self.admin_url.trim_end_matches('/')
            ))
            .query(&[("aud", self.config.audience.as_str()), ("sub", id)])
            .bearer_auth(authorization)
            .send()
            .await
            .map_err(|_| unavailable("gateway policy record request failed"))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(unavailable(
                "gateway has no readable policy record for this sandbox",
            ));
        }
        let body = bounded_body(response, 360 * 1024).await?;
        serde_json::from_slice::<PolicyRecord>(&body)
            .map(|value| value.record)
            .map_err(|_| unavailable("gateway returned an invalid policy record"))
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
    AlienError::new(ErrorData::SandboxEgressGatewayUnavailable {
        reason: reason.into(),
    })
}
async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| unavailable("gateway response failed"))?
    {
        if body.len() + chunk.len() > limit {
            return Err(unavailable("gateway response is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
async fn gateway_error(response: reqwest::Response) -> AlienError<ErrorData> {
    let body = match bounded_body(response, 4096).await {
        Ok(body) => body,
        Err(error) => return error,
    };
    let code = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value["code"].as_str().map(str::to_owned));
    match code.as_deref() {
        Some("SANDBOX_EGRESS_VERSION_IN_FUTURE") => {
            AlienError::new(ErrorData::SandboxEgressVersionInFuture)
        }
        Some("SANDBOX_EGRESS_POLICY_STORE_FULL") => {
            AlienError::new(ErrorData::SandboxEgressPolicyStoreFull)
        }
        _ => unavailable("gateway refused the policy record"),
    }
}
#[cfg(all(test, feature = "local"))]
#[path = "sandbox_egress_tests.rs"]
pub(crate) mod tests;
