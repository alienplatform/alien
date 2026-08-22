//! Vertex AI Agent Platform sandbox client.
//!
//! Talks to the regional host `https://{region}-aiplatform.googleapis.com/v1`, under a parent
//! reasoning engine `projects/{p}/locations/{r}/reasoningEngines/{engine}`. The provider goes
//! through here rather than speaking REST directly, so retry classification, redaction and error
//! typing live in one place.
//!
//! Retry classification is the load-bearing part. `create_*`, `execute` and the `pause`/`resume`/
//! `snapshot` transitions are delivered **once** — a silent re-send mints an orphan the caller has
//! no id for, or repeats a transition the server already refuses for the state the first attempt
//! produced. `get_*`/`list_*` retry; `delete_*` retries and treats a not-found as done.

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::{Operation, OperationResult};
use crate::gcp::{GcpClientConfig, ServiceOverrides};
use alien_client_core::redact_request_body;
use alien_error::{AlienError, AlienErrorData, Context, IntoAlienError};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

/// Service-override key and endpoint base for the Vertex AI host. The regional host is injected as
/// an override at construction, so the static base is a fallback that a real call never reaches.
const SERVICE_KEY: &str = "aiplatform";
const JSON_MIME: &str = "application/json";

#[derive(Debug)]
struct AgentPlatformServiceConfig;

impl GcpServiceConfig for AgentPlatformServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://aiplatform.googleapis.com/v1"
    }
    fn default_audience(&self) -> &'static str {
        "https://aiplatform.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Vertex AI Agent Platform"
    }
    fn service_key(&self) -> &'static str {
        SERVICE_KEY
    }
}

// =================================================================================================
// Errors
// =================================================================================================

/// Problems specific to driving the Agent Platform sandbox API.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentPlatformErrorData {
    /// A create, read, list or delete call to the API failed; classification is inherited from the
    /// underlying cloud error so the caller's retry decision is preserved.
    #[error(
        code = "AGENT_PLATFORM_REQUEST_FAILED",
        message = "Agent Platform request '{operation}' failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    RequestFailed {
        /// The logical call that failed (e.g. "create sandbox")
        operation: String,
        /// Resource reference or short detail
        message: String,
    },

    /// A long-running operation completed with an error status.
    #[error(
        code = "AGENT_PLATFORM_OPERATION_FAILED",
        message = "Operation '{operation}' failed (grpc {grpc_code}): {message}",
        retryable = "false",
        internal = "false"
    )]
    OperationFailed {
        /// Operation resource name
        operation: String,
        /// gRPC status code the operation reported
        grpc_code: i32,
        /// Operation error message
        message: String,
    },

    /// A long-running operation never reported done within its polling budget; carries the operation
    /// name so the caller can resume or clean up rather than being handed a bare timeout.
    #[error(
        code = "AGENT_PLATFORM_OPERATION_INCOMPLETE",
        message = "Operation '{operation}' still running after {attempts} polls: {last_state}",
        retryable = "false",
        internal = "false"
    )]
    OperationIncomplete {
        /// Operation resource name
        operation: String,
        /// Number of polls spent before giving up
        attempts: u32,
        /// Last observed state or error text
        last_state: String,
    },

    /// A sandbox template never reached the `ACTIVE` state within its polling budget.
    #[error(
        code = "AGENT_PLATFORM_TEMPLATE_NOT_ACTIVE",
        message = "Template '{template}' never became ACTIVE (last state '{state}') after {attempts} polls",
        retryable = "false",
        internal = "false"
    )]
    TemplateNotActive {
        /// Template resource name
        template: String,
        /// Last observed lifecycle state
        state: String,
        /// Number of polls spent
        attempts: u32,
    },

    /// The proxied in-sandbox execution was refused or cut short before returning a result.
    #[error(
        code = "AGENT_PLATFORM_EXECUTE_FAILED",
        message = "Execution in sandbox '{sandbox}' was refused or cut short: {message}",
        retryable = "false",
        internal = "inherit"
    )]
    ExecuteFailed {
        /// Sandbox resource name
        sandbox: String,
        /// Short detail; the request body is never carried here
        message: String,
    },

    /// A sandbox execution returned a reply the client could not read.
    #[error(
        code = "AGENT_PLATFORM_EXECUTE_OUTPUT_INVALID",
        message = "Execution in sandbox '{sandbox}' returned an unreadable reply: {message}",
        retryable = "false",
        internal = "false"
    )]
    ExecuteOutputInvalid {
        /// Sandbox resource name
        sandbox: String,
        /// What was wrong with the reply
        message: String,
    },
}

/// Result type for this client.
pub type Result<T> = alien_error::Result<T, AgentPlatformErrorData>;

// =================================================================================================
// Polling
// =================================================================================================

/// Bound on how long the client waits for a long-running operation or a template to settle. The
/// caller owns the budget so a test can drive it to exhaustion in milliseconds and production can
/// give it minutes.
#[derive(Debug, Clone, Copy)]
pub struct PollBudget {
    /// Delay between polls
    pub interval: Duration,
    /// Maximum number of polls before giving up
    pub max_attempts: u32,
}

impl Default for PollBudget {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(2),
            max_attempts: 150,
        }
    }
}

// =================================================================================================
// Wire types
// =================================================================================================

/// A reasoning engine — the parent resource sandboxes and templates hang under.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEngine {
    /// Full resource name `projects/.../reasoningEngines/{id}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-readable display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The immutable image + resources a sandbox is cut from. `customContainerEnvironment` is the field
/// name the API wants — `sandboxEnvironmentSpec` is the obvious guess and it is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomContainerEnvironment {
    /// The container image to run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_container_spec: Option<CustomContainerSpec>,
    /// Requested and limit CPU/memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ContainerResources>,
    /// Ports the container exposes
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The container image reference for a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomContainerSpec {
    /// Fully-qualified image URI
    pub image_uri: String,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// CPU and memory requests/limits, each a `{cpu, memory}` map as the API returns them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerResources {
    /// Requested resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<HashMap<String, String>>,
    /// Resource limits
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<HashMap<String, String>>,
}

/// A container port declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    /// Port number
    pub port: i32,
    /// Protocol, e.g. `TCP`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Egress policy for a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressControlConfig {
    /// Whether the sandbox may reach the public internet
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internet_access: Option<bool>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A sandbox environment template. The config is immutable once created — there is no update verb.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironmentTemplate {
    /// Full resource name; unset on a create request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-readable display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The immutable image + resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_container_environment: Option<CustomContainerEnvironment>,
    /// Egress policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub egress_control_config: Option<EgressControlConfig>,
    /// Lifecycle state, e.g. `ACTIVE`; unset on a create request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Body for creating a sandbox: from a template, or restored from a snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCreateRequest {
    /// Human-readable display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Template to cut the sandbox from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_environment_template: Option<String>,
    /// Snapshot to restore the sandbox from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_environment_snapshot: Option<String>,
    /// Time-to-live before the sandbox expires, e.g. `3600s`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// How to reach a running sandbox's proxy. `routing_token` is a short-lived bearer credential and is
/// redacted in `Debug`; `connectionInfo` is `Some({})` on a sandbox that is not yet addressable, so
/// a `None` hostname must be treated as "cannot execute yet", never as ready.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    /// Hostname of the sandbox load balancer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_hostname: Option<String>,
    /// Short-lived proxy bearer token; never log it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_token: Option<String>,
}

impl Debug for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionInfo")
            .field("load_balancer_hostname", &self.load_balancer_hostname)
            .field("routing_token", &self.routing_token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// A sandbox environment as the API reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEnvironment {
    /// Full resource name `projects/.../sandboxEnvironments/{id}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-readable display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Runtime state, e.g. `STATE_RUNNING`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Template the sandbox was cut from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_environment_template: Option<String>,
    /// When the sandbox expires
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,
    /// Proxy connection details; absent until the sandbox is addressable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_info: Option<ConnectionInfo>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A sandbox snapshot as the API reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSnapshot {
    /// Full resource name `projects/.../sandboxEnvironmentSnapshots/{id}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Preview fields not modelled above, kept rather than dropped.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The `google.protobuf.Empty` a `pause` operation resolves to. Deserializes from any object,
/// ignoring the `@type` marker, so `await_operation::<Empty>` works for value-less operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest {
    inputs: Vec<ExecuteBlob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteBlob {
    /// base64-encoded payload
    data: String,
    /// MIME type of the payload
    mime_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteResponse {
    #[serde(default)]
    outputs: Vec<ExecuteBlob>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListSandboxesResponse {
    #[serde(default)]
    sandbox_environments: Vec<SandboxEnvironment>,
    next_page_token: Option<String>,
}

// =================================================================================================
// API
// =================================================================================================

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait AgentPlatformApi: Send + Sync + Debug {
    /// Create a reasoning engine. Single-attempt; returns the operation to poll.
    async fn create_engine(&self, display_name: &str) -> Result<Operation>;
    /// Delete a reasoning engine. Retries; a not-found is success.
    async fn delete_engine(&self, engine: &str) -> Result<()>;

    /// Create a template. Single-attempt; returns the operation to poll. Config is immutable.
    async fn create_template(
        &self,
        engine: &str,
        template: SandboxEnvironmentTemplate,
    ) -> Result<Operation>;
    /// Read a template. Retries.
    async fn get_template(&self, engine: &str, template: &str) -> Result<SandboxEnvironmentTemplate>;
    /// Delete a template. Retries; a not-found is success.
    async fn delete_template(&self, engine: &str, template: &str) -> Result<()>;

    /// Create a sandbox. Single-attempt; returns the operation to poll.
    async fn create_sandbox(&self, engine: &str, request: SandboxCreateRequest) -> Result<Operation>;
    /// Read a sandbox. Retries.
    async fn get_sandbox(&self, engine: &str, sandbox: &str) -> Result<SandboxEnvironment>;
    /// List sandboxes under an engine, following pagination. Retries.
    async fn list_sandboxes(&self, engine: &str) -> Result<Vec<SandboxEnvironment>>;
    /// Delete a sandbox. Retries; a not-found is success.
    async fn delete_sandbox(&self, engine: &str, sandbox: &str) -> Result<()>;

    /// Run one request inside a sandbox through the `:execute` proxy. Single-attempt; the request
    /// body is redacted out of any error. `input` is opaque JSON bytes; the decoded reply is returned.
    async fn execute(&self, engine: &str, sandbox: &str, input: &[u8]) -> Result<Vec<u8>>;

    /// Pause a sandbox. Single-attempt state transition; returns the operation to poll.
    async fn pause(&self, engine: &str, sandbox: &str) -> Result<Operation>;
    /// Resume a sandbox. Single-attempt state transition; returns the operation to poll.
    async fn resume(&self, engine: &str, sandbox: &str) -> Result<Operation>;
    /// Snapshot a sandbox. Single-attempt state transition; returns the operation to poll.
    async fn snapshot(&self, engine: &str, sandbox: &str, display_name: &str) -> Result<Operation>;

    /// Read a long-running operation by resource name. Retries.
    async fn get_operation(&self, name: &str) -> Result<Operation>;
}

/// Client for the Agent Platform sandbox API.
#[derive(Debug)]
pub struct AgentPlatformClient {
    base: GcpClientBase,
}

impl AgentPlatformClient {
    /// Build a client against the region's `aiplatform` host. The regional endpoint is injected as a
    /// service override only when the config does not already carry one, so a test override wins.
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let mut config = config;
        let host = format!(
            "https://{}-aiplatform.googleapis.com/v1",
            config.region
        );
        config
            .service_overrides
            .get_or_insert_with(|| ServiceOverrides {
                endpoints: HashMap::new(),
            })
            .endpoints
            .entry(SERVICE_KEY.to_string())
            .or_insert(host);

        Self {
            base: GcpClientBase::new(client, config, Box::new(AgentPlatformServiceConfig)),
        }
    }

    fn engines_path(&self) -> String {
        let cfg = self.base.config();
        format!(
            "projects/{}/locations/{}/reasoningEngines",
            cfg.project_id, cfg.region
        )
    }

    fn engine_path(&self, engine: &str) -> String {
        format!("{}/{}", self.engines_path(), engine)
    }

    fn templates_path(&self, engine: &str) -> String {
        format!("{}/sandboxEnvironmentTemplates", self.engine_path(engine))
    }

    fn sandboxes_path(&self, engine: &str) -> String {
        format!("{}/sandboxEnvironments", self.engine_path(engine))
    }

    fn sandbox_path(&self, engine: &str, sandbox: &str) -> String {
        format!("{}/{}", self.sandboxes_path(engine), sandbox)
    }

    /// Poll a long-running operation to completion within `budget`, returning its typed response.
    ///
    /// On an operation error, reports `OperationFailed`; on budget exhaustion, `OperationIncomplete`
    /// carrying the operation name and last observed state — never a bare timeout.
    pub async fn await_operation<T>(&self, operation: &Operation, budget: PollBudget) -> Result<T>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let name = match &operation.name {
            Some(name) => name.clone(),
            None => {
                return Err(AlienError::new(AgentPlatformErrorData::OperationIncomplete {
                    operation: "<unnamed>".to_string(),
                    attempts: 0,
                    last_state: "the operation carried no resource name".to_string(),
                }))
            }
        };

        let mut current = operation.clone();
        for _ in 0..budget.max_attempts {
            if current.done == Some(true) {
                return Self::finish_operation::<T>(current, &name);
            }
            tokio::time::sleep(budget.interval).await;
            current = self.get_operation(&name).await?;
        }

        if current.done == Some(true) {
            return Self::finish_operation::<T>(current, &name);
        }
        Err(AlienError::new(AgentPlatformErrorData::OperationIncomplete {
            operation: name,
            attempts: budget.max_attempts,
            last_state: Self::describe_operation(&current),
        }))
    }

    /// Poll a template until it reaches `ACTIVE`, or report `TemplateNotActive` with the last state.
    pub async fn await_template_active(
        &self,
        engine: &str,
        template: &str,
        budget: PollBudget,
    ) -> Result<SandboxEnvironmentTemplate> {
        let mut last_state = "<unknown>".to_string();
        for _ in 0..budget.max_attempts {
            let current = self.get_template(engine, template).await?;
            last_state = current.state.clone().unwrap_or_default();
            if last_state == "ACTIVE" {
                return Ok(current);
            }
            tokio::time::sleep(budget.interval).await;
        }
        Err(AlienError::new(AgentPlatformErrorData::TemplateNotActive {
            template: template.to_string(),
            state: last_state,
            attempts: budget.max_attempts,
        }))
    }

    fn finish_operation<T: serde::de::DeserializeOwned>(op: Operation, name: &str) -> Result<T> {
        match op.result {
            Some(OperationResult::Error { error }) => {
                Err(AlienError::new(AgentPlatformErrorData::OperationFailed {
                    operation: name.to_string(),
                    grpc_code: error.code,
                    message: error.message,
                }))
            }
            Some(OperationResult::Response { response }) => serde_json::from_value::<T>(response)
                .into_alien_error()
                .context(AgentPlatformErrorData::RequestFailed {
                    operation: format!("operation '{name}' response"),
                    message: "response body did not match the expected type".to_string(),
                }),
            None => Err(AlienError::new(AgentPlatformErrorData::OperationIncomplete {
                operation: name.to_string(),
                attempts: 0,
                last_state: "operation reported done without a result".to_string(),
            })),
        }
    }

    fn describe_operation(op: &Operation) -> String {
        match &op.result {
            Some(OperationResult::Error { error }) => {
                format!("last error (grpc {}): {}", error.code, error.message)
            }
            _ => "operation had not completed".to_string(),
        }
    }
}

/// Maps a cloud error onto this client's enum, treating a not-found as success — best-effort delete.
fn tolerate_not_found(result: alien_client_core::Result<Operation>, operation: &str) -> Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            if matches!(
                &e.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) {
                Ok(())
            } else {
                Err::<(), _>(e).context(AgentPlatformErrorData::RequestFailed {
                    operation: operation.to_string(),
                    message: "deletion failed".to_string(),
                })
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AgentPlatformApi for AgentPlatformClient {
    async fn create_engine(&self, display_name: &str) -> Result<Operation> {
        let path = self.engines_path();
        self.base
            .execute_request_once(
                Method::POST,
                &path,
                None,
                Some(serde_json::json!({ "displayName": display_name })),
                display_name,
            )
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "create engine".to_string(),
                message: display_name.to_string(),
            })
    }

    async fn delete_engine(&self, engine: &str) -> Result<()> {
        let path = self.engine_path(engine);
        let result: alien_client_core::Result<Operation> = self
            .base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, engine)
            .await;
        tolerate_not_found(result, "delete engine")
    }

    async fn create_template(
        &self,
        engine: &str,
        template: SandboxEnvironmentTemplate,
    ) -> Result<Operation> {
        let path = self.templates_path(engine);
        self.base
            .execute_request_once(Method::POST, &path, None, Some(template), engine)
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "create template".to_string(),
                message: format!("engine '{engine}'"),
            })
    }

    async fn get_template(&self, engine: &str, template: &str) -> Result<SandboxEnvironmentTemplate> {
        let path = format!("{}/{}", self.templates_path(engine), template);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, template)
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "get template".to_string(),
                message: template.to_string(),
            })
    }

    async fn delete_template(&self, engine: &str, template: &str) -> Result<()> {
        let path = format!("{}/{}", self.templates_path(engine), template);
        let result: alien_client_core::Result<Operation> = self
            .base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, template)
            .await;
        tolerate_not_found(result, "delete template")
    }

    async fn create_sandbox(&self, engine: &str, request: SandboxCreateRequest) -> Result<Operation> {
        let path = self.sandboxes_path(engine);
        self.base
            .execute_request_once(Method::POST, &path, None, Some(request), engine)
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "create sandbox".to_string(),
                message: format!("engine '{engine}'"),
            })
    }

    async fn get_sandbox(&self, engine: &str, sandbox: &str) -> Result<SandboxEnvironment> {
        let path = self.sandbox_path(engine, sandbox);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, sandbox)
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "get sandbox".to_string(),
                message: sandbox.to_string(),
            })
    }

    async fn list_sandboxes(&self, engine: &str) -> Result<Vec<SandboxEnvironment>> {
        let path = self.sandboxes_path(engine);
        let mut sandboxes = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let query = page_token
                .as_ref()
                .map(|token| vec![("pageToken", token.clone())]);
            let page: ListSandboxesResponse = self
                .base
                .execute_request(Method::GET, &path, query, Option::<()>::None, engine)
                .await
                .context(AgentPlatformErrorData::RequestFailed {
                    operation: "list sandboxes".to_string(),
                    message: format!("engine '{engine}'"),
                })?;

            sandboxes.extend(page.sandbox_environments);
            match page.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(sandboxes)
    }

    async fn delete_sandbox(&self, engine: &str, sandbox: &str) -> Result<()> {
        let path = self.sandbox_path(engine, sandbox);
        let result: alien_client_core::Result<Operation> = self
            .base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, sandbox)
            .await;
        tolerate_not_found(result, "delete sandbox")
    }

    async fn execute(&self, engine: &str, sandbox: &str, input: &[u8]) -> Result<Vec<u8>> {
        let path = format!("{}:execute", self.sandbox_path(engine, sandbox));
        let body = ExecuteRequest {
            inputs: vec![ExecuteBlob {
                data: BASE64.encode(input),
                mime_type: JSON_MIME.to_string(),
            }],
        };

        // Single-attempt: a repeat may re-run a command the first attempt already started. The
        // request body carries the caller's command and env, so redaction runs before the error is
        // wrapped — the body must never reach a serialized error chain.
        let raw: alien_client_core::Result<ExecuteResponse> = self
            .base
            .execute_request_once(Method::POST, &path, None, Some(body), sandbox)
            .await;
        let response = redact_request_body(raw).context(AgentPlatformErrorData::ExecuteFailed {
            sandbox: sandbox.to_string(),
            message: "the API rejected or cut short the request".to_string(),
        })?;

        let blob = response.outputs.into_iter().next().ok_or_else(|| {
            AlienError::new(AgentPlatformErrorData::ExecuteOutputInvalid {
                sandbox: sandbox.to_string(),
                message: "the reply contained no outputs".to_string(),
            })
        })?;

        BASE64
            .decode(blob.data.as_bytes())
            .into_alien_error()
            .context(AgentPlatformErrorData::ExecuteOutputInvalid {
                sandbox: sandbox.to_string(),
                message: "output data was not valid base64".to_string(),
            })
    }

    async fn pause(&self, engine: &str, sandbox: &str) -> Result<Operation> {
        let path = format!("{}:pause", self.sandbox_path(engine, sandbox));
        self.base
            .execute_request_once(
                Method::POST,
                &path,
                None,
                Some(serde_json::json!({})),
                sandbox,
            )
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "pause sandbox".to_string(),
                message: sandbox.to_string(),
            })
    }

    async fn resume(&self, engine: &str, sandbox: &str) -> Result<Operation> {
        let path = format!("{}:resume", self.sandbox_path(engine, sandbox));
        self.base
            .execute_request_once(
                Method::POST,
                &path,
                None,
                Some(serde_json::json!({})),
                sandbox,
            )
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "resume sandbox".to_string(),
                message: sandbox.to_string(),
            })
    }

    async fn snapshot(&self, engine: &str, sandbox: &str, display_name: &str) -> Result<Operation> {
        let path = format!("{}:snapshot", self.sandbox_path(engine, sandbox));
        self.base
            .execute_request_once(
                Method::POST,
                &path,
                None,
                Some(serde_json::json!({ "displayName": display_name })),
                sandbox,
            )
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "snapshot sandbox".to_string(),
                message: sandbox.to_string(),
            })
    }

    async fn get_operation(&self, name: &str) -> Result<Operation> {
        self.base
            .execute_request(Method::GET, name, None, Option::<()>::None, name)
            .await
            .context(AgentPlatformErrorData::RequestFailed {
                operation: "get operation".to_string(),
                message: name.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcp::GcpCredentials;
    use httpmock::prelude::*;

    const ENGINE: &str = "eng1";
    const SANDBOX: &str = "sbx1";

    fn client(server: &MockServer) -> AgentPlatformClient {
        AgentPlatformClient::new(
            reqwest::Client::new(),
            GcpClientConfig {
                project_id: "test-project".to_string(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::AccessToken {
                    token: "test-token".to_string(),
                },
                service_overrides: Some(ServiceOverrides {
                    endpoints: HashMap::from([("aiplatform".to_string(), server.base_url())]),
                }),
                project_number: None,
            },
        )
    }

    /// A tiny budget so a never-completing operation exhausts in milliseconds rather than minutes.
    fn tiny_budget() -> PollBudget {
        PollBudget {
            interval: Duration::from_millis(1),
            max_attempts: 3,
        }
    }

    const SANDBOXES_PATH: &str =
        "/projects/test-project/locations/us-central1/reasoningEngines/eng1/sandboxEnvironments";
    const SANDBOX_PATH: &str =
        "/projects/test-project/locations/us-central1/reasoningEngines/eng1/sandboxEnvironments/sbx1";
    const OP_NAME: &str = "projects/test-project/locations/us-central1/operations/op1";
    const OP_PATH: &str = "/projects/test-project/locations/us-central1/operations/op1";

    // ---- Retry classification: a write is delivered once, a read still retries. --------------

    /// Pins the write-once / read-retries distinction that every single-attempt verb below relies
    /// on. The read is proven to retry in the same test so a bare `hits == 1` cannot pass vacuously.
    #[tokio::test]
    async fn create_sandbox_is_sent_once_where_a_read_retries() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST).path(SANDBOXES_PATH);
                then.status(503);
            })
            .await;
        let read = server
            .mock_async(|when, then| {
                when.method(GET).path(SANDBOX_PATH);
                then.status(503);
            })
            .await;

        client(&server)
            .create_sandbox(ENGINE, SandboxCreateRequest::default())
            .await
            .expect_err("create should surface the failure");
        assert_eq!(create.hits_async().await, 1, "create must not be re-sent");

        client(&server)
            .get_sandbox(ENGINE, SANDBOX)
            .await
            .expect_err("read should surface the failure");
        assert!(
            read.hits_async().await > 1,
            "a read must retry on a retryable failure"
        );
    }

    #[tokio::test]
    async fn create_engine_is_sent_once() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/projects/test-project/locations/us-central1/reasoningEngines");
                then.status(503);
            })
            .await;
        client(&server)
            .create_engine("engine-display")
            .await
            .expect_err("create should surface the failure");
        assert_eq!(create.hits_async().await, 1, "create engine must be sent once");
    }

    #[tokio::test]
    async fn create_template_is_sent_once() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST).path_contains("sandboxEnvironmentTemplates");
                then.status(503);
            })
            .await;
        client(&server)
            .create_template(ENGINE, SandboxEnvironmentTemplate::default_for_test())
            .await
            .expect_err("create should surface the failure");
        assert_eq!(create.hits_async().await, 1, "create template must be sent once");
    }

    #[tokio::test]
    async fn execute_is_sent_once() {
        let server = MockServer::start_async().await;
        let exec = server
            .mock_async(|when, then| {
                when.method(POST).path_contains(":execute");
                then.status(503);
            })
            .await;
        client(&server)
            .execute(ENGINE, SANDBOX, b"{}")
            .await
            .expect_err("execute should surface the failure");
        assert_eq!(exec.hits_async().await, 1, "execute must be sent once");
    }

    #[tokio::test]
    async fn pause_is_sent_once() {
        let server = MockServer::start_async().await;
        let pause = server
            .mock_async(|when, then| {
                when.method(POST).path_contains(":pause");
                then.status(503);
            })
            .await;
        client(&server)
            .pause(ENGINE, SANDBOX)
            .await
            .expect_err("pause should surface the failure");
        assert_eq!(pause.hits_async().await, 1, "pause must be sent once");
    }

    #[tokio::test]
    async fn resume_is_sent_once() {
        let server = MockServer::start_async().await;
        let resume = server
            .mock_async(|when, then| {
                when.method(POST).path_contains(":resume");
                then.status(503);
            })
            .await;
        client(&server)
            .resume(ENGINE, SANDBOX)
            .await
            .expect_err("resume should surface the failure");
        assert_eq!(resume.hits_async().await, 1, "resume must be sent once");
    }

    #[tokio::test]
    async fn snapshot_is_sent_once() {
        let server = MockServer::start_async().await;
        let snapshot = server
            .mock_async(|when, then| {
                when.method(POST).path_contains(":snapshot");
                then.status(503);
            })
            .await;
        client(&server)
            .snapshot(ENGINE, SANDBOX, "snap-display")
            .await
            .expect_err("snapshot should surface the failure");
        assert_eq!(snapshot.hits_async().await, 1, "snapshot must be sent once");
    }

    // ---- Redaction: an execute request body never reaches a serialized error. ----------------

    /// The execute request body carries the caller's command and env — a place a token lands. It
    /// must be absent from a serialized error, while diagnostics survive. The paired create
    /// assertion proves request bodies ARE captured, so execute's absence is redaction, not a body
    /// that was never recorded.
    #[tokio::test]
    async fn an_execute_body_is_absent_from_a_serialized_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path_contains(":execute");
                then.status(400).json_body_obj(&serde_json::json!({
                    "error": {
                        "code": 400,
                        "message": "Execution Failed. Error: DEADLINE_EXCEEDED",
                        "status": "FAILED_PRECONDITION"
                    }
                }));
            })
            .await;

        let secret_payload = br#"{"command":["echo","TOKEN-abc123-secret"]}"#;
        let encoded = BASE64.encode(secret_payload);

        let error = client(&server)
            .execute(ENGINE, SANDBOX, secret_payload)
            .await
            .expect_err("execute should fail");
        let serialized = serde_json::to_string(&error).expect("serialize error");

        assert!(
            !serialized.contains(&encoded),
            "the encoded request body leaked into the error: {serialized}"
        );
        assert!(
            !serialized.contains("TOKEN-abc123-secret"),
            "the raw command leaked into the error: {serialized}"
        );
        // Diagnostics survive, so the chain reached the serializer with content — the absence above
        // is meaningful, not vacuous.
        assert!(
            serialized.contains("FAILED_PRECONDITION"),
            "response diagnostics were dropped: {serialized}"
        );

        // Precondition: a non-secret create body IS captured in its error, proving the transport
        // records request bodies at all.
        let create_server = MockServer::start_async().await;
        create_server
            .mock_async(|when, then| {
                when.method(POST).path(SANDBOXES_PATH);
                then.status(400).json_body_obj(&serde_json::json!({
                    "error": { "code": 400, "message": "bad", "status": "INVALID_ARGUMENT" }
                }));
            })
            .await;
        let create_error = client(&create_server)
            .create_sandbox(
                ENGINE,
                SandboxCreateRequest {
                    display_name: Some("MARKER-create-body-9f".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("create should fail");
        let create_serialized = serde_json::to_string(&create_error).expect("serialize");
        assert!(
            create_serialized.contains("MARKER-create-body-9f"),
            "a create body should be captured (non-secret), proving bodies are recorded: {create_serialized}"
        );
    }

    // ---- Long-running operations: bounded, last error reported. -------------------------------

    #[tokio::test]
    async fn await_operation_returns_the_resource_when_the_operation_completes() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(OP_PATH);
                then.status(200).json_body_obj(&serde_json::json!({
                    "name": OP_NAME,
                    "done": true,
                    "response": {
                        "@type": "type.googleapis.com/google.cloud.aiplatform.v1.SandboxEnvironment",
                        "name": "projects/p/locations/us-central1/reasoningEngines/eng1/sandboxEnvironments/sbx1",
                        "state": "STATE_RUNNING"
                    }
                }));
            })
            .await;

        let pending = Operation {
            name: Some(OP_NAME.to_string()),
            ..Default::default()
        };
        let sandbox: SandboxEnvironment = client(&server)
            .await_operation(&pending, tiny_budget())
            .await
            .expect("operation should resolve to a sandbox");
        assert_eq!(sandbox.state.as_deref(), Some("STATE_RUNNING"));
    }

    /// A value-less `pause` operation resolves to `Empty` without choking on the `@type` marker.
    #[tokio::test]
    async fn await_operation_handles_a_value_less_result() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(OP_PATH);
                then.status(200).json_body_obj(&serde_json::json!({
                    "name": OP_NAME,
                    "done": true,
                    "response": { "@type": "type.googleapis.com/google.protobuf.Empty" }
                }));
            })
            .await;
        let pending = Operation {
            name: Some(OP_NAME.to_string()),
            ..Default::default()
        };
        let _empty: Empty = client(&server)
            .await_operation(&pending, tiny_budget())
            .await
            .expect("a value-less operation should resolve to Empty");
    }

    /// Budget exhaustion reports the last observed state and the operation name — not a bare timeout.
    #[tokio::test]
    async fn await_operation_reports_the_last_error_when_the_budget_runs_out() {
        let server = MockServer::start_async().await;
        let poll = server
            .mock_async(|when, then| {
                when.method(GET).path(OP_PATH);
                then.status(200).json_body_obj(&serde_json::json!({ "name": OP_NAME }));
            })
            .await;

        let pending = Operation {
            name: Some(OP_NAME.to_string()),
            ..Default::default()
        };
        let error = client(&server)
            .await_operation::<SandboxEnvironment>(&pending, tiny_budget())
            .await
            .expect_err("an operation that never completes should error");

        assert_eq!(
            error.code, "AGENT_PLATFORM_OPERATION_INCOMPLETE",
            "the budget-exhaustion error must be the incomplete variant"
        );
        assert!(
            error.message.contains(OP_NAME),
            "the error must name the operation for the caller to resume: {}",
            error.message
        );
        assert_eq!(poll.hits_async().await, 3, "polling must stop at the budget");
    }

    /// An operation that completes with an error status reports `OperationFailed`, not success.
    #[tokio::test]
    async fn await_operation_surfaces_an_operation_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(OP_PATH);
                then.status(200).json_body_obj(&serde_json::json!({
                    "name": OP_NAME,
                    "done": true,
                    "error": { "code": 9, "message": "quota exhausted" }
                }));
            })
            .await;
        let pending = Operation {
            name: Some(OP_NAME.to_string()),
            ..Default::default()
        };
        let error = client(&server)
            .await_operation::<SandboxEnvironment>(&pending, tiny_budget())
            .await
            .expect_err("an errored operation must fail");
        assert_eq!(error.code, "AGENT_PLATFORM_OPERATION_FAILED");
        assert!(error.message.contains("quota exhausted"), "{}", error.message);
    }

    // ---- Delete tolerance. --------------------------------------------------------------------

    #[tokio::test]
    async fn delete_sandbox_treats_not_found_as_success() {
        let server = MockServer::start_async().await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE).path(SANDBOX_PATH);
                then.status(404).json_body_obj(&serde_json::json!({
                    "error": { "code": 404, "message": "not found", "status": "NOT_FOUND" }
                }));
            })
            .await;

        client(&server)
            .delete_sandbox(ENGINE, SANDBOX)
            .await
            .expect("a not-found delete is success");
        assert!(delete.hits_async().await >= 1, "the delete was attempted");
    }

    /// Delete rides the retrying transport, so a transient failure is retried rather than sent once.
    #[tokio::test]
    async fn delete_sandbox_retries_a_transient_failure() {
        let server = MockServer::start_async().await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE).path(SANDBOX_PATH);
                then.status(503);
            })
            .await;
        client(&server)
            .delete_sandbox(ENGINE, SANDBOX)
            .await
            .expect_err("a transient delete failure surfaces");
        assert!(
            delete.hits_async().await > 1,
            "delete must retry on a retryable failure"
        );
    }

    // ---- Wire-shape pins. ---------------------------------------------------------------------

    /// `connectionInfo: {}` must parse as present-but-unaddressable, distinct from absent — a caller
    /// that reads `{}` as ready would fail at first execute.
    #[test]
    fn connection_info_empty_is_distinct_from_absent() {
        let running: SandboxEnvironment = serde_json::from_str(
            r#"{"name":"n","state":"STATE_RUNNING","connectionInfo":{"loadBalancerHostname":"h","routingToken":"t"}}"#,
        )
        .expect("running sandbox parses");
        assert_eq!(
            running.connection_info.as_ref().and_then(|c| c.load_balancer_hostname.as_deref()),
            Some("h")
        );

        let creating: SandboxEnvironment =
            serde_json::from_str(r#"{"name":"n","connectionInfo":{}}"#).expect("empty parses");
        let info = creating.connection_info.expect("present but empty");
        assert!(
            info.load_balancer_hostname.is_none(),
            "an empty connectionInfo is present but not addressable"
        );

        let paused: SandboxEnvironment =
            serde_json::from_str(r#"{"name":"n","state":"STATE_PAUSED"}"#).expect("no info parses");
        assert!(paused.connection_info.is_none(), "absent stays absent");
    }

    /// A routing token must not appear in a Debug rendering of a sandbox.
    #[test]
    fn a_routing_token_is_redacted_in_debug() {
        let info = ConnectionInfo {
            load_balancer_hostname: Some("host".to_string()),
            routing_token: Some("super-secret-token".to_string()),
        };
        let rendered = format!("{info:?}");
        assert!(!rendered.contains("super-secret-token"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
    }

    #[test]
    fn an_execute_reply_decodes_from_base64() {
        let reply: ExecuteResponse = serde_json::from_str(&format!(
            r#"{{"outputs":[{{"data":"{}","mimeType":"application/json"}}]}}"#,
            BASE64.encode(br#"{"op":"info"}"#)
        ))
        .expect("reply parses");
        let decoded = BASE64
            .decode(reply.outputs[0].data.as_bytes())
            .expect("decodes");
        assert_eq!(decoded, br#"{"op":"info"}"#);
    }

    impl SandboxEnvironmentTemplate {
        fn default_for_test() -> Self {
            Self {
                name: None,
                display_name: Some("tpl-display".to_string()),
                custom_container_environment: Some(CustomContainerEnvironment {
                    custom_container_spec: Some(CustomContainerSpec {
                        image_uri: "us-central1-docker.pkg.dev/p/r/agent:v1".to_string(),
                        extra: serde_json::Map::new(),
                    }),
                    resources: None,
                    ports: vec![],
                    extra: serde_json::Map::new(),
                }),
                egress_control_config: Some(EgressControlConfig {
                    internet_access: Some(true),
                    extra: serde_json::Map::new(),
                }),
                state: None,
                extra: serde_json::Map::new(),
            }
        }
    }

}
