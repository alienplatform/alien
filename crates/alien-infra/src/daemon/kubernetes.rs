use crate::kubernetes_runtime_credentials::delete_environment_secret;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::kubernetes_errors::is_remote_resource_conflict;
use crate::core::{
    direct_monitoring_auth_headers, kubernetes_branded_resource_labels,
    kubernetes_cleanup_resource_labels, kubernetes_runtime_pod_labels, linked_bindings,
    pod_template_annotations, projected_env_vars,
    reconcile_environment_secret_with_additional_secrets, EnvSecretRotationTracker,
    EnvironmentVariableBuilder, KubernetesEnvSecretPlan, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_public_endpoint::{
    daemon_public_endpoint_target, delete_kubernetes_public_endpoint,
    reconcile_kubernetes_public_endpoint, KubernetesEndpointAction, KubernetesPublicEndpointState,
};
use crate::kubernetes_workload_heartbeat::{
    emit_kubernetes_workload_heartbeat, label_selector, last_poll, readiness_holds, recovery_check,
    rollout_budget_exhausted, rollout_timeout_message, update_wait_max_polls, workload_pod_reason,
    KubernetesWorkload, KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput,
    NotReadyObservations, ReadyHold, KUBERNETES_WORKLOAD_READY_MAX_POLLS,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_resource_name, kubernetes_service_account_name, public_url_host, Daemon, DaemonCode,
    DaemonOutputs, PublicEndpointOutput, ResourceOutputs, ResourceStatus,
    ENV_ALIEN_RUNTIME_SECRETS,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, LocalObjectReference, PodSpec, PodTemplateSpec, ResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

/// Name of the daemon's container in its pod template.
const DAEMON_CONTAINER_NAME: &str = "daemon";

/// Whether every node the DaemonSet schedules on runs a ready pod.
fn daemonset_is_ready(daemonset: &DaemonSet) -> bool {
    daemonset.status.as_ref().is_some_and(|status| {
        status.number_ready >= status.desired_number_scheduled
            && status.desired_number_scheduled > 0
    })
}

/// Polls (5 s apart) a rolling update may take before the update fails: the workload-wide limit.
const UPDATE_WAIT_MAX_POLLS: u32 = KUBERNETES_WORKLOAD_READY_MAX_POLLS;

/// Whether the DaemonSet's current pod template runs on every node it schedules on: the
/// controller has seen the latest spec, every pod is updated, and every pod is available.
/// `daemonset_is_ready` alone passes right after a patch (old pods still ready) and mid-roll.
fn daemonset_rollout_complete(daemonset: &DaemonSet) -> bool {
    daemonset.status.as_ref().is_some_and(|status| {
        let desired = status.desired_number_scheduled;
        status.observed_generation.unwrap_or(0) >= daemonset.metadata.generation.unwrap_or(1)
            && desired > 0
            && status.updated_number_scheduled.unwrap_or(0) == desired
            && status.number_available.unwrap_or(0) == desired
    })
}

/// Polls (5 s apart) the create wait may take.
const CREATE_WAIT_MAX_POLLS: u32 = 60;

/// "desired 10, updated 6, available 6" for the DaemonSet, as the rollout timeout reports it.
fn daemonset_counts(daemonset: Option<&DaemonSet>) -> String {
    match daemonset.and_then(|d| d.status.as_ref()) {
        Some(status) => format!(
            "desired {}, updated {}, available {}",
            status.desired_number_scheduled,
            status.updated_number_scheduled.unwrap_or(0),
            status.number_available.unwrap_or(0)
        ),
        None => "the DaemonSet reports no status".to_string(),
    }
}

#[controller]
pub struct KubernetesDaemonController {
    /// The name of the created Kubernetes DaemonSet.
    pub(crate) daemon_set_name: Option<String>,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
    /// Tracks the env-Secret checksum so `needs_update` can detect secret
    /// rotations that config diffing cannot see (secrets are projected via
    /// secretKeyRef, never into the resource config).
    #[serde(default)]
    pub(crate) env_secret: EnvSecretRotationTracker,
    /// Public endpoint state (Service/route names, resolved public URL).
    #[serde(default)]
    pub(crate) public_endpoint: KubernetesPublicEndpointState,
    /// PriorityClass name applied to the pod spec last. The chart's class mapping is read at
    /// pod-spec build time, so a Helm upgrade that changes it is invisible to config diffing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applied_priority_class: Option<String>,
    /// The single architecture the release recorded when the pod spec was last applied (None: no
    /// record, or several). It is read at pod-spec build time, so a release that records one for
    /// an existing daemon is invisible to config diffing. Only this is compared, never the whole
    /// selector, so a pool's selector cannot make an unrecorded workload roll.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applied_architecture: Option<alien_core::instance_catalog::Architecture>,
    /// Kubernetes' explanation for a DaemonSet that cannot create pods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) workload_failure: Option<String>,
    /// True from the moment `create_start` records its namespace and name, before its first
    /// write, until the create call has succeeded. A create that stops in between must not
    /// publish outputs for a DaemonSet that may not exist.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) create_unfinished: bool,
    /// Not-ready observations of the Running DaemonSet; the third in a window fails the refresh.
    #[serde(default)]
    pub(crate) not_ready: NotReadyObservations,
    /// Ready polls of the create or update wait; the rollout completes when readiness holds.
    #[serde(default)]
    pub(crate) ready_hold: ReadyHold,
    /// Consecutive ready checks since the resource failed a health check.
    #[serde(default)]
    pub(crate) recovery_checks: u32,
    /// When the last ready check counted towards recovery.
    #[serde(default)]
    pub(crate) last_recovery_check_unix: Option<i64>,
    /// Whether the last update left the pod template as it was (no pods roll). Such an update
    /// waits for readiness only briefly.
    #[serde(default)]
    pub(crate) update_template_unchanged: bool,
    /// Set only while the RefreshFailed handler re-checks; never stored.
    #[serde(skip)]
    pub(crate) recovering: bool,
}

/// The `nodeSelector` the Pods get: the pool or stack architecture, else the single
/// architecture the build recorded for this daemon.
fn desired_node_selector(
    ctx: &ResourceControllerContext<'_>,
    config: &Daemon,
) -> Result<Option<BTreeMap<String, String>>> {
    let pool =
        alien_core::kubernetes_daemon_pool(ctx.desired_stack, config).map_err(|message| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message,
            })
        })?;
    let node_selector = alien_core::kubernetes_compute_node_selector(
        ctx.desired_stack,
        pool,
        ctx.desired_stack.built_architectures_of(&config.id),
    )
    .map_err(|message| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: config.id.clone(),
            message,
        })
    })?;
    Ok(node_selector)
}

/// The one architecture the release recorded for this workload, if any.
fn recorded_architecture(
    ctx: &ResourceControllerContext<'_>,
    config_id: &str,
) -> Option<alien_core::instance_catalog::Architecture> {
    alien_core::single_built_architecture(ctx.desired_stack.built_architectures_of(config_id))
}

#[controller]
impl KubernetesDaemonController {
    fn get_progress(&self) -> Option<&str> {
        match self.state {
            KubernetesDaemonState::WaitingForDaemonSet
            | KubernetesDaemonState::WaitingForUpdate => self.workload_failure.as_deref(),
            _ => None,
        }
    }

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;

        let daemon_set_name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;
        let service_account_name = config
            .get_permissions()
            .map(|profile| kubernetes_service_account_name(&ctx.resource_prefix, profile));
        let registry_secret_name = format!("{}-registry", daemon_set_name);
        let environment_secret_name = format!("{}-env", daemon_set_name);
        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let (legacy_registry_owner_proven, legacy_environment_owner_proven) = match workload_client
            .get_daemonset(&namespace, &daemon_set_name)
            .await
        {
            Ok(existing) => {
                if !self.is_managed_daemonset(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    &daemon_set_name,
                    &config.id,
                ) {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Refusing to create Daemon '{}' because DaemonSet '{daemon_set_name}' is not owned by this deployment",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                let pod_spec = existing
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.template.spec.as_ref());
                (
                    crate::kubernetes_registry::pod_spec_references_image_pull_secret(
                        pod_spec,
                        &registry_secret_name,
                    ),
                    crate::core::pod_spec_references_environment_secret(
                        pod_spec,
                        &environment_secret_name,
                    ),
                )
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                (false, false)
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to inspect DaemonSet '{daemon_set_name}' before Daemon creation"
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // Record where this Daemon writes before the first write (the runtime-credential,
        // registry and env Secrets come before the DaemonSet), so a create that fails part-way
        // can still be deleted. After the ownership check above: a DaemonSet that is not ours
        // is refused before anything is written, so nothing is recorded for it.
        self.namespace = Some(namespace.clone());
        self.daemon_set_name = Some(daemon_set_name.clone());
        self.create_unfinished = true;

        crate::kubernetes_runtime_credentials::reconcile(
            ctx,
            &namespace,
            &daemon_set_name,
            &config.id,
            alien_core::commands_types::CommandTargetType::Daemon,
        )
        .await?;
        let proxy_image = match &config.code {
            DaemonCode::Image { image }
                if ctx.deployment_config.image_registry.is_none()
                    && (ctx.deployment_config.runtime_identity_audience.is_none()
                        || crate::kubernetes_registry::uses_manager_proxy(ctx, image)) =>
            {
                Some(image)
            }
            _ => None,
        };
        let image_pull_secret_name = if let Some(image) = proxy_image {
            let token = crate::kubernetes_registry::pull_credential(ctx, &config.id).await?;
            let secret_name = registry_secret_name.clone();
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(
                &secrets_client,
                &namespace,
                &secret_name,
                image,
                &token,
                &config.id,
                ctx,
                legacy_registry_owner_proven,
            )
            .await?;
            Some(secret_name)
        } else {
            None
        };

        // Reconcile the per-resource env Secret (creates/updates `{daemon}-env`)
        // for any Secret-kind env var scoped to this Daemon — notably the
        // command receiver's `ALIEN_COMMANDS_TOKEN`. Matches the container path:
        // each key is rendered as a `secretKeyRef` in the DaemonSet manifest.
        let monitoring_headers = direct_monitoring_auth_headers(ctx);
        let env_secret_plan = reconcile_environment_secret_with_additional_secrets(
            "daemon",
            &config.id,
            &daemon_set_name,
            &namespace,
            &monitoring_headers,
            legacy_environment_owner_proven,
            ctx,
        )
        .await?;
        self.env_secret.record(env_secret_plan.as_ref());
        self.applied_priority_class = alien_core::kubernetes_priority_class_name(config.priority)
            .map_err(|message| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message,
            })
        })?;
        self.applied_architecture = recorded_architecture(ctx, &config.id);

        let daemonset = self
            .build_daemonset(
                config,
                &daemon_set_name,
                &namespace,
                service_account_name.as_deref(),
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;

        match workload_client
            .create_daemonset(&namespace, &daemonset)
            .await
        {
            Ok(_) => {}
            // A retry after a transient failure between a successful create
            // and state persistence (or an orphan from a prior deploy) hits
            // AlreadyExists. Adopt the existing DaemonSet when it carries our
            // managed-by labels — mirrors the container controller.
            Err(err) if is_remote_resource_conflict(&err) => {
                let existing = workload_client
                    .get_daemonset(&namespace, &daemon_set_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to read existing daemonset '{}' before adoption.",
                            daemon_set_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !self.is_managed_daemonset(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    &daemon_set_name,
                    &config.id,
                ) {
                    return Err(err.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Refusing to adopt unmanaged daemonset '{}'.",
                            daemon_set_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                info!(daemon_set_name=%daemon_set_name, namespace=%namespace, "Adopting existing Kubernetes DaemonSet");
            }
            Err(err) => {
                return Err(err.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create daemonset '{}'.", daemon_set_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.create_unfinished = false;

        info!(daemon_set_name=%daemon_set_name, namespace=%namespace, "DaemonSet creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForDaemonSet,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDaemonSet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_daemonset(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let live = self.read_daemonset(ctx).await?;
        if live.as_ref().is_some_and(daemonset_is_ready) {
            if self.readiness_holds(ctx).await? {
                self.ready_hold.reset();
                return Ok(HandlerAction::Continue {
                    state: ReconcilePublicEndpoint,
                    suggested_delay: None,
                });
            }
        } else {
            self.ready_hold.reset();
        }
        let restricted_hint = self.root_image_hint(ctx).await?;
        self.record_workload_failure(ctx).await;
        if self.workload_failure.is_none() {
            self.workload_failure = restricted_hint;
        }
        if last_poll(self._internal_stay_count, CREATE_WAIT_MAX_POLLS) {
            return Err(self.rollout_timeout(ctx, live.as_ref()).await);
        }

        Ok(HandlerAction::Stay {
            max_times: Some(CREATE_WAIT_MAX_POLLS),
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = ReconcilePublicEndpoint,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn reconcile_public_endpoint(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.reconcile_endpoint(ctx).await? {
            KubernetesEndpointAction::Ready => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            }),
            KubernetesEndpointAction::Waiting { suggested_delay } => Ok(HandlerAction::Stay {
                max_times: None,
                suggested_delay: Some(suggested_delay),
            }),
        }
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if let Some(daemon_set_name) = self.daemon_set_name.as_ref() {
            let config = ctx.desired_resource_config::<Daemon>()?;
            self.env_secret.adopt_unkeyed(
                &config.id,
                daemon_set_name,
                &ctx.deployment_config.environment_variables.variables,
                &direct_monitoring_auth_headers(ctx),
            )?;
        }
        let live = self.read_daemonset(ctx).await?;
        if let (Some(name), Some(namespace)) = (&self.daemon_set_name, &self.namespace) {
            let config = ctx.desired_resource_config::<Daemon>()?;
            crate::kubernetes_runtime_credentials::reconcile(
                ctx,
                namespace,
                name,
                &config.id,
                alien_core::commands_types::CommandTargetType::Daemon,
            )
            .await?;
            if let DaemonCode::Image { image } = &config.code {
                crate::kubernetes_registry::refresh_pull_secret(
                    ctx, namespace, name, &config.id, image,
                )
                .await?;
            }
        }

        // A linked resource can change what its binding publishes without this daemon being
        // updated, such as a sandbox reading its broker address again on its own Ready. Compare
        // before readiness: a stale binding can be why a pod is not ready.
        if let Some(env) = linked_bindings::pod_template_env(
            live.as_ref()
                .and_then(|daemonset| daemonset.spec.as_ref())
                .and_then(|spec| spec.template.spec.as_ref()),
            DAEMON_CONTAINER_NAME,
        ) {
            let config = ctx.desired_resource_config::<Daemon>()?;
            let drifted = linked_bindings::kubernetes_linked_binding_drift(
                ctx,
                &config.links,
                &config.id,
                env,
            )
            .await?;
            if !drifted.is_empty() {
                // Names only: binding values can carry secrets.
                info!(
                    daemon = %config.id,
                    variables = ?drifted,
                    reason = "linked binding drift",
                    "Refreshing the daemon's linked bindings"
                );
                return Ok(HandlerAction::Continue {
                    state: UpdateStart,
                    suggested_delay: None,
                });
            }
        }
        if live.is_none() {
            let config = ctx.desired_resource_config::<Daemon>()?;
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Daemon daemonset is missing".to_string(),
            }));
        }
        // While recovering from a failed refresh, "ready" also means the rollout is complete:
        // during a stalled rolling update the old pods keep every node ready.
        let healthy = live.as_ref().is_some_and(daemonset_is_ready)
            && (!self.recovering || live.as_ref().is_some_and(daemonset_rollout_complete));
        if !healthy {
            let config = ctx.desired_resource_config::<Daemon>()?;
            // Not ready (or still rolling) is a health observation, not drift: it is retried
            // (three observations in a window fail the refresh), and the refresh that follows a
            // failure re-checks and recovers on its own.
            let name = self.daemon_set_name.clone().unwrap_or_default();
            let pod_reason = match self.namespace.as_deref() {
                Some(namespace) => {
                    workload_pod_reason(
                        ctx,
                        namespace,
                        &self.build_labels(&name),
                        self.workload_failure.as_deref(),
                    )
                    .await
                }
                None => None,
            };
            let counts = daemonset_counts(live.as_ref());
            let error = AlienError::new(ErrorData::WorkloadNotReady {
                resource_id: config.id.clone(),
                message: match pod_reason {
                    Some(reason) => {
                        format!("DaemonSet '{name}' is not fully ready: {counts}; {reason}")
                    }
                    None => format!("DaemonSet '{name}' is not fully ready: {counts}"),
                },
            });
            return Err(self.not_ready.error(
                error,
                self.recovering,
                chrono::Utc::now().timestamp(),
            ));
        }

        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        if let (Some(daemon_set_name), Some(namespace)) = (&self.daemon_set_name, &self.namespace) {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let daemonset = workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get daemonset '{}'", daemon_set_name),
                    resource_id: Some(config.id.clone()),
                })?;
            let labels = self.build_labels(daemon_set_name);
            emit_kubernetes_workload_heartbeat(
                ctx,
                KubernetesWorkloadHeartbeatInput {
                    deployment_id: None,
                    resource_id: config.id.clone(),
                    resource_type: Daemon::RESOURCE_TYPE,
                    data_kind: KubernetesWorkloadDataKind::Daemon,
                    command_supported: config.commands_enabled,
                    namespace: namespace.clone(),
                    workload_name: daemon_set_name.clone(),
                    workload_kind: alien_core::KubernetesWorkloadKind::DaemonSet,
                    workload: KubernetesWorkload::DaemonSet(daemonset),
                    label_selector: label_selector(&labels),
                },
            )
            .await?;
        }

        // Keep the public endpoint reconciled on every Ready tick, like the
        // container controller: managed-TLS certificate rotations, LB/DNS
        // drift, and a deleted Service/route are otherwise never re-applied
        // until an unrelated config change triggers the Update flow.
        if let KubernetesEndpointAction::Waiting { suggested_delay } =
            self.reconcile_endpoint(ctx).await?
        {
            return Ok(HandlerAction::Stay {
                max_times: None,
                suggested_delay: Some(suggested_delay),
            });
        }

        if let (Some(daemon_set_name), Some(namespace)) = (&self.daemon_set_name, &self.namespace) {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let desired_labels =
                self.workload_labels(ctx, &config.id, self.build_labels(daemon_set_name));
            let mut existing = workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to inspect DaemonSet '{daemon_set_name}' for ownership migration"
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            if !crate::core::kubernetes_labels_match_current_scope(
                existing.metadata.labels.as_ref(),
                &desired_labels,
            ) {
                if !self.is_managed_daemonset(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    daemon_set_name,
                    &config.id,
                ) {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Refusing to migrate foreign DaemonSet '{daemon_set_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                existing
                    .metadata
                    .labels
                    .get_or_insert_default()
                    .extend(desired_labels);
                workload_client
                    .update_daemonset(namespace, daemon_set_name, &existing)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to migrate cleanup ownership for DaemonSet '{daemon_set_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.not_ready.clear();
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let daemon_set_name = self.daemon_set_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "DaemonSet name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let existing = workload_client
            .get_daemonset(namespace, daemon_set_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get daemonset '{}' before update",
                    daemon_set_name
                ),
                resource_id: Some(config.id.clone()),
            })?;
        if !self.is_managed_daemonset(
            ctx,
            existing.metadata.labels.as_ref(),
            daemon_set_name,
            &config.id,
        ) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Refusing to update DaemonSet '{daemon_set_name}' because it is not owned by Daemon '{}' in this deployment",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
        let resource_version = existing.metadata.resource_version.clone();
        let generation_before = existing.metadata.generation;
        let registry_secret_name = format!("{}-registry", daemon_set_name);
        let environment_secret_name = format!("{}-env", daemon_set_name);
        let pod_spec = existing
            .spec
            .as_ref()
            .and_then(|spec| spec.template.spec.as_ref());
        let legacy_registry_owner_proven =
            crate::kubernetes_registry::pod_spec_references_image_pull_secret(
                pod_spec,
                &registry_secret_name,
            );
        let legacy_environment_owner_proven =
            crate::core::pod_spec_references_environment_secret(pod_spec, &environment_secret_name);

        let service_account_name = config
            .get_permissions()
            .map(|profile| kubernetes_service_account_name(&ctx.resource_prefix, profile));
        crate::kubernetes_runtime_credentials::reconcile(
            ctx,
            &namespace,
            &daemon_set_name,
            &config.id,
            alien_core::commands_types::CommandTargetType::Daemon,
        )
        .await?;
        let proxy_image = match &config.code {
            DaemonCode::Image { image }
                if ctx.deployment_config.image_registry.is_none()
                    && (ctx.deployment_config.runtime_identity_audience.is_none()
                        || crate::kubernetes_registry::uses_manager_proxy(ctx, image)) =>
            {
                Some(image)
            }
            _ => None,
        };
        let image_pull_secret_name = if let Some(image) = proxy_image {
            let token = crate::kubernetes_registry::pull_credential(ctx, &config.id).await?;
            let secret_name = registry_secret_name.clone();
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(
                &secrets_client,
                namespace,
                &secret_name,
                image,
                &token,
                &config.id,
                ctx,
                legacy_registry_owner_proven,
            )
            .await?;
            Some(secret_name)
        } else {
            None
        };

        // Reconcile the env Secret so token/secret changes propagate on update
        // and the pod-template checksum annotation rolls the DaemonSet.
        let monitoring_headers = direct_monitoring_auth_headers(ctx);
        let env_secret_plan = reconcile_environment_secret_with_additional_secrets(
            "daemon",
            &config.id,
            daemon_set_name,
            namespace,
            &monitoring_headers,
            legacy_environment_owner_proven,
            ctx,
        )
        .await?;
        self.env_secret.record(env_secret_plan.as_ref());
        self.applied_priority_class = alien_core::kubernetes_priority_class_name(config.priority)
            .map_err(|message| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message,
            })
        })?;
        self.applied_architecture = recorded_architecture(ctx, &config.id);

        let mut new_daemonset = self
            .build_daemonset(
                config,
                daemon_set_name,
                namespace,
                service_account_name.as_deref(),
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;
        new_daemonset.metadata.resource_version = resource_version;

        let updated = workload_client
            .update_daemonset(namespace, daemon_set_name, &new_daemonset)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update daemonset '{}'.", daemon_set_name),
                resource_id: Some(config.id.clone()),
            })?;
        self.update_template_unchanged =
            generation_before.is_some_and(|before| Some(before) == updated.metadata.generation);

        Ok(HandlerAction::Continue {
            state: WaitingForUpdate,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let live = self.read_daemonset(ctx).await?;
        if live.as_ref().is_some_and(daemonset_rollout_complete) {
            if self.readiness_holds(ctx).await? {
                self.ready_hold.reset();
                return Ok(HandlerAction::Continue {
                    state: ReconcilePublicEndpointAfterUpdate,
                    suggested_delay: None,
                });
            }
        } else {
            self.ready_hold.reset();
        }
        let restricted_hint = self.root_image_hint(ctx).await?;
        self.record_workload_failure(ctx).await;
        if self.workload_failure.is_none() {
            self.workload_failure = restricted_hint;
        }
        let max_polls = update_wait_max_polls(self.update_template_unchanged);
        if last_poll(self._internal_stay_count, max_polls) {
            return Err(self.rollout_timeout(ctx, live.as_ref()).await);
        }

        Ok(HandlerAction::Stay {
            max_times: Some(max_polls),
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = ReconcilePublicEndpointAfterUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn reconcile_public_endpoint_after_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.reconcile_endpoint(ctx).await? {
            KubernetesEndpointAction::Ready => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            }),
            KubernetesEndpointAction::Waiting { suggested_delay } => Ok(HandlerAction::Stay {
                max_times: None,
                suggested_delay: Some(suggested_delay),
            }),
        }
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;

        // Nothing recorded: this controller wrote nothing (a create refused before its first
        // write), or an earlier create failed part-way and its record was dropped (a restart of
        // the create from a fresh state) or never kept (an older controller), so objects may
        // remain. Do not look for them: say what could remain and finish.
        if self.namespace.is_none() && self.daemon_set_name.is_none() {
            let name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
            warn!(
                resource_id=%config.id,
                daemon_set_name=%name,
                namespace=?self.get_kubernetes_namespace(ctx).ok(),
                "No DaemonSet is recorded, so nothing was deleted; an earlier create that failed part-way may have left the DaemonSet and its -runtime, -env and -registry Secrets"
            );
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let legacy_owner_proven = if let Some(daemon_set_name) = self.daemon_set_name.as_deref() {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(existing) => {
                    if !self.is_managed_daemonset(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        daemon_set_name,
                        &config.id,
                    ) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Refusing to delete DaemonSet '{daemon_set_name}' because it is not owned by Daemon '{}' in this deployment",
                                config.id
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    true
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    false
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to inspect DaemonSet '{daemon_set_name}' before deletion"
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        } else {
            false
        };

        // Tear down the public endpoint (Service/route) before the workload,
        // mirroring the container controller's delete order.
        let namespace_owned = namespace.clone();
        delete_kubernetes_public_endpoint(
            ctx,
            &config.id,
            &namespace_owned,
            self.daemon_set_name.as_deref().unwrap_or_default(),
            "daemon",
            legacy_owner_proven,
            &mut self.public_endpoint,
        )
        .await?;

        if let Some(daemon_set_name) = self.daemon_set_name.as_deref() {
            delete_environment_secret(
                "daemon",
                &config.id,
                daemon_set_name,
                namespace,
                ctx,
                legacy_owner_proven,
            )
            .await?;
        }

        if let Some(daemon_set_name) = &self.daemon_set_name {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let existing = match workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(existing) => existing,
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    delete_environment_secret(
                        "daemon",
                        &config.id,
                        daemon_set_name,
                        namespace,
                        ctx,
                        false,
                    )
                    .await?;
                    self.daemon_set_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to inspect DaemonSet '{daemon_set_name}' before deletion"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };
            let desired_labels =
                self.workload_labels(ctx, &config.id, self.build_labels(daemon_set_name));
            if !self.is_managed_daemonset(
                ctx,
                existing.metadata.labels.as_ref(),
                daemon_set_name,
                &config.id,
            ) || !crate::core::kubernetes_labels_have_compatible_scope(
                existing.metadata.labels.as_ref(),
                &desired_labels,
            ) {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Refusing to delete DaemonSet '{daemon_set_name}' because it is not owned by Daemon '{}' in this deployment",
                        config.id
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
            match workload_client
                .delete_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    delete_environment_secret(
                        "daemon",
                        &config.id,
                        daemon_set_name,
                        namespace,
                        ctx,
                        false,
                    )
                    .await?;
                    self.daemon_set_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete daemonset '{}'.", daemon_set_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDeletion,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        if let Some(daemon_set_name) = &self.daemon_set_name {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(_) => {
                    debug!(daemon_set_name=%daemon_set_name, "Daemon daemonset still exists");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    delete_environment_secret(
                        "daemon",
                        &config.id,
                        daemon_set_name,
                        namespace,
                        ctx,
                        false,
                    )
                    .await?;
                    self.daemon_set_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get daemonset '{}'.", daemon_set_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Stay {
            max_times: Some(60),
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    // A failed health check is checked again: the resource returns to Running when every node
    // runs a ready pod, and keeps reporting its error while it does not.
    #[handler(
        state = RefreshFailed,
        on_failure = RefreshFailed,
        status = ResourceStatus::RefreshFailed,
    )]
    async fn refresh_failed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.recovering = true;
        let checked = self.ready(ctx).await;
        self.recovering = false;
        match checked {
            // Back to Running only after the same run of ready checks as containers and
            // workers: one ready read can be a pod that is about to crash again.
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay,
            }) => {
                if recovery_check(
                    &mut self.recovery_checks,
                    &mut self.last_recovery_check_unix,
                    true,
                    chrono::Utc::now().timestamp(),
                ) {
                    Ok(HandlerAction::Continue {
                        state: Ready,
                        suggested_delay,
                    })
                } else {
                    Ok(HandlerAction::Stay {
                        max_times: None,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            other => {
                recovery_check(
                    &mut self.recovery_checks,
                    &mut self.last_recovery_check_unix,
                    false,
                    0,
                );
                other
            }
        }
    }
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn reapplies_after_failed_refresh(&self) -> bool {
        true
    }

    /// Secret-typed env vars never enter the resource config on Kubernetes
    /// (they're projected via secretKeyRef), so config diffing alone cannot
    /// see secret rotations. Schedule an update when the snapshot-derived
    /// env-secret checksum drifts from the one applied last; the update
    /// re-reconciles the Secret and rolls pods via the checksum annotation.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let Some(daemon_set_name) = self.daemon_set_name.as_ref() else {
            return Ok(false);
        };
        let config = ctx.desired_resource_config::<Daemon>()?;
        let desired_priority_class = alien_core::kubernetes_priority_class_name(config.priority)
            .map_err(|message| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message,
                })
            })?;
        if desired_priority_class != self.applied_priority_class {
            return Ok(true);
        }
        if recorded_architecture(ctx, &config.id) != self.applied_architecture {
            return Ok(true);
        }
        let monitoring_headers = direct_monitoring_auth_headers(ctx);
        Ok(self.env_secret.drifted_with_additional_secrets(
            &config.id,
            daemon_set_name,
            &ctx.deployment_config.environment_variables.variables,
            &monitoring_headers,
        )?)
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if self.create_unfinished {
            return None;
        }
        self.daemon_set_name.as_ref().map(|daemon_set_name| {
            ResourceOutputs::new(DaemonOutputs {
                daemon_name: daemon_set_name.clone(),
                running: true,
                public_endpoints: if self.public_endpoint.named_public_endpoints.is_empty() {
                    self.public_endpoint
                        .effective_public_url()
                        .map(|url| {
                            std::collections::HashMap::from([(
                                "default".to_string(),
                                PublicEndpointOutput {
                                    host: public_url_host(&url).unwrap_or_default(),
                                    protocol: alien_core::ExposeProtocol::Http,
                                    port: alien_core::public_url_port(&url).unwrap_or(443),
                                    url,
                                    wildcard_host: None,
                                    load_balancer_endpoint: self
                                        .public_endpoint
                                        .load_balancer_endpoint
                                        .clone(),
                                },
                            )])
                        })
                        .unwrap_or_default()
                } else {
                    self.public_endpoint.named_public_endpoints.clone()
                },
            })
        })
    }
}

impl KubernetesDaemonController {
    /// Reconcile the Daemon's public endpoint (Service + platform route) —
    /// shared by the create and update flows. A daemon with no HTTP endpoint
    /// resolves to `Ready` immediately (the target is non-public).
    async fn reconcile_endpoint(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<KubernetesEndpointAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;
        let daemon_set_name = self.daemon_set_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "DaemonSet name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let labels = self.build_labels(&daemon_set_name);
        reconcile_kubernetes_public_endpoint(
            ctx,
            daemon_public_endpoint_target(
                &config.id,
                &daemon_set_name,
                &namespace,
                labels,
                &config.public_endpoints,
                config
                    .health_check
                    .as_ref()
                    .map(|check| check.path.as_str()),
            )?,
            &mut self.public_endpoint,
        )
        .await
    }

    /// Remember why pods cannot be created, when the mapped PriorityClass is missing.
    /// Fails the rollout with the opt-out message when pods of the desired
    /// template are blocked by the restricted profile; otherwise a hint for a
    /// read-only crash loop, if any.
    async fn root_image_hint(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<String>> {
        let mut restricted_hint = None;
        if let (Some(namespace), Some((selector, template))) = (
            self.namespace.as_deref(),
            self.read_daemonset(ctx).await?.and_then(|daemonset| {
                let spec = daemonset.spec?;
                Some((spec.selector.match_labels?, spec.template))
            }),
        ) {
            let config = ctx.desired_resource_config::<Daemon>()?;
            restricted_hint = crate::core::fail_if_root_image_blocked(
                ctx, namespace, &selector, &template, &config.id,
            )
            .await?;
        }
        Ok(restricted_hint)
    }

    async fn record_workload_failure(&mut self, ctx: &ResourceControllerContext<'_>) {
        self.workload_failure = match (
            ctx.desired_resource_config::<Daemon>(),
            self.namespace.as_deref(),
            self.daemon_set_name.as_deref(),
        ) {
            (Ok(config), Some(namespace), Some(name)) => {
                crate::kubernetes_priority_events::missing_priority_class_progress(
                    ctx,
                    config.priority,
                    namespace,
                    name,
                )
                .await
            }
            _ => None,
        };
    }

    /// Records a poll that found the DaemonSet ready; true once readiness has held.
    async fn readiness_holds(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let name = self.daemon_set_name.clone().unwrap_or_default();
        let namespace = self.namespace.clone().unwrap_or_default();
        let selector = self.build_labels(&name);
        readiness_holds(ctx, &mut self.ready_hold, &namespace, &selector).await
    }

    /// The error for a DaemonSet that did not become ready in time: the counts and the first
    /// pod that is not ready, with its reason. Retryable: a retry applies the spec again.
    async fn rollout_timeout(
        &self,
        ctx: &ResourceControllerContext<'_>,
        live: Option<&DaemonSet>,
    ) -> AlienError<ErrorData> {
        let resource_id = ctx
            .desired_resource_config::<Daemon>()
            .map(|config| config.id.clone())
            .unwrap_or_default();
        let name = self.daemon_set_name.clone().unwrap_or_default();
        let pod_reason = match self.namespace.as_deref() {
            Some(namespace) => {
                workload_pod_reason(
                    ctx,
                    namespace,
                    &self.build_labels(&name),
                    self.workload_failure.as_deref(),
                )
                .await
            }
            None => self.workload_failure.clone(),
        };
        rollout_budget_exhausted(AlienError::new(ErrorData::WorkloadNotReady {
            resource_id,
            message: rollout_timeout_message(
                "DaemonSet",
                &name,
                &daemonset_counts(live),
                pod_reason.as_deref(),
            ),
        }))
    }

    /// The live DaemonSet, or `None` when it does not exist.
    async fn read_daemonset(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<DaemonSet>> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let daemon_set_name = self.daemon_set_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "DaemonSet name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        match workload_client
            .get_daemonset(namespace, daemon_set_name)
            .await
        {
            Ok(daemonset) => Ok(Some(daemonset)),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to get daemonset '{}'.", daemon_set_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    async fn build_daemonset(
        &self,
        config: &Daemon,
        daemon_set_name: &str,
        namespace: &str,
        service_account_name: Option<&str>,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<DaemonSet> {
        let selector_labels = self.build_labels(daemon_set_name);
        let labels = self.workload_labels(ctx, &config.id, selector_labels.clone());
        // Source-built daemons are supported: `alien build` compiles the
        // source into an image whose compiled binary is the direct
        // entrypoint, deployed here as a DaemonSet. Reaching here with
        // unbuilt source means the build step was skipped.
        let image = match &config.code {
            DaemonCode::Image { image } => ctx.deployment_config.workload_image(image),
            DaemonCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message:
                        "Daemon still has unbuilt source code. Run 'alien build' first; it compiles the source into an image the controller can deploy."
                            .to_string(),
                }));
            }
        };

        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_daemon_runtime_env_vars(ctx)?
            .add_direct_monitoring_auth_headers(ctx)
            // Cross-target parity with the local controller: apps read
            // ALIEN_PUBLIC_ENDPOINTS_JSON to build their own absolute URLs.
            .add_current_resource_public_endpoint(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        // Command-enabled Daemons no longer get `ALIEN_TRANSPORT=passthrough`.
        // Their receiver config (`ALIEN_COMMANDS_*`) is injected per-resource into
        // `config.environment` by the manager/operator snapshot; the
        // `ALIEN_COMMANDS_TOKEN` Secret is projected via secretKeyRef like any
        // other resource secret (handled below).

        let (mut env_map, bindings) = env_builder.build_with_bindings();
        env_map.remove(ENV_ALIEN_RUNTIME_SECRETS);

        // Daemons project Secret-kind env vars (e.g. ALIEN_COMMANDS_TOKEN) as
        // secretKeyRefs and never load secrets at runtime, so the ALIEN_SECRETS
        // vault-load pointer is stripped from the manifest.
        let env_vars = projected_env_vars(env_secret_plan, bindings, env_map)?;

        let security = crate::core::pod_security(&config.id, config.security.as_ref(), &[])?;
        let container = Container {
            name: DAEMON_CONTAINER_NAME.to_string(),
            security_context: security.container,
            volume_mounts: (!security.mounts.is_empty()).then_some(security.mounts),
            image: Some(image),
            // The daemon contract: `command` overrides the image default
            // entrypoint, and the declared ResourceSpecs become real
            // requests/limits — without them the pod schedules as BestEffort
            // and is first in line for eviction. Mirrors the container
            // controller.
            command: config.command.clone(),
            ports: (!config.public_endpoints.is_empty()).then(|| {
                config
                    .public_endpoints
                    .iter()
                    .map(|endpoint| ContainerPort {
                        container_port: endpoint.port as i32,
                        name: Some(format!("tcp-{}", endpoint.port)),
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    })
                    .collect()
            }),
            env: Some(env_vars),
            resources: Some(ResourceRequirements {
                requests: Some(BTreeMap::from([
                    ("cpu".to_string(), Quantity(config.cpu.min.clone())),
                    ("memory".to_string(), Quantity(config.memory.min.clone())),
                ])),
                limits: Some(BTreeMap::from([
                    ("cpu".to_string(), Quantity(config.cpu.desired.clone())),
                    (
                        "memory".to_string(),
                        Quantity(config.memory.desired.clone()),
                    ),
                ])),
                ..Default::default()
            }),
            ..Default::default()
        };

        let image_pull_secrets = crate::kubernetes_registry::workload_pull_secrets(
            ctx.deployment_config,
            image_pull_secret_name,
        );

        let node_selector = desired_node_selector(ctx, config)?;
        let priority_class_name = alien_core::kubernetes_priority_class_name(config.priority)
            .map_err(|message| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message,
                })
            })?;
        let mut pod_spec = PodSpec {
            node_selector,
            priority_class_name,
            service_account_name: service_account_name.map(str::to_owned),
            automount_service_account_token: service_account_name.is_none().then_some(false),
            security_context: security.pod,
            volumes: (!security.volumes.is_empty()).then_some(security.volumes),
            containers: vec![container],
            restart_policy: Some("Always".to_string()),
            // Kubernetes would otherwise inject `<SERVICE>_SERVICE_HOST`, `<SERVICE>_PORT_<n>_TCP*`
            // and similar variables for every Service in the namespace. A workload sets its own
            // variables explicitly, which Kubernetes does not overwrite; Alien's bindings carry each
            // address, so the extra variables only add names a workload can collide with.
            enable_service_links: Some(false),
            image_pull_secrets,
            // Honor the configured stop grace period during pod shutdown
            // (K8s default is 30s when unset).
            termination_grace_period_seconds: config.stop_grace_period_seconds.map(i64::from),
            ..Default::default()
        };
        crate::sandbox::kubernetes_broker_token::project_into(&mut pod_spec, &config.links);

        if ctx.deployment_config.runtime_identity_audience.is_some() {
            crate::kubernetes_runtime_credentials::mount(
                &mut pod_spec,
                DAEMON_CONTAINER_NAME,
                daemon_set_name,
            )?;
        }
        let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());
        // Roll pods when the env Secret changes (e.g. token rotation) by stamping
        // its checksum onto the pod template — matches the container controller.
        let pod_annotations = pod_template_annotations(env_secret_plan, &config.environment)?;

        Ok(DaemonSet {
            metadata: ObjectMeta {
                name: Some(daemon_set_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DaemonSetSpec {
                selector: LabelSelector {
                    match_labels: Some(selector_labels),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(pod_labels),
                        annotations: pod_annotations,
                        ..Default::default()
                    }),
                    spec: Some(pod_spec),
                },
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn build_labels(&self, daemon_set_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), daemon_set_name.to_string());
        labels.insert("managed-by".to_string(), "runtime".to_string());
        labels.insert("component".to_string(), "daemon".to_string());
        labels
    }

    fn workload_labels(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        mut labels: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        labels.extend(kubernetes_branded_resource_labels(ctx, resource_id));
        labels
    }

    /// Whether an existing DaemonSet carries our managed-by labels and may be
    /// adopted on an AlreadyExists create. Twin of the container controller's
    /// `is_managed_workload`.
    fn is_managed_daemonset(
        &self,
        ctx: &ResourceControllerContext<'_>,
        labels: Option<&BTreeMap<String, String>>,
        daemon_set_name: &str,
        resource_id: &str,
    ) -> bool {
        let desired = self.workload_labels(ctx, resource_id, self.build_labels(daemon_set_name));
        crate::core::kubernetes_labels_match_identity(
            labels,
            &desired,
            &["managed-by", "component", "app"],
        ) && crate::core::kubernetes_labels_have_compatible_scope(labels, &desired)
    }

    fn get_kubernetes_namespace(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        let k8s_config = ctx.get_kubernetes_config()?;
        match k8s_config {
            alien_core::KubernetesClientConfig::InCluster { namespace, .. }
            | alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. }
            | alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes".to_string(),
                        message: "Kubernetes namespace is not configured".to_string(),
                    })
                })
            }
        }
    }
}

async fn create_registry_pull_secret(
    secrets_client: &std::sync::Arc<dyn alien_k8s_clients::SecretsApi>,
    namespace: &str,
    secret_name: &str,
    proxy_host: &str,
    deployment_token: &str,
    resource_id: &str,
    ctx: &ResourceControllerContext<'_>,
    legacy_owner_proven: bool,
) -> Result<()> {
    crate::kubernetes_registry::ensure_registry_pull_secret(
        secrets_client,
        namespace,
        secret_name,
        proxy_host,
        deployment_token,
        kubernetes_cleanup_resource_labels(ctx, resource_id),
        legacy_owner_proven,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kubernetes_manifest_test_support::{
        assert_secret_key_ref, daemonset_env, pod_template_checksum_annotation, secret_env_var,
        KubernetesManifestTestHarness,
    };
    use crate::core::{
        direct_monitoring_auth_headers, environment_secret_plan,
        environment_secret_plan_with_additional_secrets, ResourceController,
        OTEL_EXPORTER_OTLP_HEADERS, OTEL_EXPORTER_OTLP_METRICS_HEADERS,
    };
    use alien_core::{
        OtlpConfig, Resource, ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_LAMBDA_MODE,
        ENV_ALIEN_RUNTIME_SEND_OTLP, ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT,
        ENV_ALIEN_WORKER_GRPC_ADDRESS,
    };

    fn daemonset_status(
        generation: i64,
        observed: i64,
        desired: i32,
        updated: i32,
        available: i32,
    ) -> DaemonSet {
        use k8s_openapi::api::apps::v1::DaemonSetStatus;
        DaemonSet {
            metadata: ObjectMeta {
                generation: Some(generation),
                ..Default::default()
            },
            status: Some(DaemonSetStatus {
                observed_generation: Some(observed),
                desired_number_scheduled: desired,
                number_ready: available,
                updated_number_scheduled: Some(updated),
                number_available: Some(available),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_stale_generation_is_not_a_complete_rollout() {
        // Old pods are all ready right after the patch, before the controller saw the new spec.
        let stale = daemonset_status(2, 1, 10, 10, 10);
        assert!(daemonset_is_ready(&stale));
        assert!(!daemonset_rollout_complete(&stale));
    }

    #[test]
    fn a_roll_with_pods_not_yet_updated_is_not_complete() {
        let mid_roll = daemonset_status(2, 2, 10, 4, 10);
        assert!(daemonset_is_ready(&mid_roll));
        assert!(!daemonset_rollout_complete(&mid_roll));
    }

    #[test]
    fn an_updated_pod_that_is_not_available_is_not_complete() {
        let one_unavailable = daemonset_status(2, 2, 10, 10, 9);
        assert!(!daemonset_rollout_complete(&one_unavailable));
    }

    #[test]
    fn a_fully_rolled_daemonset_is_complete_and_recovers_on_the_next_check() {
        assert!(!daemonset_rollout_complete(&daemonset_status(
            2, 2, 10, 6, 10
        )));
        assert!(daemonset_rollout_complete(&daemonset_status(
            2, 2, 10, 10, 10
        )));
        assert!(!daemonset_rollout_complete(&daemonset_status(
            2, 2, 0, 0, 0
        )));
        assert!(!daemonset_rollout_complete(&DaemonSet::default()));
    }

    #[test]
    fn the_update_wait_is_the_thirty_minute_workload_limit() {
        // 5 s polls: a large cluster's roll takes far longer than the old 5 minute bound.
        assert_eq!(UPDATE_WAIT_MAX_POLLS * 5, 30 * 60);
    }

    fn manifest_test_daemon(environment: &[(&str, &str)]) -> Daemon {
        let mut config = Daemon::new("agent".to_string())
            .code(DaemonCode::Image {
                image: "registry.example.com/agent:1".to_string(),
            })
            .permissions("default".to_string())
            .build();
        for (name, value) in environment {
            config
                .environment
                .insert(name.to_string(), value.to_string());
        }
        config
    }

    /// A cluster that keeps Secrets and refuses the DaemonSet at admission, the way a
    /// `restricted` namespace refuses a pod that runs as root.
    struct RefusingCluster {
        secrets:
            std::sync::Arc<std::sync::Mutex<BTreeMap<String, k8s_openapi::api::core::v1::Secret>>>,
        harness: KubernetesManifestTestHarness,
        /// A DaemonSet already on the cluster under this Daemon's name, if any.
        existing_daemonset:
            std::sync::Arc<std::sync::Mutex<Option<k8s_openapi::api::apps::v1::DaemonSet>>>,
        daemonset_deletes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        /// Whether the DaemonSet create call succeeds (it stores what it was given).
        create_succeeds: std::sync::Arc<std::sync::atomic::AtomicBool>,
        /// Whether the DaemonSet create call answers that the DaemonSet already exists.
        create_conflicts: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    async fn refusing_cluster() -> RefusingCluster {
        use alien_core::{EnvironmentVariable, EnvironmentVariableType, ENV_ALIEN_DEPLOYMENT_ID};
        use alien_k8s_clients::deployments::{DeploymentApi, MockDeploymentApi};
        use alien_k8s_clients::secrets::{MockSecretsApi, SecretsApi};
        use alien_k8s_clients::services::{MockServiceApi, ServiceApi};
        use std::sync::{Arc, Mutex};

        let not_found = |name: &str| {
            AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Object".to_string(),
                resource_name: name.to_string(),
            })
        };
        let secrets_state: Arc<Mutex<BTreeMap<String, k8s_openapi::api::core::v1::Secret>>> =
            Arc::default();
        let existing_daemonset: Arc<Mutex<Option<k8s_openapi::api::apps::v1::DaemonSet>>> =
            Arc::default();
        let daemonset_deletes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let create_succeeds = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let create_conflicts = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut secrets = MockSecretsApi::new();
        let created = secrets_state.clone();
        secrets.expect_create_secret().returning(move |_, secret| {
            let name = secret.metadata.name.clone().unwrap_or_default();
            created.lock().unwrap().insert(name, secret.clone());
            Ok(secret.clone())
        });
        let read = secrets_state.clone();
        secrets.expect_get_secret().returning(move |_, name| {
            read.lock().unwrap().get(name).cloned().ok_or_else(|| {
                AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Secret".to_string(),
                    resource_name: name.to_string(),
                })
            })
        });
        let updated = secrets_state.clone();
        secrets
            .expect_update_secret()
            .returning(move |_, name, secret| {
                updated
                    .lock()
                    .unwrap()
                    .insert(name.to_string(), secret.clone());
                Ok(secret.clone())
            });
        let deleted = secrets_state.clone();
        secrets.expect_delete_secret().returning(move |_, name| {
            deleted.lock().unwrap().remove(name);
            Ok(())
        });
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);

        let mut daemonsets = MockDeploymentApi::new();
        let existing = existing_daemonset.clone();
        daemonsets.expect_get_daemonset().returning(move |_, name| {
            existing
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| not_found(name))
        });
        let counted = daemonset_deletes.clone();
        daemonsets.expect_delete_daemonset().returning(move |_, _| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Default::default())
        });
        let existing = existing_daemonset.clone();
        let succeeds = create_succeeds.clone();
        let conflicts = create_conflicts.clone();
        daemonsets
            .expect_create_daemonset()
            .returning(move |_, daemonset| {
                if conflicts.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceConflict {
                            resource_type: "DaemonSet".to_string(),
                            resource_name: "agent".to_string(),
                            message: "already exists".to_string(),
                        },
                    ));
                }
                if succeeds.load(std::sync::atomic::Ordering::SeqCst) {
                    *existing.lock().unwrap() = Some(daemonset.clone());
                    return Ok(daemonset.clone());
                }
                Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: "admission webhook denied the request".to_string(),
                }))
            });
        let daemonsets: Arc<dyn DeploymentApi> = Arc::new(daemonsets);
        let services: Arc<dyn ServiceApi> = Arc::new(MockServiceApi::new());

        use crate::core::PlatformServiceProvider as _;
        let routes = crate::core::DefaultPlatformServiceProvider::default()
            .get_kubernetes_route_client(&alien_core::KubernetesClientConfig::Manual {
                server_url: "https://kubernetes.test".to_string(),
                certificate_authority_data: None,
                insecure_skip_tls_verify: Some(true),
                client_certificate_data: None,
                client_key_data: None,
                token: None,
                username: None,
                password: None,
                namespace: Some("test-ns".to_string()),
                additional_headers: Default::default(),
            })
            .await
            .expect("route client");
        let mut provider = crate::core::MockPlatformServiceProvider::new();
        provider.expect_runtime_credential().returning(|_, _| {
            Ok(alien_client_core::runtime_credentials::RuntimeCapability {
                token: "ax_cap_v1_test".to_string(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(60),
            })
        });
        provider
            .expect_get_kubernetes_secrets_client()
            .returning(move |_| Ok(secrets.clone()));
        provider
            .expect_get_kubernetes_deployment_client()
            .returning(move |_| Ok(daemonsets.clone()));
        provider
            .expect_get_kubernetes_service_client()
            .returning(move |_| Ok(services.clone()));
        // Nothing here has a route; the client is fetched and never called.
        provider
            .expect_get_kubernetes_route_client()
            .returning(move |_| Ok(routes.clone()));

        let mut variables = vec![secret_env_var("APP_SECRET", "s3cret", None)];
        variables.push(EnvironmentVariable {
            name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            value: "dep-1".to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        });
        let mut harness =
            KubernetesManifestTestHarness::new(Resource::new(manifest_test_daemon(&[])), variables)
                .with_service_provider(Arc::new(provider));
        harness.deployment_config_mut().runtime_identity_audience =
            Some("https://manager.example".to_string());
        RefusingCluster {
            secrets: secrets_state,
            harness,
            existing_daemonset,
            daemonset_deletes,
            create_succeeds,
            create_conflicts,
        }
    }

    fn deleted(action: &KubernetesDaemonHandlerAction) -> bool {
        matches!(
            action,
            KubernetesDaemonHandlerAction::Continue {
                state: KubernetesDaemonState::Deleted,
                ..
            }
        )
    }

    /// A DaemonSet refused at admission leaves the runtime-credential and env Secrets written
    /// before it. The state saved after the failed create names them, so delete removes them.
    #[tokio::test]
    async fn a_daemon_whose_create_fails_after_its_secrets_were_written_is_deleted_with_them() {
        let cluster = refusing_cluster().await;
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        controller
            .create_start(&ctx)
            .await
            .expect_err("the cluster refuses the DaemonSet");
        let written: Vec<String> = cluster.secrets.lock().unwrap().keys().cloned().collect();
        assert_eq!(written, ["agent-env", "agent-runtime"]);
        assert_eq!(controller.namespace.as_deref(), Some("test-ns"));
        assert_eq!(controller.daemon_set_name.as_deref(), Some("agent"));
        assert!(controller.build_outputs().is_none());
        let action = controller.delete_start(&ctx).await.expect("delete");
        assert!(deleted(&action), "{action:?}");
        assert!(cluster.secrets.lock().unwrap().is_empty());
    }

    /// A create refused because an unlabelled DaemonSet of that name exists records nothing; the
    /// delete that follows completes and leaves that object untouched.
    #[tokio::test]
    async fn a_delete_after_a_create_refused_for_a_foreign_daemonset_completes_and_leaves_it() {
        let cluster = refusing_cluster().await;
        *cluster.existing_daemonset.lock().unwrap() =
            Some(k8s_openapi::api::apps::v1::DaemonSet::default());
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        controller.create_start(&ctx).await.expect_err("not ours");
        assert!(controller.namespace.is_none() && controller.daemon_set_name.is_none());
        assert!(
            cluster.secrets.lock().unwrap().is_empty(),
            "nothing was written for a DaemonSet that is not ours"
        );
        let action = controller
            .delete_start(&ctx)
            .await
            .expect("delete completes");
        assert!(deleted(&action), "{action:?}");
        assert_eq!(
            cluster
                .daemonset_deletes
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the foreign DaemonSet is untouched"
        );
    }

    /// A DaemonSet that carries this deployment's labels is the one a recorded delete removes.
    fn owned_daemonset(
        controller: &KubernetesDaemonController,
        ctx: &ResourceControllerContext<'_>,
    ) -> k8s_openapi::api::apps::v1::DaemonSet {
        let mut labels = controller.workload_labels(ctx, "agent", controller.build_labels("agent"));
        labels.insert("managed-by".to_string(), "runtime".to_string());
        k8s_openapi::api::apps::v1::DaemonSet {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// (Guard; passes the same on the code before this change.) A DaemonSet that carries this
    /// deployment's labels and a recorded name is deleted.
    #[tokio::test]
    async fn a_recorded_daemon_with_its_own_daemonset_is_deleted_as_before() {
        let cluster = refusing_cluster().await;
        let ctx = cluster.harness.ctx();
        let controller = KubernetesDaemonController::default();
        *cluster.existing_daemonset.lock().unwrap() = Some(owned_daemonset(&controller, &ctx));
        let mut saved = KubernetesDaemonController {
            namespace: Some("test-ns".to_string()),
            daemon_set_name: Some("agent".to_string()),
            ..Default::default()
        };
        saved.delete_start(&ctx).await.expect("delete");
        assert_eq!(
            cluster
                .daemonset_deletes
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    /// (Guard; passes the same on the code before this change.) A Daemon created normally, then
    /// deleted: its DaemonSet and Secrets go, and its outputs are published after the create.
    #[tokio::test]
    async fn a_daemon_created_normally_is_deleted_with_its_daemonset_and_secrets() {
        let cluster = refusing_cluster().await;
        cluster
            .create_succeeds
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        controller.create_start(&ctx).await.expect("create");
        let outputs = controller.build_outputs().expect("outputs after a create");
        assert_eq!(
            outputs
                .downcast_ref::<DaemonOutputs>()
                .expect("daemon outputs")
                .daemon_name,
            "agent"
        );
        assert!(!cluster.secrets.lock().unwrap().is_empty());
        controller.delete_start(&ctx).await.expect("delete");
        assert_eq!(
            cluster
                .daemonset_deletes
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(cluster.secrets.lock().unwrap().is_empty());
    }

    /// A create that failed publishes no outputs, as before the create recorded its name early.
    #[tokio::test]
    async fn a_failed_create_publishes_no_outputs() {
        let cluster = refusing_cluster().await;
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        controller.create_start(&ctx).await.expect_err("refused");
        assert!(
            controller.daemon_set_name.is_some(),
            "the name is recorded for delete"
        );
        assert!(controller.build_outputs().is_none());
    }

    /// After a failed create, a retry that succeeds publishes the outputs.
    #[tokio::test]
    async fn a_successful_retry_after_a_failed_create_publishes_outputs() {
        let cluster = refusing_cluster().await;
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        controller.create_start(&ctx).await.expect_err("refused");
        assert!(controller.build_outputs().is_none());
        cluster
            .create_succeeds
            .store(true, std::sync::atomic::Ordering::SeqCst);
        controller
            .create_start(&ctx)
            .await
            .expect("the retry succeeds");
        assert!(controller.build_outputs().is_some());
    }

    /// A create answered "already exists" by our own DaemonSet (a retry after a lost answer) is
    /// adopted, and that counts as a finished create.
    #[tokio::test]
    async fn adopting_our_own_daemonset_after_a_conflict_finishes_the_create() {
        let cluster = refusing_cluster().await;
        cluster
            .create_conflicts
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let ctx = cluster.harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        *cluster.existing_daemonset.lock().unwrap() = Some(owned_daemonset(&controller, &ctx));
        controller.create_start(&ctx).await.expect("adopted");
        assert!(!controller.create_unfinished);
        assert!(controller.build_outputs().is_some());
    }

    /// The flag is left out of saved state when false, and state saved without it loads.
    #[test]
    fn the_unfinished_flag_is_omitted_when_false_and_absent_state_loads() {
        let mut controller = manifest_test_controller();
        let state = serde_json::to_value(&controller).unwrap();
        assert!(state.get("createUnfinished").is_none(), "{state}");
        let loaded: KubernetesDaemonController = serde_json::from_value(state).unwrap();
        assert!(!loaded.create_unfinished);
        controller.create_unfinished = true;
        let state = serde_json::to_value(&controller).unwrap();
        assert_eq!(
            state.get("createUnfinished"),
            Some(&serde_json::json!(true))
        );
    }

    /// Through the executor: a Daemon create the cluster refuses after its Secrets were written
    /// ends the step with an error, and the executor then saves the controller. That saved
    /// record (namespace, name, unfinished flag) is what the following delete reads: it removes
    /// the Secrets and the resource ends Deleted.
    #[tokio::test]
    async fn a_failed_create_leaves_a_record_through_the_executor_that_the_delete_uses() {
        use crate::core::state_utils::StackResourceStateExt as _;
        let cluster = refusing_cluster().await;
        let (provider, client_config, deployment_config) = {
            let ctx = cluster.harness.ctx();
            (
                ctx.service_provider.clone(),
                ctx.client_config.clone(),
                ctx.deployment_config.clone(),
            )
        };
        let stack = alien_core::Stack::new("daemon-failed-create".to_string())
            .add(
                manifest_test_daemon(&[]),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let executor = crate::core::StackExecutor::builder(&stack, client_config.clone())
            .deployment_config(&deployment_config)
            .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
            .service_provider(provider.clone())
            .build()
            .expect("executor");
        let mut state = alien_core::StackState::new(alien_core::Platform::Kubernetes);
        for _ in 0..60 {
            state = executor.step(state).await.expect("step").next_state;
            if state
                .resources
                .get("agent")
                .is_some_and(|r| r.status == alien_core::ResourceStatus::ProvisionFailed)
            {
                break;
            }
        }
        let failed = state
            .resources
            .get("agent")
            .expect("the daemon is in the state");
        assert_eq!(failed.status, alien_core::ResourceStatus::ProvisionFailed);
        let saved = failed
            .get_internal_controller_typed::<KubernetesDaemonController>()
            .expect("the saved controller");
        assert_eq!(saved.namespace.as_deref(), Some("test-ns"));
        assert!(saved.daemon_set_name.is_some());
        assert!(saved.create_unfinished);
        assert!(saved.build_outputs().is_none());
        assert_eq!(
            cluster.secrets.lock().unwrap().len(),
            2,
            "written before the refusal"
        );

        let deletion = crate::core::StackExecutor::for_deletion_with_service_provider(
            client_config,
            &deployment_config,
            provider,
            None,
        )
        .expect("deletion executor");
        for _ in 0..60 {
            if state
                .resources
                .get("agent")
                .is_none_or(|r| r.status == alien_core::ResourceStatus::Deleted)
            {
                break;
            }
            state = deletion.step(state).await.expect("step").next_state;
        }
        assert!(
            state
                .resources
                .get("agent")
                .is_none_or(|r| r.status == alien_core::ResourceStatus::Deleted),
            "{:?}",
            state.resources.get("agent").map(|r| r.status)
        );
        assert!(cluster.secrets.lock().unwrap().is_empty());
    }

    /// Nothing recorded means this controller wrote nothing: the delete completes, twice over,
    /// without one call to the cluster (a provider with no client expectations panics on any).
    #[tokio::test]
    async fn a_delete_with_nothing_recorded_completes_without_a_cluster_call() {
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(manifest_test_daemon(&[])), vec![])
                .with_service_provider(std::sync::Arc::new(
                    crate::core::MockPlatformServiceProvider::new(),
                ));
        let ctx = harness.ctx();
        let mut controller = KubernetesDaemonController::default();
        for _ in 0..2 {
            let action = controller.delete_start(&ctx).await.expect("delete");
            assert!(deleted(&action), "{action:?}");
            assert!(controller.namespace.is_none() && controller.daemon_set_name.is_none());
        }
    }

    fn manifest_test_controller() -> KubernetesDaemonController {
        KubernetesDaemonController {
            daemon_set_name: Some("agent".to_string()),
            namespace: Some("test-ns".to_string()),
            ..Default::default()
        }
    }

    /// Writes the daemon's DaemonSet, as the controller renders it, for the Kind test that
    /// applies it to a namespace enforcing the `restricted` Pod Security profile. See
    /// `write_the_default_deployment_for_the_kind_test` in the container controller.
    #[tokio::test]
    #[ignore = "writes manifests for the Kind pod security test"]
    async fn write_the_default_daemonset_for_the_kind_test() {
        let dir = std::env::var("ALIEN_KIND_MANIFEST_DIR").expect("ALIEN_KIND_MANIFEST_DIR");
        let image = std::env::var("ALIEN_KIND_IMAGE").expect("ALIEN_KIND_IMAGE");
        let mut config = match std::env::var("ALIEN_KIND_CONFIG") {
            Ok(path) => serde_json::from_str(&std::fs::read_to_string(path).expect("config"))
                .expect("daemon config"),
            Err(_) => {
                let mut config = manifest_test_daemon(&[]);
                config.security = Some(alien_core::ContainerSecurity {
                    profile: alien_core::ContainerSecurityProfile::Restricted,
                    run_as_user: None,
                    run_as_group: None,
                    read_only_root_filesystem: None,
                    writable_paths: Vec::new(),
                });
                config
            }
        };
        let name = std::env::var("ALIEN_KIND_NAME").unwrap_or_else(|_| "agent".to_string());
        let out = std::env::var("ALIEN_KIND_OUT").unwrap_or_else(|_| "daemon.yaml".to_string());
        config.code = DaemonCode::Image { image };
        let mut harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        if let Ok(registry) = std::env::var("ALIEN_KIND_IMAGE_REGISTRY") {
            harness = harness.with_image_registry(&registry);
        }
        let daemonset = manifest_test_controller()
            .build_daemonset(
                &config,
                &name,
                "default",
                Some("default"),
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");
        let mut value = serde_json::to_value(&daemonset).expect("serialize");
        value["apiVersion"] = "apps/v1".into();
        value["kind"] = "DaemonSet".into();
        // The Kind test chooses the namespace.
        value["metadata"]
            .as_object_mut()
            .expect("metadata")
            .remove("namespace");
        std::fs::write(
            std::path::Path::new(&dir).join(out),
            serde_json::to_string_pretty(&value).expect("json"),
        )
        .expect("write manifest");
    }

    /// A release without a `security` field renders no pod security; only an
    /// explicit `restricted` profile restricts the DaemonSet's pod.
    #[tokio::test]
    async fn daemon_pod_is_restricted_only_when_the_release_says_so() {
        let render = |security: Option<alien_core::ContainerSecurity>| async move {
            let mut config = manifest_test_daemon(&[]);
            config.security = security;
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            manifest_test_controller()
                .build_daemonset(
                    &config,
                    "agent",
                    "test-ns",
                    None,
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("daemonset manifest")
                .spec
                .unwrap()
                .template
                .spec
                .unwrap()
        };
        let old = render(None).await;
        assert!(old.security_context.is_none());
        assert!(old.volumes.is_none());
        assert!(old.containers[0].security_context.is_none());
        assert!(old.containers[0].volume_mounts.is_none());

        let restricted = render(Some(alien_core::ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Restricted,
            run_as_user: Some(1000),
            run_as_group: Some(1000),
            read_only_root_filesystem: None,
            writable_paths: vec!["/cache".to_string()],
        }))
        .await;
        assert_eq!(
            restricted.security_context.as_ref().unwrap().run_as_user,
            Some(1000)
        );
        assert_eq!(
            restricted
                .security_context
                .as_ref()
                .unwrap()
                .run_as_non_root,
            Some(true)
        );
        let container_context = restricted.containers[0]
            .security_context
            .as_ref()
            .expect("container security context");
        assert_eq!(container_context.allow_privilege_escalation, Some(false));
        let mounts: Vec<_> = restricted.containers[0]
            .volume_mounts
            .as_ref()
            .expect("mounts")
            .iter()
            .map(|mount| mount.mount_path.as_str())
            .collect();
        assert!(
            mounts.contains(&"/tmp") && mounts.contains(&"/cache"),
            "{mounts:?}"
        );
    }

    #[tokio::test]
    async fn daemon_without_profile_does_not_mount_a_workload_identity() {
        let mut config = manifest_test_daemon(&[("OBJECTS_BUCKET", "objects")]);
        config.permissions = None;
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let manifest = manifest_test_controller()
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                None,
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");
        let spec = manifest.spec.unwrap().template.spec.unwrap();
        assert_eq!(spec.service_account_name, None);
        assert_eq!(spec.automount_service_account_token, Some(false));
        assert_eq!(spec.containers.len(), 1);
        assert!(spec.containers[0]
            .env
            .as_ref()
            .unwrap()
            .iter()
            .any(|entry| {
                entry.name == "OBJECTS_BUCKET" && entry.value.as_deref() == Some("objects")
            }));
    }

    #[tokio::test]
    async fn daemonset_manifest_projects_secrets_and_never_carries_alien_secrets() {
        let variables = vec![
            secret_env_var("APP_SECRET", "s3cret", None),
            secret_env_var(
                ENV_ALIEN_COMMANDS_TOKEN,
                "receiver-token",
                Some(vec!["agent"]),
            ),
        ];
        // Simulate a config injected by an older manager that still collapsed
        // secrets: the pointer must be stripped from the manifest regardless.
        let config = manifest_test_daemon(&[
            ("APP_ENV", "prod"),
            (
                ENV_ALIEN_SECRETS,
                "{\"keys\":[\"APP_SECRET\"],\"hash\":\"legacy\"}",
            ),
            (ENV_ALIEN_RUNTIME_SECRETS, "vault://legacy-runtime"),
        ]);
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables.clone())
                .with_monitoring(OtlpConfig {
                    logs_endpoint: "https://manager.test/v1/logs".to_string(),
                    logs_auth_header: "authorization=Bearer logs-token".to_string(),
                    metrics_endpoint: Some("https://manager.test/v1/metrics".to_string()),
                    metrics_auth_header: Some("authorization=Bearer metrics-token".to_string()),
                    resource_attributes: Default::default(),
                });
        let monitoring_headers = direct_monitoring_auth_headers(&harness.ctx());
        let plan = environment_secret_plan_with_additional_secrets(
            "agent",
            "agent",
            &variables,
            &monitoring_headers,
        )
        .expect("checksum key")
        .expect("plan");
        let controller = manifest_test_controller();

        let daemonset = controller
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                Some("agent-sa"),
                None,
                Some(&plan),
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");

        let env = daemonset_env(&daemonset);

        // App secret and the command receiver token are native projections.
        assert_secret_key_ref(&env, "APP_SECRET", "agent-env");
        assert_secret_key_ref(&env, ENV_ALIEN_COMMANDS_TOKEN, "agent-env");
        assert_secret_key_ref(&env, OTEL_EXPORTER_OTLP_HEADERS, "agent-env");
        assert_secret_key_ref(&env, OTEL_EXPORTER_OTLP_METRICS_HEADERS, "agent-env");

        // The runtime vault-load pointer never reaches the manifest.
        assert!(
            !env.iter().any(|var| matches!(
                var.name.as_str(),
                ENV_ALIEN_SECRETS | ENV_ALIEN_RUNTIME_SECRETS
            )),
            "vault pointers must not appear in a Kubernetes DaemonSet manifest"
        );

        // Plain vars still flow through.
        let app_env = env
            .iter()
            .find(|var| var.name == "APP_ENV")
            .expect("plain APP_ENV");
        assert_eq!(app_env.value.as_deref(), Some("prod"));

        // No worker-era runtime env leaks into Daemon manifests.
        for name in [
            ENV_ALIEN_TRANSPORT,
            ENV_ALIEN_WORKER_GRPC_ADDRESS,
            ENV_ALIEN_RUNTIME_SEND_OTLP,
            ENV_ALIEN_LAMBDA_MODE,
        ] {
            assert!(
                !env.iter().any(|var| var.name == name),
                "worker-era env var '{name}' must not appear in a Daemon manifest"
            );
        }

        // The pod template carries the checksum that rolls pods on rotation.
        assert_eq!(
            pod_template_checksum_annotation(&daemonset.spec.expect("spec").template),
            Some(plan.checksum)
        );
    }

    /// Service-link variables of the namespace's Services must not reach the daemon.
    #[tokio::test]
    async fn daemonset_pod_does_not_get_service_link_variables() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let pod = manifest_test_controller()
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                Some("agent-sa"),
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest")
            .spec
            .expect("spec")
            .template
            .spec
            .expect("pod spec");
        assert_eq!(pod.enable_service_links, Some(false));
    }

    #[tokio::test]
    async fn daemonset_pod_spec_carries_the_class_mapped_to_the_declared_priority() {
        unsafe {
            std::env::set_var(
                alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES,
                r#"{"critical":"vendor-core","low":"vendor-batch"}"#,
            )
        };
        let render = |priority: Option<alien_core::WorkloadPriority>| async move {
            let mut config = manifest_test_daemon(&[]);
            config.priority = priority;
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let pod = manifest_test_controller()
                .build_daemonset(
                    &config,
                    "agent",
                    "test-ns",
                    Some("agent-sa"),
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("daemonset manifest")
                .spec
                .expect("spec")
                .template
                .spec
                .expect("pod spec");
            (
                pod.priority_class_name.clone(),
                serde_json::to_string(&pod).unwrap(),
            )
        };
        use alien_core::WorkloadPriority::*;
        assert_eq!(render(Some(Low)).await.0.as_deref(), Some("vendor-batch"));
        let (unmapped, unmapped_json) = render(Some(High)).await;
        let (none, none_json) = render(None).await;
        assert_eq!((unmapped, none), (None, None));
        assert_eq!(unmapped_json, none_json);
        assert!(!none_json.contains("priorityClassName"));
    }

    /// The runtime-less daemon contract on Kubernetes: `command` overrides
    /// the image entrypoint, ResourceSpecs become requests/limits (never
    /// BestEffort), and the stop grace period reaches the pod spec.
    #[tokio::test]
    async fn daemonset_manifest_carries_command_resources_and_grace_period() {
        let mut config = manifest_test_daemon(&[]);
        config.command = Some(vec!["/agent".to_string(), "--poll".to_string()]);
        config.cpu = alien_core::ResourceSpec {
            min: "0.25".to_string(),
            desired: "1".to_string(),
        };
        config.memory = alien_core::ResourceSpec {
            min: "256Mi".to_string(),
            desired: "1Gi".to_string(),
        };
        config.stop_grace_period_seconds = Some(90);
        config.public_endpoints = vec![alien_core::PublicEndpoint {
            name: "web".to_string(),
            port: 8080,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        }];
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let controller = manifest_test_controller();

        let daemonset = controller
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                Some("agent-sa"),
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");

        let pod_spec = daemonset
            .spec
            .expect("spec")
            .template
            .spec
            .expect("pod spec");
        assert_eq!(pod_spec.termination_grace_period_seconds, Some(90));

        let container = &pod_spec.containers[0];
        assert_eq!(
            container.command.as_deref(),
            Some(&["/agent".to_string(), "--poll".to_string()][..])
        );

        let resources = container.resources.as_ref().expect("resources");
        let requests = resources.requests.as_ref().expect("requests");
        assert_eq!(requests["cpu"].0, "0.25");
        assert_eq!(requests["memory"].0, "256Mi");
        let limits = resources.limits.as_ref().expect("limits");
        assert_eq!(limits["cpu"].0, "1");
        assert_eq!(limits["memory"].0, "1Gi");

        // The declared endpoint's port must reach the pod spec so the
        // endpoint Service has a port to target.
        let ports = container.ports.as_ref().expect("ports");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].container_port, 8080);
    }

    /// Outputs report the reconciled endpoint URL — a daemon that declared a
    /// public endpoint must not claim `running` with an empty endpoint map.
    #[test]
    fn build_outputs_includes_public_endpoint_url() {
        let controller = KubernetesDaemonController {
            daemon_set_name: Some("agent".to_string()),
            namespace: Some("test-ns".to_string()),
            public_endpoint: KubernetesPublicEndpointState {
                public_url: Some("https://agent.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let outputs = controller.build_outputs().expect("outputs");
        let daemon_outputs = outputs
            .downcast_ref::<DaemonOutputs>()
            .expect("daemon outputs");
        assert_eq!(
            daemon_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.url.as_str()),
            Some("https://agent.example.test")
        );
        assert_eq!(
            daemon_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.host.as_str()),
            Some("agent.example.test")
        );
    }

    #[tokio::test]
    async fn daemonset_secret_rotation_changes_the_rendered_pod_template() {
        let config = manifest_test_daemon(&[]);
        let controller = manifest_test_controller();

        let mut daemonsets = Vec::new();
        for value in ["v1", "v1", "v2"] {
            let variables = vec![secret_env_var("APP_SECRET", value, None)];
            let plan = environment_secret_plan("agent", "agent", &variables)
                .expect("checksum key")
                .expect("plan");
            let harness =
                KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
            let daemonset = controller
                .build_daemonset(
                    &config,
                    "agent",
                    "test-ns",
                    Some("agent-sa"),
                    None,
                    Some(&plan),
                    &harness.ctx(),
                )
                .await
                .expect("daemonset manifest");
            daemonsets.push(daemonset);
        }

        assert_eq!(
            daemonsets[0].spec.as_ref().expect("spec").template,
            daemonsets[1].spec.as_ref().expect("spec").template,
            "identical secret values must render an identical pod template"
        );
        assert_ne!(
            pod_template_checksum_annotation(&daemonsets[0].spec.as_ref().expect("spec").template),
            pod_template_checksum_annotation(&daemonsets[2].spec.as_ref().expect("spec").template),
            "rotating a secret value must change the pod template (rollout)"
        );
    }

    #[test]
    fn needs_update_follows_the_built_architecture_a_release_records() {
        use alien_core::instance_catalog::Architecture::{Arm64, X86_64};
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let mut controller = manifest_test_controller();
        let mut stack = harness.ctx().desired_stack.clone();
        // The stack also pins a node selector of its own (the pool architecture): it must
        // not make a workload with no record, or a multi-architecture one, roll.
        crate::core::kubernetes_manifest_test_support::add_x86_64_pool(&mut stack);
        let id = config.id.clone();
        let wants_update = |controller: &KubernetesDaemonController, stack: &alien_core::Stack| {
            let mut ctx = harness.ctx();
            ctx.desired_stack = stack;
            controller.needs_update(&ctx).expect("needs_update")
        };

        // A release without the record, or a multi-architecture one: nothing to apply.
        assert!(!wants_update(&controller, &stack));
        stack
            .built_architectures
            .insert(id.clone(), vec![X86_64, Arm64]);
        assert!(!wants_update(&controller, &stack));

        // A release that records one architecture updates the existing workload once.
        stack.built_architectures.insert(id.clone(), vec![X86_64]);
        assert!(wants_update(&controller, &stack));
        controller.applied_architecture = Some(X86_64);
        assert!(!wants_update(&controller, &stack));

        // A later release that changes it updates again; one that drops it updates once more.
        stack.built_architectures.insert(id.clone(), vec![Arm64]);
        assert!(wants_update(&controller, &stack));
        stack.built_architectures.clear();
        assert!(wants_update(&controller, &stack));
    }

    #[test]
    fn needs_update_detects_env_secret_rotation() {
        let config = manifest_test_daemon(&[]);
        let original = vec![secret_env_var("APP_SECRET", "v1", None)];
        let original_plan = environment_secret_plan("agent", "agent", &original)
            .expect("checksum key")
            .expect("plan");

        let mut controller = manifest_test_controller();
        controller.env_secret.record(Some(&original_plan));

        let harness =
            KubernetesManifestTestHarness::new(Resource::new(config.clone()), original.clone());
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        let rotated = vec![secret_env_var("APP_SECRET", "v2", None)];
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), rotated);
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        controller.env_secret.record(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));
    }

    /// See `check_certificate_wait`: the same wait, on first install and on update.
    #[tokio::test]
    async fn endpoint_waits_for_the_certificate_on_create_and_update() {
        use crate::core::kubernetes_manifest_test_support::check_certificate_wait;
        let config = Daemon::new("agent".to_string())
            .code(DaemonCode::Image {
                image: "registry.example.com/agent:1".to_string(),
            })
            .permissions("default".to_string())
            .public_endpoint(alien_core::PublicEndpoint {
                name: "default".into(),
                port: 8080,
                protocol: alien_core::ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();
        let mut harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        for (state, status) in [
            (
                KubernetesDaemonState::ReconcilePublicEndpoint,
                ResourceStatus::Provisioning,
            ),
            (
                KubernetesDaemonState::ReconcilePublicEndpointAfterUpdate,
                ResourceStatus::Updating,
            ),
        ] {
            let controller = KubernetesDaemonController {
                state,
                ..manifest_test_controller()
            };
            check_certificate_wait(&mut harness, controller, "agent", status, |c, at| {
                c.public_endpoint.wait_started_unix.insert(
                    crate::kubernetes_public_endpoint::EndpointWait::Certificate,
                    at,
                );
            })
            .await;
        }
    }

    /// A refresh-failed daemon is retried by applying its current spec again, so a retry does
    /// not run the same failed readiness check.
    #[test]
    fn a_retry_of_a_refresh_failed_daemon_applies_the_spec_again() {
        use crate::core::state_utils::StackResourceStateExt;
        let config = manifest_test_daemon(&[]);
        let mut record = alien_core::StackResourceState::new_pending(
            Daemon::RESOURCE_TYPE.to_string(),
            Resource::new(config),
            Some(alien_core::ResourceLifecycle::Live),
            vec![],
        );
        let mut controller = manifest_test_controller();
        controller.state = KubernetesDaemonState::RefreshFailed;
        record.status = ResourceStatus::RefreshFailed;
        record.error = Some(crate::core::state_utils::saved_refresh_error("x", true));
        record
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();

        assert!(record.retry_failed().unwrap());

        assert_eq!(record.status, ResourceStatus::Updating);
        let resumed = record.get_internal_controller().unwrap().unwrap();
        let resumed = resumed
            .as_any()
            .downcast_ref::<KubernetesDaemonController>()
            .unwrap();
        assert_eq!(resumed.state, KubernetesDaemonState::UpdateStart);
    }

    fn daemonset_status_body(
        labels: &std::collections::BTreeMap<String, String>,
        ready: i32,
    ) -> serde_json::Value {
        serde_json::json!({
            "apiVersion":"apps/v1", "kind":"DaemonSet",
            "metadata":{"name":"agent", "generation":2, "labels":labels},
            "spec":{"selector":{"matchLabels":{"app":"agent"}},"template":{"spec":{"containers":[]}}},
            "status":{"desiredNumberScheduled":10, "numberReady":ready, "observedGeneration":2,
                "updatedNumberScheduled":ready, "numberAvailable":ready}
        })
    }

    /// A healthy DaemonSet observed in the middle of a roll is not drift: the check fails with
    /// a retryable not-ready error, and the refresh that follows recovers once every node runs
    /// a ready pod.
    #[tokio::test]
    async fn a_daemonset_observed_mid_roll_recovers_when_it_is_fully_ready() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let server = httpmock::MockServer::start_async().await;
        let mut ctx = harness.ctx();
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();

        for (path, kind) in [("pods", "PodList"), ("events", "EventList")] {
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path(format!("/api/v1/namespaces/test-ns/{path}"));
                then.status(200).json_body(
                    serde_json::json!({"apiVersion":"v1","kind":kind,"metadata":{},"items":[]}),
                );
            });
        }
        let mut controller = manifest_test_controller();
        let labels = controller.workload_labels(&ctx, "agent", controller.build_labels("agent"));
        controller.state = KubernetesDaemonState::Ready;
        let mut rolling = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(200)
                .json_body(daemonset_status_body(&labels, 6));
        });
        let error = controller.step(&ctx).await.expect_err("mid-roll");
        assert_eq!(error.code, "WORKLOAD_NOT_READY");
        assert!(error.retryable);

        // The refresh failed (the retry budget of the Ready check ran out).
        controller.state = KubernetesDaemonState::RefreshFailed;
        let error = controller.step(&ctx).await.expect_err("still rolling");
        assert_eq!(error.code, "WORKLOAD_NOT_READY");
        assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);

        rolling.delete();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(200)
                .json_body(daemonset_status_body(&labels, 10));
        });
        // One ready read is not recovery: three ready checks, 10 s apart, are.
        for check in 1..=3 {
            controller
                .step(&ctx)
                .await
                .expect("a ready check of the refresh");
            if check < 3 {
                assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
                controller.last_recovery_check_unix = controller
                    .last_recovery_check_unix
                    .map(|counted| counted - 20);
            }
        }
        assert_eq!(controller.get_status(), ResourceStatus::Running);
    }

    /// Three not-ready observations in a window fail the refresh; the error carries the counts.
    /// A DaemonSet that stays mid-roll is retryable twice, then final (RefreshFailed).
    #[tokio::test]
    async fn a_daemonset_not_ready_for_three_observations_fails_its_refresh() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let server = httpmock::MockServer::start_async().await;
        let mut ctx = harness.ctx();
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
        for (path, kind) in [("pods", "PodList"), ("events", "EventList")] {
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path(format!("/api/v1/namespaces/test-ns/{path}"));
                then.status(200).json_body(
                    serde_json::json!({"apiVersion":"v1","kind":kind,"metadata":{},"items":[]}),
                );
            });
        }
        let mut controller = manifest_test_controller();
        let labels = controller.workload_labels(&ctx, "agent", controller.build_labels("agent"));
        controller.state = KubernetesDaemonState::Ready;
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(200)
                .json_body(daemonset_status_body(&labels, 6));
        });
        for observation in 1..=3 {
            let error = controller.step(&ctx).await.expect_err("not ready");
            assert_eq!(error.code, "WORKLOAD_NOT_READY");
            assert!(error.message.contains("desired 10"), "{}", error.message);
            assert_eq!(
                error.retryable,
                observation < 3,
                "observation {observation}"
            );
        }
    }

    /// A DaemonSet that is gone is drift, not a health observation: re-checking never brings it
    /// back, so the error is not retryable and says the object is missing.
    #[tokio::test]
    async fn a_deleted_daemonset_is_drift() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let server = httpmock::MockServer::start_async().await;
        let mut ctx = harness.ctx();
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(404).json_body(serde_json::json!({
                "kind":"Status","apiVersion":"v1","status":"Failure",
                "reason":"NotFound","code":404,"message":"agent: NotFound",
                "details":{"name":"agent"}
            }));
        });
        let mut controller = manifest_test_controller();
        controller.state = KubernetesDaemonState::Ready;
        let error = controller.step(&ctx).await.expect_err("missing");
        assert_eq!(error.code, "RESOURCE_DRIFT");
        assert!(!error.retryable);
        assert!(error.message.contains("missing"), "{}", error.message);
    }

    /// A roll that stalls (a node has no room for the new pod) ends with the counts and the
    /// first pod that is not ready, not with the generic polling timeout.
    #[tokio::test]
    async fn a_daemon_update_wait_that_times_out_says_why() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let server = httpmock::MockServer::start_async().await;
        let mut ctx = harness.ctx();
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/v1/namespaces/test-ns/events");
            then.status(200).json_body(
                serde_json::json!({"apiVersion":"v1","kind":"EventList","metadata":{},"items":[]}),
            );
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/v1/namespaces/test-ns/pods");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"v1","kind":"PodList","metadata":{},
                "items":[{
                    "metadata":{"name":"agent-x7k2p"},
                    "status":{"phase":"Pending","conditions":[{
                        "type":"PodScheduled","status":"False","reason":"Unschedulable",
                        "message":"0/10 nodes are available: Insufficient cpu"
                    }]}
                }]
            }));
        });
        let mut controller = manifest_test_controller();
        let labels = controller.workload_labels(&ctx, "agent", controller.build_labels("agent"));
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"DaemonSet",
                "metadata":{"name":"agent", "generation":2, "labels":labels},
                "spec":{"selector":{"matchLabels":{"app":"agent"}},"template":{"spec":{"containers":[]}}},
                "status":{"desiredNumberScheduled":10, "numberReady":6, "updatedNumberScheduled":6,
                    "numberAvailable":6, "observedGeneration":2}
            }));
        });
        controller.state = KubernetesDaemonState::WaitingForUpdate;
        controller._internal_stay_count = Some(UPDATE_WAIT_MAX_POLLS - 1);

        let error = controller.step(&ctx).await.expect_err("times out");

        assert_eq!(error.code, "WORKLOAD_NOT_READY");
        // The whole budget is spent: final, so the executor does not wait out ~10 more budgets.
        assert!(!error.retryable);
        for part in [
            "desired 10, updated 6, available 6",
            "pod agent-x7k2p is Pending",
            "0/10 nodes are available: Insufficient cpu",
        ] {
            assert!(error.message.contains(part), "{}", error.message);
        }
    }

    /// During a stalled rolling update the old pods keep every node ready, so `numberReady`
    /// alone is no proof of recovery: a refresh-failed DaemonSet whose rollout is unfinished
    /// does not count ready checks, and resets any it had.
    #[tokio::test]
    async fn a_stalled_rollout_with_ready_old_pods_does_not_recover_a_failed_refresh() {
        let config = manifest_test_daemon(&[]);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let server = httpmock::MockServer::start_async().await;
        let mut ctx = harness.ctx();
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
        for (path, kind) in [("pods", "PodList"), ("events", "EventList")] {
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path(format!("/api/v1/namespaces/test-ns/{path}"));
                then.status(200).json_body(
                    serde_json::json!({"apiVersion":"v1","kind":kind,"metadata":{},"items":[]}),
                );
            });
        }
        let mut controller = manifest_test_controller();
        let labels = controller.workload_labels(&ctx, "agent", controller.build_labels("agent"));
        // All 10 old pods are ready, but only 4 run the current template.
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/daemonsets/agent");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"DaemonSet",
                "metadata":{"name":"agent", "generation":2, "labels":labels},
                "spec":{"selector":{"matchLabels":{"app":"agent"}},"template":{"spec":{"containers":[]}}},
                "status":{"desiredNumberScheduled":10, "numberReady":10, "updatedNumberScheduled":4,
                    "numberAvailable":10, "observedGeneration":2}
            }));
        });
        controller.state = KubernetesDaemonState::RefreshFailed;
        controller.recovery_checks = 2;
        let error = controller
            .step(&ctx)
            .await
            .expect_err("an unfinished rollout is not a recovery");
        assert_eq!(error.code, "WORKLOAD_NOT_READY");
        assert_eq!(controller.recovery_checks, 0);
        assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
    }
}
