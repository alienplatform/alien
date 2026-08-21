//! Kubernetes sandbox provider: a pod under a sandboxed runtime class, reached over the agent
//! protocol.
//!
//! The application never holds a cluster credential. It asks the operator's broker for a
//! session, and gets back a pod address plus a capability scoped to that session. Claiming a pod
//! is a `PATCH` on pods, which does not belong in the binding: `pods/exec`
//! would reach every pod in the namespace.
//!
//! It authenticates to the broker with the ServiceAccount token Kubernetes already mounted in
//! its pod. Nothing of Alien's is created, rotated or torn down for this.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::agent_protocol::{self, AgentTransport};
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_core::bindings::KubernetesSandboxBinding;
use alien_core::{Platform, SandboxCapabilities};
use alien_error::{AlienError, Context, IntoAlienError};

/// What the broker hands back for a claimed session.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimResponse {
    session_id: String,
    endpoint: String,
    capability: String,
    expires_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRequest<'a> {
    sandbox_id: &'a str,
    session_id: &'a str,
}

/// A Sandbox backed by pods under a sandboxed runtime class.
#[derive(Debug)]
pub struct KubernetesSandbox {
    sandbox_id: String,
    broker_url: String,
    token_path: String,
    binding_name: String,
    client: reqwest::Client,
    /// Claims this process has made, so a later call can address the session it already has.
    ///
    /// The capability is short-lived and the endpoint is a pod IP, so this is a cache of live
    /// sessions rather than durable state. A session this process did not claim is not
    /// reachable, which is what `reconnect` means here.
    claims: Mutex<BTreeMap<String, ClaimResponse>>,
}

impl KubernetesSandbox {
    /// Builds a provider from its binding.
    pub fn new(
        binding_name: &str,
        binding: &KubernetesSandboxBinding,
        sandbox_id: &str,
    ) -> Result<Self> {
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
            sandbox_id: sandbox_id.to_string(),
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
        tokio::fs::read_to_string(&self.token_path)
            .await
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: self.binding_name.clone(),
                env_var: alien_core::bindings::binding_env_var_name(&self.binding_name),
                reason: format!(
                    "could not read the ServiceAccount token at '{}'",
                    self.token_path
                ),
            })
    }

    fn claimed(&self, session_id: &str) -> Option<ClaimResponse> {
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .get(session_id)
            .cloned()
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
        session_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let claim = self.claimed(session_id).ok_or_else(|| {
            self.failed(
                "sandbox.agent",
                &format!(
                    "session '{session_id}' was not claimed by this process; a pod IP and a \
                     capability are only reachable by the caller that claimed them"
                ),
            )
        })?;

        if claim.expires_at <= chrono::Utc::now().timestamp() {
            return Err(self.failed(
                "sandbox.agent",
                &format!(
                    "the capability for session '{session_id}' expired; the agent would refuse \
                     this with a 401 that reads like a broken sandbox"
                ),
            ));
        }

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
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Kubernetes)
            .expect("Kubernetes has a sandbox backend")
    }

    /// Claims a warm pod through the broker.
    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        let session_id = request
            .session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

        let response = self
            .client
            .post(format!("{}/v1/sandbox/sessions", self.broker_url))
            .bearer_auth(self.identity_token().await?)
            .json(&ClaimRequest {
                sandbox_id: &self.sandbox_id,
                session_id: &session_id,
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
            return Err(self.failed(
                "sandbox.create",
                &format!("the sandbox broker returned {status}: {body}"),
            ));
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

        Ok(SandboxSession {
            session_id: claim.session_id,
            state: SandboxSessionState::Running,
            // A released pod is deleted rather than fenced, so a session never outlives its own
            // generation.
            generation: 1,
        })
    }

    /// Only sessions this process claimed are addressable.
    ///
    /// A capability is minted to the caller that claimed the pod, so another process holding the
    /// same session id has nothing to reach it with. Returning `None` rather than erroring: the
    /// session may well exist, this caller simply cannot address it.
    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        Ok(self.claimed(session_id).map(|claim| SandboxSession {
            session_id: claim.session_id,
            state: SandboxSessionState::Running,
            generation: 1,
        }))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            if let Some(existing) = self.get(id).await? {
                return Ok(existing);
            }
        }

        self.create(request).await
    }

    async fn list(&self) -> Result<Vec<SandboxSession>> {
        Ok(self
            .claims
            .lock()
            .expect("no panic holds this lock")
            .values()
            .map(|claim| SandboxSession {
                session_id: claim.session_id.clone(),
                state: SandboxSessionState::Running,
                generation: 1,
            })
            .collect())
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        agent_protocol::run_command(self, session_id, request).await
    }

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        agent_protocol::read_file(self, session_id, path).await
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        agent_protocol::write_files(self, session_id, files).await
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        agent_protocol::mkdir(self, session_id, path).await
    }

    async fn preview(&self, _session_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.failed(
            "preview",
            "preview needs a gateway that validates a session-and-port capability, and that \
             gateway does not exist yet",
        ))
    }

    async fn suspend(&self, _session_id: &str) -> Result<()> {
        Err(self.failed("suspendResume", "a pod cannot be suspended and resumed"))
    }

    async fn resume(&self, _session_id: &str) -> Result<()> {
        Err(self.failed("suspendResume", "a pod cannot be suspended and resumed"))
    }

    async fn snapshot(&self, _session_id: &str) -> Result<String> {
        Err(self.failed("snapshot", "a pod has no snapshot primitive"))
    }

    /// Releases the session, which deletes its pod.
    ///
    /// Idempotent: a session this process never claimed is already in the desired end state.
    async fn terminate(&self, session_id: &str) -> Result<()> {
        let Some(claim) = self.claimed(session_id) else {
            return Ok(());
        };

        let response = self
            .client
            .delete(format!(
                "{}/v1/sandbox/{}/sessions/{}",
                self.broker_url, self.sandbox_id, claim.session_id
            ))
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
            return Err(self.failed(
                "sandbox.terminate",
                &format!("the sandbox broker returned {status}: {body}"),
            ));
        }

        self.claims
            .lock()
            .expect("no panic holds this lock")
            .remove(session_id);

        Ok(())
    }
}
