//! The host side of the step protocol: the controller Alien's executor runs for a custom
//! resource. Each `step` goes to the extension's controller through a [`Transport`], and the
//! executor's synchronous calls between steps are answered from the view in the last reply.

use super::protocol::{
    codes, ControllerView, Credentials, DependencyView, ResponseBody, StepRequest, Transition,
    TransitionPreview, PROTOCOL_VERSION,
};
use super::transport::{Transport, TransportFailure};
use crate::core::{
    ControllerFactory, ResourceController, ResourceControllerContext, ResourceControllerStepResult,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    DeploymentConfig, EnvironmentVariableType, ResourceOutputs, ResourceStatus, StackSettings,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

/// The type tag of the proxy's saved state.
pub const PROXY_CONTROLLER_TYPE: &str = "ExtensionProxyController";

/// How long a controller has for one step.
pub const STEP_DEADLINE: Duration = Duration::from_secs(60);

/// How much longer than the deadline the host waits before it gives up on a reply. The
/// controller cuts its own work at the deadline and answers; this covers one that doesn't.
pub const HOST_GRACE: Duration = Duration::from_secs(2);

/// How long a controller has to finish a delete before its resource ends DeleteFailed: the
/// longest delete wait of Alien's own controllers (180 polls 10 seconds apart). A wall-clock
/// bound, not a step count, since the controller picks how soon it is stepped again.
pub const DELETE_TIMEOUT: chrono::Duration = chrono::Duration::minutes(30);

/// Where a step's `credentials` come from. The host picks one per way of running a deployment:
/// credentials minted for the extension's identity, the admin's on a local pipe, and so on.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait CredentialSource: Send + Sync + Debug {
    async fn credentials(&self, ctx: &ResourceControllerContext<'_>) -> Result<Credentials>;
}

/// What a deployment's proxies reach their controller with.
#[derive(Debug, Clone)]
struct Endpoint {
    deployment_id: String,
    transport: Arc<dyn Transport>,
    /// `None` sends `credentials: null`, and the controller uses its own environment.
    credentials: Option<Arc<dyn CredentialSource>>,
    deadline: Duration,
}

/// Creates the proxy for one custom type and platform in one deployment's registry.
///
/// The executor rebuilds a controller from its saved JSON on every step, which can't carry a
/// transport. A rebuilt proxy asks the step's registry for a fresh one from this factory and
/// takes its endpoint, so two deployments with the same extension reach their own controllers.
#[derive(Debug, Clone)]
pub struct ProxyFactory {
    endpoint: Endpoint,
}

impl ProxyFactory {
    pub fn new(
        deployment_id: impl Into<String>,
        transport: Arc<dyn Transport>,
        credentials: Option<Arc<dyn CredentialSource>>,
    ) -> Self {
        Self {
            endpoint: Endpoint {
                deployment_id: deployment_id.into(),
                transport,
                credentials,
                deadline: STEP_DEADLINE,
            },
        }
    }

    /// A shorter step deadline, for tests of what happens when a controller misses it.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_step_deadline(mut self, deadline: Duration) -> Self {
        self.endpoint.deadline = deadline;
        self
    }
}

impl ControllerFactory for ProxyFactory {
    fn create(&self) -> Box<dyn ResourceController> {
        Box::new(ExtensionProxyController {
            // Alien's own controllers start in their first handler's status.
            status: ResourceStatus::Provisioning,
            remote: None,
            pending: Vec::new(),
            in_flight_step_id: None,
            pending_import: None,
            imported_outputs: None,
            delete_started_at: None,
            endpoint: Some(self.endpoint.clone()),
        })
    }
}

/// Saved as a custom resource's `internal_state`, inside alien-infra's own envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionProxyController {
    /// What `get_status` reports: the last reply's status, or one derived here after a
    /// transition the controller hasn't seen yet.
    status: ResourceStatus,
    /// The last reply's view. `None` until the first reply.
    remote: Option<ControllerView>,
    /// Transitions started since the last reply, sent with the next request.
    pending: Vec<Transition>,
    /// The id of a request that got no reply. The resend carries it, so a controller that
    /// already ran the step answers from its replay cache.
    in_flight_step_id: Option<String>,
    /// An importer's fields, sent with the first step for the controller to load.
    pending_import: Option<Value>,
    /// The importer's outputs, the resource's outputs until the first reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    imported_outputs: Option<ResourceOutputs>,
    /// When the delete this resource is in started. No reply resets it, only an update or a
    /// retry the executor starts, so a controller gets no fresh [`DELETE_TIMEOUT`] by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delete_started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip)]
    endpoint: Option<Endpoint>,
}

impl ExtensionProxyController {
    /// The proxy for a resource an importer filled: the controller loads `fields` on the first
    /// step, so storing the import never waits on the controller. `outputs` are the resource's
    /// until a reply brings the controller's.
    pub fn imported(
        fields: Value,
        status: ResourceStatus,
        outputs: Option<ResourceOutputs>,
    ) -> Self {
        Self {
            status,
            remote: None,
            pending: Vec::new(),
            in_flight_step_id: None,
            pending_import: Some(fields),
            imported_outputs: outputs,
            delete_started_at: None,
            endpoint: None,
        }
    }

    /// The endpoint of the factory this deployment's registry holds for the resource.
    fn endpoint(&self, ctx: &ResourceControllerContext<'_>) -> Result<Endpoint> {
        if let Some(endpoint) = &self.endpoint {
            return Ok(endpoint.clone());
        }
        let resource_type = ctx.desired_config.resource_type();
        let fresh = ctx
            .registry
            .get_controller(resource_type.clone(), ctx.platform)?;
        fresh
            .as_any()
            .downcast_ref::<ExtensionProxyController>()
            .and_then(|proxy| proxy.endpoint.clone())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ControllerNotAvailable {
                    resource_type,
                    platform: ctx.platform,
                })
            })
    }

    /// The status a transition leads to, or the controller's refusal of it. A preview holds for
    /// the state in the view. Once a transition is pending, the controller has moved past that
    /// state, so the status is derived here instead.
    fn preview(
        &self,
        pick: fn(&ControllerView) -> &TransitionPreview,
        derived: ResourceStatus,
    ) -> std::result::Result<ResourceStatus, String> {
        match self.remote.as_ref() {
            Some(view) if self.pending.is_empty() => match pick(view) {
                TransitionPreview::Allowed { status } => Ok(*status),
                TransitionPreview::Refused { message } => Err(message.clone()),
            },
            _ => Ok(derived),
        }
    }

    /// A new transition makes the next request a new step, so it gets a new id.
    fn record(&mut self, transition: Transition, status: ResourceStatus) {
        self.pending.push(transition);
        self.in_flight_step_id = None;
        self.status = status;
    }

    /// During a delete only `deleted` and `delete-failed` move the status off Deleting, as with
    /// Alien's own delete handlers; any other status would let the executor treat the resource
    /// as not deleting and drop the delete's deadline.
    fn adopt(&mut self, view: ControllerView) {
        self.status = match view.status {
            ResourceStatus::Deleted | ResourceStatus::DeleteFailed => view.status,
            _ if self.delete_started_at.is_some() => ResourceStatus::Deleting,
            status => status,
        };
        self.remote = Some(view);
        self.pending.clear();
        self.in_flight_step_id = None;
        self.pending_import = None;
        self.imported_outputs = None;
    }

    async fn request(
        &self,
        ctx: &ResourceControllerContext<'_>,
        endpoint: &Endpoint,
        step_id: String,
    ) -> Result<StepRequest> {
        let resource_id = ctx.desired_config.id().to_string();
        let own = ctx.state.resources.get(&resource_id);
        let mut dependencies = BTreeMap::new();
        for dependency in own.iter().flat_map(|state| &state.dependencies) {
            let Some(state) = ctx.state.resources.get(dependency.id()) else {
                continue;
            };
            dependencies.insert(
                dependency.id().to_string(),
                DependencyView {
                    resource_type: state.resource_type.clone(),
                    status: state.status,
                    outputs: state.outputs.as_ref().map(to_json).transpose()?,
                    binding_params: state.remote_binding_params.clone(),
                },
            );
        }
        let credentials = match &endpoint.credentials {
            Some(source) => Some(source.credentials(ctx).await?),
            None => None,
        };
        let deadline = chrono::Utc::now()
            + chrono::Duration::from_std(endpoint.deadline).unwrap_or(chrono::Duration::MAX);
        Ok(StepRequest {
            protocol: PROTOCOL_VERSION,
            step_id,
            deployment_id: endpoint.deployment_id.clone(),
            resource_id,
            resource_type: ctx.desired_config.resource_type().to_string(),
            platform: ctx.platform,
            resource_prefix: ctx.resource_prefix.to_string(),
            initial_setup_authority: ctx.initial_setup_authority,
            lifecycle: own.and_then(|state| state.lifecycle),
            resource: to_json(ctx.desired_config)?,
            previous_resource: own
                .and_then(|state| state.previous_config.as_ref())
                .map(to_json)
                .transpose()?,
            dependencies,
            deployment_config: to_json(&deployment_config_for_extension(ctx.deployment_config))?,
            setup_run: ctx.deployment_config.setup_run,
            setup_run_id: ctx.deployment_config.setup_run_id.clone(),
            state: self.remote.as_ref().map(|view| view.state.clone()),
            pending_import: self.pending_import.clone(),
            transitions: self.pending.clone(),
            credentials,
            deadline: deadline.to_rfc3339(),
        })
    }
}

/// Codes of Alien's own the executor acts on, which a controller's failed step may carry: a
/// dependency that is not ready makes the step wait, and a resource already gone ends a delete.
const DEPENDENCY_NOT_READY: &str = "DEPENDENCY_NOT_READY";
const REMOTE_RESOURCE_NOT_FOUND: &str = "REMOTE_RESOURCE_NOT_FOUND";

/// The shortest a running custom resource waits between steps: as long as Alien's own running
/// resources wait between heartbeats.
pub const READY_STEP_DELAY: Duration = Duration::from_secs(30);

/// The deployment's settings as an extension's controller gets them. A controller may run on
/// a server a third party hosts, so only the settings it can act on go: no secrets, nothing that
/// reaches Alien itself, and no held values. Every field is named here, so a field added to
/// `DeploymentConfig` doesn't reach a controller until someone decides it should.
pub fn deployment_config_for_extension(config: &DeploymentConfig) -> DeploymentConfig {
    let DeploymentConfig {
        setup_run,
        setup_run_id,
        deployment_name,
        stack_settings,
        management_config,
        // Picks the deployment's managing-role trust.
        management_external_id: _,
        gcp_federation_scope,
        mut environment_variables,
        // Inputs the manager holds, secret ones and generated secrets among them.
        input_values: _,
        value_slots: _,
        stored_secret_input_ids: _,
        allow_frozen_changes,
        // The compute backend's management tokens.
        compute_backend: _,
        // Bindings can hold credentials inline; `stack_settings` carries a copy, cleared below.
        external_bindings: _,
        base_platform,
        label_domain,
        observe_label_selector,
        observe_all_namespaces,
        public_endpoints,
        // Managed certificates.
        domain_metadata: _,
        // The log export's auth header.
        monitoring: _,
        manager_url,
        // The deployment's registry token.
        deployment_token: _,
        native_image_host,
        volume_restores,
        deletion_policy,
    } = config.clone();
    environment_variables
        .variables
        .retain(|variable| variable.var_type == EnvironmentVariableType::Plain);
    DeploymentConfig {
        setup_run,
        setup_run_id,
        deployment_name,
        stack_settings: stack_settings_for_extension(&stack_settings),
        management_config,
        management_external_id: None,
        gcp_federation_scope,
        environment_variables,
        input_values: Default::default(),
        value_slots: Vec::new(),
        stored_secret_input_ids: None,
        allow_frozen_changes,
        compute_backend: None,
        external_bindings: Default::default(),
        base_platform,
        label_domain,
        observe_label_selector,
        observe_all_namespaces,
        public_endpoints,
        domain_metadata: None,
        monitoring: None,
        manager_url,
        deployment_token: None,
        native_image_host,
        volume_restores,
        deletion_policy,
    }
}

/// The stack settings as an extension's hooks and controller get them: without the external
/// bindings, which can hold credentials inline. Every field is named here, as in
/// [`deployment_config_for_extension`].
pub fn stack_settings_for_extension(settings: &StackSettings) -> StackSettings {
    let StackSettings {
        setup_management,
        quota_warning_overrides,
        endpoint_access,
        network,
        domains,
        kubernetes,
        compute,
        public_endpoints,
        deployment_model,
        airgapped,
        updates,
        telemetry,
        heartbeats,
        external_bindings: _,
    } = settings.clone();
    StackSettings {
        setup_management,
        quota_warning_overrides,
        endpoint_access,
        network,
        domains,
        kubernetes,
        compute,
        public_endpoints,
        deployment_model,
        airgapped,
        updates,
        telemetry,
        heartbeats,
        external_bindings: None,
    }
}

fn outputs_type(outputs: &ResourceOutputs) -> alien_core::ResourceType {
    outputs.as_resource_outputs().get_resource_type()
}

fn to_json(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value)
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: "cannot serialize the step request".to_string(),
            resource_id: None,
        })
}

/// An error with the step's own code and retryable flag, which the executor reads to retry
/// or fail the resource.
fn step_error(
    resource_id: &str,
    code: &str,
    message: String,
    retryable: bool,
) -> AlienError<ErrorData> {
    let mut error = AlienError::new(ErrorData::ExtensionStepFailed {
        resource_id: resource_id.to_string(),
        message,
    });
    error.code = code.to_string();
    error.retryable = retryable;
    error
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ResourceController for ExtensionProxyController {
    fn controller_type(&self) -> &'static str {
        PROXY_CONTROLLER_TYPE
    }

    fn to_json_value(&self) -> serde_json::Result<Value> {
        serde_json::to_value(self)
    }

    async fn step(
        &mut self,
        ctx: &ResourceControllerContext,
    ) -> Result<ResourceControllerStepResult> {
        let resource_id = ctx.desired_config.id().to_string();
        let resource_type = ctx.desired_config.resource_type();
        let deleting = self.status == ResourceStatus::Deleting
            || self.pending.iter().any(|transition| {
                matches!(
                    transition,
                    Transition::DeleteStart | Transition::TeardownStart
                )
            });
        let delete_time_left = if deleting {
            let started = *self.delete_started_at.get_or_insert_with(chrono::Utc::now);
            let left = (started + DELETE_TIMEOUT - chrono::Utc::now())
                .to_std()
                .ok()
                .filter(|left| !left.is_zero());
            if left.is_none() {
                return Err(AlienError::new(ErrorData::ExtensionStepFailed {
                    resource_id,
                    message: format!(
                        "the controller did not finish deleting the resource within {} minutes; a retry gives it another {}",
                        DELETE_TIMEOUT.num_minutes(),
                        DELETE_TIMEOUT.num_minutes()
                    ),
                }));
            }
            left
        } else {
            self.delete_started_at = None;
            None
        };
        let endpoint = self.endpoint(ctx)?;
        let step_id = self
            .in_flight_step_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        if let Err(TransportFailure {
            code,
            message,
            retryable,
        }) = endpoint.transport.ready().await
        {
            return Err(step_error(&resource_id, code, message, retryable));
        }
        let request = self.request(ctx, &endpoint, step_id).await?;
        let response = match endpoint
            .transport
            .step(&request, endpoint.deadline + HOST_GRACE)
            .await
        {
            Ok(response) => response,
            Err(TransportFailure {
                code,
                message,
                retryable,
            }) => {
                // A lost reply keeps the step id, so the resend can be answered from the
                // controller's replay cache. Any other failure fails the step for good.
                if !retryable {
                    self.in_flight_step_id = None;
                }
                return Err(step_error(&resource_id, code, message, retryable));
            }
        };
        let outputs = match &response.body {
            ResponseBody::Stepped { controller, .. } | ResponseBody::Failed { controller, .. } => {
                controller.outputs.as_ref()
            }
            ResponseBody::Refused { .. } => None,
        };
        if let Some(other) = outputs.map(outputs_type).filter(|t| *t != resource_type) {
            self.in_flight_step_id = None;
            return Err(step_error(
                &resource_id,
                codes::PROTOCOL_ERROR,
                format!(
                    "the controller answered with outputs of type '{other}' for a resource of type '{resource_type}'"
                ),
                false,
            ));
        }
        if let ResponseBody::Stepped { heartbeats, .. } = &response.body {
            let foreign = heartbeats.iter().find(|heartbeat| {
                heartbeat.resource_id != resource_id
                    || heartbeat.resource_type != resource_type
                    || heartbeat
                        .deployment_id
                        .as_ref()
                        .is_some_and(|id| *id != endpoint.deployment_id)
            });
            if let Some(heartbeat) = foreign {
                self.in_flight_step_id = None;
                return Err(step_error(
                    &resource_id,
                    codes::PROTOCOL_ERROR,
                    format!(
                        "the controller reported a heartbeat for resource '{}' of type '{}' in deployment {:?}; a controller reports only for the resource it steps",
                        heartbeat.resource_id, heartbeat.resource_type, heartbeat.deployment_id
                    ),
                    false,
                ));
            }
        }
        match response.body {
            ResponseBody::Refused {
                error,
                supported_protocols: Some(supported),
            } if !supported.contains(&PROTOCOL_VERSION) => {
                self.in_flight_step_id = None;
                Err(step_error(
                    &resource_id,
                    codes::PROTOCOL_UNSUPPORTED,
                    format!(
                        "the controller speaks step protocols {supported:?} but this deployment speaks {PROTOCOL_VERSION}: {}",
                        error.message
                    ),
                    false,
                ))
            }
            ResponseBody::Refused { error, .. } => {
                self.in_flight_step_id = None;
                Err(step_error(
                    &resource_id,
                    codes::STEP_REFUSED,
                    format!(
                        "the controller refused the step ({}): {}",
                        error.code, error.message
                    ),
                    false,
                ))
            }
            ResponseBody::Stepped {
                controller,
                suggested_delay_ms,
                heartbeats,
            } => {
                self.adopt(controller);
                for heartbeat in heartbeats {
                    ctx.emit_heartbeat(heartbeat);
                }
                // A running resource stepped sooner would keep the whole deployment polling; it
                // waits at least as long as Alien's own running resources do.
                let suggested_delay = match suggested_delay_ms.map(Duration::from_millis) {
                    delay if self.status == ResourceStatus::Running => {
                        Some(delay.unwrap_or_default().max(READY_STEP_DELAY))
                    }
                    delay => delay,
                };
                // A longer wait would hold the delete past its deadline.
                let suggested_delay = match delete_time_left {
                    Some(left) => suggested_delay.map(|delay| delay.min(left)),
                    None => suggested_delay,
                };
                Ok(ResourceControllerStepResult { suggested_delay })
            }
            ResponseBody::Failed {
                mut controller,
                error,
            } => {
                let deleting = self.status == ResourceStatus::Deleting;
                // A failed step keeps what it didn't replace: the outputs the resource had, and
                // an importer's fields the controller didn't load, which the next step carries
                // again.
                if controller.outputs.is_none() {
                    controller.outputs = self.get_outputs();
                }
                let unloaded = if controller.state.is_null() {
                    self.pending_import.take()
                } else {
                    None
                };
                self.adopt(controller);
                self.pending_import = unloaded;
                // Alien acts on its own codes, so a controller keeps only those it may answer
                // with: its refusal and its deadline, a dependency not ready yet outside a
                // delete, and a resource already gone while it deletes. A delete waits on no
                // dependency, and the executor waits on this code without spending the retry
                // budget, so a delete answering it would never end. Any other code goes in the
                // message, so none passes for a failure of the host's own, such as an
                // unreachable controller.
                let kept = match error.code.as_str() {
                    codes::STEP_REFUSED => Some(false),
                    codes::STEP_DEADLINE => Some(error.retryable),
                    DEPENDENCY_NOT_READY if !deleting => Some(error.retryable),
                    REMOTE_RESOURCE_NOT_FOUND if deleting => Some(false),
                    _ => None,
                };
                if let Some(retryable) = kept {
                    return Err(step_error(
                        &resource_id,
                        &error.code,
                        error.message,
                        retryable,
                    ));
                }
                Err(step_error(
                    &resource_id,
                    "EXTENSION_STEP_FAILED",
                    format!("the controller failed ({}): {}", error.code, error.message),
                    error.retryable,
                ))
            }
        }
    }

    fn transition_to_failure(&mut self) {
        let status = match self.remote.as_ref() {
            // A failed delete is DeleteFailed whatever the view names, as with every delete
            // handler of Alien's own; a controller can't turn it into a status that keeps the
            // delete waiting.
            _ if self.delete_started_at.is_some() => ResourceStatus::DeleteFailed,
            Some(view) if self.pending.is_empty() => view.on_failure_status,
            _ => match self.status {
                ResourceStatus::Pending | ResourceStatus::Provisioning => {
                    ResourceStatus::ProvisionFailed
                }
                ResourceStatus::Updating => ResourceStatus::UpdateFailed,
                ResourceStatus::Deleting => ResourceStatus::DeleteFailed,
                ResourceStatus::Running => ResourceStatus::RefreshFailed,
                failed_or_gone => failed_or_gone,
            },
        };
        self.record(Transition::Failure, status);
    }

    /// The customer's delete starts even when the extension's controller would refuse it: the
    /// controller gets the transition with its next step, and its refusal fails that step, so
    /// this resource ends DeleteFailed with the controller's message while the rest of the
    /// stack deletes. The resources this one depends on stay until it is deleted, since Alien
    /// deletes a resource only after everything that depends on it.
    fn transition_to_delete_start(&mut self) -> Result<()> {
        let status = self
            .preview(|view| &view.delete_start, ResourceStatus::Deleting)
            .unwrap_or(ResourceStatus::Deleting);
        self.record(Transition::DeleteStart, status);
        Ok(())
    }

    /// As [`Self::transition_to_delete_start`]: a teardown is the customer's to start.
    fn transition_to_teardown_start(&mut self) -> Result<()> {
        let status = self
            .preview(|view| &view.teardown_start, ResourceStatus::Deleting)
            .unwrap_or(ResourceStatus::Deleting);
        self.record(Transition::TeardownStart, status);
        Ok(())
    }

    fn transition_to_update(&mut self) -> Result<()> {
        let status = self
            .preview(|view| &view.update_start, ResourceStatus::Updating)
            .map_err(|message| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message,
                    resource_id: None,
                })
            })?;
        self.record(Transition::UpdateStart, status);
        Ok(())
    }

    fn get_status(&self) -> ResourceStatus {
        self.status
    }

    fn get_progress(&self) -> Option<&str> {
        self.remote.as_ref()?.progress.as_deref()
    }

    fn get_outputs(&self) -> Option<ResourceOutputs> {
        match &self.remote {
            Some(view) => view.outputs.clone(),
            None => self.imported_outputs.clone(),
        }
    }

    fn get_binding_params(&self) -> Result<Option<Value>> {
        Ok(self
            .remote
            .as_ref()
            .and_then(|view| view.binding_params.clone()))
    }

    /// A retry also gives a delete a fresh [`DELETE_TIMEOUT`], as it gives Alien's own
    /// controllers a fresh polling budget.
    fn reset_stay_count(&mut self) {
        self.delete_started_at = None;
        let status = self.status;
        self.record(Transition::ResetStayCount, status);
    }

    fn box_clone(&self) -> Box<dyn ResourceController> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The view a controller in its Ready state answers with.
    fn ready_view() -> ControllerView {
        ControllerView {
            state: json!({ "state": "ready" }),
            status: ResourceStatus::Running,
            progress: None,
            outputs: None,
            binding_params: None,
            on_failure_status: ResourceStatus::RefreshFailed,
            update_start: TransitionPreview::Allowed {
                status: ResourceStatus::Updating,
            },
            delete_start: TransitionPreview::Allowed {
                status: ResourceStatus::Deleting,
            },
            teardown_start: TransitionPreview::Allowed {
                status: ResourceStatus::Deleting,
            },
        }
    }

    fn proxy(view: Option<ControllerView>, status: ResourceStatus) -> ExtensionProxyController {
        ExtensionProxyController {
            status,
            remote: view,
            pending: Vec::new(),
            in_flight_step_id: Some("lost-reply".to_string()),
            pending_import: None,
            imported_outputs: None,
            delete_started_at: None,
            endpoint: None,
        }
    }

    fn ready() -> ExtensionProxyController {
        proxy(Some(ready_view()), ResourceStatus::Running)
    }

    #[test]
    fn with_nothing_pending_the_view_answers() {
        let mut ready = ready();
        ready.transition_to_failure();
        assert_eq!(ready.get_status(), ResourceStatus::RefreshFailed);

        let mut updating = self::ready();
        updating.transition_to_update().unwrap();
        assert_eq!(updating.get_status(), ResourceStatus::Updating);
        assert_eq!(updating.pending, vec![Transition::UpdateStart]);
    }

    /// The view describes the controller before a pending transition, so its failure status
    /// is stale: a failed update of a Ready resource is UpdateFailed, as Alien's own
    /// controllers report it, never the RefreshFailed the Ready view names.
    #[test]
    fn with_a_transition_pending_the_failure_status_follows_the_local_status() {
        let mut update = ready();
        update.transition_to_update().unwrap();
        update.transition_to_failure();
        assert_eq!(update.get_status(), ResourceStatus::UpdateFailed);
        assert_eq!(
            update.pending,
            vec![Transition::UpdateStart, Transition::Failure]
        );

        let mut delete = ready();
        delete.transition_to_delete_start().unwrap();
        delete.transition_to_failure();
        assert_eq!(delete.get_status(), ResourceStatus::DeleteFailed);

        let mut teardown = ready();
        teardown.transition_to_teardown_start().unwrap();
        teardown.transition_to_failure();
        assert_eq!(teardown.get_status(), ResourceStatus::DeleteFailed);

        let mut retried = ready();
        retried.reset_stay_count();
        retried.transition_to_failure();
        assert_eq!(retried.get_status(), ResourceStatus::RefreshFailed);
    }

    /// Before the first reply there is no view to ask.
    #[test]
    fn without_a_view_the_failure_status_follows_the_local_status() {
        for (status, failed) in [
            (
                ResourceStatus::Provisioning,
                ResourceStatus::ProvisionFailed,
            ),
            (ResourceStatus::Running, ResourceStatus::RefreshFailed),
            (ResourceStatus::Updating, ResourceStatus::UpdateFailed),
            (ResourceStatus::Deleting, ResourceStatus::DeleteFailed),
            (ResourceStatus::UpdateFailed, ResourceStatus::UpdateFailed),
        ] {
            let mut fresh = proxy(None, status);
            fresh.transition_to_failure();
            assert_eq!(fresh.get_status(), failed, "from {status:?}");
        }

        let mut imported = ExtensionProxyController::imported(
            json!({ "state": "ready" }),
            ResourceStatus::Running,
            None,
        );
        imported.transition_to_update().unwrap();
        assert_eq!(imported.get_status(), ResourceStatus::Updating);
        imported.transition_to_failure();
        assert_eq!(imported.get_status(), ResourceStatus::UpdateFailed);
    }

    #[test]
    fn a_refused_preview_refuses_the_transition_and_records_nothing() {
        let mut view = ready_view();
        view.update_start = TransitionPreview::Refused {
            message: "Cannot transition to update from state: CreateWait".to_string(),
        };
        let mut creating = proxy(Some(view), ResourceStatus::Provisioning);
        let error = creating.transition_to_update().unwrap_err();
        assert!(error.message.contains("CreateWait"), "{}", error.message);
        assert_eq!(creating.get_status(), ResourceStatus::Provisioning);
        assert!(creating.pending.is_empty());
        assert_eq!(creating.in_flight_step_id.as_deref(), Some("lost-reply"));
    }

    /// A controller can't keep a customer from starting a delete or teardown of its resource;
    /// its refusal reaches it with the next step (see `step_protocol_edges.rs`).
    #[test]
    fn a_refused_delete_or_teardown_is_recorded_for_the_controller_to_answer() {
        for teardown in [false, true] {
            let mut view = ready_view();
            let refused = TransitionPreview::Refused {
                message: "the vendor says no".to_string(),
            };
            view.delete_start = refused.clone();
            view.teardown_start = refused;
            let mut held = proxy(Some(view), ResourceStatus::Running);
            if teardown {
                held.transition_to_teardown_start().unwrap();
            } else {
                held.transition_to_delete_start().unwrap();
            }
            assert_eq!(held.get_status(), ResourceStatus::Deleting);
            assert_eq!(held.pending.len(), 1);
        }
    }

    /// A request with a new transition is a new step; reusing the lost reply's id would let a
    /// replay cache answer it with the reply to the old request.
    #[test]
    fn a_new_transition_drops_the_lost_replys_step_id() {
        let mut lost = ready();
        lost.transition_to_failure();
        assert_eq!(lost.in_flight_step_id, None);
    }

    #[test]
    fn the_saved_state_round_trips_through_the_deserializer() {
        let mut ready = ready();
        ready.transition_to_update().unwrap();
        let saved = crate::core::serialize_controller(&ready).unwrap();
        assert_eq!(saved["type"], PROXY_CONTROLLER_TYPE);
        let loaded = crate::core::deserialize_controller(saved.clone()).unwrap();
        assert_eq!(
            crate::core::serialize_controller(loaded.as_ref()).unwrap(),
            saved
        );
        assert_eq!(loaded.get_status(), ResourceStatus::Updating);
    }

    /// Each value the redaction strips is in the config, so leaving any one strip out fails.
    #[test]
    fn secrets_stay_out_of_the_deployment_config_a_controller_gets() {
        use alien_core::{
            EnvironmentVariable, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
            OtlpConfig, StorageBinding,
        };
        let variable = |name: &str, value: &str, var_type| EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type,
            target_resources: None,
        };
        let mut config = DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![
                    variable("PLAIN", "PLAIN-VALUE", EnvironmentVariableType::Plain),
                    variable(
                        "SECRET",
                        "SECRET-ENV-VALUE",
                        EnvironmentVariableType::Secret,
                    ),
                ],
                hash: "h".to_string(),
                created_at: String::new(),
            })
            .external_bindings(Default::default())
            .allow_frozen_changes(false)
            .manager_url("https://manager.example".to_string())
            .build();
        config.deployment_token = Some("SECRET-REGISTRY-TOKEN".to_string());
        config.compute_backend = Some(
            serde_json::from_value(json!({
                "type": "horizon",
                "url": "https://horizon.example",
                "clusters": {
                    "machines": { "clusterId": "c1", "managementToken": "SECRET-CLUSTER-TOKEN" }
                }
            }))
            .unwrap(),
        );
        config.monitoring = Some(OtlpConfig {
            logs_endpoint: "https://manager.example/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer SECRET-LOG-TOKEN".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: Default::default(),
        });
        config.domain_metadata = Some(
            serde_json::from_value(json!({
                "baseDomain": "vpc.direct",
                "publicSubdomain": "k8f2j3",
                "hostedZoneId": "Z1",
                "resources": {
                    "api": {
                        "fqdn": "api.k8f2j3.vpc.direct",
                        "certificateId": "cert-1",
                        "certificateStatus": "issued",
                        "dnsStatus": "active",
                        "privateKey": "SECRET-PRIVATE-KEY"
                    }
                }
            }))
            .unwrap(),
        );
        config.external_bindings = ExternalBindings(
            [(
                "uploads".to_string(),
                ExternalBinding::Storage(StorageBinding::s3("SECRET-EXTERNAL-BUCKET")),
            )]
            .into(),
        );
        // The manager merges held inputs, secret ones too, into `input_values` before a step.
        config
            .input_values
            .insert("dbPassword".to_string(), json!("SECRET-HELD-INPUT"));
        // The Operator copies the chart's bindings into the stack settings.
        config.stack_settings.external_bindings = Some(ExternalBindings(
            [(
                "cache".to_string(),
                ExternalBinding::Storage(StorageBinding::s3("SECRET-SETTINGS-BINDING")),
            )]
            .into(),
        ));
        let secrets = [
            "SECRET-ENV-VALUE",
            "SECRET-REGISTRY-TOKEN",
            "SECRET-CLUSTER-TOKEN",
            "SECRET-LOG-TOKEN",
            "SECRET-PRIVATE-KEY",
            "SECRET-EXTERNAL-BUCKET",
            "SECRET-HELD-INPUT",
            "SECRET-SETTINGS-BINDING",
        ];
        let whole = serde_json::to_string(&config).unwrap();
        for secret in secrets {
            assert!(whole.contains(secret), "the fixture lacks {secret}");
        }

        let sent = serde_json::to_string(&deployment_config_for_extension(&config)).unwrap();
        for secret in secrets {
            assert!(
                !sent.contains(secret),
                "{secret} reaches the controller: {sent}"
            );
        }
        assert!(sent.contains("PLAIN-VALUE"), "{sent}");
        assert!(sent.contains("https://manager.example"), "{sent}");
    }
}
