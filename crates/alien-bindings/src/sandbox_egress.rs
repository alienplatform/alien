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
/// How long publishing a new grant may take in all, however many replicas are away.
const GRANT_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);
/// One request to one replica; a replica that is powered off must not hold a write for long.
const REPLICA_REQUEST: std::time::Duration = std::time::Duration::from_secs(3);
/// A write may wait on the gateway's own fan-out to a peer that is away (its timeout is 5 s).
const REPLICA_WRITE: std::time::Duration = std::time::Duration::from_secs(8);
static WRITER_ID: LazyLock<String> = LazyLock::new(|| {
    ed25519_compact::Seed::generate()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
});
static LAST_MILLIS: AtomicU64 = AtomicU64::new(0);
/// The admin protocol that serves per-sandbox credential rules and the stable sandbox CA.
const CREDENTIALS_PROTOCOL: u32 = 2;

/// A token for the backend's authenticated agent transport. Debug never exposes it.
pub struct EgressDelivery {
    pub(crate) token: String,
    pub(crate) policy: String,
    /// The sandbox's CA, installed with the first delivery so every process the sandbox starts
    /// trusts it. `None` from a gateway that predates per-sandbox credentials.
    pub(crate) ca: Option<String>,
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
    /// Stands in for DNS so tests can name several replicas on one host.
    #[cfg(test)]
    replica_addresses: Option<Vec<std::net::SocketAddr>>,
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
            #[cfg(test)]
            replica_addresses: None,
        })
    }
    /// Proxy authority, without credentials, for native proxy environments and network rules.
    pub(crate) fn gateway_authority(&self) -> &str {
        &self.gateway
    }
    /// Every gateway replica's proxy authority (`ip:port`). One entry when the binding does not
    /// list fixed replica addresses.
    pub(crate) fn proxy_authorities(&self) -> Vec<String> {
        let port = self
            .gateway
            .rsplit_once(':')
            .map(|(_, port)| port.to_string());
        match (&self.config.replicas, port) {
            (Some(replicas), Some(port)) if !replicas.addresses.is_empty() => replicas
                .addresses
                .iter()
                .map(|address| format!("{address}:{port}"))
                .collect(),
            _ => vec![self.gateway.clone()],
        }
    }
    /// Admin authority exposed by the same Container link; requests still require admin tokens.
    pub(crate) fn gateway_admin_authority(&self) -> &str {
        &self.admin_url[url::Position::BeforeHost..url::Position::AfterPort]
    }
    /// Nonsecret launch metadata. AWS injects its own sandbox id alongside this payload.
    pub fn bootstrap_payload(&self) -> String {
        serde_json::json!({"gateway":self.gateway,"audience":self.config.audience}).to_string()
    }
    /// The deployment's current signing key, read from the vault on every use. The binding's
    /// `kid` is only what the key was when the binding was rendered: a linked workload is not
    /// re-rendered when the key changes, and tokens must carry the key id the gateway trusts now.
    async fn issuer(&self) -> Result<EgressTokenIssuer> {
        let secret = self.vault.get_secret(&self.config.signing_key.key).await?;
        let issuer = EgressTokenIssuer::from_vault_document(&secret)?;
        if issuer.kid() != self.config.kid {
            tracing::debug!(
                binding_kid = %self.config.kid,
                vault_kid = %issuer.kid(),
                "sandbox egress signing key changed since this binding was rendered; using the vault's"
            );
        }
        Ok(issuer)
    }
    /// Install policy before the provider delivers the sandbox's identity token.
    pub async fn create(
        &self,
        id: &str,
        policy: &EgressPolicy,
        expires_at: i64,
    ) -> Result<EgressDelivery> {
        let token = self.mint_identity(id, expires_at).await?;
        let policy = self.write_grant(id, policy).await?;
        let ca = self.sandbox_ca(id).await?;
        Ok(EgressDelivery { token, policy, ca })
    }
    /// Publish the policy of a sandbox whose identity was minted earlier (Azure learns its
    /// sandbox id only after the platform create). It is a first write, so it follows the
    /// new-grant rule: one acknowledging replica is enough.
    pub async fn publish_new_grant(&self, id: &str, policy: &EgressPolicy) -> Result<()> {
        policy
            .validate()
            .map_err(crate::providers::sandbox::egress::policy_error)?;
        self.write_grant(id, policy).await.map(|_| ())
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
        let ca = self.sandbox_ca(id).await?;
        Ok(EgressDelivery { token, policy, ca })
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
    /// Publish a brand-new grant. A new grant has nothing to narrow, so it counts once at least
    /// one replica acknowledges it: the replica that was away starts empty, refuses every
    /// connection until it has synchronized from its peer, and so can only ever serve this grant
    /// or none (fail closed). Updates and revocations keep the every-replica rule because a
    /// replica that missed a narrowing would keep serving the wider policy.
    ///
    /// The whole attempt is bounded (`GRANT_BUDGET`), and a failure because no replica answered
    /// is retryable: a replica that is away is a condition that passes.
    async fn write_grant(&self, id: &str, policy: &EgressPolicy) -> Result<String> {
        if self.config.replicas.is_none() {
            return self.write(id, Some(policy)).await;
        }
        let issuer = self.issuer().await?;
        let version = self.next_version()?;
        let record = issuer.mint_record(
            &self.config.audience,
            id,
            (version, &WRITER_ID),
            Some(policy),
        )?;
        let body = serde_json::json!({"record":record});
        let deadline = tokio::time::Instant::now() + GRANT_BUDGET;
        let mut failure = unavailable("no gateway replica acknowledged the new grant");
        // The deadline covers every step of the attempt: the address lookup, the protocol reads,
        // the writes and the read-backs, not only the pauses between rounds.
        let attempt = async {
            loop {
                let targets = match self.resolved_targets().await {
                    Ok(targets) if !targets.is_empty() => targets,
                    Ok(_) => Vec::new(),
                    Err(error) => {
                        failure = error;
                        Vec::new()
                    }
                };
                let min = self
                    .config
                    .replicas
                    .as_ref()
                    .map_or(0, |replicas| replicas.min_protocol);
                let mut acknowledged = 0usize;
                let sends = targets.iter().map(|target| {
                let body = &body;
                let issuer = &issuer;
                async move {
                    let authorization = issuer.mint_admin(
                        &self.config.audience,
                        id,
                        EgressAdminOperation::Put,
                        now_unix()?,
                    )?;
                    if min > 0 {
                        // A replica that cannot be reached is away, not old.
                        if let Ok(served) = self.served_protocol(target).await {
                            if served < min {
                                return Err(unavailable(
                                    "the egress gateway is older than this release needs; re-run setup to update the egress gateway",
                                ));
                            }
                        } else {
                            return Ok(None);
                        }
                    }
                    let response = self
                        .admin
                        .put(format!("{target}/v1/policies"))
                        .timeout(REPLICA_WRITE)
                        .bearer_auth(authorization)
                        .json(body)
                        .send()
                        .await;
                    match response {
                        Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {
                            Ok(Some(true))
                        }
                        Ok(response)
                            if response.status() != reqwest::StatusCode::SERVICE_UNAVAILABLE =>
                        {
                            Err(gateway_error(response).await)
                        }
                        // A 503 with the gateway's own code (store full) is a refusal, not a
                        // missing acknowledgement: surface it as `gateway_error` does.
                        Ok(response) => {
                            let answer = bounded_body(response, 4096).await.unwrap_or_default();
                            let code = serde_json::from_slice::<serde_json::Value>(&answer)
                                .ok()
                                .and_then(|value| value["code"].as_str().map(str::to_owned));
                            if code.as_deref() == Some("SANDBOX_EGRESS_POLICY_STORE_FULL") {
                                return Err(AlienError::new(ErrorData::SandboxEgressPolicyStoreFull));
                            }
                            // A gateway applies a write before it fans it out to its peer, so
                            // while the peer is away it answers 503 even though it holds the
                            // grant. Count that replica only if it serves exactly this record.
                            Ok(self.holds_record(target, id, &body["record"]).await.then_some(true))
                        }
                        // A timeout or connection failure: same read-back.
                        _ => Ok(self.holds_record(target, id, &body["record"]).await.then_some(true)),
                    }
                }
            });
                for outcome in futures::future::join_all(sends).await {
                    match outcome {
                        Ok(Some(_)) => acknowledged += 1,
                        Ok(None) => {}
                        // Old gateway or a refusal that retrying cannot fix: surface it at once.
                        Err(error) => return Err(error),
                    }
                }
                if acknowledged > 0 {
                    return Ok(record);
                }
                if tokio::time::Instant::now() + std::time::Duration::from_millis(500) >= deadline {
                    let mut exhausted = failure.clone();
                    exhausted.retryable = true;
                    return Err(exhausted);
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        };
        match tokio::time::timeout_at(deadline, attempt).await {
            Ok(result) => result,
            Err(_) => {
                failure.retryable = true;
                Err(failure)
            }
        }
    }

    /// Whether this replica serves exactly `record` for `id` (read with a Read-scoped token).
    async fn holds_record(&self, target: &str, id: &str, record: &serde_json::Value) -> bool {
        #[derive(Deserialize)]
        struct Held {
            record: String,
        }
        let Ok(issuer) = self.issuer().await else {
            return false;
        };
        let Ok(authorization) = now_unix().and_then(|now| {
            issuer.mint_admin(&self.config.audience, id, EgressAdminOperation::Read, now)
        }) else {
            return false;
        };
        let Ok(response) = self
            .admin
            .get(format!("{target}/v1/policies/record"))
            .query(&[("aud", self.config.audience.as_str()), ("sub", id)])
            .timeout(REPLICA_REQUEST)
            .bearer_auth(authorization)
            .send()
            .await
        else {
            return false;
        };
        if response.status() != reqwest::StatusCode::OK {
            return false;
        }
        let Ok(body) = bounded_body(response, 360 * 1024).await else {
            return false;
        };
        serde_json::from_slice::<Held>(&body)
            .is_ok_and(|held| record.as_str() == Some(held.record.as_str()))
    }

    /// The admin protocol a replica serves; an error means it could not be reached or answered.
    async fn served_protocol(&self, target: &str) -> Result<u32> {
        #[derive(Deserialize)]
        struct Protocol {
            protocol: u32,
        }
        let response = self
            .admin
            .get(format!("{target}/v1/protocol"))
            .timeout(REPLICA_REQUEST)
            .send()
            .await
            .map_err(|_| unavailable("gateway protocol request failed"))?;
        match response.status() {
            reqwest::StatusCode::OK => {
                let body = bounded_body(response, 1024).await?;
                Ok(serde_json::from_slice::<Protocol>(&body)
                    .map(|p| p.protocol)
                    .unwrap_or(0))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(0),
            _ => Err(unavailable("gateway protocol request was not answered")),
        }
    }

    fn next_version(&self) -> Result<u64> {
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
        Ok(millis.max(previous + 1))
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
        let operation = || {
            if policy.is_some() {
                EgressAdminOperation::Put
            } else {
                EgressAdminOperation::Delete
            }
        };
        let method = if policy.is_some() {
            reqwest::Method::PUT
        } else {
            reqwest::Method::DELETE
        };
        // A replicated write counts only once every ready replica holds it. Rolling
        // updates briefly change the ready set, so those writes retry for longer.
        let attempts = if self.config.replicas.is_some() {
            10
        } else {
            3
        };
        let backoff = |attempt: u64| std::time::Duration::from_millis(100 * (attempt + 1));
        if policy.is_none() && self.config.replicas.is_some() {
            self.revoke(&body, &method, id, &issuer, attempts, backoff)
                .await?;
            return Ok(record);
        }
        // Partial fan-out may already have applied this write. Retry the exact record;
        // allocating a newer version here could overwrite another writer's decision.
        for attempt in 0..attempts {
            let last = attempt + 1 == attempts;
            let targets = match self.write_targets().await {
                Ok(targets) => targets,
                Err(error) if last => return Err(exhausted(error)),
                Err(_) => {
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
            };
            if let Some(min) = self
                .config
                .replicas
                .as_ref()
                .map(|replicas| replicas.min_protocol)
                .filter(|min| *min > 0)
            {
                match self.protocol_check(&targets, min).await {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(unavailable(
                            "the egress gateway is older than this release needs; re-run setup to update the egress gateway",
                        ))
                    }
                    Err(error) if last => return Err(exhausted(error)),
                    Err(_) => {
                        tokio::time::sleep(backoff(attempt)).await;
                        continue;
                    }
                }
            }
            let mut acknowledged = true;
            for target in &targets {
                let authorization =
                    issuer.mint_admin(&self.config.audience, id, operation(), now_unix()?)?;
                let response = self
                    .admin
                    .request(method.clone(), format!("{target}/v1/policies"))
                    .bearer_auth(authorization)
                    .json(&body)
                    .send()
                    .await;
                match response {
                    Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {}
                    Ok(response) => {
                        let transient =
                            response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE;
                        let error = gateway_error(response).await;
                        let passing =
                            transient && error.code == "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE";
                        if !passing {
                            return Err(error);
                        }
                        if last {
                            return Err(exhausted(error));
                        }
                        acknowledged = false;
                        break;
                    }
                    Err(_) if last => {
                        return Err(exhausted(unavailable(
                            "gateway policy request failed after repeated attempts",
                        )))
                    }
                    Err(_) => {
                        acknowledged = false;
                        break;
                    }
                }
            }
            if acknowledged {
                return Ok(record);
            }
            tokio::time::sleep(backoff(attempt)).await;
        }
        unreachable!("every final attempt returns")
    }

    /// Delete a grant on every replica that answers, right away. An unreachable or refusing
    /// replica never stops the delete reaching the others: the reachable replicas are already clean when this
    /// returns a retryable error for the ones that have not acknowledged. A replica that was away
    /// takes the tombstone from its peers before it serves again (it starts empty and refuses until
    /// synchronized, or catches up on its next pass).
    async fn revoke(
        &self,
        body: &serde_json::Value,
        method: &reqwest::Method,
        id: &str,
        issuer: &EgressTokenIssuer,
        attempts: u64,
        backoff: impl Fn(u64) -> std::time::Duration,
    ) -> Result<()> {
        let mut done: std::collections::HashSet<String> = Default::default();
        // Replicas that answered definitely with a refusal; asking them again changes nothing.
        let mut refused: std::collections::HashSet<String> = Default::default();
        let mut refusal: Option<AlienError<ErrorData>> = None;
        let mut failure = unavailable("a gateway replica did not acknowledge the revocation");
        for attempt in 0..attempts {
            // A lookup that fails now may work on the next attempt (a replica is being replaced).
            let (targets, complete) = match self.revocation_targets().await {
                Ok(found) => found,
                Err(error) => {
                    failure = error;
                    if attempt + 1 < attempts {
                        tokio::time::sleep(backoff(attempt)).await;
                    }
                    continue;
                }
            };
            let mut pending = false;
            for target in &targets {
                if done.contains(target) || refused.contains(target) {
                    continue;
                }
                let authorization = issuer.mint_admin(
                    &self.config.audience,
                    id,
                    EgressAdminOperation::Delete,
                    now_unix()?,
                )?;
                let response = self
                    .admin
                    .request(method.clone(), format!("{target}/v1/policies"))
                    .timeout(REPLICA_REQUEST)
                    .bearer_auth(authorization)
                    .json(body)
                    .send()
                    .await;
                match response {
                    Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {
                        done.insert(target.clone());
                    }
                    // 503: the replica is away or catching up, a condition that passes.
                    Ok(response)
                        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE =>
                    {
                        failure = gateway_error(response).await;
                        pending = true;
                    }
                    // Any other answer (401, 400, ...) is a refusal that retrying cannot fix. The
                    // remaining replicas still get the revocation, and a replica that has not
                    // answered yet is still retried: the refusal decides only once every
                    // replica has answered.
                    Ok(response) => {
                        let error = gateway_error(response).await;
                        refused.insert(target.clone());
                        refusal.get_or_insert(error);
                    }
                    // A connection failure or timeout passes too.
                    Err(_) => pending = true,
                }
            }
            if !pending && complete {
                return match refusal {
                    Some(error) => Err(error),
                    None => Ok(()),
                };
            }
            if attempt + 1 < attempts {
                tokio::time::sleep(backoff(attempt)).await;
            }
        }
        // The reachable replicas are clean already; trying again reaches the others.
        failure.retryable = true;
        Err(failure)
    }

    /// Every replica address that resolves now, and whether that is all of them.
    async fn revocation_targets(&self) -> Result<(Vec<String>, bool)> {
        match self.write_targets().await {
            Ok(targets) => Ok((targets, true)),
            Err(error) => {
                let targets = self.resolved_targets().await.unwrap_or_default();
                if targets.is_empty() {
                    Err(error)
                } else {
                    Ok((targets, false))
                }
            }
        }
    }

    /// Admin origins that must all acknowledge a write: the single admin address, or
    /// every ready replica. Fewer resolvable replicas than the gateway runs refuses the
    /// write, so no replica keeps serving a policy the caller believes was replaced.
    async fn write_targets(&self) -> Result<Vec<String>> {
        let targets = self.resolved_targets().await?;
        let expected = self
            .config
            .replicas
            .as_ref()
            .map_or(1, |r| r.count as usize);
        if targets.len() < expected {
            return Err(unavailable(&format!(
                "{} of {} gateway replicas are ready; a policy write must reach every replica",
                targets.len(),
                expected
            )));
        }
        Ok(targets)
    }

    /// The admin origins that resolve now, however many there are.
    async fn resolved_targets(&self) -> Result<Vec<String>> {
        let Some(replicas) = &self.config.replicas else {
            return Ok(vec![self
                .admin_url
                .as_str()
                .trim_end_matches('/')
                .to_string()]);
        };
        #[cfg(test)]
        let resolved = self.replica_addresses.clone();
        #[cfg(not(test))]
        let resolved: Option<Vec<std::net::SocketAddr>> = None;
        let fixed: Option<Vec<std::net::SocketAddr>> = if replicas.addresses.is_empty() {
            None
        } else {
            Some(
                replicas
                    .addresses
                    .iter()
                    .map(|ip| {
                        ip.parse::<std::net::Ipv4Addr>()
                            .map(|ip| std::net::SocketAddr::from((ip, replicas.port)))
                            .map_err(|_| unavailable("invalid gateway replica address"))
                    })
                    .collect::<Result<_>>()?,
            )
        };
        let mut addresses: Vec<_> = match fixed.or(resolved) {
            Some(addresses) => addresses,
            None => tokio::net::lookup_host((replicas.dns.as_str(), replicas.port))
                .await
                .map_err(|_| unavailable("could not resolve the gateway replicas"))?
                .collect(),
        };
        addresses.retain(std::net::SocketAddr::is_ipv4);
        addresses.sort();
        addresses.dedup();
        let scheme = self.admin_url.scheme();
        Ok(addresses
            .into_iter()
            .map(|address| format!("{scheme}://{address}"))
            .collect())
    }

    /// Every target must serve admin protocol `min` or newer. An older gateway (or one that has
    /// no `/v1/protocol` at all) is refused outright: gateway VMs change only when setup is
    /// re-run, so retrying cannot fix it. A target that cannot be reached is a retryable error.
    async fn protocol_check(&self, targets: &[String], min: u32) -> Result<bool> {
        #[derive(Deserialize)]
        struct Protocol {
            protocol: u32,
        }
        for target in targets {
            let response = self
                .admin
                .get(format!("{target}/v1/protocol"))
                .send()
                .await
                .map_err(|_| unavailable("gateway protocol request failed"))?;
            let status = response.status();
            let served = if status == reqwest::StatusCode::OK {
                let body = bounded_body(response, 1024).await?;
                serde_json::from_slice::<Protocol>(&body)
                    .ok()
                    .map(|p| p.protocol)
            } else if status == reqwest::StatusCode::NOT_FOUND {
                Some(0)
            } else {
                return Err(unavailable("gateway protocol request was not answered"));
            };
            if served.is_none_or(|served| served < min) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Replace a sandbox's credential rules on every gateway replica. Returns once every replica
    /// acknowledged the new set, so the sandbox's next request uses it. An empty list removes
    /// every rule. A gateway older than admin protocol 2 is refused, never written to.
    ///
    /// A replica that is away or catching up makes the call fail with a retryable error once
    /// the attempts run out; retrying the call is safe (the newest set wins).
    pub async fn update_credentials(
        &self,
        id: &str,
        rules: &[alien_core::SandboxCredentialRule],
    ) -> Result<()> {
        crate::providers::sandbox::egress::validate_credentials(rules)?;
        let issuer = self.issuer().await?;
        let version = self.next_version()?;
        let record =
            issuer.mint_credentials(&self.config.audience, id, (version, &WRITER_ID), rules)?;
        let body = serde_json::json!({ "record": record });
        drop(record);
        let attempts: u64 = if self.config.replicas.is_some() {
            10
        } else {
            3
        };
        let backoff = |attempt: u64| std::time::Duration::from_millis(100 * (attempt + 1));
        let old = || {
            unavailable(
                "the egress gateway does not serve per-sandbox credentials yet; deploy a release that installs a newer egress gateway and re-run setup",
            )
        };
        // The exact record on every attempt: a partial fan-out may already have applied it.
        for attempt in 0..attempts {
            let last = attempt + 1 == attempts;
            let retry = |error: AlienError<ErrorData>| {
                if last {
                    Err(exhausted(error))
                } else {
                    Ok(())
                }
            };
            let targets = match self.write_targets().await {
                Ok(targets) => targets,
                Err(error) => {
                    retry(error)?;
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
            };
            match self.protocol_check(&targets, CREDENTIALS_PROTOCOL).await {
                Ok(true) => {}
                Ok(false) => return Err(old()),
                Err(error) => {
                    retry(error)?;
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
            }
            let mut acknowledged = true;
            for target in &targets {
                let authorization = issuer.mint_admin(
                    &self.config.audience,
                    id,
                    EgressAdminOperation::Put,
                    now_unix()?,
                )?;
                let response = self
                    .admin
                    .put(format!("{target}/v1/credentials"))
                    .timeout(REPLICA_WRITE)
                    .bearer_auth(authorization)
                    .json(&body)
                    .send()
                    .await;
                match response {
                    Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {}
                    // The route is unknown: an older gateway that answers the protocol request
                    // with a newer number cannot exist, but never ack on it.
                    Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
                        return Err(old())
                    }
                    Ok(response) => {
                        let transient =
                            response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE;
                        let error = credentials_error(response).await;
                        if !(transient && error.code == "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE") {
                            return Err(error);
                        }
                        retry(error)?;
                        acknowledged = false;
                        break;
                    }
                    Err(_) => {
                        retry(unavailable(
                            "gateway credential request failed after repeated attempts",
                        ))?;
                        acknowledged = false;
                        break;
                    }
                }
            }
            if acknowledged {
                return Ok(());
            }
            tokio::time::sleep(backoff(attempt)).await;
        }
        unreachable!("every final attempt returns")
    }

    /// The PEM of the sandbox's CA, which the sandbox trusts for intercepted HTTPS. `None` from
    /// a gateway that predates per-sandbox credentials: its CA would change with rule hosts.
    pub async fn sandbox_ca(&self, id: &str) -> Result<Option<String>> {
        // The CA derives from the deployment's shared secret, so every replica issues the same
        // one: ask each in turn, so one replica being away never blocks a sandbox's creation.
        let mut targets = self.resolved_targets().await.unwrap_or_default();
        // The configured admin address is the last resort, whatever the lookup returned.
        let admin = self.admin_url.as_str().trim_end_matches('/').to_string();
        if !targets.contains(&admin) {
            targets.push(admin);
        }
        let mut last = unavailable("gateway did not issue the sandbox CA");
        for target in targets {
            match self.sandbox_ca_from(&target, id).await {
                Ok(ca) => return Ok(ca),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    async fn sandbox_ca_from(&self, target: &str, id: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Issued {
            certificate: String,
        }
        if self.served_protocol(target).await? < CREDENTIALS_PROTOCOL {
            return Ok(None);
        }
        let authorization = self.issuer().await?.mint_admin_method(
            &self.config.audience,
            id,
            "POST",
            now_unix()?,
        )?;
        let response = self
            .admin
            .post(format!("{target}/v1/ca"))
            .timeout(REPLICA_REQUEST)
            .bearer_auth(authorization)
            .json(&serde_json::json!({"aud": self.config.audience, "sub": id}))
            .send()
            .await
            .map_err(|_| unavailable("gateway CA request failed"))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(unavailable("gateway did not issue the sandbox CA"));
        }
        let body = bounded_body(response, 16 * 1024).await?;
        serde_json::from_slice::<Issued>(&body)
            .map(|issued| Some(issued.certificate))
            .map_err(|_| unavailable("gateway returned an invalid sandbox CA"))
    }

    /// Read the recovery record without renewing the sandbox identity.
    pub async fn policy_record(&self, id: &str) -> Result<String> {
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
                self.admin_url.as_str().trim_end_matches('/')
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
/// A failure that persisted through every attempt is a condition that passes (a replica being
/// replaced), so the caller may try again later. A refusal of the old protocol never goes
/// through here.
fn exhausted(mut error: AlienError<ErrorData>) -> AlienError<ErrorData> {
    error.retryable = true;
    error
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
/// A refusal of a credential write. The gateway's message is never included: it is fixed text,
/// but this keeps any value out of the error even from a gateway that misbehaves.
async fn credentials_error(response: reqwest::Response) -> AlienError<ErrorData> {
    let status = response.status().as_u16();
    let body = bounded_body(response, 4096).await.unwrap_or_default();
    let code = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value["code"].as_str().map(str::to_owned))
        .filter(|code| code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'));
    match code.as_deref() {
        Some("SANDBOX_EGRESS_VERSION_IN_FUTURE") => {
            AlienError::new(ErrorData::SandboxEgressVersionInFuture)
        }
        Some("SANDBOX_EGRESS_POLICY_STORE_FULL") => {
            AlienError::new(ErrorData::SandboxEgressPolicyStoreFull)
        }
        other => unavailable(&format!(
            "gateway refused the credential rules (HTTP {status}{})",
            other.map(|code| format!(", {code}")).unwrap_or_default()
        )),
    }
}
async fn gateway_error(response: reqwest::Response) -> AlienError<ErrorData> {
    let status = response.status().as_u16();
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
        // The status and the gateway's own error code say which replica refused and why.
        other => unavailable(&format!(
            "gateway refused the policy record (HTTP {status}{})",
            other.map(|code| format!(", {code}")).unwrap_or_default()
        )),
    }
}
#[cfg(all(test, feature = "local"))]
#[path = "sandbox_credentials_tests.rs"]
mod credentials_tests;
#[cfg(all(test, feature = "local"))]
#[path = "sandbox_egress_tests.rs"]
pub(crate) mod tests;
