//! The messages of the step protocol. The same JSON travels as one object per line over stdin
//! and stdout, and as the body of `POST /v1/step`.

use alien_core::{
    InitialSetupAuthority, Platform, ResourceHeartbeat, ResourceLifecycle, ResourceOutputs,
    ResourceStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The protocol version this side speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// Error codes of the step protocol. A controller's failed step keeps only `EXTENSION_STEP_REFUSED`,
/// `EXTENSION_STEP_DEADLINE`, `DEPENDENCY_NOT_READY`, and `REMOTE_RESOURCE_NOT_FOUND` while it deletes;
/// any other code reaches the resource inside the message of an `EXTENSION_STEP_FAILED`, so none
/// can pass for a failure of the host's own.
pub mod codes {
    /// The connection failed, the process exited, no reply came by the deadline, or a 5xx.
    pub const UNREACHABLE: &str = "EXTENSION_CONTROLLER_UNREACHABLE";
    /// The controller's server answered 401 or 403.
    pub const AUTH_REJECTED: &str = "EXTENSION_AUTH_REJECTED";
    /// One side speaks a protocol version the other doesn't.
    pub const PROTOCOL_UNSUPPORTED: &str = "EXTENSION_PROTOCOL_UNSUPPORTED";
    /// The reply isn't a step reply, or names another step.
    pub const PROTOCOL_ERROR: &str = "EXTENSION_PROTOCOL_ERROR";
    /// The controller refused the step for a reason other than the protocol version.
    pub const STEP_REFUSED: &str = "EXTENSION_STEP_REFUSED";
    /// The controller's step reached the request's deadline before it finished.
    pub const STEP_DEADLINE: &str = "EXTENSION_STEP_DEADLINE";
}

/// One step of a custom resource, sent to its controller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepRequest {
    /// The protocol version of the request.
    pub protocol: u32,
    /// This attempt's id. A resend after a lost reply carries the same one.
    pub step_id: String,
    /// The deployment the step belongs to.
    pub deployment_id: String,
    /// The resource's id in the stack.
    pub resource_id: String,
    /// The custom type, such as `acme/dns-zone`.
    pub resource_type: String,
    /// The platform the resource runs on.
    pub platform: Platform,
    /// The deployment's prefix for cloud resource names.
    pub resource_prefix: String,
    /// Whether initial setup ran from a setup template or with the admin's credentials.
    pub initial_setup_authority: InitialSetupAuthority,
    /// The resource's lifecycle in the stack.
    pub lifecycle: Option<ResourceLifecycle>,
    /// The resource's config, with its `type` and `id`.
    pub resource: Value,
    /// The config before the change, during an update.
    pub previous_resource: Option<Value>,
    /// What the controller may read about the resources this one depends on, by id.
    pub dependencies: BTreeMap<String, DependencyView>,
    /// The deployment's settings, without the values that are secret
    /// (see `proxy::deployment_config_for_extension`).
    pub deployment_config: Value,
    /// True when this step runs as part of initial setup with the admin's credentials.
    pub setup_run: bool,
    /// The setup run's id; each run gets a new one.
    pub setup_run_id: Option<String>,
    /// The controller's state exactly as its last reply returned it; `None` on the first step.
    pub state: Option<Value>,
    /// On the first step of a resource an importer filled, the fields it wrote. The controller
    /// loads them as its state, without a cloud call, before it steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_import: Option<Value>,
    /// Transitions started since the last reply, to apply in order before stepping.
    pub transitions: Vec<Transition>,
    /// Credentials for this step; `None` when the controller uses its own environment.
    pub credentials: Option<Credentials>,
    /// An RFC 3339 time the controller replies before.
    pub deadline: String,
}

/// What a controller may read about another resource. Never that resource's saved state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyView {
    /// The dependency's type, built in or custom.
    pub resource_type: String,
    /// Its status.
    pub status: ResourceStatus,
    /// Its outputs, tagged with their type. Kept as JSON so a controller built against an
    /// older Alien still reads a request whose dependency has an output type it doesn't know.
    pub outputs: Option<Value>,
    /// Its binding parameters, when the stack publishes them.
    pub binding_params: Option<Value>,
}

/// A transition Alien started between steps.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transition {
    ResetStayCount,
    UpdateStart,
    DeleteStart,
    TeardownStart,
    Failure,
}

/// Credentials for one step, for the cloud the resource runs on.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "platform", rename_all = "camelCase")]
pub enum Credentials {
    #[serde(rename_all = "camelCase")]
    Aws {
        account_id: String,
        region: String,
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
        expires_at: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Gcp {
        project_id: String,
        region: String,
        access_token: String,
        expires_at: String,
    },
    #[serde(rename_all = "camelCase")]
    Azure {
        subscription_id: String,
        tenant_id: String,
        access_token: String,
        expires_at: String,
    },
}

// The secret fields never reach a log line.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::Aws {
                account_id, region, ..
            } => f
                .debug_struct("Aws")
                .field("account_id", account_id)
                .field("region", region)
                .finish_non_exhaustive(),
            Credentials::Gcp {
                project_id, region, ..
            } => f
                .debug_struct("Gcp")
                .field("project_id", project_id)
                .field("region", region)
                .finish_non_exhaustive(),
            Credentials::Azure {
                subscription_id,
                tenant_id,
                ..
            } => f
                .debug_struct("Azure")
                .field("subscription_id", subscription_id)
                .field("tenant_id", tenant_id)
                .finish_non_exhaustive(),
        }
    }
}

/// The controller's reply to one step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepResponse {
    /// The protocol version of the reply.
    pub protocol: u32,
    /// The request's `step_id`.
    pub step_id: String,
    #[serde(flatten)]
    pub body: ResponseBody,
}

/// What happened in the step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum ResponseBody {
    /// The step ran.
    #[serde(rename_all = "camelCase")]
    Stepped {
        controller: ControllerView,
        /// How long to wait before the next step; `None` steps again at once.
        suggested_delay_ms: Option<u64>,
        /// Health reports for this resource; a controller with none may leave the field out.
        #[serde(default)]
        heartbeats: Vec<ResourceHeartbeat>,
    },
    /// The step ran and failed.
    #[serde(rename_all = "camelCase")]
    Failed {
        controller: ControllerView,
        error: StepError,
    },
    /// The controller didn't run the step.
    #[serde(rename_all = "camelCase")]
    Refused {
        error: StepError,
        /// The protocol versions the controller speaks, when it says.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        supported_protocols: Option<Vec<u32>>,
    },
}

/// The answers Alien needs between steps, computed by the controller from its current state.
/// Alien can't wait on a round trip when it asks them, so every reply carries them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControllerView {
    /// The controller's state, opaque to Alien and sent back unchanged in the next request.
    pub state: Value,
    /// The resource's status now.
    pub status: ResourceStatus,
    /// A line the dashboard shows next to the resource.
    pub progress: Option<String>,
    /// The resource's outputs, with `type` set to the resource type.
    pub outputs: Option<ResourceOutputs>,
    /// The resource's binding parameters.
    pub binding_params: Option<Value>,
    /// The status the resource gets if Alien marks this step failed.
    pub on_failure_status: ResourceStatus,
    /// What starting an update from this state does.
    pub update_start: TransitionPreview,
    /// What starting a delete from this state does.
    pub delete_start: TransitionPreview,
    /// What starting a teardown of what initial setup created does, from this state.
    pub teardown_start: TransitionPreview,
}

/// What a transition does from the state in the view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum TransitionPreview {
    /// The transition starts, and the resource gets `status`.
    Allowed { status: ResourceStatus },
    /// The controller can't start it from this state.
    Refused { message: String },
}

/// An error a controller reports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Asks a controller to load a state without stepping and without a cloud call, so a release
/// can check that an importer's fields load. Sent over stdin and stdout only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadRequest {
    pub protocol: u32,
    pub op: LoadOp,
    pub resource_type: String,
    pub platform: Platform,
    /// The fields to load, without the saved state's envelope.
    pub state: Value,
}

/// The `op` of a [`LoadRequest`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LoadOp {
    Load,
}

/// A controller's answer to a [`LoadRequest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadResponse {
    pub protocol: u32,
    #[serde(flatten)]
    pub result: LoadResult,
}

/// The view of the loaded state, or why the controller can't read it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LoadResult {
    View(ControllerView),
    Refused(String),
}
