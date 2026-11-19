//! The controller module's side of the step protocol. A controller crate's library holds
//! `#[controller]` structs and the function that registers them, which
//! [`export_controllers!`](crate::export_controllers) names. Built for `wasm32-wasip1` it is
//! the extension's controller module, stepped one step per fresh instance by the process that
//! steps the deployment.
//!
//! The module never sees a credential and has no socket. Its cloud calls go through [`Cloud`],
//! which hands each request to the host to check, sign and send;
//! [`ControllerContextExt::aws_sdk_config`] gives an AWS SDK client that sends through it.
//!
//! The same library builds natively, for its own tests: [`testing::answer`] steps its
//! controllers there, with the test answering their calls.
//!
//! # A step may run again
//!
//! Alien sends a step again, from the same state, when it got no reply to it: the reply was
//! lost, the step was stopped at its deadline, or the process that ran it restarted. The calls
//! the first run made have happened. So every call a handler makes must be safe to repeat, as
//! the calls of Alien's own controllers are: create with a name or a token the cloud
//! deduplicates on and treat "already exists" as created, and treat "not found" on a delete as
//! deleted.
//!
//! # The deadline
//!
//! Every step has a deadline, [`ResourceControllerContext::time_left`]. A call started after it
//! is not sent, and a call still open at it is cut. A handler that then returns an error fails
//! the step with `EXTENSION_STEP_DEADLINE`, whatever error it returns, and Alien runs it again;
//! one that returns a result is taken at its word. Shortly after the deadline the host stops
//! the instance, and Alien runs the step again. A handler that waits for the cloud returns
//! `Stay` with a delay and is stepped again; it never loops until the cloud is done.

mod aws_sdk;
mod cloud;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;

pub use aws_sdk::{aws_sdk_error, ControllerContextExt};
pub use cloud::{Cloud, CloudRequest, CloudResponse, CloudTarget, HttpRequest};

use crate::{AlienResource, ResourceTypeOf};
use alien_core::{
    Platform, Resource, ResourceDefinition, ResourceHeartbeat, ResourceOutputs, ResourceRef,
    ResourceStatus,
};
use alien_error::{AlienError, AlienErrorData};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// The step protocol version a module speaks.
const PROTOCOL_VERSION: u32 = 1;

/// The reply code of a step that reached its deadline, which Alien retries.
const STEP_DEADLINE: &str = "EXTENSION_STEP_DEADLINE";

/// Keys an importer's fields must not set: the controller adds them itself.
const RESERVED_STATE_KEYS: [&str; 3] = ["type", "_controllerStateVersion", "internalStayCount"];

/// Errors a controller module's handlers return, with the codes and flags Alien's own
/// controllers and cloud clients use.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The resource's config can't be applied as it is.
    #[error(
        code = "RESOURCE_CONFIG_INVALID",
        message = "Resource configuration is invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    ResourceConfigInvalid {
        /// What is wrong with the config
        message: String,
        /// The resource, when the error names one
        resource_id: Option<String>,
    },

    /// A handler answered `Stay` more times than it allows itself.
    #[error(
        code = "POLLING_TIMEOUT",
        message = "Polling timeout in state {state} after {max_times} attempts",
        retryable = "true",
        internal = "false"
    )]
    PollingTimeout {
        /// The state that kept waiting
        state: String,
        /// How many times it may wait
        max_times: u32,
    },

    /// A cloud call failed. Wraps the call's own error and keeps its flags.
    #[error(
        code = "CLOUD_PLATFORM_ERROR",
        message = "Cloud platform operation failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    CloudPlatformError {
        /// What the handler was doing
        message: String,
        /// The resource, when the error names one
        resource_id: Option<String>,
    },

    /// The cloud answered 404. While a resource deletes, Alien takes this as already gone.
    #[error(
        code = "REMOTE_RESOURCE_NOT_FOUND",
        message = "{message}",
        retryable = "true",
        internal = "false",
        http_status_code = 404
    )]
    RemoteResourceNotFound {
        /// What was called and what it answered
        message: String,
    },

    /// The cloud answered 401 or 403.
    #[error(
        code = "REMOTE_ACCESS_DENIED",
        message = "{message}",
        retryable = "true",
        internal = "false",
        http_status_code = 403
    )]
    RemoteAccessDenied {
        /// What was called and what it answered
        message: String,
    },

    /// The cloud answered 409.
    #[error(
        code = "REMOTE_RESOURCE_CONFLICT",
        message = "{message}",
        retryable = "true",
        internal = "false",
        http_status_code = 409
    )]
    RemoteResourceConflict {
        /// What was called and what it answered
        message: String,
    },

    /// The cloud throttled the call.
    #[error(
        code = "RATE_LIMIT_EXCEEDED",
        message = "Rate limit exceeded: {message}",
        retryable = "true",
        internal = "false"
    )]
    RateLimitExceeded {
        /// What was called and what it answered
        message: String,
    },

    /// The cloud answered 5xx.
    #[error(
        code = "REMOTE_SERVICE_UNAVAILABLE",
        message = "Service unavailable: {message}",
        retryable = "true",
        internal = "false"
    )]
    RemoteServiceUnavailable {
        /// What was called and what it answered
        message: String,
    },

    /// The cloud answered with a status that is no success and none of the above.
    #[error(
        code = "HTTP_RESPONSE_ERROR",
        message = "{message}",
        retryable = "true",
        internal = "true"
    )]
    HttpResponseError {
        /// What was called and what it answered
        message: String,
        /// The status the call got
        http_status: u16,
    },

    /// The resource names a dependency its step knows nothing about.
    #[error(
        code = "DEPENDENCY_NOT_FOUND",
        message = "Resource '{resource_id}' depends on '{dependency_id}' which is not defined in the stack",
        retryable = "false",
        internal = "false"
    )]
    DependencyNotFound {
        /// The resource the step is for
        resource_id: String,
        /// The dependency it asked about
        dependency_id: String,
    },

    /// A dependency has no outputs yet. Alien steps the resource again when it has.
    #[error(
        code = "DEPENDENCY_NOT_READY",
        message = "Resource '{resource_id}' is waiting for dependency '{dependency_id}' to be ready",
        retryable = "true",
        internal = "false"
    )]
    DependencyNotReady {
        /// The resource the step is for
        resource_id: String,
        /// The dependency it waits for
        dependency_id: String,
    },

    /// The host sent the call and got no answer, could not connect, or a hook stopped it
    /// before it was sent in a way another try may pass.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "The call failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    HttpRequestFailed {
        /// What the host reported
        message: String,
    },

    /// The host refused the call: its endpoint, a header, its size, or the step's limit of
    /// calls. The same call is refused again.
    #[error(
        code = "EXTENSION_CALL_REFUSED",
        message = "The host refused the call: {message}",
        retryable = "false",
        internal = "false"
    )]
    CallRefused {
        /// Why the host refused it
        message: String,
    },

    /// The module could not make the request, or could not read the host's reply to it.
    /// Another try makes the same request.
    #[error(
        code = "EXTENSION_REQUEST_INVALID",
        message = "The request could not be made: {message}",
        retryable = "false",
        internal = "false"
    )]
    RequestInvalid {
        /// What went wrong
        message: String,
    },

    /// The cloud answered with a body that did not read as the type asked for.
    #[error(
        code = "UNEXPECTED_RESPONSE",
        message = "{message}",
        retryable = "false",
        internal = "false"
    )]
    UnexpectedResponse {
        /// What the body was read as, and why it did not read
        message: String,
    },

    /// The step's deadline passed before the call started.
    #[error(
        code = "EXTENSION_STEP_DEADLINE",
        message = "The step reached its deadline before {call}",
        retryable = "true",
        internal = "false"
    )]
    StepDeadline {
        /// The call that did not start
        call: String,
    },
}

/// The result of a handler and of everything it calls in this module.
pub type Result<T> = alien_error::Result<T, ErrorData>;

/// What the step request says about a resource this one depends on. A controller reads a
/// dependency's outputs, never its saved state.
#[derive(Deserialize)]
struct Dependency {
    outputs: Option<Value>,
}

/// What a step's handlers read, built from the step request.
pub struct ResourceControllerContext<'a> {
    /// The resource's config: the one to reach on a create or an update, the current one on a
    /// delete.
    pub desired_config: &'a Resource,
    /// The platform the resource runs on.
    pub platform: Platform,
    /// The deployment's prefix for cloud resource names.
    pub resource_prefix: &'a str,
    previous_config: Option<&'a Resource>,
    dependencies: &'a BTreeMap<String, Dependency>,
    cloud: Cloud,
    // A native build's handlers are `Send`, so the context they borrow is `Sync`.
    heartbeats: Mutex<Vec<ResourceHeartbeat>>,
}

impl ResourceControllerContext<'_> {
    /// The resource's config as `T`.
    pub fn desired_resource_config<T: ResourceDefinition + 'static>(&self) -> Result<&T> {
        self.desired_config.downcast_ref::<T>().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("the resource is a {}", self.desired_config.resource_type()),
                resource_id: Some(self.desired_config.id().to_string()),
            })
        })
    }

    /// The config before the change, as `T`. Only an update has one.
    pub fn previous_resource_config<T: ResourceDefinition + 'static>(&self) -> Result<&T> {
        let invalid = |message: String| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message,
                resource_id: Some(self.desired_config.id().to_string()),
            })
        };
        let previous = self
            .previous_config
            .ok_or_else(|| invalid("No previous configuration available".to_string()))?;
        previous.downcast_ref::<T>().ok_or_else(|| {
            invalid(format!(
                "the previous config is a {}",
                previous.resource_type()
            ))
        })
    }

    /// The step's cloud, through the host.
    pub fn cloud(&self) -> &Cloud {
        &self.cloud
    }

    /// How long the step has before its deadline; zero once it has passed.
    pub fn time_left(&self) -> Duration {
        self.cloud.time_left()
    }

    /// Reports the resource's health. The report is for the resource the step is for: Alien
    /// refuses the step's reply when it names another. A step that fails reports nothing.
    pub fn emit_heartbeat(&self, heartbeat: ResourceHeartbeat) {
        self.heartbeats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(heartbeat);
    }
}

/// What a step suggests about the next one.
pub struct ResourceControllerStepResult {
    /// How long Alien waits before the next step; `None` steps again at once.
    pub suggested_delay: Option<Duration>,
}

/// The trait `#[controller]` implements, with every method the macro can write. The step
/// protocol carries only what [`Controllers`] reads, so the seven methods from `needs_update` to
/// `is_interruptible` never reach Alien.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourceController: std::fmt::Debug {
    fn controller_type(&self) -> &'static str;
    fn to_json_value(&self) -> serde_json::Result<Value>;
    async fn step(
        &mut self,
        context: &ResourceControllerContext,
    ) -> Result<ResourceControllerStepResult>;
    fn transition_to_failure(&mut self);
    fn transition_to_delete_start(&mut self) -> Result<()>;
    fn transition_to_teardown_start(&mut self) -> Result<()> {
        self.transition_to_delete_start()
    }
    fn transition_to_update(&mut self) -> Result<()>;
    fn needs_update(&self, _context: &ResourceControllerContext<'_>) -> Result<bool> {
        Ok(false)
    }
    fn nothing_deleted_yet(&self) -> bool {
        false
    }
    fn has_runtime_setup_inputs(&self) -> bool {
        false
    }
    fn requires_convergence_reconciliation(&self) -> bool {
        false
    }
    fn dependents_may_update(&self) -> bool {
        false
    }
    fn updates_during_dependency_change(&self, _dependency: &ResourceRef) -> bool {
        false
    }
    fn is_interruptible(&self) -> bool {
        false
    }
    fn get_status(&self) -> ResourceStatus;
    fn get_progress(&self) -> Option<&str> {
        None
    }
    fn get_outputs(&self) -> Option<ResourceOutputs>;
    fn get_binding_params(&self) -> Result<Option<Value>> {
        Ok(None)
    }
    fn reset_stay_count(&mut self);
    fn box_clone(&self) -> Box<dyn ResourceController>;
    fn as_any(&self) -> &dyn std::any::Any;
}

type Load = Box<dyn Fn(Value) -> std::result::Result<Box<dyn ResourceController>, String>>;
type Read = fn(Value) -> std::result::Result<Resource, String>;

struct Registration {
    prototype: Box<dyn ResourceController>,
    load: Load,
    import: Load,
    read: Read,
}

/// The controllers a module runs, which the function `export_controllers!` names registers.
#[derive(Default)]
pub struct Controllers {
    by_type: BTreeMap<(String, String), Registration>,
    /// Registrations no host could use. The module answers every request with them.
    problems: Vec<String>,
}

/// A registration's key: the resource type, and the platform as the protocol writes it.
fn key(resource_type: &str, platform: Platform) -> (String, String) {
    (resource_type.to_string(), platform.to_string())
}

impl Controllers {
    /// Runs `controller` for resources of `resource_type` on `platform`. A new resource starts
    /// from `controller` as it is.
    ///
    /// A module runs the controllers of cloud platforms. `Platform::Local` is the controller
    /// binary's: `alien dev` runs it as a process.
    pub fn register<R, C>(
        &mut self,
        resource_type: ResourceTypeOf<R>,
        platform: Platform,
        controller: C,
    ) where
        R: AlienResource,
        C: ResourceController + DeserializeOwned + 'static,
    {
        let key = key(&resource_type.resource_type().0, platform);
        if self.by_type.contains_key(&key) {
            self.problems.push(format!(
                "two controllers are registered for {} on {platform}",
                key.0
            ));
            return;
        }
        let controller_type = controller.controller_type();
        let load: Load = Box::new(move |mut value| {
            let object = value.as_object_mut().ok_or("the state is not an object")?;
            let tag = object.remove("type");
            object.remove("_controllerStateVersion");
            if tag.as_ref().and_then(Value::as_str) != Some(controller_type) {
                return Err(format!("the state is for {tag:?}, not {controller_type}"));
            }
            serde_json::from_value::<C>(value)
                .map(|c| Box::new(c) as Box<dyn ResourceController>)
                .map_err(|e| e.to_string())
        });
        let import: Load = Box::new(|value| {
            let object = value
                .as_object()
                .ok_or("the imported state is not an object")?;
            if let Some(key) = RESERVED_STATE_KEYS
                .iter()
                .find(|key| object.contains_key(**key))
            {
                return Err(format!(
                    "the imported state sets '{key}', which the controller owns"
                ));
            }
            serde_json::from_value::<C>(value)
                .map(|c| Box::new(c) as Box<dyn ResourceController>)
                .map_err(|e| e.to_string())
        });
        self.by_type.insert(
            key,
            Registration {
                prototype: Box::new(controller),
                load,
                import,
                read: |mut value| {
                    value
                        .as_object_mut()
                        .ok_or("the resource is not an object")?
                        .remove("type");
                    serde_json::from_value::<R>(value)
                        .map(Resource::new)
                        .map_err(|e| e.to_string())
                },
            },
        );
    }

    fn registration(
        &self,
        resource_type: &str,
        platform: Platform,
    ) -> std::result::Result<&Registration, String> {
        self.by_type
            .get(&key(resource_type, platform))
            .ok_or_else(|| format!("no controller for {resource_type} on {platform}"))
    }
}

/// The fields of the step request a module reads. The host sends `credentials` as null and
/// adds `cloud`, where the step's cloud calls go.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepRequest {
    resource_type: String,
    platform: Platform,
    resource_prefix: String,
    resource: Value,
    previous_resource: Option<Value>,
    state: Option<Value>,
    #[serde(default)]
    pending_import: Option<Value>,
    transitions: Vec<Transition>,
    dependencies: BTreeMap<String, Dependency>,
    cloud: Option<CloudTarget>,
    deadline: DateTime<Utc>,
}

/// A transition Alien started between steps.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Transition {
    ResetStayCount,
    UpdateStart,
    DeleteStart,
    TeardownStart,
    Failure,
}

/// A `load` request: an importer's fields to load without stepping and without a cloud call.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadRequest {
    resource_type: String,
    platform: Platform,
    state: Value,
}

/// What every request has, read before its schema, so a request of a newer protocol is refused
/// for its version and not for a field this module doesn't know.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Head {
    protocol: u32,
    /// `load` or `controllers`; a step has none.
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    step_id: String,
}

fn refused(step_id: &str, message: String) -> Value {
    json!({
        "protocol": PROTOCOL_VERSION, "stepId": step_id, "result": "refused",
        "error": { "code": "EXTENSION_STEP_REFUSED", "message": message, "retryable": false },
        "supportedProtocols": [PROTOCOL_VERSION],
    })
}

fn preview(
    controller: &dyn ResourceController,
    apply: fn(&mut dyn ResourceController) -> Result<()>,
) -> Value {
    let mut clone = controller.box_clone();
    match apply(clone.as_mut()) {
        Ok(()) => json!({ "outcome": "allowed", "status": clone.get_status() }),
        Err(error) => json!({ "outcome": "refused", "message": error.message }),
    }
}

/// The view Alien reads between steps, as a controller process answers it.
fn view(controller: &dyn ResourceController) -> std::result::Result<Value, String> {
    let mut state = controller.to_json_value().map_err(|e| e.to_string())?;
    let object = state
        .as_object_mut()
        .ok_or("the controller is not an object")?;
    object.insert("type".into(), controller.controller_type().into());
    object.insert("_controllerStateVersion".into(), 1.into());
    let on_failure_status = {
        let mut clone = controller.box_clone();
        clone.transition_to_failure();
        clone.get_status()
    };
    Ok(json!({
        "state": state,
        "status": controller.get_status(),
        "progress": controller.get_progress(),
        "outputs": controller.get_outputs(),
        "bindingParams": controller
            .get_binding_params()
            .map_err(|e| format!("{}: {}", e.code, e.message))?,
        "onFailureStatus": on_failure_status,
        "updateStart": preview(controller, |c| c.transition_to_update()),
        "deleteStart": preview(controller, |c| c.transition_to_delete_start()),
        "teardownStart": preview(controller, |c| c.transition_to_teardown_start()),
    }))
}

impl Controllers {
    /// Answers one request: a step, or the `load` or `controllers` op. `Err` is a request the
    /// module can't answer in the protocol at all.
    fn answer(&self, input: &[u8]) -> std::result::Result<Value, String> {
        if !self.problems.is_empty() {
            return Err(self.problems.join("; "));
        }
        let head: Head = serde_json::from_slice(input)
            .map_err(|e| format!("the request is not one of the step protocol: {e}"))?;
        let unsupported = || {
            format!(
                "the request uses step protocol {} but this controller speaks {PROTOCOL_VERSION}",
                head.protocol
            )
        };
        match head.op.as_deref() {
            None if head.protocol != PROTOCOL_VERSION => Ok(json!({
                "protocol": PROTOCOL_VERSION, "stepId": head.step_id, "result": "refused",
                "error": {
                    "code": "EXTENSION_PROTOCOL_UNSUPPORTED",
                    "message": unsupported(),
                    "retryable": false,
                },
                "supportedProtocols": [PROTOCOL_VERSION],
            })),
            None => Ok(self.step(&head.step_id, input)),
            Some("load") if head.protocol != PROTOCOL_VERSION => {
                Ok(json!({ "protocol": PROTOCOL_VERSION, "refused": unsupported() }))
            }
            Some("load") => Ok(self.load(input)),
            Some("controllers") => Ok(self.list()),
            Some(other) => Err(format!("this controller module has no `{other}` op")),
        }
    }

    /// The platforms of each type's controllers, as a controller binary's `--controllers`
    /// prints them, for `alien extensions build`.
    fn list(&self) -> Value {
        let mut listed: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (resource_type, platform) in self.by_type.keys() {
            listed.entry(resource_type).or_default().push(platform);
        }
        json!({ "protocol": PROTOCOL_VERSION, "controllers": listed })
    }

    /// Loads an importer's fields without stepping.
    fn load(&self, input: &[u8]) -> Value {
        let result = serde_json::from_slice::<LoadRequest>(input)
            .map_err(|e| format!("not a load request: {e}"))
            .and_then(|request| {
                let registration = self.registration(&request.resource_type, request.platform)?;
                let controller = (registration.import)(request.state)?;
                view(controller.as_ref())
            });
        match result {
            Ok(view) => json!({ "protocol": PROTOCOL_VERSION, "view": view }),
            Err(message) => json!({ "protocol": PROTOCOL_VERSION, "refused": message }),
        }
    }

    /// Answers one step request: rebuilds the controller from the request, applies the pending
    /// transitions, steps it once, and answers with its view.
    fn step(&self, step_id: &str, input: &[u8]) -> Value {
        let request: StepRequest = match serde_json::from_slice(input) {
            Ok(request) => request,
            Err(e) => return refused(step_id, format!("cannot read the request: {e}")),
        };
        let registration = match self.registration(&request.resource_type, request.platform) {
            Ok(registration) => registration,
            Err(e) => return refused(step_id, e),
        };
        let loaded = match (&request.state, &request.pending_import) {
            (Some(state), _) => (registration.load)(state.clone()),
            (None, Some(imported)) => (registration.import)(imported.clone()),
            (None, None) => Ok(registration.prototype.box_clone()),
        };
        let mut controller = match loaded {
            Ok(controller) => controller,
            Err(e) => return refused(step_id, format!("cannot load the state: {e}")),
        };
        for transition in &request.transitions {
            let (name, applied) = match transition {
                Transition::ResetStayCount => {
                    controller.reset_stay_count();
                    ("reset-stay-count", Ok(()))
                }
                Transition::UpdateStart => ("update-start", controller.transition_to_update()),
                Transition::DeleteStart => {
                    ("delete-start", controller.transition_to_delete_start())
                }
                Transition::TeardownStart => {
                    ("teardown-start", controller.transition_to_teardown_start())
                }
                Transition::Failure => {
                    controller.transition_to_failure();
                    ("failure", Ok(()))
                }
            };
            if let Err(e) = applied {
                return refused(
                    step_id,
                    format!("cannot apply {name}: {}: {}", e.code, e.message),
                );
            }
        }
        let desired = match (registration.read)(request.resource.clone()) {
            Ok(desired) => desired,
            Err(e) => return refused(step_id, e),
        };
        let previous = match request
            .previous_resource
            .clone()
            .map(registration.read)
            .transpose()
        {
            Ok(previous) => previous,
            Err(e) => return refused(step_id, e),
        };
        let ctx = ResourceControllerContext {
            desired_config: &desired,
            platform: request.platform,
            resource_prefix: &request.resource_prefix,
            previous_config: previous.as_ref(),
            dependencies: &request.dependencies,
            cloud: Cloud::new(request.cloud.clone(), request.deadline),
            heartbeats: Mutex::default(),
        };
        // A handler's future is ready whenever it is polled: every call it makes blocks in the
        // host until it is answered.
        let stepped = pollster::block_on(controller.step(&ctx));
        let view = match view(controller.as_ref()) {
            Ok(view) => view,
            Err(e) => return refused(step_id, e),
        };
        match stepped {
            Ok(result) => json!({
                "protocol": PROTOCOL_VERSION, "stepId": step_id, "result": "stepped",
                "controller": view,
                "suggestedDelayMs": result
                    .suggested_delay
                    .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                "heartbeats": *ctx.heartbeats.lock().unwrap_or_else(PoisonError::into_inner),
            }),
            // After a call cut or refused for the deadline the host gives a failed step the
            // deadline's code, so the handler's own error is reported here.
            Err(error) => {
                let (code, retryable) = reply_code(&error);
                json!({
                    "protocol": PROTOCOL_VERSION, "stepId": step_id, "result": "failed",
                    "controller": view,
                    "error": {
                        "code": code,
                        "message": with_causes(&error),
                        "retryable": retryable,
                    },
                })
            }
        }
    }
}

/// The codes Alien acts on in a failed step: a dependency not ready yet, a resource already gone
/// while it deletes, and a step that ran out of time.
const CODES_ALIEN_ACTS_ON: [&str; 3] = [
    "DEPENDENCY_NOT_READY",
    "REMOTE_RESOURCE_NOT_FOUND",
    STEP_DEADLINE,
];

/// The code and retry flag a failed step replies with: the code of the first error in the chain
/// that Alien acts on, as Alien's executor looks for it in its own controllers' errors, so a 404
/// under a handler's `.context(...)` still counts; otherwise the outermost error's. The retry
/// flag is the outermost error's, which the handler set, except for a dependency not ready and
/// a step out of time, which are retried whatever wraps them.
fn reply_code(error: &AlienError<ErrorData>) -> (String, bool) {
    if CODES_ALIEN_ACTS_ON.contains(&error.code.as_str()) {
        return (error.code.clone(), error.retryable);
    }
    let mut cause = error.source.as_deref();
    while let Some(source) = cause {
        if CODES_ALIEN_ACTS_ON.contains(&source.code.as_str()) {
            let retryable = match source.code.as_str() {
                "DEPENDENCY_NOT_READY" | STEP_DEADLINE => source.retryable,
                _ => error.retryable,
            };
            return (source.code.clone(), retryable);
        }
        cause = source.source.as_deref();
    }
    (error.code.clone(), error.retryable)
}

/// The error's message and its causes', since the reply carries one message and the cause is
/// what a developer needs to see (an SDK's service error, a refused call).
fn with_causes(error: &AlienError<ErrorData>) -> String {
    let mut message = error.message.clone();
    let mut cause = error.source.as_deref();
    while let Some(source) = cause {
        message.push_str(&format!("; caused by {}: {}", source.code, source.message));
        cause = source.source.as_deref();
    }
    message
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn alloc(len: i32) -> i32 {
    crate::abi::alloc(len)
}

/// Packs a reply as the controller ABI returns it: its pointer and length, and bit 63 when the
/// module could not answer and the bytes say why.
#[cfg(target_arch = "wasm32")]
fn pack(reply: std::result::Result<Vec<u8>, String>) -> i64 {
    let (bytes, error) = match reply {
        Ok(bytes) => (bytes, false),
        Err(reason) => (reason.into_bytes(), true),
    };
    let packed = bytes.as_ptr() as u32 as u64 | (bytes.len() as u64) << 32 | (error as u64) << 63;
    // The host reads the reply after this call returns, and the instance ends with the step.
    std::mem::forget(bytes);
    packed as i64
}

/// # Safety
/// The host wrote `len` bytes at `ptr`, memory `alien_alloc` returned.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub unsafe fn step_export(register: fn(&mut Controllers), ptr: i32, len: i32) -> i64 {
    // SAFETY: the caller's contract.
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let mut controllers = Controllers::default();
    register(&mut controllers);
    pack(
        controllers
            .answer(input)
            .map(|reply| reply.to_string().into_bytes()),
    )
}

/// Declares the controller module: the function that registers its controllers.
///
/// ```ignore
/// use alien_extension_sdk::controller_prelude::*;
///
/// pub fn register(controllers: &mut Controllers) {
///     controllers.register(DnsZone::RESOURCE_TYPE, Platform::Aws, aws::AwsDnsZoneController::default());
/// }
///
/// alien_extension_sdk::export_controllers!(register);
/// ```
///
/// Built for `wasm32-wasip1`, it writes the functions Alien calls in the module:
/// `alien extensions build` builds the crate's library into the bundle's `controller.wasm`.
/// Built natively it writes nothing, and the library's tests step `register`'s controllers
/// with `controller_module::testing::answer`.
#[macro_export]
macro_rules! export_controllers {
    ($register:path) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn alien_controller_abi_v1() {}

            #[unsafe(no_mangle)]
            pub extern "C" fn alien_alloc(len: i32) -> i32 {
                $crate::controller_module::alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn alien_controller_step(ptr: i32, len: i32) -> i64 {
                // SAFETY: the host wrote the request into memory `alien_alloc` returned.
                unsafe { $crate::controller_module::step_export($register, ptr, len) }
            }
        };
    };
}
