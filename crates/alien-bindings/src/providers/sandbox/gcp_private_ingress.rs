//! Reaching sandboxes whose template turned on private ingress.
//!
//! A template that names a network attachment (the gateway's egress path) must also enable Private
//! Service Connect ingress, and a sandbox of such a template has no public host: `:execute` answers
//! 400 for it (measured). The only way in is the deployment's PSC endpoint, an internal address
//! forwarding to the tenant's sandbox router, with the same edge credential the preview path signs
//! and the sandbox's routing token. This wraps the Agent Platform client so `execute` takes that
//! path and nothing else changes: the provider's call sites keep calling `execute`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alien_error::AlienError;
use alien_gcp_clients::gcp::agent_platform::{
    AgentPlatformApi, AgentPlatformErrorData, ReasoningEngine, SandboxCreateRequest,
    SandboxEnvironment, SandboxEnvironmentTemplate,
};
use alien_gcp_clients::gcp::iam::IamPolicy;
use alien_gcp_clients::gcp::longrunning::Operation;
use async_trait::async_trait;

use super::gcp_agent_platform::EdgeTokenSigner;

type Result<T> = std::result::Result<T, AlienError<AgentPlatformErrorData>>;

/// A routing token is good for hours; refreshed this often so a replaced one is picked up.
const ROUTING_TOKEN_REUSE: Duration = Duration::from_secs(300);
/// An edge credential with less than this left is minted again.
const EDGE_REFRESH_MARGIN_SECONDS: i64 = 60;
/// The edge forwards to the declared port; a request is cut off after this long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(150);

#[derive(Debug)]
pub struct PrivateIngressClient {
    inner: Arc<dyn AgentPlatformApi>,
    /// `address:port` of the deployment's PSC endpoint.
    endpoint: String,
    agent_port: u16,
    signer: Arc<dyn EdgeTokenSigner>,
    http: reqwest::Client,
    routing: Mutex<HashMap<String, (String, Instant)>>,
    edge: Mutex<Option<(String, i64)>>,
}

impl PrivateIngressClient {
    pub fn new(
        inner: Arc<dyn AgentPlatformApi>,
        endpoint: impl Into<String>,
        agent_port: u16,
        signer: Arc<dyn EdgeTokenSigner>,
    ) -> Self {
        Self {
            inner,
            endpoint: endpoint.into(),
            agent_port,
            signer,
            http: reqwest::Client::new(),
            routing: Mutex::new(HashMap::new()),
            edge: Mutex::new(None),
        }
    }

    async fn routing_token(&self, engine: &str, sandbox: &str) -> Result<String> {
        if let Some((token, at)) = self.routing.lock().expect("lock").get(sandbox) {
            if at.elapsed() < ROUTING_TOKEN_REUSE {
                return Ok(token.clone());
            }
        }
        let found = self.inner.get_sandbox(engine, sandbox).await?;
        let token = found
            .connection_info
            .and_then(|info| info.routing_token)
            .ok_or_else(|| {
                AlienError::new(AgentPlatformErrorData::ExecuteFailed {
                    sandbox: sandbox.to_string(),
                    message: "the sandbox has no routing token yet".to_string(),
                })
            })?;
        self.routing
            .lock()
            .expect("lock")
            .insert(sandbox.to_string(), (token.clone(), Instant::now()));
        Ok(token)
    }

    async fn edge_jwt(&self, sandbox: &str) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        if let Some((jwt, expires_at)) = self.edge.lock().expect("lock").as_ref() {
            if *expires_at - now > EDGE_REFRESH_MARGIN_SECONDS {
                return Ok(jwt.clone());
            }
        }
        let signed = self.signer.sign().await.map_err(|_| {
            AlienError::new(AgentPlatformErrorData::ExecuteFailed {
                sandbox: sandbox.to_string(),
                message: "could not sign the sandbox edge credential".to_string(),
            })
        })?;
        *self.edge.lock().expect("lock") = Some((signed.jwt.clone(), signed.expires_at));
        Ok(signed.jwt)
    }
}

#[async_trait]
impl AgentPlatformApi for PrivateIngressClient {
    async fn execute(&self, engine: &str, sandbox: &str, input: &[u8]) -> Result<Vec<u8>> {
        let failed = |message: String| {
            AlienError::new(AgentPlatformErrorData::ExecuteFailed {
                sandbox: sandbox.to_string(),
                message,
            })
        };
        let routing = self.routing_token(engine, sandbox).await?;
        let jwt = self.edge_jwt(sandbox).await?;
        let response = self
            .http
            .post(format!("http://{}/", self.endpoint))
            .bearer_auth(jwt)
            .header("X-Sandbox-Routing-Token", routing)
            .header("X-Sandbox-Port", self.agent_port.to_string())
            .header("content-type", "application/json")
            .timeout(REQUEST_TIMEOUT)
            .body(input.to_vec())
            .send()
            .await
            .map_err(|error| failed(format!("the private endpoint did not answer: {error}")))?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| {
            failed(format!("the private endpoint cut the reply short: {error}"))
        })?;
        if !status.is_success() {
            // The request body never appears here; the reply is the agent's or the edge's own.
            let reply = String::from_utf8_lossy(&body[..body.len().min(300)]).to_string();
            return Err(failed(format!(
                "the private endpoint answered {status}: Error Details: {reply}"
            )));
        }
        Ok(body.to_vec())
    }

    async fn create_engine(
        &self,
        display_name: &str,
        description: Option<String>,
    ) -> Result<Operation> {
        self.inner.create_engine(display_name, description).await
    }
    async fn delete_engine(&self, engine: &str) -> Result<()> {
        self.inner.delete_engine(engine).await
    }
    async fn get_engine_iam_policy(&self, engine: &str) -> Result<IamPolicy> {
        self.inner.get_engine_iam_policy(engine).await
    }
    async fn set_engine_iam_policy(&self, engine: &str, policy: IamPolicy) -> Result<IamPolicy> {
        self.inner.set_engine_iam_policy(engine, policy).await
    }
    async fn list_engines(&self) -> Result<Vec<ReasoningEngine>> {
        self.inner.list_engines().await
    }
    async fn create_template(
        &self,
        engine: &str,
        template: SandboxEnvironmentTemplate,
    ) -> Result<Operation> {
        self.inner.create_template(engine, template).await
    }
    async fn get_template(
        &self,
        engine: &str,
        template: &str,
    ) -> Result<SandboxEnvironmentTemplate> {
        self.inner.get_template(engine, template).await
    }
    async fn list_templates(&self, engine: &str) -> Result<Vec<SandboxEnvironmentTemplate>> {
        self.inner.list_templates(engine).await
    }
    async fn delete_template(&self, engine: &str, template: &str) -> Result<()> {
        self.inner.delete_template(engine, template).await
    }
    async fn create_sandbox(
        &self,
        engine: &str,
        request: SandboxCreateRequest,
    ) -> Result<Operation> {
        self.inner.create_sandbox(engine, request).await
    }
    async fn get_sandbox(&self, engine: &str, sandbox: &str) -> Result<SandboxEnvironment> {
        self.inner.get_sandbox(engine, sandbox).await
    }
    async fn list_sandboxes(&self, engine: &str) -> Result<Vec<SandboxEnvironment>> {
        self.inner.list_sandboxes(engine).await
    }
    async fn list_sandboxes_by_display_names(
        &self,
        engine: &str,
        display_names: &[String],
    ) -> Result<Vec<SandboxEnvironment>> {
        self.inner
            .list_sandboxes_by_display_names(engine, display_names)
            .await
    }
    async fn delete_sandbox(&self, engine: &str, sandbox: &str) -> Result<()> {
        self.inner.delete_sandbox(engine, sandbox).await
    }
    async fn pause_sandbox(&self, engine: &str, sandbox: &str) -> Result<Operation> {
        self.inner.pause_sandbox(engine, sandbox).await
    }
    async fn resume_sandbox(&self, engine: &str, sandbox: &str) -> Result<Operation> {
        self.inner.resume_sandbox(engine, sandbox).await
    }
    async fn snapshot(&self, engine: &str, sandbox: &str, display_name: &str) -> Result<Operation> {
        self.inner.snapshot(engine, sandbox, display_name).await
    }
    async fn get_operation(&self, name: &str) -> Result<Operation> {
        self.inner.get_operation(name).await
    }
}
