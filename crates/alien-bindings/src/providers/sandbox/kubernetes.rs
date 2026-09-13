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
use std::sync::Mutex;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::agent_protocol::{self, AgentTransport};
use crate::traits::{
    Binding, CommandOutput, CreateSandboxRequest, JobPoll, JobStart, PreviewCapability,
    ResolvedSandbox, RunCommandRequest, Sandbox, SandboxInstance, SandboxState,
};
use alien_core::bindings::KubernetesSandboxBinding;
use alien_core::{Platform, SandboxCapabilities, SandboxCapability};
use alien_error::{AlienError, Context, IntoAlienError};

/// What the broker hands back for a claimed sandbox.
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
    resource_id: String,
    broker_url: String,
    token_path: String,
    binding_name: String,
    client: reqwest::Client,
    /// Claims this process has made, so a later call can address the sandbox it already has.
    ///
    /// The capability is short-lived and the endpoint is a pod IP, so this is a cache of live
    /// sandboxes rather than durable state. A sandbox this process did not claim is not
    /// reachable, which is what `reconnect` means here.
    claims: Mutex<BTreeMap<String, ClaimResponse>>,
}

impl KubernetesSandbox {
    /// Builds a provider from its binding.
    pub fn new(
        binding_name: &str,
        binding: &KubernetesSandboxBinding,
        resource_id: &str,
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

    /// Refused rather than dropped: a claim reaches a pod that is already running under the
    /// deadline the declaration fixed, so a lifetime accepted here would never be applied.
    fn refuse_create_time_lifetime(&self, request: &CreateSandboxRequest) -> Result<()> {
        if request.timeout_ms.is_none() {
            return Ok(());
        }
        Err(self.failed(
            SandboxCapability::SandboxLifetime.as_str(),
            "a pod is claimed from a warm pool already running under the deadline the sandbox \
             declared, so a claim cannot choose its own",
        ))
    }

    fn claimed(&self, sandbox_id: &str) -> Option<ClaimResponse> {
        self.claims
            .lock()
            .expect("no panic holds this lock")
            .get(sandbox_id)
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
        sandbox_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let claim = self.claimed(sandbox_id).ok_or_else(|| {
            self.failed(
                "sandbox.agent",
                &format!(
                    "sandbox '{sandbox_id}' was not claimed by this process; a pod IP and a \
                     capability are only reachable by the caller that claimed them"
                ),
            )
        })?;

        if claim.expires_at <= chrono::Utc::now().timestamp() {
            return Err(self.failed(
                "sandbox.agent",
                &format!(
                    "the capability for sandbox '{sandbox_id}' expired; the agent would refuse \
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

    /// Narrows the platform ceiling to this binding: the pool's pods carry
    /// `activeDeadlineSeconds` from the declaration and are already running when a claim reaches
    /// them, so nothing a create says can set a deadline. The declared ceiling still applies.
    fn capabilities(&self) -> SandboxCapabilities {
        let mut capabilities = SandboxCapabilities::for_platform(Platform::Kubernetes)
            .expect("Kubernetes has a sandbox backend");
        capabilities.sandbox_lifetime = false;
        capabilities
    }

    /// Claims a warm pod through the broker.
    async fn create(&self, request: CreateSandboxRequest) -> Result<SandboxInstance> {
        self.refuse_create_time_lifetime(&request)?;
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

        Ok(SandboxInstance {
            sandbox_id: claim.session_id,
            state: SandboxState::Running,
            // A released pod is deleted rather than fenced, so a sandbox never outlives its own
            // generation.
            generation: 1,
        })
    }

    /// Only sandboxes this process claimed are addressable.
    ///
    /// A capability is minted to the caller that claimed the pod, so another process holding the
    /// same sandbox id has nothing to reach it with. Returning `None` rather than erroring: the
    /// sandbox may well exist, this caller simply cannot address it.
    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        Ok(self.claimed(sandbox_id).map(|claim| SandboxInstance {
            sandbox_id: claim.session_id,
            state: SandboxState::Running,
            generation: 1,
        }))
    }

    async fn get_or_create(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        // Refused on the reconnect path too: a claimed pod honors a lifetime no more than a
        // freshly claimed one would.
        self.refuse_create_time_lifetime(&request)?;
        if let Some(id) = request.sandbox_id.as_deref() {
            if let Some(existing) = self.get(id).await? {
                return Ok(ResolvedSandbox::found(existing));
            }
        }

        self.create(request).await.map(ResolvedSandbox::created)
    }

    async fn list(&self) -> Result<Vec<SandboxInstance>> {
        Ok(self
            .claims
            .lock()
            .expect("no panic holds this lock")
            .values()
            .map(|claim| SandboxInstance {
                sandbox_id: claim.session_id.clone(),
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

    async fn preview(&self, _sandbox_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.failed(
            "preview",
            "preview needs a gateway that validates a sandbox-and-port capability, which this \
             backend has none of",
        ))
    }

    async fn pause(&self, _sandbox_id: &str) -> Result<()> {
        Err(self.failed("pauseResume", "a pod cannot be paused and resumed"))
    }

    async fn resume(&self, _sandbox_id: &str) -> Result<()> {
        Err(self.failed("pauseResume", "a pod cannot be paused and resumed"))
    }

    async fn snapshot(&self, _sandbox_id: &str) -> Result<String> {
        Err(self.failed("snapshot", "a pod has no snapshot primitive"))
    }

    /// Releases the sandbox, which deletes its pod.
    ///
    /// Idempotent: a sandbox this process never claimed is already in the desired end state.
    async fn terminate(&self, sandbox_id: &str) -> Result<()> {
        let Some(claim) = self.claimed(sandbox_id) else {
            return Ok(());
        };

        let response = self
            .client
            .delete(format!(
                "{}/v1/sandbox/{}/sessions/{}",
                self.broker_url, self.resource_id, claim.session_id
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
            .remove(sandbox_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    delete(move |Path(addressed): Path<(String, String)>| {
                        let released = Arc::clone(&released);
                        async move {
                            released
                                .lock()
                                .expect("no panic holds this lock")
                                .push(addressed);
                            StatusCode::NO_CONTENT
                        }
                    }),
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
}
