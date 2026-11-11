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
}

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

/// Asks the broker for a session and a fresh capability. `None` when no pod carries the id.
async fn fetch_claim(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<Option<ClaimResponse>> {
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
        return Ok(None);
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
        .map(Some)
}

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
/// A 401 is the broker refusing the token itself. The broker accepts only a token projected for
/// its own audience, which a workload that links the sandbox has mounted; a pod started before the
/// sandbox was linked, or before an upgrade, has none until it is rolled, so the message says what
/// to look for and that retrying after the roll is the fix.
fn broker_refusal(status: reqwest::StatusCode, body: &str) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return format!(
            "the sandbox broker refused this workload's token ({status}). The broker accepts \
             only a ServiceAccount token projected for the audience `{}`, mounted at the \
             sandbox binding's token path in every workload that links the sandbox. A workload \
             started before that link or before the last upgrade has none until it is \
             restarted; try again once it has been rolled",
            "alien-sandbox-broker"
        );
    }
    format!("the sandbox broker returned {status}: {body}")
}

/// Reads the pod's ServiceAccount token. Per call: Kubernetes rotates projected tokens in place.
async fn read_identity_token(token_path: &str, binding_name: &str) -> Result<String> {
    tokio::fs::read_to_string(token_path)
        .await
        .into_alien_error()
        .context(ErrorData::BindingConfigInvalid {
            binding_name: binding_name.to_string(),
            env_var: alien_core::bindings::binding_env_var_name(binding_name),
            reason: format!("could not read the ServiceAccount token at '{token_path}'"),
        })
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
    /// and from the broker otherwise. `None` when no pod carries the id, which is a clean
    /// not-found rather than an error.
    async fn claimed(&self, sandbox_id: &str) -> Result<Option<ClaimResponse>> {
        if let Some(claim) = self.cached(sandbox_id) {
            if claim.expires_at - chrono::Utc::now().timestamp() > CAPABILITY_REFRESH_MARGIN_SECONDS
            {
                return Ok(Some(claim));
            }
        }

        let token = self.identity_token().await?;
        let Some(claim) = fetch_claim(&self.client, &self.session_url(sandbox_id), &token).await?
        else {
            self.claims
                .lock()
                .expect("no panic holds this lock")
                .remove(sandbox_id);
            return Ok(None);
        };
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .insert(claim.session_id.clone(), claim.clone());
        Ok(Some(claim))
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

        let response = self
            .client
            .post(format!("{}/v1/sandbox/sessions", self.broker_url))
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

        self.claims
            .lock()
            .expect("no panic holds this lock")
            .insert(claim.session_id.clone(), claim.clone());

        // Another process created this id between the caller's lookup and this claim; the broker
        // answered with its session, which already carries its own policy.
        if !claim.created {
            return Ok(ResolvedSandbox::found(SandboxInstance {
                sandbox_id: claim.session_id,
                state: SandboxState::Running,
                generation: claim.generation,
            }));
        }

        if let Some(manager) = &self.egress_manager {
            let installation = async {
                let identity = Self::egress_identity(&claim)?;
                let delivery = manager.create(identity, &policy, claim.expires_at).await?;
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

        Ok(ResolvedSandbox::created(SandboxInstance {
            sandbox_id: claim.session_id,
            state: SandboxState::Running,
            generation: claim.generation,
        }))
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
    /// An id no pod carries is `None`.
    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        Ok(self
            .claimed(sandbox_id)
            .await?
            .map(|claim| SandboxInstance {
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
            if let Some(existing) = self.get(id).await? {
                if let Some(policy) = request.egress.clone() {
                    self.update_egress(id, policy).await?;
                }
                return Ok(ResolvedSandbox::found(existing));
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
    use super::*;

    #[test]
    fn a_refused_token_names_the_audience_and_says_to_retry_after_the_roll() {
        let message = broker_refusal(reqwest::StatusCode::UNAUTHORIZED, "");
        assert!(message.contains("alien-sandbox-broker"), "{message}");
        assert!(message.contains("try again"), "{message}");
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

        // A second call is served from the capability this process now holds.
        sandbox.get("known").await.unwrap().unwrap();
        assert_eq!(*lookups.lock().unwrap(), 2, "one lookup per id");

        // Close to expiry, the next use asks the broker again.
        sandbox
            .claims
            .lock()
            .unwrap()
            .get_mut("known")
            .unwrap()
            .expires_at = chrono::Utc::now().timestamp() + 10;
        sandbox.get("known").await.unwrap().unwrap();
        assert_eq!(*lookups.lock().unwrap(), 3, "refreshed before it expired");
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
