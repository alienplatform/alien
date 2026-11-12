//! The controller side of the step protocol: a process that holds an extension's controllers and
//! answers steps and `load` messages for them.
//!
//! For each step it rebuilds the controller from the request, applies the pending transitions,
//! steps it once in a context built from the request, and answers with the view.

use super::protocol::{
    codes, ControllerView, Credentials, LoadRequest, LoadResponse, LoadResult, ResponseBody,
    StepError, StepRequest, StepResponse, Transition, TransitionPreview, PROTOCOL_VERSION,
};
use crate::core::{
    serialize_controller, HeartbeatCollector, PlatformServiceProvider, ResourceController,
    ResourceControllerContext, ResourceRegistry,
};
use crate::error::Result;
use alien_core::{
    AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials, ClientConfig,
    CustomResource, DeploymentConfig, GcpClientConfig, GcpCredentials, Platform, Resource,
    ResourceDefinition, ResourceEntry, ResourceLifecycle, ResourceOutputs, ResourceRef,
    ResourceType, Stack, StackResourceState, StackState,
};
use futures::future::{BoxFuture, FutureExt, Shared};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Keys an importer's fields must not set: the controller adds them itself.
const RESERVED_STATE_KEYS: [&str; 3] = ["type", "_controllerStateVersion", "internalStayCount"];

/// The credential fields the step log never writes.
const SECRET_CREDENTIAL_KEYS: [&str; 3] = ["secretAccessKey", "sessionToken", "accessToken"];

/// How many replies the replay cache keeps.
const REPLAY_CAPACITY: usize = 64;

/// Builds the `ClientConfig` a step's handlers use from the credentials the step carries.
pub type ClientConfigFor =
    fn(Option<&Credentials>, Platform) -> std::result::Result<ClientConfig, String>;

type LoadState =
    Box<dyn Fn(Value) -> std::result::Result<Box<dyn ResourceController>, String> + Send + Sync>;
type ReadResource = Box<dyn Fn(Value) -> std::result::Result<Resource, String> + Send + Sync>;

/// One controller the host runs, for one resource type on one platform.
struct Registration {
    prototype: Box<dyn ResourceController>,
    load: LoadState,
    import: LoadState,
    resource: ReadResource,
}

/// A step's reply, shared by every request with its id, or `None` when its run panicked. The
/// run is a task of its own, so a request that goes away, as one whose client disconnects does,
/// doesn't stop it, and the resend gets its reply.
type SharedReply = Shared<BoxFuture<'static, Option<StepResponse>>>;

/// Wraps each run, as a test host does to give it its own context.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) type RunScope =
    Arc<dyn Fn(BoxFuture<'static, StepResponse>) -> BoxFuture<'static, StepResponse> + Send + Sync>;

/// What runs a step. It is apart from the host, so a run can outlive the request that started
/// it.
#[derive(Clone)]
struct Runner {
    controllers: Arc<HashMap<(String, Platform), Registration>>,
    client_config: ClientConfigFor,
    service_provider: Arc<dyn PlatformServiceProvider>,
    #[cfg(any(test, feature = "test-utils"))]
    scope: Option<RunScope>,
    /// Each handled step is appended here as one `{request, response}` line.
    log: Option<PathBuf>,
}

/// Answers steps and `load` messages for the controllers registered on it.
pub struct ControllerHost {
    runner: Runner,
    /// Replies by deployment and step id, so a resend after a lost reply gets the same answer
    /// without running the handler again, and a resend that arrives while the step still runs
    /// waits for its reply instead of running it a second time. In memory only, so a restart
    /// loses it.
    replay: Mutex<VecDeque<((String, String), SharedReply, tokio::task::AbortHandle)>>,
}

fn refused(step_id: &str, code: &str, message: String) -> StepResponse {
    StepResponse {
        protocol: PROTOCOL_VERSION,
        step_id: step_id.to_string(),
        body: ResponseBody::Refused {
            error: StepError {
                code: code.to_string(),
                message,
                retryable: false,
            },
            supported_protocols: Some(vec![PROTOCOL_VERSION]),
        },
    }
}

fn preview(
    controller: &dyn ResourceController,
    apply: fn(&mut dyn ResourceController) -> Result<()>,
) -> TransitionPreview {
    let mut clone = controller.box_clone();
    match apply(clone.as_mut()) {
        Ok(()) => TransitionPreview::Allowed {
            status: clone.get_status(),
        },
        Err(error) => TransitionPreview::Refused {
            message: error.message,
        },
    }
}

/// The view of a controller as it is now.
pub(crate) fn view_of(
    controller: &dyn ResourceController,
) -> std::result::Result<ControllerView, String> {
    let state = serialize_controller(controller).map_err(|e| e.to_string())?;
    let on_failure_status = {
        let mut clone = controller.box_clone();
        clone.transition_to_failure();
        clone.get_status()
    };
    Ok(ControllerView {
        state,
        status: controller.get_status(),
        progress: controller.get_progress().map(str::to_owned),
        outputs: controller.get_outputs(),
        binding_params: controller.get_binding_params().map_err(|e| e.message)?,
        on_failure_status,
        update_start: preview(controller, |c| c.transition_to_update()),
        delete_start: preview(controller, |c| c.transition_to_delete_start()),
        teardown_start: preview(controller, |c| c.transition_to_teardown_start()),
    })
}

/// The `ClientConfig` for a step's credentials. A step on a cloud platform carries
/// credentials from every driver, so one without them is refused; `alien dev` sends none on
/// `Platform::Local`, whose state lives in the controller's working directory.
pub fn step_client_config(
    credentials: Option<&Credentials>,
    platform: Platform,
) -> std::result::Result<ClientConfig, String> {
    match (credentials, platform) {
        (
            Some(Credentials::Aws {
                account_id,
                region,
                access_key_id,
                secret_access_key,
                session_token,
                ..
            }),
            Platform::Aws,
        ) => Ok(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: account_id.clone(),
            region: region.clone(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.clone(),
                session_token: session_token.clone(),
            },
            service_overrides: None,
        }))),
        (
            Some(Credentials::Gcp {
                project_id,
                region,
                access_token,
                ..
            }),
            Platform::Gcp,
        ) => Ok(ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: project_id.clone(),
            region: region.clone(),
            credentials: GcpCredentials::AccessToken {
                token: access_token.clone(),
            },
            service_overrides: None,
            project_number: None,
        }))),
        (
            Some(Credentials::Azure {
                subscription_id,
                tenant_id,
                access_token,
                ..
            }),
            Platform::Azure,
        ) => Ok(ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id: tenant_id.clone(),
            region: None,
            credentials: AzureCredentials::AccessToken {
                token: access_token.clone(),
            },
            service_overrides: None,
        }))),
        (None, Platform::Local) => Ok(ClientConfig::Local {
            state_directory: std::env::current_dir()
                .map_err(|e| format!("the controller's working directory: {e}"))?
                .display()
                .to_string(),
        }),
        (None, platform) => Err(format!("the step on {platform:?} carries no credentials")),
        (Some(credentials), platform) => Err(format!(
            "the step on {platform:?} carries {credentials:?}, credentials for another platform"
        )),
    }
}

/// Reads a resource from a request: the type's config with its `type` tag.
fn read_resource<R: ResourceDefinition + DeserializeOwned + 'static>(
    mut value: Value,
) -> std::result::Result<Resource, String> {
    value
        .as_object_mut()
        .ok_or("the resource is not an object")?
        .remove("type");
    serde_json::from_value::<R>(value)
        .map(Resource::new)
        .map_err(|e| e.to_string())
}

impl ControllerHost {
    /// A host with no controllers yet.
    pub fn new(
        client_config: ClientConfigFor,
        service_provider: Arc<dyn PlatformServiceProvider>,
        log: Option<PathBuf>,
    ) -> Self {
        Self {
            runner: Runner {
                controllers: Arc::new(HashMap::new()),
                client_config,
                service_provider,
                #[cfg(any(test, feature = "test-utils"))]
                scope: None,
                log,
            },
            replay: Mutex::new(VecDeque::new()),
        }
    }

    /// Runs `controller`, whose config type is `R`, for `resource_type` on `platform`. A new
    /// resource starts from `controller` as it is.
    pub fn register<R, C>(
        &mut self,
        resource_type: ResourceType,
        platform: Platform,
        controller: C,
    ) -> std::result::Result<(), String>
    where
        R: ResourceDefinition + DeserializeOwned + 'static,
        C: ResourceController + DeserializeOwned + 'static,
    {
        let key = (resource_type.0.to_string(), platform);
        let controllers = Arc::get_mut(&mut self.runner.controllers)
            .ok_or("controllers are registered before the host answers a step")?;
        if controllers.contains_key(&key) {
            return Err(format!(
                "two controllers are registered for {} on {platform:?}",
                key.0
            ));
        }
        let controller_type = controller.controller_type();
        let load: LoadState = Box::new(move |mut value| {
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
        let import: LoadState = Box::new(|value| {
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
        controllers.insert(
            key,
            Registration {
                prototype: Box::new(controller),
                load,
                import,
                resource: Box::new(read_resource::<R>),
            },
        );
        Ok(())
    }

    /// Runs every step inside `scope`.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn scope_runs(&mut self, scope: RunScope) {
        self.runner.scope = Some(scope);
    }

    /// The type and platform of every controller the host runs.
    pub fn registrations(&self) -> impl Iterator<Item = (&str, Platform)> {
        self.runner
            .controllers
            .keys()
            .map(|(resource_type, platform)| (resource_type.as_str(), *platform))
    }

    /// Answers one step, from the replay cache when this deployment already sent its id.
    /// Panics when the step's handler did, so the request goes unanswered.
    pub async fn handle(&self, request: StepRequest) -> StepResponse {
        if request.protocol != PROTOCOL_VERSION {
            return refused(
                &request.step_id,
                codes::PROTOCOL_UNSUPPORTED,
                format!(
                    "the request uses step protocol {} but this controller speaks {PROTOCOL_VERSION}",
                    request.protocol
                ),
            );
        }
        let key = (request.deployment_id.clone(), request.step_id.clone());
        // The run logs its own request, whether or not that request is still there to answer.
        let (reply, replayed) = {
            let mut replay = self.replay.lock().expect("replay lock");
            match replay.iter().find(|(id, _, _)| *id == key) {
                Some((_, reply, _)) => (reply.clone(), true),
                None => {
                    // Only a step whose run finished leaves; while every cached step still
                    // runs, the cache grows past its capacity, since a resend of one evicted
                    // mid-run would run it again.
                    while replay.len() >= REPLAY_CAPACITY {
                        let Some(done) = replay.iter().position(|(_, _, run)| run.is_finished())
                        else {
                            break;
                        };
                        replay.remove(done);
                    }
                    let runner = self.runner.clone();
                    let owned = request.clone();
                    let run = async move {
                        let response = runner.run(&owned).await;
                        runner.append_log(&owned, &response);
                        response
                    }
                    .boxed();
                    #[cfg(any(test, feature = "test-utils"))]
                    let run = match &self.runner.scope {
                        Some(scope) => scope(run),
                        None => run,
                    };
                    let task = tokio::spawn(run);
                    let run = task.abort_handle();
                    let reply: SharedReply = async move { task.await.ok() }.boxed().shared();
                    replay.push_back((key, reply.clone(), run));
                    (reply, false)
                }
            }
        };
        let Some(response) = reply.clone().await else {
            // The handler may have acted before it panicked, so the step is not answered as
            // refused: this request fails as an unreachable controller's would, and a resend
            // runs the step again.
            self.replay
                .lock()
                .expect("replay lock")
                .retain(|(_, cached, _)| !cached.ptr_eq(&reply));
            std::panic::resume_unwind(Box::new(format!(
                "step {} panicked before it answered",
                request.step_id
            )));
        };
        if replayed {
            self.runner.append_log(&request, &response);
        }
        response
    }

    /// Loads an importer's fields without stepping and without a cloud call.
    pub fn load(&self, request: LoadRequest) -> LoadResponse {
        let result = if request.protocol != PROTOCOL_VERSION {
            LoadResult::Refused(format!(
                "the request uses step protocol {} but this controller speaks {PROTOCOL_VERSION}",
                request.protocol
            ))
        } else {
            match self
                .runner
                .registration(&request.resource_type, request.platform)
                .and_then(|registration| (registration.import)(request.state))
                .and_then(|controller| view_of(controller.as_ref()))
            {
                Ok(view) => LoadResult::View(view),
                Err(message) => LoadResult::Refused(message),
            }
        };
        LoadResponse {
            protocol: PROTOCOL_VERSION,
            result,
        }
    }
}

impl Runner {
    fn append_log(&self, request: &StepRequest, response: &StepResponse) {
        use std::io::Write as _;
        let Some(path) = &self.log else { return };
        let mut request = json!(request);
        if let Some(Value::Object(credentials)) = request.get_mut("credentials") {
            for (key, value) in credentials.iter_mut() {
                if SECRET_CREDENTIAL_KEYS.contains(&key.as_str()) && !value.is_null() {
                    *value = Value::String("<redacted>".to_string());
                }
            }
        }
        // One write per line: steps handled concurrently append to the same file, and an
        // append-mode write lands whole, where `writeln!` may split a line across writes.
        let line = format!("{}\n", json!({ "request": request, "response": response }));
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .expect("the step log is writable");
    }

    fn registration(
        &self,
        resource_type: &str,
        platform: Platform,
    ) -> std::result::Result<&Registration, String> {
        self.controllers
            .get(&(resource_type.to_string(), platform))
            .ok_or_else(|| format!("no controller for {resource_type} on {platform:?}"))
    }

    async fn run(&self, request: &StepRequest) -> StepResponse {
        let step_id = request.step_id.as_str();
        let registration = match self.registration(&request.resource_type, request.platform) {
            Ok(registration) => registration,
            Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
        };
        let loaded = match (&request.state, &request.pending_import) {
            (Some(state), _) => (registration.load)(state.clone()),
            (None, Some(imported)) => (registration.import)(imported.clone()),
            (None, None) => Ok(registration.prototype.box_clone()),
        };
        let mut controller = match loaded {
            Ok(controller) => controller,
            Err(e) => {
                return refused(
                    step_id,
                    codes::STEP_REFUSED,
                    format!("cannot load the state: {e}"),
                )
            }
        };
        for transition in &request.transitions {
            let applied = match transition {
                Transition::ResetStayCount => {
                    controller.reset_stay_count();
                    Ok(())
                }
                Transition::UpdateStart => controller.transition_to_update(),
                Transition::DeleteStart => controller.transition_to_delete_start(),
                Transition::TeardownStart => controller.transition_to_teardown_start(),
                Transition::Failure => {
                    controller.transition_to_failure();
                    Ok(())
                }
            };
            if let Err(e) = applied {
                return refused(
                    step_id,
                    codes::STEP_REFUSED,
                    format!("cannot apply {transition:?}: {}", e.message),
                );
            }
        }

        let desired = match (registration.resource)(request.resource.clone()) {
            Ok(desired) => desired,
            Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
        };
        let previous = match request
            .previous_resource
            .clone()
            .map(|value| (registration.resource)(value))
            .transpose()
        {
            Ok(previous) => previous,
            Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
        };
        let mut deployment_config: DeploymentConfig =
            match serde_json::from_value(request.deployment_config.clone()) {
                Ok(config) => config,
                Err(e) => return refused(step_id, codes::STEP_REFUSED, e.to_string()),
            };
        deployment_config.setup_run = request.setup_run;
        deployment_config.setup_run_id = request.setup_run_id.clone();
        let client_config =
            match (self.client_config)(request.credentials.as_ref(), request.platform) {
                Ok(config) => config,
                Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
            };
        let deadline = match chrono::DateTime::parse_from_rfc3339(&request.deadline) {
            Ok(deadline) => deadline.with_timezone(&chrono::Utc),
            Err(e) => {
                return refused(
                    step_id,
                    codes::STEP_REFUSED,
                    format!(
                        "the deadline '{}' is not an RFC 3339 time: {e}",
                        request.deadline
                    ),
                )
            }
        };

        // A one-resource stack state, plus what the request says about the dependencies.
        let lifecycle = request.lifecycle.unwrap_or(ResourceLifecycle::Live);
        let mut stack_state = StackState::new(request.platform);
        stack_state.resource_prefix = request.resource_prefix.clone();
        let mut own = StackResourceState::builder()
            .resource_type(request.resource_type.clone())
            .status(controller.get_status())
            .config(desired.clone())
            .maybe_previous_config(previous)
            .lifecycle(lifecycle)
            .dependencies(
                request
                    .dependencies
                    .iter()
                    .map(|(id, view)| {
                        ResourceRef::new(ResourceType::from(view.resource_type.as_str()), id)
                    })
                    .collect(),
            )
            .build();
        own.outputs = controller.get_outputs();
        stack_state
            .resources
            .insert(request.resource_id.clone(), own);
        for (id, view) in &request.dependencies {
            let config =
                match CustomResource::from_untagged(&view.resource_type, json!({ "id": id })) {
                    Ok(config) => Resource::new(config),
                    Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
                };
            let mut state = StackResourceState::builder()
                .resource_type(view.resource_type.clone())
                .status(view.status)
                .config(config)
                .build();
            state.outputs = match view
                .outputs
                .clone()
                .map(serde_json::from_value::<ResourceOutputs>)
            {
                None => None,
                Some(Ok(outputs)) => Some(outputs),
                Some(Err(e)) => {
                    return refused(
                        step_id,
                        codes::STEP_REFUSED,
                        format!("the outputs of dependency '{id}' are not resource outputs: {e}"),
                    )
                }
            };
            state.remote_binding_params = view.binding_params.clone();
            stack_state.resources.insert(id.clone(), state);
        }
        let desired_stack = Stack::new(request.deployment_id.clone())
            .add_entry(ResourceEntry {
                config: desired.clone(),
                lifecycle,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            })
            .build();
        let registry = Arc::new(ResourceRegistry::new());
        let heartbeat_collector = HeartbeatCollector::default();
        let ctx = ResourceControllerContext {
            desired_config: &desired,
            platform: request.platform,
            client_config,
            state: &stack_state,
            resource_prefix: &request.resource_prefix,
            registry: &registry,
            desired_stack: &desired_stack,
            service_provider: &self.service_provider,
            deployment_config: &deployment_config,
            initial_setup_authority: request.initial_setup_authority,
            heartbeat_collector: heartbeat_collector.clone(),
        };

        // A deadline already past leaves no time: the step fails as STEP_DEADLINE, retryable.
        let remaining = (deadline - chrono::Utc::now()).to_std().unwrap_or_default();
        let stepped = tokio::time::timeout(remaining, controller.step(&ctx)).await;

        let view = match view_of(controller.as_ref()) {
            Ok(view) => view,
            Err(e) => return refused(step_id, codes::STEP_REFUSED, e),
        };
        let body = match stepped {
            Ok(Ok(result)) => ResponseBody::Stepped {
                controller: view,
                suggested_delay_ms: result.suggested_delay.map(|d| d.as_millis() as u64),
                heartbeats: heartbeat_collector.drain(),
            },
            Ok(Err(error)) => ResponseBody::Failed {
                controller: view,
                error: StepError {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                },
            },
            Err(_) => ResponseBody::Failed {
                controller: view,
                error: StepError {
                    code: codes::STEP_DEADLINE.to_string(),
                    message: "the step did not finish before the request's deadline".to_string(),
                    retryable: true,
                },
            },
        };
        StepResponse {
            protocol: PROTOCOL_VERSION,
            step_id: step_id.to_string(),
            body,
        }
    }
}

impl ControllerHost {
    /// Reads one message per line on stdin and writes one reply per line on stdout, until
    /// stdin closes. Anything else the process prints, its own or a library's, goes to stderr:
    /// a stray line on stdout would be read as the reply.
    pub fn serve_stdio(&self, runtime: &tokio::runtime::Runtime) -> std::io::Result<()> {
        use std::io::{BufRead, Write};
        let requests = request_channel()?;
        let mut replies = reply_channel()?;
        for line in requests.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = self.reply_to_line(&line, runtime);
            writeln!(replies, "{reply}")?;
            replies.flush()?;
        }
        Ok(())
    }

    fn reply_to_line(&self, line: &str, runtime: &tokio::runtime::Runtime) -> String {
        let raw: Value = match serde_json::from_str(line) {
            Ok(raw) => raw,
            Err(e) => {
                let response = refused(
                    "",
                    codes::STEP_REFUSED,
                    format!("cannot read the message: {e}"),
                );
                return serde_json::to_string(&response).expect("a reply serializes");
            }
        };
        if raw.get("op").and_then(Value::as_str) == Some("load") {
            let response = match serde_json::from_value::<LoadRequest>(raw) {
                Ok(request) => self.load(request),
                Err(e) => LoadResponse {
                    protocol: PROTOCOL_VERSION,
                    result: LoadResult::Refused(format!("cannot read the load message: {e}")),
                },
            };
            return serde_json::to_string(&response).expect("a reply serializes");
        }
        let step_id = raw
            .get("stepId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let response = match serde_json::from_value::<StepRequest>(raw) {
            Ok(request) => runtime.block_on(self.handle(request)),
            Err(e) => refused(
                &step_id,
                codes::STEP_REFUSED,
                format!("cannot read the request: {e}"),
            ),
        };
        serde_json::to_string(&response).expect("a reply serializes")
    }
}

/// Where requests come from: the process's stdin, kept for them alone. On Unix the stdin
/// descriptor is duplicated, close-on-exec, for requests and then pointed at `/dev/null`, so a
/// process the controller starts can't read the requests meant for it.
#[cfg(unix)]
fn request_channel() -> std::io::Result<Box<dyn std::io::BufRead>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let null = std::fs::File::open("/dev/null")?;
    // SAFETY: duplicates and replaces this process's own standard input; the new descriptor is
    // owned by the returned `File` alone.
    unsafe {
        let requests = libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 0);
        if requests < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::dup2(null.as_raw_fd(), libc::STDIN_FILENO) < 0 {
            let error = std::io::Error::last_os_error();
            libc::close(requests);
            return Err(error);
        }
        Ok(Box::new(std::io::BufReader::new(
            std::fs::File::from_raw_fd(requests),
        )))
    }
}

#[cfg(not(unix))]
fn request_channel() -> std::io::Result<Box<dyn std::io::BufRead>> {
    Ok(Box::new(std::io::stdin().lock()))
}

/// Where replies go: the process's stdout, kept for them alone. On Unix the stdout descriptor is
/// duplicated for replies and then pointed at stderr, so whatever else writes to stdout lands
/// in stderr.
#[cfg(unix)]
fn reply_channel() -> std::io::Result<Box<dyn std::io::Write>> {
    use std::os::fd::FromRawFd;
    std::io::Write::flush(&mut std::io::stdout())?;
    // SAFETY: duplicates and replaces this process's own standard descriptors; the new
    // descriptor is owned by the returned `File` alone.
    unsafe {
        // Close-on-exec, so a process the controller starts doesn't inherit the reply channel.
        let replies = libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 0);
        if replies < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) < 0 {
            let error = std::io::Error::last_os_error();
            libc::close(replies);
            return Err(error);
        }
        Ok(Box::new(std::fs::File::from_raw_fd(replies)))
    }
}

#[cfg(not(unix))]
fn reply_channel() -> std::io::Result<Box<dyn std::io::Write>> {
    Ok(Box::new(std::io::stdout()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MockPlatformServiceProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn request(step_id: &str) -> StepRequest {
        serde_json::from_value(json!({
            "protocol": PROTOCOL_VERSION, "stepId": step_id, "deploymentId": "dep_1",
            "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": "test",
            "resourcePrefix": "p", "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap()
    }

    /// A host whose every run is `run` in place of the step's own.
    fn host_running(
        run: impl Fn() -> BoxFuture<'static, StepResponse> + Send + Sync + 'static,
    ) -> ControllerHost {
        let mut host = ControllerHost::new(
            step_client_config,
            Arc::new(MockPlatformServiceProvider::new()),
            None,
        );
        host.scope_runs(Arc::new(move |_| run()));
        host
    }

    /// A step whose handler panicked may have acted, so it is not answered as refused: the
    /// request fails as an unreachable controller would, and a resend runs the step again.
    #[tokio::test]
    async fn a_step_that_panics_is_not_answered_and_a_resend_runs_it_again() {
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = runs.clone();
        let host = Arc::new(host_running(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async { panic!("the handler panicked") }.boxed()
        }));
        for _ in 0..2 {
            let host = host.clone();
            let outcome = tokio::spawn(async move { host.handle(request("s1")).await }).await;
            match outcome {
                Err(error) => assert!(error.is_panic(), "{error}"),
                Ok(response) => panic!("no reply, not {response:?}"),
            }
        }
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    /// Steps that were all still running when the cache passed its capacity leave it once they
    /// finish, so the cache comes back to its capacity at the next new step.
    #[tokio::test]
    async fn the_replay_cache_shrinks_back_to_its_capacity() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let held = gate.clone();
        let host = Arc::new(host_running(move || {
            let gate = held.clone();
            async move {
                gate.acquire().await.unwrap().forget();
                refused("", codes::STEP_REFUSED, "done".to_string())
            }
            .boxed()
        }));
        let over = REPLAY_CAPACITY + 6;
        let waiting: Vec<_> = (0..over)
            .map(|n| {
                let host = host.clone();
                tokio::spawn(async move { host.handle(request(&format!("s{n}"))).await })
            })
            .collect();
        while host.replay.lock().unwrap().len() < over {
            tokio::task::yield_now().await;
        }
        gate.add_permits(over + 1);
        for waiter in waiting {
            waiter.await.unwrap();
        }
        host.handle(request("next")).await;
        assert_eq!(host.replay.lock().unwrap().len(), REPLAY_CAPACITY);
    }
}
