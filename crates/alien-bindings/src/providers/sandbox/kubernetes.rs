//! Kubernetes sandbox provider: a pod under a sandboxed runtime class, reached over the agent
//! protocol.
//!
//! The application never holds a cluster credential. It asks the operator's broker for a
//! sandbox, and gets back a pod address plus a capability scoped to that sandbox. Claiming a pod
//! is a `PATCH` on pods, which does not belong in the binding: `pods/exec`
//! would reach every pod in the namespace.
//!
//! It authenticates to the broker with the ServiceAccount token Kubernetes already mounted in
//! its pod. Nothing of Alien's is created, rotated or torn down for this.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[path = "kubernetes_egress.rs"]
mod egress;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::agent_protocol::{self, AgentTransport};
use crate::traits::{
    Binding, CommandOutput, CreateSandboxRequest, InteractiveCommand, JobPoll, JobStart,
    PreviewCapability, ResolvedSandbox, RunCommandRequest, Sandbox, SandboxInstance, SandboxState,
};
use alien_core::bindings::KubernetesSandboxBinding;
use alien_core::{Platform, SandboxCapabilities, SandboxCapability};
use alien_error::{AlienError, Context, IntoAlienError};

/// A capability with less than this left is replaced before it is used, so a request built at the
/// end of its life does not reach the agent after it has expired.
const CAPABILITY_REFRESH_MARGIN_SECONDS: i64 = 60;

/// What the broker lists for a sandbox.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionList {
    session_ids: Vec<String>,
}

/// What the broker hands back for a claimed sandbox.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimResponse {
    #[serde(default)]
    agent_session_id: Option<String>,
    session_id: String,
    endpoint: String,
    capability: String,
    expires_at: i64,
    /// What the pod records about the session; absent from a broker that predates them.
    #[serde(default = "first_generation")]
    generation: u64,
    /// Whether this request created the session. A broker that predates the field only ever
    /// created.
    #[serde(default = "yes")]
    created: bool,
    /// The tenant key the session was created with.
    #[serde(default)]
    tenant_key: Option<String>,
    /// Unix seconds at which the session ends; absent when it has no lifetime (or the broker
    /// predates the field).
    #[serde(default)]
    deadline: Option<i64>,
    /// Whether the creator is still setting the session up (its egress policy). Absent from a
    /// broker that predates the field, whose sessions are never marked.
    #[serde(default)]
    setup_pending: bool,
}

/// How long the gateway identity of a session without a lifetime is issued for. The issuer caps
/// every identity at this length anyway.
const GATEWAY_IDENTITY_FALLBACK_SECONDS: i64 = 86_400;

fn yes() -> bool {
    true
}

fn first_generation() -> u64 {
    1
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRequest<'a> {
    sandbox_id: &'a str,
    session_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant_key: Option<&'a str>,
    /// How long the session may live from the claim; the broker refuses more than the sandbox
    /// declares.
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
}

/// What asking the broker about a session id found.
enum Lookup {
    Found(ClaimResponse),
    /// No pod carries the id: a clean not-found.
    NotFound,
    /// The broker has no route for a lookup by id (405, or 404 from one that does not know the
    /// path): an Operator that predates this client.
    Unsupported,
}

/// Asks the broker for a session and a fresh capability.
async fn lookup_claim(client: &reqwest::Client, url: &str, token: &str) -> Result<Lookup> {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::OperationNotSupported {
            operation: "sandbox.get".to_string(),
            reason: "the sandbox broker is unreachable".to_string(),
        })?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Lookup::NotFound);
    }
    if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
        return Ok(Lookup::Unsupported);
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "sandbox.get".to_string(),
            reason: broker_refusal(status, &body),
        }));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::UnexpectedResponseFormat {
            provider: "kubernetes-sandbox".to_string(),
            binding_name: "sandbox.get".to_string(),
            field: "body".to_string(),
            response_json: "the broker returned a body this provider cannot parse".to_string(),
        })
        .map(Lookup::Found)
}

/// Like [`lookup_claim`], where a broker without the lookup is an error. `None` when no pod
/// carries the id.
async fn fetch_claim(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<Option<ClaimResponse>> {
    match lookup_claim(client, url, token).await? {
        Lookup::Found(claim) => Ok(Some(claim)),
        Lookup::NotFound => Ok(None),
        Lookup::Unsupported => Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "sandbox.get".to_string(),
            reason: "the sandbox broker predates lookups by id; upgrade the Operator".to_string(),
        })),
    }
}

/// How long a caller waits for another caller's setup of a session it found.
const SETUP_WAIT: std::time::Duration = std::time::Duration::from_secs(150);
#[cfg(not(test))]
const SETUP_POLL: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(test)]
const SETUP_POLL: std::time::Duration = std::time::Duration::from_millis(5);

/// One session's agent for as long as an interactive command runs.
///
/// A command can outlive any capability, and its input is many short requests, so each request
/// checks the capability it holds and asks the broker for a new one before it expires.
#[derive(Debug)]
struct SessionAgent {
    client: reqwest::Client,
    session_url: String,
    token_path: String,
    binding_name: String,
    sandbox_id: String,
    claim: tokio::sync::Mutex<ClaimResponse>,
}

#[async_trait]
impl super::interactive::BoundAgent for SessionAgent {
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let mut claim = self.claim.lock().await;
        if claim.expires_at - chrono::Utc::now().timestamp() <= CAPABILITY_REFRESH_MARGIN_SECONDS {
            let token = read_identity_token(&self.token_path, &self.binding_name).await?;
            *claim = fetch_claim(&self.client, &self.session_url, &token)
                .await?
                .ok_or_else(|| {
                    AlienError::new(ErrorData::OperationNotSupported {
                        operation: "sandbox.agent".to_string(),
                        reason: format!(
                            "sandbox '{}' no longer exists; it ended while a command was running",
                            self.sandbox_id
                        ),
                    })
                })?;
        }
        Ok(self
            .client
            .request(method, format!("{}{path}", claim.endpoint))
            .bearer_auth(&claim.capability))
    }

    fn provider(&self) -> &'static str {
        "kubernetes-sandbox"
    }
}

/// What a broker answer that is not a success means to the caller.
///
/// A 401 is the broker refusing the token itself: neither the token projected for the broker's
/// audience nor the default ServiceAccount token was accepted for this deployment.
fn broker_refusal(status: reqwest::StatusCode, body: &str) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return format!(
            "the sandbox broker refused this workload's token ({status}). It accepts the \
             ServiceAccount token projected for the audience `alien-sandbox-broker` (mounted at \
             the sandbox binding's token path in every workload that links the sandbox) and, \
             until the workload is rolled, the default ServiceAccount token of a workload of \
             this deployment"
        );
    }
    format!("the sandbox broker returned {status}: {body}")
}

/// The ServiceAccount token the kubelet mounts in every pod.
const DEFAULT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

/// Reads the pod's ServiceAccount token. Per call: Kubernetes rotates projected tokens in place.
///
/// A workload started before it was linked to the sandbox, or before an upgrade, has no projected
/// token at `token_path`; it falls back to the default mount, which the broker still accepts, so
/// the workload keeps working until it is rolled.
async fn read_identity_token(token_path: &str, binding_name: &str) -> Result<String> {
    match tokio::fs::read_to_string(token_path).await {
        Ok(token) => Ok(token),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::read_to_string(DEFAULT_TOKEN_PATH)
                .await
                .into_alien_error()
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: format!(
                        "could not read the ServiceAccount token at '{token_path}' or at \
                         '{DEFAULT_TOKEN_PATH}'"
                    ),
                })
        }
        Err(error) => Err(error)
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                env_var: alien_core::bindings::binding_env_var_name(binding_name),
                reason: format!("could not read the ServiceAccount token at '{token_path}'"),
            }),
    }
}

/// A Sandbox backed by pods under a sandboxed runtime class.
#[derive(Debug)]
pub struct KubernetesSandbox {
    egress_manager: Option<Arc<crate::sandbox_egress::SandboxEgressManager>>,
    default_policy: Option<alien_core::EgressPolicy>,
    policy_write: tokio::sync::Mutex<()>,
    egress: Option<alien_core::SandboxEgress>,
    resource_id: String,
    broker_url: String,
    token_path: String,
    binding_name: String,
    client: reqwest::Client,
    /// Capabilities this process holds, so most calls need no round trip to the broker.
    ///
    /// Only a cache. The broker reads sessions off the pods themselves, so any process can look
    /// a session up by id, and a capability that is close to expiring is replaced the same way.
    claims: Mutex<BTreeMap<String, ClaimResponse>>,
}

impl KubernetesSandbox {
    /// Builds a provider from its binding.
    pub fn new(
        binding_name: &str,
        binding: &KubernetesSandboxBinding,
        resource_id: &str,
    ) -> Result<Self> {
        Self::new_with_egress_manager(binding_name, binding, resource_id, None)
    }

    pub(crate) fn new_with_egress_manager(
        binding_name: &str,
        binding: &KubernetesSandboxBinding,
        resource_id: &str,
        egress_manager: Option<Arc<crate::sandbox_egress::SandboxEgressManager>>,
    ) -> Result<Self> {
        let gateway = binding.egress.as_ref().and_then(|value| value.gateway());
        if gateway.is_some() != egress_manager.is_some() {
            return Err(super::egress::unsupported("gateway-backed sandbox"));
        }
        let default_policy = binding.default_policy.clone().or_else(|| {
            binding
                .egress
                .as_ref()
                .and_then(|value| value.default_policy().cloned())
        });
        if egress_manager.is_none() {
            super::egress::require_default(
                default_policy.as_ref(),
                binding.egress.as_ref().map(|value| value.declaration()),
            )?;
        }

        let value = |field: &'static str, value: alien_core::bindings::BindingValue<String>| {
            value.into_value(binding_name, field).map_err(|error| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: error.to_string(),
                })
            })
        };

        Ok(Self {
            egress_manager,
            default_policy,
            policy_write: tokio::sync::Mutex::new(()),
            egress: binding
                .egress
                .as_ref()
                .map(|egress| egress.declaration().clone()),
            resource_id: resource_id.to_string(),
            broker_url: value("brokerUrl", binding.broker_url.clone())?
                .trim_end_matches('/')
                .to_string(),
            token_path: value("tokenPath", binding.token_path.clone())?,
            binding_name: binding_name.to_string(),
            client: reqwest::Client::new(),
            claims: Mutex::new(BTreeMap::new()),
        })
    }

    /// Reads the pod's ServiceAccount token.
    ///
    /// Read per call rather than cached: Kubernetes rotates projected tokens in place, and a
    /// cached copy becomes a token the apiserver refuses at the least convenient moment.
    async fn identity_token(&self) -> Result<String> {
        read_identity_token(&self.token_path, &self.binding_name).await
    }

    fn cached(&self, sandbox_id: &str) -> Option<ClaimResponse> {
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .get(sandbox_id)
            .cloned()
    }

    fn session_url(&self, session_id: &str) -> String {
        format!(
            "{}/v1/sandbox/{}/sessions/{session_id}",
            self.broker_url, self.resource_id
        )
    }

    /// The session with this id, from the cache while its capability has more than a minute left
    /// and the session has not reached its deadline, and from the broker otherwise. `None` when
    /// no pod carries the id, which is a clean not-found rather than an error.
    async fn claimed(&self, sandbox_id: &str) -> Result<Option<ClaimResponse>> {
        if let Some(claim) = self.cached(sandbox_id) {
            let now = chrono::Utc::now().timestamp();
            let capability_current = claim.expires_at - now > CAPABILITY_REFRESH_MARGIN_SECONDS;
            let session_running = claim.deadline.is_none_or(|deadline| deadline > now);
            if capability_current && session_running {
                return Ok(Some(claim));
            }
        }
        self.claimed_by_broker(sandbox_id).await
    }

    /// The session with this id as the broker answers it now, whatever the cache holds.
    async fn claimed_by_broker(&self, sandbox_id: &str) -> Result<Option<ClaimResponse>> {
        let token = self.identity_token().await?;
        match lookup_claim(&self.client, &self.session_url(sandbox_id), &token).await? {
            Lookup::Found(claim) => Ok(Some(self.remember(claim))),
            Lookup::NotFound => {
                self.forget(sandbox_id);
                Ok(None)
            }
            Lookup::Unsupported => Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox.get".to_string(),
                reason: "the sandbox broker predates lookups by id; upgrade the Operator"
                    .to_string(),
            })),
        }
    }

    fn remember(&self, claim: ClaimResponse) -> ClaimResponse {
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .insert(claim.session_id.clone(), claim.clone());
        claim
    }

    fn forget(&self, sandbox_id: &str) {
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .remove(sandbox_id);
    }

    /// Waits until the creator of `claim`'s session has finished setting it up. A session the
    /// creator ended because its setup failed is an error here, not a session to use.
    async fn after_setup(&self, mut claim: ClaimResponse) -> Result<ClaimResponse> {
        let started = std::time::Instant::now();
        while claim.setup_pending {
            if started.elapsed() >= SETUP_WAIT {
                return Err(self.failed(
                    "sandbox.getOrCreate",
                    &format!(
                        "sandbox '{}' is still being set up by the process that created it",
                        claim.session_id
                    ),
                ));
            }
            tokio::time::sleep(SETUP_POLL).await;
            claim = self
                .claimed_by_broker(&claim.session_id)
                .await?
                .ok_or_else(|| {
                    self.failed(
                        "sandbox.getOrCreate",
                        &format!(
                            "sandbox '{}' ended while the process that created it was setting it up",
                            claim.session_id
                        ),
                    )
                })?;
        }
        Ok(claim)
    }

    /// Tells the broker this process has finished setting the session up. A broker that predates
    /// the route never marked the session, so there is nothing to clear.
    async fn setup_done(&self, session_id: &str) {
        let outcome = async {
            let response = self
                .client
                .post(format!("{}/ready", self.session_url(session_id)))
                .bearer_auth(self.identity_token().await?)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::OperationNotSupported {
                    operation: "sandbox.create".to_string(),
                    reason: "the sandbox broker is unreachable".to_string(),
                })?;
            Ok::<_, AlienError<ErrorData>>(response.status())
        }
        .await;
        match outcome {
            Ok(status)
                if status.is_success()
                    || status == reqwest::StatusCode::NOT_FOUND
                    || status == reqwest::StatusCode::METHOD_NOT_ALLOWED => {}
            // Callers that find the session wait at most the broker's setup window.
            Ok(status) => {
                tracing::warn!(%status, session_id, "the broker did not record the end of setup")
            }
            Err(error) => {
                tracing::warn!(%error, session_id, "the broker did not record the end of setup")
            }
        }
    }

    async fn create_resolved(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        self.validate_egress(request.egress.as_ref())?;
        let _policy_write = self.policy_write.lock().await;
        let policy = request
            .egress
            .clone()
            .or_else(|| self.default_policy.clone())
            .unwrap_or(alien_core::EgressPolicy::Internet);
        let sandbox_id = request
            .sandbox_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

        // A caller that will set the session up says so, so that others who find it wait. A query
        // parameter: a broker that predates it ignores it.
        let setup = if self.egress_manager.is_some() {
            "?setup=pending"
        } else {
            ""
        };
        let response = self
            .client
            .post(format!("{}/v1/sandbox/sessions{setup}", self.broker_url))
            .bearer_auth(self.identity_token().await?)
            .json(&ClaimRequest {
                sandbox_id: &self.resource_id,
                session_id: &sandbox_id,
                tenant_key: request.tenant_key.as_deref(),
                timeout_ms: request.timeout_ms,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::OperationNotSupported {
                operation: "sandbox.create".to_string(),
                reason: "the sandbox broker is unreachable".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            // 503 is the pool being empty, which refills on the controller's next health tick.
            // Saying so is the difference between a caller retrying and a caller giving up.
            return Err(self.failed("sandbox.create", &broker_refusal(status, &body)));
        }

        let claim: ClaimResponse = response.json().await.into_alien_error().context(
            ErrorData::UnexpectedResponseFormat {
                provider: "kubernetes-sandbox".to_string(),
                binding_name: "sandbox.create".to_string(),
                field: "body".to_string(),
                response_json: "the broker returned a body this provider cannot parse".to_string(),
            },
        )?;

        let claim = self.remember(claim);

        // Another process created this id between the caller's lookup and this claim; the broker
        // answered with its session, which already carries its own policy.
        if !claim.created {
            let claim = self.after_setup(claim).await?;
            self.require_same_tenant(&claim, request.tenant_key.as_deref())?;
            return Ok(ResolvedSandbox::found(SandboxInstance {
                sandbox_id: claim.session_id,
                state: SandboxState::Running,
                generation: claim.generation,
            }));
        }

        if let Some(manager) = &self.egress_manager {
            let installation = async {
                let identity = Self::egress_identity(&claim)?;
                // The identity token lives as long as the session. `claim.expires_at` is when
                // the agent capability needs renewing (15 minutes), not when the session ends:
                // using it here would cut off the session's internet access after one capability.
                let session_ends_at = match claim.deadline {
                    Some(deadline) => deadline,
                    None => crate::sandbox_egress::now_unix()? + GATEWAY_IDENTITY_FALLBACK_SECONDS,
                };
                let delivery = manager.create(identity, &policy, session_ends_at).await?;
                self.deliver_egress(&claim.session_id, Some(&delivery.token), &delivery.policy)
                    .await
            }
            .await;
            if let Err(error) = installation {
                // Release the claimed pod even if publishing its initial policy failed.
                self.terminate(&claim.session_id)
                    .await
                    .context(ErrorData::SandboxEgressGatewayUnavailable {
                    reason:
                        "initial policy delivery failed and the claimed pod could not be cleaned up"
                            .into(),
                })?;
                return Err(error);
            }
        }

        if self.egress_manager.is_some() {
            self.setup_done(&claim.session_id).await;
        }

        Ok(ResolvedSandbox::created(SandboxInstance {
            sandbox_id: claim.session_id,
            state: SandboxState::Running,
            generation: claim.generation,
        }))
    }

    /// An existing session answers only to the tenant key it was created with. A request with a
    /// different key, or with none for a keyed session (or the reverse), is refused.
    fn require_same_tenant(&self, claim: &ClaimResponse, requested: Option<&str>) -> Result<()> {
        if claim.tenant_key.as_deref() == requested {
            return Ok(());
        }
        Err(self.failed(
            "sandbox.getOrCreate",
            &format!(
                "sandbox '{}' exists with a different tenant key than the request names",
                claim.session_id
            ),
        ))
    }

    fn failed(&self, operation: &str, reason: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: operation.to_string(),
            reason: reason.to_string(),
        })
    }
}

#[async_trait]
impl AgentTransport for KubernetesSandbox {
    async fn request(
        &self,
        sandbox_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let claim = self.claimed(sandbox_id).await?.ok_or_else(|| {
            self.failed(
                "sandbox.agent",
                &format!("sandbox '{sandbox_id}' does not exist; it may have been terminated"),
            )
        })?;

        Ok(self
            .client
            .request(method, format!("{}{path}", claim.endpoint))
            .bearer_auth(claim.capability))
    }

    fn provider(&self) -> &'static str {
        "kubernetes-sandbox"
    }
}

impl Binding for KubernetesSandbox {}

#[async_trait]
impl Sandbox for KubernetesSandbox {
    fn egress_declaration(&self) -> Option<&alien_core::SandboxEgress> {
        self.egress.as_ref()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// What a Kubernetes sandbox offers: the platform table, unchanged.
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Kubernetes)
            .expect("Kubernetes has a sandbox backend")
    }

    /// Claims a warm pod through the broker.
    async fn create(&self, request: CreateSandboxRequest) -> Result<SandboxInstance> {
        self.create_resolved(request)
            .await
            .map(|resolved| resolved.sandbox)
    }

    /// Any process of the deployment can look a session up by id: the broker reads it off the pod.
    /// An id no pod carries is `None`. Like `get_or_create`, this asks the broker every time: a
    /// cached capability only says the pod existed when it was minted, and another process may
    /// have ended the session since. It also waits for a creator that is still setting the
    /// session up.
    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        let Some(claim) = self.claimed_by_broker(sandbox_id).await? else {
            return Ok(None);
        };
        let claim = self.after_setup(claim).await?;
        Ok(Some(SandboxInstance {
            sandbox_id: claim.session_id,
            state: SandboxState::Running,
            generation: claim.generation,
        }))
    }

    async fn get_or_create(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        self.validate_egress(request.egress.as_ref())?;
        // `timeout_ms` applies when this call creates the session. A session that already exists
        // keeps the deadline it was created with.
        if let Some(id) = request.sandbox_id.as_deref() {
            // The broker decides whether the session exists: a cached capability only says the
            // pod existed when it was minted, and the Operator may have ended it since.
            let token = self.identity_token().await?;
            match lookup_claim(&self.client, &self.session_url(id), &token).await? {
                Lookup::Found(claim) => {
                    let claim = self.remember(claim);
                    // Another process may still be setting its egress up.
                    let claim = self.after_setup(claim).await?;
                    // The id names one session; a caller with a different tenant key does not get
                    // it, and does not get to change its egress policy either.
                    self.require_same_tenant(&claim, request.tenant_key.as_deref())?;
                    if let Some(policy) = request.egress.clone() {
                        self.update_egress(id, policy).await?;
                    }
                    return Ok(ResolvedSandbox::found(SandboxInstance {
                        sandbox_id: claim.session_id,
                        state: SandboxState::Running,
                        generation: claim.generation,
                    }));
                }
                Lookup::NotFound => self.forget(id),
                // An Operator that predates lookups by id: create as it always has. It cannot
                // check a tenant key, so a caller that set one is refused rather than served
                // without the check.
                Lookup::Unsupported => {
                    if request.tenant_key.is_some() {
                        return Err(self.failed(
                            "sandbox.getOrCreate",
                            "the sandbox broker predates lookups by id and cannot honour a tenantKey; upgrade the Operator",
                        ));
                    }
                }
            }
        }

        self.create_resolved(request).await
    }

    async fn update_egress(
        &self,
        sandbox_id: &str,
        policy: alien_core::EgressPolicy,
    ) -> Result<()> {
        self.validate_egress(Some(&policy))?;
        let _policy_write = self.policy_write.lock().await;
        let manager = self
            .egress_manager
            .as_ref()
            .ok_or_else(|| super::egress::unsupported("runtime policy"))?;
        let claim = self
            .claimed(sandbox_id)
            .await?
            .ok_or_else(|| self.failed("sandbox.updateEgress", "the sandbox does not exist"))?;
        let identity = Self::egress_identity(&claim)?;
        manager.update(identity, &policy).await?;
        let record = manager.policy_record(identity).await?;
        self.deliver_egress(sandbox_id, None, &record).await
    }

    async fn list(&self) -> Result<Vec<SandboxInstance>> {
        let response = self
            .client
            .get(format!(
                "{}/v1/sandbox/{}/sessions",
                self.broker_url, self.resource_id
            ))
            .bearer_auth(self.identity_token().await?)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::OperationNotSupported {
                operation: "sandbox.list".to_string(),
                reason: "the sandbox broker is unreachable".to_string(),
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(self.failed("sandbox.list", &broker_refusal(status, &body)));
        }
        let list: SessionList = response.json().await.into_alien_error().context(
            ErrorData::UnexpectedResponseFormat {
                provider: "kubernetes-sandbox".to_string(),
                binding_name: "sandbox.list".to_string(),
                field: "body".to_string(),
                response_json: "the broker returned a body this provider cannot parse".to_string(),
            },
        )?;
        Ok(list
            .session_ids
            .into_iter()
            .map(|sandbox_id| SandboxInstance {
                sandbox_id,
                state: SandboxState::Running,
                generation: 1,
            })
            .collect())
    }

    async fn run_command(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        agent_protocol::run_command(self, sandbox_id, request).await
    }

    async fn run_interactive(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<InteractiveCommand> {
        let claim = self.claimed(sandbox_id).await?.ok_or_else(|| {
            self.failed(
                "sandbox.agent",
                &format!("sandbox '{sandbox_id}' does not exist; it may have been terminated"),
            )
        })?;
        let agent = Arc::new(SessionAgent {
            client: self.client.clone(),
            session_url: self.session_url(&claim.session_id),
            token_path: self.token_path.clone(),
            binding_name: self.binding_name.clone(),
            sandbox_id: claim.session_id.clone(),
            claim: tokio::sync::Mutex::new(claim),
        });
        super::interactive::run_interactive(agent, request).await
    }

    async fn start_job(&self, sandbox_id: &str, request: RunCommandRequest) -> Result<JobStart> {
        agent_protocol::start_job(self, sandbox_id, request).await
    }

    async fn poll_job(
        &self,
        sandbox_id: &str,
        job_id: &str,
        since_seq: Option<u64>,
    ) -> Result<JobPoll> {
        agent_protocol::poll_job(self, sandbox_id, job_id, since_seq).await
    }

    async fn cancel_job(&self, sandbox_id: &str, job_id: &str) -> Result<()> {
        agent_protocol::cancel_job(self, sandbox_id, job_id).await
    }

    async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        agent_protocol::read_file(self, sandbox_id, path).await
    }

    async fn write_files(&self, sandbox_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        agent_protocol::write_files(self, sandbox_id, files).await
    }

    /// Mints a capability for one declared port of the session's own server and returns the
    /// agent's in-cluster URL for it. Only workloads that link the sandbox reach that address.
    async fn preview(&self, sandbox_id: &str, port: u16) -> Result<PreviewCapability> {
        let claim = self.claimed(sandbox_id).await?.ok_or_else(|| {
            self.failed(
                "sandbox.preview",
                &format!("sandbox '{sandbox_id}' does not exist; it may have been terminated"),
            )
        })?;
        let response = self
            .client
            .post(format!("{}/preview", self.session_url(&claim.session_id)))
            .bearer_auth(self.identity_token().await?)
            .json(&serde_json::json!({ "port": port }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::OperationNotSupported {
                operation: "sandbox.preview".to_string(),
                reason: "the sandbox broker is unreachable".to_string(),
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let reason = if status == reqwest::StatusCode::FORBIDDEN {
                format!("port {port} is not one of this sandbox's declared preview ports")
            } else {
                broker_refusal(status, "")
            };
            return Err(self.failed("sandbox.preview", &reason));
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Grant {
            endpoint: String,
            capability: String,
            expires_at: i64,
        }
        let grant: Grant =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::OperationNotSupported {
                    operation: "sandbox.preview".to_string(),
                    reason: "the sandbox broker's answer was unreadable".to_string(),
                })?;
        Ok(PreviewCapability {
            endpoint: grant.endpoint,
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                format!("Bearer {}", grant.capability),
            )]),
            allowed_ports: vec![port],
            expires_in_seconds: u64::try_from(grant.expires_at - chrono::Utc::now().timestamp())
                .unwrap_or(0),
        })
    }

    async fn pause(&self, _sandbox_id: &str) -> Result<()> {
        Err(self.failed("pauseResume", "a pod cannot be paused and resumed"))
    }

    /// A pod is never paused, so a sandbox that exists is already running and resuming it is a
    /// no-op. One that does not exist cannot come back.
    async fn resume(&self, sandbox_id: &str) -> Result<()> {
        match self.claimed(sandbox_id).await? {
            Some(_) => Ok(()),
            None => Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "sandboxTerminated".to_string(),
                reason: format!("sandbox.resume: no sandbox '{sandbox_id}' is running"),
            })),
        }
    }

    async fn snapshot(&self, _sandbox_id: &str) -> Result<String> {
        Err(self.failed("snapshot", "a pod has no snapshot primitive"))
    }

    /// Releases the sandbox, which deletes its pod.
    ///
    /// Idempotent: a sandbox this process never claimed is already in the desired end state.
    async fn terminate(&self, sandbox_id: &str) -> Result<()> {
        let Some(claim) = self.claimed(sandbox_id).await? else {
            return Ok(());
        };

        let response = self
            .client
            .delete(self.session_url(&claim.session_id))
            .bearer_auth(self.identity_token().await?)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::OperationNotSupported {
                operation: "sandbox.terminate".to_string(),
                reason: "the sandbox broker is unreachable".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(self.failed("sandbox.terminate", &broker_refusal(status, &body)));
        }

        if let Some(manager) = &self.egress_manager {
            if let Some(identity) = claim.agent_session_id.as_deref() {
                manager.delete(identity).await?;
            }
        }
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .remove(sandbox_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// An Operator that predates `tenantKey` and `timeoutMs` rejects unknown fields, so a caller
    /// that sets neither must send neither: the body is what an old Operator accepts.
    #[test]
    fn a_claim_omits_the_fields_the_caller_did_not_set() {
        let body = serde_json::to_value(ClaimRequest {
            sandbox_id: "sbx",
            session_id: "s1",
            tenant_key: None,
            timeout_ms: None,
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"sandboxId": "sbx", "sessionId": "s1"})
        );
        let body = serde_json::to_value(ClaimRequest {
            sandbox_id: "sbx",
            session_id: "s1",
            tenant_key: Some("t"),
            timeout_ms: Some(5),
        })
        .unwrap();
        assert_eq!(
            body["tenantKey"], "t",
            "a tenant the caller set is never dropped"
        );
        assert_eq!(body["timeoutMs"], 5);
    }

    #[tokio::test]
    async fn the_token_falls_back_to_the_default_mount_when_the_projected_file_is_missing() {
        let error = read_identity_token("/nonexistent/alien/token", "sbx")
            .await
            .expect_err("neither file exists in the test environment");
        let message = format!("{error:?}");
        assert!(message.contains(DEFAULT_TOKEN_PATH), "{message}");
    }

    use super::*;

    #[test]
    fn a_refused_token_names_the_audience_and_the_accepted_tokens() {
        let message = broker_refusal(reqwest::StatusCode::UNAUTHORIZED, "");
        assert!(message.contains("alien-sandbox-broker"), "{message}");
        assert!(
            message.contains("default ServiceAccount token"),
            "{message}"
        );
        assert_eq!(
            broker_refusal(reqwest::StatusCode::NOT_FOUND, "no such pod"),
            "the sandbox broker returned 404 Not Found: no such pod"
        );
    }
    use alien_core::bindings::BindingValue;
    use axum::extract::{Path, Request};
    use axum::http::StatusCode;
    use axum::routing::{delete, post};
    use axum::{Json, Router};
    use std::net::SocketAddr;
    use std::sync::Arc;

    /// Copied verbatim from `broker_router` in `alien-infra`'s `sandbox::kubernetes_route`, so a
    /// client path the broker does not route falls through to the fallback below.
    const BROKER_RELEASE_ROUTE: &str = "/v1/sandbox/{sandbox}/sessions/{session}";

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        format!("http://{address}")
    }

    fn sandbox_over(broker: String, token: &tempfile::NamedTempFile) -> KubernetesSandbox {
        std::fs::write(token.path(), "service-account-token").expect("the token is written");
        KubernetesSandbox::new(
            "sbx",
            &KubernetesSandboxBinding {
                default_policy: None,
                egress: None,
                namespace: BindingValue::Value("alien".to_string()),
                runtime_class: BindingValue::Value("gvisor".to_string()),
                selector: BindingValue::Value("alien/sandbox=sbx-pool".to_string()),
                broker_url: BindingValue::Value(broker),
                key_name: BindingValue::Value("sandbox-key".to_string()),
                token_path: BindingValue::Value(token.path().display().to_string()),
            },
            "sbx-pool",
        )
        .expect("the binding is complete")
    }

    /// A pod is never paused, so resuming a running sandbox succeeds without changing anything
    /// and resuming one the broker does not know is refused.
    #[tokio::test]
    async fn resume_is_a_no_op_for_a_running_sandbox_and_refused_for_an_unknown_one() {
        let broker = serve(Router::new().route(
            BROKER_RELEASE_ROUTE,
            axum::routing::get(|Path((_, session)): Path<(String, String)>| async move {
                if session == "running" {
                    Ok(Json(serde_json::json!({
                        "sessionId": "running",
                        "endpoint": "http://10.0.0.1:8080",
                        "capability": "cap",
                        "expiresAt": chrono::Utc::now().timestamp() + 300,
                    })))
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            }),
        ))
        .await;
        let token = tempfile::NamedTempFile::new().expect("a token file");
        let sandbox = sandbox_over(broker, &token);

        sandbox.resume("running").await.expect("a no-op");
        let error = sandbox.resume("gone").await.expect_err("nothing to resume");
        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    }

    /// Terminate releases the claim at the route the broker serves, with the resource and the
    /// session in the order it serves them.
    ///
    /// The broker is the only thing that can delete the pod, so a DELETE to a path it does not
    /// route leaves the sandbox running and the claim held while the caller is told it is gone.
    /// Mutation check: change the path in `terminate` and the fallback records the request.
    #[tokio::test]
    async fn terminate_releases_at_the_route_the_broker_serves() {
        let released = Arc::new(Mutex::new(Vec::new()));
        let unrouted = Arc::new(Mutex::new(Vec::new()));
        let released_seen = Arc::clone(&released);
        let unrouted_seen = Arc::clone(&unrouted);

        let broker = serve(
            Router::new()
                .route(
                    "/v1/sandbox/sessions",
                    post(|| async {
                        Json(serde_json::json!({
                            "sessionId": "s1",
                            "endpoint": "http://10.0.0.1:8080",
                            "capability": "cap",
                            "expiresAt": chrono::Utc::now().timestamp() + 300,
                        }))
                    }),
                )
                .route(
                    BROKER_RELEASE_ROUTE,
                    // Once released, the broker finds no pod carrying the session.
                    axum::routing::get(|| async { StatusCode::NOT_FOUND }).delete(
                        move |Path(addressed): Path<(String, String)>| {
                            let released = Arc::clone(&released);
                            async move {
                                released
                                    .lock()
                                    .expect("no panic holds this lock")
                                    .push(addressed);
                                StatusCode::NO_CONTENT
                            }
                        },
                    ),
                )
                .fallback(move |request: Request| {
                    let unrouted = Arc::clone(&unrouted);
                    async move {
                        unrouted
                            .lock()
                            .expect("no panic holds this lock")
                            .push(format!("{} {}", request.method(), request.uri().path()));
                        StatusCode::NOT_FOUND
                    }
                }),
        )
        .await;

        let token = tempfile::NamedTempFile::new().expect("a token file");
        let sandbox = sandbox_over(broker, &token);

        let claimed = sandbox
            .create(CreateSandboxRequest {
                sandbox_id: Some("s1".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
                ..Default::default()
            })
            .await
            .expect("the broker claims a pod");

        let released_claim = sandbox.terminate(&claimed.sandbox_id).await;

        assert!(
            unrouted_seen
                .lock()
                .expect("no panic holds this lock")
                .is_empty(),
            "terminate reached a path the broker does not route: {:?}",
            unrouted_seen.lock().expect("no panic holds this lock")
        );
        released_claim.expect("the broker releases the claim");
        assert_eq!(
            *released_seen.lock().expect("no panic holds this lock"),
            vec![("sbx-pool".to_string(), "s1".to_string())],
            "the release names the resource and then the session"
        );
        assert!(
            sandbox
                .get(&claimed.sandbox_id)
                .await
                .expect("a released sandbox reads back")
                .is_none(),
            "a released sandbox is no longer claimed"
        );
    }

    /// A second process has no claim of its own and still reaches the session by id, because the
    /// broker reads it off the pod. An id no pod carries is a clean not-found, and a capability
    /// near its end is replaced before a request is built with it.
    #[tokio::test]
    async fn another_process_reattaches_by_id_and_capabilities_are_refreshed() {
        use axum::routing::get;
        let lookups = Arc::new(Mutex::new(0_u32));
        let counted = Arc::clone(&lookups);
        let broker = serve(Router::new().route(
            BROKER_RELEASE_ROUTE,
            get(move |Path((_, session)): Path<(String, String)>| {
                let counted = Arc::clone(&counted);
                async move {
                    *counted.lock().expect("no panic holds this lock") += 1;
                    if session == "known" {
                        Ok(Json(serde_json::json!({
                            "agentSessionId": "uid-1",
                            "sessionId": "known",
                            "endpoint": "http://10.0.0.1:8080",
                            "capability": "fresh",
                            "expiresAt": chrono::Utc::now().timestamp() + 900,
                        })))
                    } else {
                        Err(StatusCode::NOT_FOUND)
                    }
                }
            }),
        ))
        .await;
        let token = tempfile::NamedTempFile::new().expect("a token file");
        let sandbox = sandbox_over(broker, &token);

        let found = sandbox
            .get("known")
            .await
            .expect("lookup works")
            .expect("a process that never claimed it still finds it");
        assert_eq!(found.sandbox_id, "known");
        assert!(sandbox
            .get("probe-id")
            .await
            .expect("an unknown id is not an error")
            .is_none());

        // Every `get` asks the broker; the capability it returns is remembered for later calls.
        sandbox.get("known").await.unwrap().unwrap();
        assert_eq!(*lookups.lock().unwrap(), 3, "one lookup per call");
    }

    fn session_body(session: &str, pending: bool, deadline: Option<i64>) -> serde_json::Value {
        let mut body = serde_json::json!({
            "agentSessionId": "uid-1", "sessionId": session,
            "endpoint": "http://10.0.0.1:8080", "capability": "c",
            "expiresAt": chrono::Utc::now().timestamp() + 900, "created": false,
        });
        if pending {
            body["setupPending"] = true.into();
        }
        if let Some(deadline) = deadline {
            body["deadline"] = deadline.into();
        }
        body
    }

    /// A session another process is still setting up is not handed out until that process says
    /// it is done; if the creator ends the session because its setup failed, the caller gets an
    /// error and not a session without its egress.
    #[tokio::test]
    async fn a_session_still_being_set_up_by_its_creator_is_waited_for() {
        use axum::routing::get;
        let lookups = Arc::new(Mutex::new(0));
        let seen = Arc::clone(&lookups);
        let broker = serve(Router::new().route(
            BROKER_RELEASE_ROUTE,
            get(move |Path((_, session)): Path<(String, String)>| {
                let seen = Arc::clone(&seen);
                async move {
                    let mut lookups = seen.lock().unwrap();
                    *lookups += 1;
                    match (session.as_str(), *lookups) {
                        // Pending for the first two lookups, then the creator reports.
                        ("ready-later", 1 | 2) => Ok(Json(session_body(&session, true, None))),
                        ("ready-later", _) => Ok(Json(session_body(&session, false, None))),
                        // The creator's setup failed and it ended the session.
                        (_, 1) => Ok(Json(session_body(&session, true, None))),
                        _ => Err(StatusCode::NOT_FOUND),
                    }
                }
            }),
        ))
        .await;
        let token = tempfile::NamedTempFile::new().unwrap();
        let sandbox = sandbox_over(broker, &token);

        let found = sandbox
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some("ready-later".to_string()),
                ..Default::default()
            })
            .await
            .expect("found once the creator reports");
        assert!(!found.created);
        assert_eq!(
            *lookups.lock().unwrap(),
            3,
            "it waited for two pending answers"
        );

        *lookups.lock().unwrap() = 0;
        let error = sandbox
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some("failed".to_string()),
                ..Default::default()
            })
            .await
            .expect_err("the creator's setup failed");
        assert!(error.to_string().contains("ended while"), "{error}");
    }

    /// The broker, not the cache, says whether a session still exists: the Operator may have
    /// ended it at its deadline while the capability this process holds is still unexpired.
    #[tokio::test]
    async fn get_or_create_asks_the_broker_even_when_the_cached_capability_is_unexpired() {
        use axum::routing::get;
        let gone = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&gone);
        let broker = serve(
            Router::new()
                .route(
                    BROKER_RELEASE_ROUTE,
                    get(move |Path((_, session)): Path<(String, String)>| {
                        let flag = Arc::clone(&flag);
                        async move {
                            if *flag.lock().unwrap() {
                                Err(StatusCode::NOT_FOUND)
                            } else {
                                Ok(Json(session_body(&session, false, None)))
                            }
                        }
                    }),
                )
                .route(
                    "/v1/sandbox/sessions",
                    axum::routing::post(|| async {
                        let mut body = session_body("s1", false, None);
                        body["created"] = true.into();
                        Json(body)
                    }),
                ),
        )
        .await;
        let token = tempfile::NamedTempFile::new().unwrap();
        let sandbox = sandbox_over(broker, &token);
        let request = || CreateSandboxRequest {
            sandbox_id: Some("s1".to_string()),
            ..Default::default()
        };

        assert!(!sandbox.get_or_create(request()).await.unwrap().created);
        *gone.lock().unwrap() = true;
        assert!(
            sandbox.get_or_create(request()).await.unwrap().created,
            "the session ended; a replacement is created"
        );

        // `get` does not trust a cached session either: it was terminated by another process.
        sandbox.remember(serde_json::from_value(session_body("s1", false, None)).unwrap());
        assert!(sandbox.get("s1").await.unwrap().is_none());
    }

    /// `get` waits for a creator that is still setting the session up, as `get_or_create` does.
    #[tokio::test]
    async fn get_waits_for_a_session_that_is_still_being_set_up() {
        use axum::routing::get;
        let lookups = Arc::new(Mutex::new(0));
        let seen = Arc::clone(&lookups);
        let broker = serve(Router::new().route(
            BROKER_RELEASE_ROUTE,
            get(move |Path((_, session)): Path<(String, String)>| {
                let seen = Arc::clone(&seen);
                async move {
                    let mut lookups = seen.lock().unwrap();
                    *lookups += 1;
                    Json(session_body(&session, *lookups <= 2, None))
                }
            }),
        ))
        .await;
        let token = tempfile::NamedTempFile::new().unwrap();
        let sandbox = sandbox_over(broker, &token);
        sandbox.get("s1").await.unwrap().expect("found after setup");
        assert_eq!(*lookups.lock().unwrap(), 3);
    }

    /// A new SDK with an Operator that predates lookups by id: the lookup answers 405, and the
    /// create goes on as it always did. A caller that set a tenant key is refused, because that
    /// Operator cannot check it.
    #[tokio::test]
    async fn get_or_create_falls_back_to_create_on_an_operator_without_lookups() {
        use axum::routing::post;
        let posts = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&posts);
        let broker = serve(
            Router::new()
                .route(
                    BROKER_RELEASE_ROUTE,
                    // The only method an old Operator routes here.
                    axum::routing::delete(|| async { StatusCode::NO_CONTENT }),
                )
                .route(
                    "/v1/sandbox/sessions",
                    post(move |Json(body): Json<serde_json::Value>| {
                        let seen = Arc::clone(&seen);
                        async move {
                            seen.lock().unwrap().push(body.clone());
                            let mut reply =
                                session_body(body["sessionId"].as_str().unwrap(), false, None);
                            reply["created"] = true.into();
                            Json(reply)
                        }
                    }),
                ),
        )
        .await;
        let token = tempfile::NamedTempFile::new().unwrap();
        let sandbox = sandbox_over(broker, &token);

        let created = sandbox
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some("s1".to_string()),
                ..Default::default()
            })
            .await
            .expect("falls back to the old create");
        assert!(created.created);
        assert_eq!(posts.lock().unwrap().len(), 1);

        let refused = sandbox
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some("s2".to_string()),
                tenant_key: Some("tenant".to_string()),
                ..Default::default()
            })
            .await
            .expect_err("a tenant key cannot be honoured");
        assert!(refused.to_string().contains("tenantKey"), "{refused}");
        assert_eq!(posts.lock().unwrap().len(), 1, "nothing was created for it");
    }

    /// `get_or_create` from a new process per command: the known id is found (`created` false,
    /// generation from the pod), a new id is created, and a create that lost a race to another
    /// process is reported as found.
    #[tokio::test]
    async fn get_or_create_reports_whether_this_call_created_the_session() {
        use axum::routing::{get, post};
        let expires = chrono::Utc::now().timestamp() + 900;
        let broker = serve(
            Router::new()
                .route(
                    BROKER_RELEASE_ROUTE,
                    get(
                        move |Path((_, session)): Path<(String, String)>| async move {
                            if session == "known" {
                                Ok(Json(serde_json::json!({
                                    "agentSessionId": "uid-1", "sessionId": "known",
                                    "endpoint": "http://10.0.0.1:8080", "capability": "c",
                                    "expiresAt": expires, "generation": 3, "created": false,
                                })))
                            } else {
                                Err(StatusCode::NOT_FOUND)
                            }
                        },
                    ),
                )
                .route(
                    "/v1/sandbox/sessions",
                    post(move |Json(body): Json<serde_json::Value>| async move {
                        let id = body["sessionId"].as_str().unwrap().to_string();
                        // "raced" was created by another process while this one was looking.
                        let created = id != "raced";
                        Json(serde_json::json!({
                            "agentSessionId": "uid-2", "sessionId": id,
                            "endpoint": "http://10.0.0.2:8080", "capability": "c",
                            "expiresAt": expires, "generation": 1, "created": created,
                        }))
                    }),
                ),
        )
        .await;
        let token = tempfile::NamedTempFile::new().expect("a token file");
        std::fs::write(token.path(), "service-account-token").unwrap();
        let sandbox = sandbox_over(broker, &token);
        let request = |id: &str| CreateSandboxRequest {
            sandbox_id: Some(id.to_string()),
            ..Default::default()
        };

        let known = sandbox
            .get_or_create(request("known"))
            .await
            .expect("found");
        assert!(!known.created);
        assert_eq!(
            known.sandbox.generation, 3,
            "the pod's generation, not a constant"
        );

        let fresh = sandbox
            .get_or_create(request("fresh"))
            .await
            .expect("created");
        assert!(fresh.created);

        let raced = sandbox
            .get_or_create(request("raced"))
            .await
            .expect("found");
        assert!(!raced.created, "losing the create race is a find");
    }

    /// An id names one session. A request carrying another tenant key (or none for a keyed
    /// session, or one for an unkeyed session) is refused on every path that returns an existing
    /// session: the lookup, and the answer to a create that lost a race.
    #[tokio::test]
    async fn get_or_create_refuses_an_existing_session_with_another_tenant_key() {
        use axum::routing::{get, post};
        let expires = chrono::Utc::now().timestamp() + 900;
        let session = move |id: &str, key: Option<&str>, created: bool| {
            let mut body = serde_json::json!({
                "agentSessionId": "uid-1", "sessionId": id,
                "endpoint": "http://10.0.0.1:8080", "capability": "c",
                "expiresAt": expires, "generation": 1, "created": created,
            });
            if let Some(key) = key {
                body["tenantKey"] = serde_json::json!(key);
            }
            body
        };
        let broker = serve(
            Router::new()
                .route(
                    BROKER_RELEASE_ROUTE,
                    get(move |Path((_, id)): Path<(String, String)>| async move {
                        match id.as_str() {
                            "keyed" => Ok(Json(session("keyed", Some("a"), false))),
                            "open" => Ok(Json(session("open", None, false))),
                            _ => Err(StatusCode::NOT_FOUND),
                        }
                    }),
                )
                .route(
                    "/v1/sandbox/sessions",
                    // "raced" was created for tenant "a" by another process during this call.
                    post(move |Json(body): Json<serde_json::Value>| async move {
                        let id = body["sessionId"].as_str().unwrap().to_string();
                        Json(session(&id, Some("a"), false))
                    }),
                ),
        )
        .await;
        let token = tempfile::NamedTempFile::new().expect("a token file");
        std::fs::write(token.path(), "service-account-token").unwrap();
        let sandbox = sandbox_over(broker, &token);
        let request = |id: &str, tenant: Option<&str>| CreateSandboxRequest {
            sandbox_id: Some(id.to_string()),
            tenant_key: tenant.map(str::to_string),
            ..Default::default()
        };

        for (id, tenant) in [
            ("keyed", Some("b")),
            ("keyed", None),
            ("open", Some("b")),
            ("raced", Some("b")),
            ("raced", None),
        ] {
            let error = sandbox
                .get_or_create(request(id, tenant))
                .await
                .expect_err(&format!("{id} with {tenant:?} must be refused"));
            assert!(
                error.to_string().contains("different tenant key"),
                "{error}"
            );
        }
        assert!(sandbox
            .get_or_create(request("keyed", Some("a")))
            .await
            .is_ok());
        assert!(sandbox.get_or_create(request("open", None)).await.is_ok());
        assert!(sandbox
            .get_or_create(request("raced", Some("a")))
            .await
            .is_ok());
    }

    /// A command's input is many short requests over hours, so the agent handle replaces its
    /// capability before it expires instead of letting a request reach the pod with a dead one.
    #[tokio::test]
    async fn an_interactive_command_refreshes_its_capability_before_it_expires() {
        use super::super::interactive::BoundAgent;
        let broker = serve(Router::new().route(
            BROKER_RELEASE_ROUTE,
            axum::routing::get(|| async {
                Json(serde_json::json!({
                    "agentSessionId": "uid-1",
                    "sessionId": "s1",
                    "endpoint": "http://10.0.0.9:8080",
                    "capability": "second",
                    "expiresAt": chrono::Utc::now().timestamp() + 900,
                }))
            }),
        ))
        .await;
        let token = tempfile::NamedTempFile::new().expect("a token file");
        std::fs::write(token.path(), "service-account-token").unwrap();
        let first = ClaimResponse {
            agent_session_id: None,
            session_id: "s1".to_string(),
            endpoint: "http://10.0.0.1:8080".to_string(),
            capability: "first".to_string(),
            expires_at: chrono::Utc::now().timestamp() + 20,
            generation: 1,
            created: false,
            tenant_key: None,
            deadline: None,
            setup_pending: false,
        };
        let agent = SessionAgent {
            client: reqwest::Client::new(),
            session_url: format!("{broker}/v1/sandbox/sbx-pool/sessions/s1"),
            token_path: token.path().display().to_string(),
            binding_name: "sbx".to_string(),
            sandbox_id: "s1".to_string(),
            claim: tokio::sync::Mutex::new(first),
        };

        let request = agent
            .request(reqwest::Method::POST, "/v1/interactive/input")
            .await
            .expect("a request is built")
            .build()
            .unwrap();
        assert_eq!(
            request.url().as_str(),
            "http://10.0.0.9:8080/v1/interactive/input"
        );
        assert_eq!(
            request.headers()["authorization"].to_str().unwrap(),
            "Bearer second"
        );
    }
}
