//! A controller that runs as a WebAssembly module in the process that steps the deployment: the
//! manager, the Operator or the setup CLI.
//!
//! Each step runs in a fresh instance of the module, which has no socket, no file and no
//! environment. It gets the step without its credentials, and with `cloud`, a
//! [`CloudTarget`], in their place. What it needs from outside it asks its host for, through two
//! imports:
//!
//! - `alien_cloud_v1::send` takes a [`CloudRequest`] as JSON. The host picks the endpoint from a
//!   table of the cloud's own API endpoints, runs the deployment's interceptors on the request,
//!   signs it with the step's credentials, and returns a [`CloudResponse`] as JSON.
//! - `alien_http_v1::send` takes an [`HttpRequest`] as JSON, to an origin the extension declared.
//!   The host sends it with none of the deployment's credentials and returns a
//!   [`CloudResponse`].
//!
//! A request the host refuses or can't send comes back with the import's error bit set and a
//! line of text that starts `refused:`, for a call another try sends no differently, or
//! `failed:`, for one that was sent or may pass on another try. A step the module fails after a
//! call whose response an interceptor refused is not sent again, since the call happened. The
//! host follows no redirect: the module gets the 3xx.

mod egress;
mod endpoint;
mod send;

pub use egress::HttpRequest;
pub use endpoint::{CloudRequest, CloudResponse, MAX_REQUEST_BODY};

use super::protocol::{
    codes, CloudTarget, LoadOp, LoadRequest, LoadResponse, LoadResult, ResponseBody, StepRequest,
    StepResponse, PROTOCOL_VERSION,
};
use super::transport::{
    check_reply_version, parse_response, Transport, TransportFailure, CONNECT_TIMEOUT,
};
use super::HOST_GRACE;
use crate::error::{ErrorData, Result};
use alien_client_core::interceptor::InterceptorChain;
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::{ControllerModule, StepFailure, StepLimits};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore};

/// How many steps one deployment's controller modules run at once in a process. A step holds a
/// thread until its module answers, so this is the most threads one deployment can hold.
pub const STEPS_PER_DEPLOYMENT: usize = 2;

/// How many steps, loads and compiles one project's controller modules run at once in a process.
/// A quarter of the process's: a project with many deployments leaves the other three quarters
/// to the other projects, and still runs eight deployments' steps at once.
pub const STEPS_PER_PROJECT: usize = 16;

/// How many steps, loads and compiles all of a process's controller modules run at once. Each
/// holds a thread and up to a module's memory limit, so this bounds both for the process,
/// whatever the number of deployments.
pub const STEPS_PER_PROCESS: usize = 64;

/// How many requests one step may send through its two imports.
pub const CALLS_PER_STEP: usize = 50;

/// The largest response the host reads for a module. A larger one fails the call.
pub const MAX_RESPONSE_BYTES: usize = 8 << 20;

pub use super::transport::Interceptors;

/// What every controller module in a process shares: the client their cloud calls go out on,
/// and each deployment's and each project's limit. A process makes one and builds every
/// [`WasmTransport`] from it.
pub struct WasmHost {
    client: reqwest::Client,
    deployments: TenantLimits,
    projects: TenantLimits,
    running: Arc<Semaphore>,
    #[cfg(any(test, feature = "test-utils"))]
    testing: Testing,
}

/// What only a test changes.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
struct Testing {
    /// API origins whose requests go to a server on this machine instead, after every check
    /// and after signing.
    routes: HashMap<String, String>,
    local_egress: bool,
    fuel: Option<u64>,
}

impl std::fmt::Debug for WasmHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHost")
            .field("deployments", &self.deployments)
            .field("projects", &self.projects)
            .finish_non_exhaustive()
    }
}

impl WasmHost {
    /// This process's host. Every loader steps modules through it, so the limits on concurrent
    /// steps count all of the process's deployments.
    pub fn shared() -> Result<Arc<Self>> {
        static SHARED: std::sync::OnceLock<Arc<WasmHost>> = std::sync::OnceLock::new();
        if let Some(host) = SHARED.get() {
            return Ok(host.clone());
        }
        let host = Arc::new(Self::new()?);
        Ok(SHARED.get_or_init(|| host).clone())
    }

    pub fn new() -> Result<Self> {
        // A redirect would carry the signature or the token to wherever it points.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "the client for controller modules' cloud calls could not be built"
                    .to_string(),
                operation: None,
                resource_id: None,
            })?;
        Ok(Self {
            client,
            deployments: TenantLimits::new(STEPS_PER_DEPLOYMENT),
            projects: TenantLimits::new(STEPS_PER_PROJECT),
            running: Arc::new(Semaphore::new(STEPS_PER_PROCESS)),
            #[cfg(any(test, feature = "test-utils"))]
            testing: Testing::default(),
        })
    }

    /// Waits for one of `project`'s slots, then one of the process's. A step, a load and a
    /// compile of a controller module each hold a thread and a module's memory, so each runs
    /// in one. The wait is async and holds no thread.
    pub async fn slot(&self, project: &str) -> std::result::Result<ModuleSlot, AcquireError> {
        let project = self.projects.acquire(project).await?;
        let running = self.running.clone().acquire_owned().await?;
        Ok(ModuleSlot {
            _project: project,
            _running: running,
        })
    }

    /// The transport for extension `extension`'s controller `module` in a deployment of
    /// `project`, which may send requests to `egress`, the origins the extension declares, each
    /// written `https://host[:port]`. The project's deployments share its limit of steps.
    ///
    /// The transport keeps the compiled module. A host builds one when it steps a deployment
    /// and drops it after, so the module stays compiled only as long as the runtime's cache
    /// keeps it.
    pub fn transport(
        self: &Arc<Self>,
        project: &str,
        extension: &str,
        module: ControllerModule,
        egress: &[String],
    ) -> Result<WasmTransport> {
        let egress = egress
            .iter()
            .map(|origin| {
                egress::declared_origin(origin, self.local_egress()).map_err(|reason| {
                    AlienError::new(ErrorData::ExtensionBundleRefused {
                        extension: extension.to_string(),
                        object: format!("declared origin '{}'", origin.escape_debug()),
                        reason,
                    })
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(WasmTransport {
            host: self.clone(),
            project: project.into(),
            extension: extension.to_string(),
            module,
            egress: egress.into(),
            interceptors: Arc::new(|_, _| InterceptorChain::default()),
        })
    }

    fn local_egress(&self) -> bool {
        #[cfg(any(test, feature = "test-utils"))]
        return self.testing.local_egress;
        #[cfg(not(any(test, feature = "test-utils")))]
        false
    }

    /// Sends the requests for API origin `origin` (`https://sts.us-east-1.amazonaws.com`) to a
    /// test's server at `stub` instead, after every check and after signing.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn routing(mut self, origin: &str, stub: &str) -> Self {
        self.testing
            .routes
            .insert(origin.to_string(), stub.to_string());
        self
    }

    /// Admits declared origins over `http` on this machine, for a test's server.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn allowing_local_egress(mut self) -> Self {
        self.testing.local_egress = true;
        self
    }

    /// Another fuel budget per step, for tests of a step that spends it.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_step_fuel(mut self, fuel: u64) -> Self {
        self.testing.fuel = Some(fuel);
        self
    }

    /// Another limit of steps per deployment.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_steps_per_deployment(mut self, steps: usize) -> Self {
        self.deployments = TenantLimits::new(steps);
        self
    }

    /// Another limit of steps per project.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_steps_per_project(mut self, steps: usize) -> Self {
        self.projects = TenantLimits::new(steps);
        self
    }

    /// Another limit of steps for the process.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_steps_per_process(mut self, steps: usize) -> Self {
        self.running = Arc::new(Semaphore::new(steps));
        self
    }
}

/// Bounds how many steps each tenant's controller modules run at once, a tenant being a
/// deployment or a project. A module that loops or waits on a slow call holds a thread for the
/// rest of its step. Bounded per tenant, the threads it can hold are a few, and other tenants'
/// steps find theirs.
///
/// The wait for a slot is async and holds no thread.
#[derive(Debug)]
pub struct TenantLimits {
    per_tenant: usize,
    tenants: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

/// One of a tenant's slots, free again when dropped.
struct Slot {
    /// `None` while the step waits for it.
    permit: Option<OwnedSemaphorePermit>,
    tenant: String,
    tenants: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

/// One of a project's slots and one of the process's, free again when dropped.
pub struct ModuleSlot {
    _project: Slot,
    _running: OwnedSemaphorePermit,
}

impl TenantLimits {
    pub fn new(per_tenant: usize) -> Self {
        Self {
            per_tenant,
            tenants: Arc::default(),
        }
    }

    /// Waits for one of `tenant`'s slots.
    async fn acquire(&self, tenant: &str) -> std::result::Result<Slot, AcquireError> {
        let semaphore = self
            .tenants
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(tenant.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_tenant)))
            .clone();
        // Made before the wait, so a step dropped while it waits cleans up as one that ran.
        let mut slot = Slot {
            permit: None,
            tenant: tenant.to_string(),
            tenants: self.tenants.clone(),
        };
        slot.permit = Some(semaphore.acquire_owned().await?);
        Ok(slot)
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.tenants.lock().unwrap().len()
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut tenants = self.tenants.lock().unwrap_or_else(PoisonError::into_inner);
        self.permit.take();
        // Every step of the tenant that runs or waits holds the semaphore too. Once the map
        // alone does, the entry goes, so the map holds only tenants with a step.
        if tenants
            .get(&self.tenant)
            .is_some_and(|semaphore| Arc::strong_count(semaphore) == 1)
        {
            tenants.remove(&self.tenant);
        }
    }
}

/// Runs each step of one extension's controller module for one deployment.
#[derive(Clone)]
pub struct WasmTransport {
    host: Arc<WasmHost>,
    project: Arc<str>,
    extension: String,
    module: ControllerModule,
    egress: Arc<[String]>,
    interceptors: Interceptors,
}

impl std::fmt::Debug for WasmTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmTransport")
            .field("extension", &self.extension)
            .field("module", &self.module.sha256())
            .field("egress", &self.egress)
            .finish_non_exhaustive()
    }
}

impl WasmTransport {
    /// Runs the deployment's interceptors on every call the module sends to its cloud, as on
    /// Alien's own controllers' calls: on the request before it is signed, and on the response
    /// before the module reads it.
    pub fn with_interceptors(mut self, interceptors: Interceptors) -> Self {
        self.interceptors = interceptors;
        self
    }

    /// Asks the module to load `state` for `resource_type` on `platform`, without stepping and
    /// without a cloud call, as `StdioTransport::load` asks a controller process: the module
    /// gets no credentials, so any call it tries is refused.
    pub async fn load(
        &self,
        resource_type: &str,
        platform: Platform,
        state: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<LoadResult, TransportFailure> {
        let input = serde_json::to_vec(&LoadRequest {
            protocol: PROTOCOL_VERSION,
            op: LoadOp::Load,
            resource_type: resource_type.to_string(),
            platform,
            state,
        })
        .map_err(|e| TransportFailure::protocol_error(format!("cannot serialize the load: {e}")))?;
        let running = self
            .host
            .slot(&self.project)
            .await
            .map_err(|error| TransportFailure::unreachable(error.to_string()))?;
        let limits = self.limits(timeout);
        let capabilities = Box::new(send::StepCapabilities {
            runtime: tokio::runtime::Handle::current(),
            host: self.host.clone(),
            credentials: None,
            egress: Arc::from(Vec::new()),
            interceptors: self.interceptors.clone(),
            ends_at: Instant::now() + timeout,
            deadline_hit: Arc::default(),
            response_refused: Arc::default(),
            abandoned: Arc::default(),
            calls: 0,
        });
        let module = self.module.clone();
        let reply = tokio::task::spawn_blocking(move || {
            let _running = running;
            module.step(&input, &limits, capabilities)
        })
        .await
        .map_err(|error| {
            TransportFailure::unreachable(format!("the controller module's load stopped: {error}"))
        })?
        .map_err(|failure| {
            TransportFailure::protocol_error(format!(
                "the controller module could not answer the load: {failure:?}"
            ))
        })?;
        let raw: serde_json::Value = serde_json::from_slice(&reply)
            .map_err(|e| TransportFailure::protocol_error(format!("the reply is not JSON: {e}")))?;
        check_reply_version(&raw)?;
        let response: LoadResponse = serde_json::from_value(raw).map_err(|e| {
            TransportFailure::protocol_error(format!("the reply is not a load reply: {e}"))
        })?;
        Ok(response.result)
    }

    fn limits(&self, deadline: Duration) -> StepLimits {
        #[allow(unused_mut)]
        let mut limits = StepLimits::within(deadline);
        #[cfg(any(test, feature = "test-utils"))]
        if let Some(fuel) = self.host.testing.fuel {
            limits.fuel = fuel;
        }
        limits
    }
}

/// A step as a controller module gets it: the request without its credentials, and where its
/// cloud calls go.
#[derive(Serialize)]
struct ModuleStep<'a> {
    #[serde(flatten)]
    request: &'a StepRequest,
    cloud: Option<CloudTarget>,
}

#[async_trait]
impl Transport for WasmTransport {
    fn intercepted(&self, interceptors: Interceptors) -> Option<Arc<dyn Transport>> {
        Some(Arc::new(self.clone().with_interceptors(interceptors)))
    }

    async fn step(
        &self,
        request: &StepRequest,
        timeout: Duration,
    ) -> std::result::Result<StepResponse, TransportFailure> {
        // As a controller process gets it: the module's deadline is counted from when it can
        // start on the step. Its calls end there, so it can still answer, and it is stopped
        // `HOST_GRACE` later if it has not.
        let Some(deadline) = timeout
            .checked_sub(HOST_GRACE)
            .filter(|deadline| !deadline.is_zero())
        else {
            return Err(TransportFailure::protocol_error(format!(
                "a step's timeout of {timeout:?} leaves the controller module no time before the host's grace of {HOST_GRACE:?}"
            )));
        };
        let slot = self
            .host
            .deployments
            .acquire(&request.deployment_id)
            .await
            .map_err(|error| TransportFailure::unreachable(error.to_string()))?;
        // After the deployment's own slot, so a deployment that floods steps waits on itself
        // and holds at most its few of its project's and the process's slots.
        let running = self
            .host
            .slot(&self.project)
            .await
            .map_err(|error| TransportFailure::unreachable(error.to_string()))?;

        let mut step = request.clone();
        let credentials = step.credentials.take();
        step.deadline = (chrono::Utc::now()
            + chrono::Duration::from_std(deadline).unwrap_or(chrono::Duration::MAX))
        .to_rfc3339();
        let input = serde_json::to_vec(&ModuleStep {
            cloud: credentials.as_ref().map(CloudTarget::from),
            request: &step,
        })
        .map_err(|e| TransportFailure::protocol_error(format!("cannot serialize the step: {e}")))?;

        let limits = self.limits(timeout);
        let deadline_hit = Arc::new(AtomicBool::new(false));
        let response_refused = Arc::new(AtomicBool::new(false));
        // The runner drops a step it no longer waits for, as on a lost lease, while the module
        // runs on: from then on its calls are refused.
        let abandoned = SetOnDrop(Arc::new(AtomicBool::new(false)));
        let capabilities = Box::new(send::StepCapabilities {
            runtime: tokio::runtime::Handle::current(),
            host: self.host.clone(),
            credentials,
            egress: self.egress.clone(),
            interceptors: self.interceptors.clone(),
            ends_at: Instant::now() + deadline,
            deadline_hit: deadline_hit.clone(),
            response_refused: response_refused.clone(),
            abandoned: abandoned.0.clone(),
            calls: 0,
        });
        let module = self.module.clone();
        let stepped = tokio::task::spawn_blocking(move || {
            // Held until the module stops, also when the caller gave up on the step first.
            let _slots = (slot, running);
            module.step(&input, &limits, capabilities)
        })
        .await;
        let deadline_hit = deadline_hit.load(Ordering::Relaxed);
        let mut answer = match stepped {
            Ok(Ok(reply)) => parse_response(&String::from_utf8_lossy(&reply), request),
            Ok(Err(failure)) => Err(step_failure(failure, deadline, deadline_hit, &limits)),
            Err(error) => Err(TransportFailure::unreachable(format!(
                "the controller module's step stopped: {error}"
            ))),
        };
        // The module's view stands. A failure after a call ran out of time is the deadline's,
        // which the step is sent again for.
        if let Ok(StepResponse {
            body: ResponseBody::Failed { error, .. },
            ..
        }) = &mut answer
        {
            if deadline_hit {
                error.message = format!(
                    "a call ran out of the step's {deadline:?} deadline, and the controller module failed ({}): {}",
                    error.code, error.message
                );
                error.code = codes::STEP_DEADLINE.to_string();
                error.retryable = true;
            }
        }
        // However the step ends after a call whose response was refused, it is final: the call
        // was sent, and a resend would repeat it.
        if response_refused.load(Ordering::Relaxed) {
            match &mut answer {
                Ok(StepResponse {
                    body: ResponseBody::Failed { error, .. },
                    ..
                }) => error.retryable = false,
                Ok(_) => {}
                Err(failure) => failure.retryable = false,
            }
        }
        answer
    }
}

/// Sets its flag when dropped.
struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// The failure of a step the module did not answer, after one of its calls ran out of time when
/// `deadline_hit`.
fn step_failure(
    failure: StepFailure,
    deadline: Duration,
    deadline_hit: bool,
    limits: &StepLimits,
) -> TransportFailure {
    match failure {
        // Once a call ran out of the step's time, the module's next path depended on that call,
        // and which limit stops it first is a race: however it stops, the step is sent again, as
        // one the module never answered.
        StepFailure::Fuel | StepFailure::Deadline | StepFailure::Memory(_) | StepFailure::Trap(_)
            if deadline_hit =>
        {
            TransportFailure {
                code: codes::STEP_DEADLINE,
                message: format!(
                    "a call of the controller module was not answered within the step's {deadline:?} deadline"
                ),
                retryable: true,
            }
        }
        // The same step spends the same fuel again.
        StepFailure::Fuel => TransportFailure {
            code: codes::STEP_DEADLINE,
            message: "the controller module ran more computation than one step may, and was stopped; it would stop at the same point again".to_string(),
            retryable: false,
        },
        StepFailure::Output(bytes) => TransportFailure::protocol_error(format!(
            "the controller module answered with {bytes} bytes, more than {} MiB",
            limits.output_bytes >> 20
        )),
        StepFailure::NoReply(message) => TransportFailure::protocol_error(format!(
            "the controller module could not answer the step: {message}"
        )),
        StepFailure::Deadline => TransportFailure {
            code: codes::STEP_DEADLINE,
            message: format!(
                "the controller module had not answered {HOST_GRACE:?} after the step's {deadline:?} deadline and was stopped"
            ),
            retryable: true,
        },
        // As a controller process that crashed: the next try starts a fresh instance.
        StepFailure::Memory(message) => TransportFailure::unreachable(format!(
            "the controller module needed more than its {} bytes of memory: {message}",
            limits.memory_bytes
        )),
        StepFailure::Trap(message) => {
            TransportFailure::unreachable(format!("the controller module trapped: {message}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map of deployments holds one only while a step of it runs or waits, whether the
    /// step ran or was dropped while it waited.
    #[tokio::test]
    async fn a_deployment_s_limit_is_dropped_with_its_last_step() {
        let limits = TenantLimits::new(1);
        let running = limits.acquire("dep_a").await.unwrap();
        let other = limits.acquire("dep_b").await.unwrap();
        assert_eq!(limits.tracked(), 2);

        // A second step of the deployment waits, and is dropped before it gets the slot.
        let waiting = tokio::time::timeout(Duration::from_millis(50), limits.acquire("dep_a"));
        assert!(waiting.await.is_err());
        assert_eq!(limits.tracked(), 2);

        drop(running);
        assert_eq!(limits.tracked(), 1);
        drop(other);
        assert_eq!(limits.tracked(), 0);

        // The slot is free again for the deployment's next step.
        let again = limits.acquire("dep_a").await.unwrap();
        assert_eq!(limits.tracked(), 1);
        drop(again);
        assert_eq!(limits.tracked(), 0);
    }

    #[test]
    fn a_step_s_cloud_names_no_secret() {
        let credentials = super::super::protocol::Credentials::Aws {
            account_id: "123456789012".into(),
            region: "eu-west-1".into(),
            access_key_id: "ASIAKEYID".into(),
            secret_access_key: "the-secret".into(),
            session_token: Some("the-session-token".into()),
            expires_at: None,
        };
        assert_eq!(
            serde_json::to_value(CloudTarget::from(&credentials)).unwrap(),
            serde_json::json!({
                "platform": "aws", "accountId": "123456789012", "region": "eu-west-1"
            })
        );
    }
}
