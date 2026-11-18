use super::kubernetes_network::{cluster_dns_config, GatewayDns};
use crate::kubernetes_runtime_credentials::delete_environment_secret;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::kubernetes_errors::is_remote_resource_conflict;
use crate::core::{
    direct_monitoring_auth_headers, kubernetes_branded_resource_labels,
    kubernetes_cleanup_resource_labels, kubernetes_runtime_pod_labels, linked_bindings,
    pod_template_annotations, projected_env_vars,
    reconcile_environment_secret_with_additional_secrets, EnvSecretRotationTracker,
    EnvironmentVariableBuilder, KubernetesEnvSecretPlan, ResourceController,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_public_endpoint::{
    container_public_endpoint_target, delete_kubernetes_public_endpoint,
    reconcile_kubernetes_public_endpoint, KubernetesEndpointAction, KubernetesPublicEndpointState,
};
use crate::kubernetes_workload_heartbeat::{
    assess_rollout, deployment_has_ready_replicas, deployment_rollout_complete,
    emit_kubernetes_workload_heartbeat, label_selector, last_poll, readiness_holds, recovery_check,
    recovery_restart_free, rollout_budget_exhausted, rollout_timeout_message,
    stalled_rollout_message, statefulset_has_ready_replicas, unchanged_update_rechecks,
    update_wait_max_polls, workload_fully_ready, workload_pod_reason, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput, NotReadyObservations, ReadyHold,
    RolloutWait, RolloutWatch, KUBERNETES_WORKLOAD_READY_MAX_POLLS,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_resource_name, kubernetes_service_account_name, public_url_host, Container,
    ContainerCode, ContainerOutputs, ContainerStatus, PublicEndpointOutput, ResourceOutputs,
    ResourceStatus, ENV_ALIEN_RUNTIME_SECRETS,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    Capabilities, Container as K8sContainer, ContainerPort, EnvVar, HTTPGetAction,
    LocalObjectReference, PersistentVolumeClaim, PersistentVolumeClaimSpec, PodSecurityContext,
    PodSpec, PodTemplateSpec, Probe, ResourceRequirements, SeccompProfile, SecretVolumeSource,
    SecurityContext, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

#[cfg(test)]
use crate::core::applicable_secret_environment_variables;

// Cold Kubernetes nodes can spend several minutes pulling application images,
// especially when the registry endpoint is a remote proxy. Treat that as a
// legitimate provisioning phase; terminal pull failures still surface through
// workload heartbeats and events.

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

/// The container's image when it is served by the manager's registry, whose pull needs the
/// deployment's token. An image from any other registry is pulled with the access the cluster
/// already has: no Secret is created and the token is never offered to that registry. With no
/// token, an image on the manager's registry is still returned so the missing token fails loudly.
fn image_needing_pull_secret<'a>(
    code: &'a ContainerCode,
    manager_url: Option<&str>,
    mirrored: bool,
) -> Option<&'a str> {
    let ContainerCode::Image { image } = code else {
        return None;
    };
    // A site mirror authenticates with the installation's own pull secrets. The manager's
    // token must never be sent to it.
    if mirrored {
        return None;
    }
    let manager_url = manager_url?;
    let manager_host = alien_core::image_rewrite::strip_url_scheme(manager_url);
    let image_host = image.split('/').next().unwrap_or_default();
    (image_host == manager_host).then_some(image.as_str())
}

/// Longer than the gateway's 30 s resynchronization interval plus resolver caching.
const GATEWAY_READY_DELAY_SECONDS: i32 = 45;

/// Whether `config` is the sandbox gateway on an image that reads its configuration and replica
/// secret from mounted files. Then the controller mounts both Secrets and reads neither.
fn gateway_mounts_config(config: &Container) -> bool {
    config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID
        && matches!(&config.code, ContainerCode::Image { image }
            if alien_core::sandbox_egress::gateway_image_reads_config_files(image))
}

/// Environment entry holding the gateway settings a running replica cannot refresh.
const GATEWAY_RESTART_SETTINGS_ENV: &str = "ALIEN_EGRESS_GATEWAY_RESTART_SETTINGS";

fn first_declared_container_port(config: &Container) -> Option<u16> {
    config.ports.first().map(|port| port.port)
}

fn deployment_failure_progress(deployment: &Deployment) -> Option<String> {
    let status = deployment.status.as_ref()?;
    if status.observed_generation < deployment.metadata.generation {
        return None;
    }
    let failure = status
        .conditions
        .as_ref()?
        .iter()
        .find(|condition| condition.type_ == "ReplicaFailure" && condition.status == "True")?;
    let reason = failure.reason.as_deref().unwrap_or("ReplicaFailure");
    let message = failure
        .message
        .as_deref()
        .unwrap_or("Could not create replicas");
    Some(alien_core::kubernetes_priority_class_failure_hint(
        &format!("{reason}: {message}"),
    ))
}

fn workload_ready(
    desired_replicas: i32,
    ready_replicas: Option<i32>,
    replicas: Option<i32>,
    observed_generation: Option<i64>,
    generation: Option<i64>,
) -> bool {
    if desired_replicas == 0 {
        // Kubernetes omits zero replica counters. Wait for the controller to
        // observe this generation before accepting the scaled-down workload.
        return observed_generation.is_some_and(|observed| observed >= generation.unwrap_or(1))
            && ready_replicas.unwrap_or(0) == 0
            && replicas.unwrap_or(0) == 0;
    }
    replicas.is_some_and(|total| {
        total > 0 && ready_replicas.unwrap_or(0) >= desired_replicas.min(total)
    })
}

fn statefulset_rollout_complete(
    statefulset: &StatefulSet,
    desired_replicas: i32,
    expected_generation: Option<i64>,
) -> bool {
    let (Some(generation), Some(status)) = (statefulset.metadata.generation, &statefulset.status)
    else {
        return false;
    };

    Some(generation) == expected_generation
        && status
            .observed_generation
            .is_some_and(|observed| observed >= generation)
        && status.updated_replicas.unwrap_or(0) == desired_replicas
        && status.replicas == desired_replicas
        && status.ready_replicas.unwrap_or(0) >= desired_replicas
        && status.current_revision.is_some()
        && status.current_revision == status.update_revision
}

pub(super) fn kubernetes_port_name(port: &alien_core::ContainerPort) -> String {
    format!("tcp-{}", port.port)
}

/// DNS name of a container's internal Service: `<name>.<namespace>.svc.<domain>`.
/// Without a known cluster domain the name stops at `.svc`, which the pod's
/// resolver search path completes; a guessed domain would be wrong on clusters
/// that don't use `cluster.local`.
fn internal_dns_name(workload_name: &str, namespace: Option<&str>, domain: Option<&str>) -> String {
    let service = match namespace {
        Some(namespace) => format!("{workload_name}.{namespace}.svc"),
        None => format!("{workload_name}.svc"),
    };
    match domain {
        Some(domain) => format!("{service}.{domain}"),
        None => service,
    }
}

#[controller]
pub struct KubernetesContainerController {
    /// The name of the created Kubernetes Deployment or StatefulSet.
    pub(crate) workload_name: Option<String>,
    /// Whether the workload is a StatefulSet (true) or Deployment (false).
    pub(crate) is_stateful: bool,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
    /// The service name for the container (for binding construction)
    pub(crate) service_name: Option<String>,
    /// Headless discovery for the ready replicas of an internet-only Container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) peer_service_name: Option<String>,
    /// The first declared service port for binding construction.
    pub(crate) service_port: Option<u16>,
    #[serde(default)]
    pub(crate) network_policy_name: Option<String>,
    /// All declared ports, retained for per-port internal binding URLs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) service_ports: Vec<u16>,
    /// The container ID (for binding construction)
    pub(crate) container_id: Option<String>,
    /// Whether the previous health check also found zero ready replicas.
    #[serde(default)]
    pub(crate) zero_ready_observed: bool,
    /// Not-ready observations of the Running workload; the third in a window fails the refresh.
    #[serde(default)]
    pub(crate) not_ready: NotReadyObservations,
    /// Ready polls of the create or update wait; the rollout completes when readiness holds.
    #[serde(default)]
    pub(crate) ready_hold: ReadyHold,
    /// Whether the last update left the pod template as it was (no pods roll). Such an update
    /// waits for readiness only briefly.
    #[serde(default)]
    pub(crate) update_template_unchanged: bool,
    /// When the current rollout was first seen unfinished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) rollout_watch: Option<RolloutWatch>,
    /// Consecutive healthy checks since the resource failed a health check.
    #[serde(default)]
    pub(crate) recovery_checks: u32,
    /// Set only while RefreshFailed runs the health check; a recovery needs a finished rollout.
    /// Set only while the RefreshFailed handler runs its health check; never stored.
    #[serde(skip)]
    pub(crate) recovering: bool,
    /// When the last healthy check counted towards recovery.
    #[serde(default)]
    pub(crate) last_recovery_check_unix: Option<i64>,
    /// Public endpoint route/certificate state.
    pub(crate) public_endpoint: KubernetesPublicEndpointState,
    /// Tracks the env-Secret checksum so `needs_update` can detect secret
    /// rotations that config diffing cannot see (secrets are projected via
    /// secretKeyRef, never into the resource config).
    #[serde(default)]
    pub(crate) env_secret: EnvSecretRotationTracker,
    /// Generation returned by the Kubernetes update call. A subsequent read of
    /// the previous generation must not complete this rollout.
    #[serde(default)]
    pub(crate) update_generation: Option<i64>,
    /// Kubernetes' explanation for a workload that cannot create replicas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) workload_failure: Option<String>,
    /// PriorityClass name applied to the pod spec last. The chart's class mapping is read at
    /// pod-spec build time, so a Helm upgrade that changes it is invisible to config diffing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applied_priority_class: Option<String>,
    /// The single architecture the release recorded when the pod spec was last applied (None: no
    /// record, or several). It is read at pod-spec build time, so a release that records one for
    /// an existing container is invisible to config diffing. Only this is compared, never the whole
    /// selector, so a pool's selector cannot make an unrecorded workload roll.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applied_architecture: Option<alien_core::instance_catalog::Architecture>,
    /// Resolver last applied to the sandbox gateway's pods. Read when the pod spec is built, so a
    /// later change (the Operator can read the cluster DNS Service, or its address moved) is
    /// invisible to config diffing. `None` for a gateway created before this was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applied_gateway_dns: Option<GatewayDns>,
}

/// The `nodeSelector` the Pod gets: the pool or stack architecture, else the single
/// architecture the build recorded for this container.
fn desired_node_selector(
    ctx: &ResourceControllerContext<'_>,
    config: &Container,
) -> Result<Option<BTreeMap<String, String>>> {
    let pool =
        alien_core::kubernetes_container_pool(ctx.desired_stack, config).map_err(|message| {
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
impl KubernetesContainerController {
    fn get_progress(&self) -> Option<&str> {
        match self.state {
            KubernetesContainerState::WaitingForWorkload
            | KubernetesContainerState::WaitingForUpdate => Some(
                self.workload_failure
                    .as_deref()
                    .unwrap_or("Waiting for workload readiness."),
            ),
            _ => None,
        }
    }

    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;

        info!(id=%config.id, "Initiating Kubernetes Container creation");

        let container_name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;
        let registry_secret_name = format!("{}-registry", container_name);
        let environment_secret_name = format!("{}-env", container_name);
        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let (legacy_registry_owner_proven, legacy_environment_owner_proven) = if config.stateful {
            match deployment_client
                .get_statefulset(&namespace, &container_name)
                .await
            {
                Ok(existing) => {
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        &container_name,
                        &config.id,
                    ) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Refusing to create Container '{}' because StatefulSet '{container_name}' is not owned by this deployment",
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
                            "Failed to inspect StatefulSet '{container_name}' before Container creation"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            match deployment_client
                .get_deployment(&namespace, &container_name)
                .await
            {
                Ok(existing) => {
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        &container_name,
                        &config.id,
                    ) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Refusing to create Container '{}' because Deployment '{container_name}' is not owned by this deployment",
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
                            "Failed to inspect Deployment '{container_name}' before Container creation"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        };

        // Store data needed for binding construction
        self.container_id = Some(config.id.clone());
        self.service_name = Some(container_name.clone());
        self.service_port = first_declared_container_port(config);
        self.service_ports = config.ports.iter().map(|port| port.port).collect();
        self.namespace = Some(namespace.clone());
        // Generate ServiceAccount name following Helm naming convention
        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        crate::kubernetes_runtime_credentials::reconcile(
            ctx,
            &namespace,
            &container_name,
            &config.id,
            alien_core::commands_types::CommandTargetType::Container,
        )
        .await?;
        let image_pull_secret_name = if let Some(image) = image_needing_pull_secret(
            &config.code,
            ctx.deployment_config.manager_url.as_deref(),
            ctx.deployment_config.image_registry.is_some(),
        ) {
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
        let env_secret_plan = self
            .reconcile_environment_secret(
                config,
                &container_name,
                &namespace,
                legacy_environment_owner_proven,
                ctx,
            )
            .await?;
        self.refresh_gateway_dns(
            config,
            crate::sandbox::kubernetes_egress::detected_cluster_domain(),
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
        self.reconcile_network_policy(config, &container_name, &namespace, ctx)
            .await?;
        self.reconcile_internal_service(config, &container_name, &namespace, ctx)
            .await?;

        self.update_generation = None;
        self.is_stateful = config.stateful;
        self.workload_name = Some(container_name.clone());

        if config.stateful {
            // Create StatefulSet for stateful containers
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let statefulset = self
                .build_statefulset(
                    config,
                    &container_name,
                    &namespace,
                    &service_account_name,
                    image_pull_secret_name.as_deref(),
                    env_secret_plan.as_ref(),
                    ctx,
                )
                .await?;

            match deployment_client
                .create_statefulset(&namespace, &statefulset)
                .await
            {
                Ok(_) => {
                    info!(statefulset_name=%container_name, namespace=%namespace, "StatefulSet creation initiated");
                }
                Err(err) if is_remote_resource_conflict(&err) => {
                    let existing = deployment_client
                        .get_statefulset(&namespace, &container_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to read existing statefulset '{}' before adoption.",
                                container_name
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        &container_name,
                        &config.id,
                    ) {
                        return Err(err.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Refusing to adopt unmanaged statefulset '{}'.",
                                container_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    let mut adopted = existing;
                    let spec = adopted.spec.as_mut().ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!("Existing statefulset '{container_name}' has no spec"),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                    let desired = statefulset
                        .spec
                        .as_ref()
                        .expect("generated workload has a spec");
                    // Keep immutable identity and storage fields while converging the failed create.
                    spec.template = desired.template.clone();
                    spec.replicas = desired.replicas;
                    let updated = deployment_client
                        .update_statefulset(&namespace, &container_name, &adopted)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to reconcile adopted statefulset '{container_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    self.update_generation =
                        Some(updated.metadata.generation.ok_or_else(|| {
                            AlienError::new(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Adopted statefulset '{container_name}' has no generation"
                                ),
                                resource_id: Some(config.id.clone()),
                            })
                        })?);
                    info!(statefulset_name=%container_name, namespace=%namespace, "Adopting existing Kubernetes StatefulSet");
                }
                Err(err) => {
                    return Err(err.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create statefulset '{}'.", container_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            // Create Deployment for stateless containers
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let deployment = self
                .build_deployment(
                    config,
                    &container_name,
                    &namespace,
                    &service_account_name,
                    image_pull_secret_name.as_deref(),
                    env_secret_plan.as_ref(),
                    ctx,
                )
                .await?;

            match deployment_client
                .create_deployment(&namespace, &deployment)
                .await
            {
                Ok(_) => {
                    info!(deployment_name=%container_name, namespace=%namespace, "Deployment creation initiated");
                }
                Err(err) if is_remote_resource_conflict(&err) => {
                    let existing = deployment_client
                        .get_deployment(&namespace, &container_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to read existing deployment '{}' before adoption.",
                                container_name
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        &container_name,
                        &config.id,
                    ) {
                        return Err(err.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Refusing to adopt unmanaged deployment '{}'.",
                                container_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    let mut adopted = existing;
                    let spec = adopted.spec.as_mut().ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!("Existing deployment '{container_name}' has no spec"),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                    let desired = deployment
                        .spec
                        .as_ref()
                        .expect("generated workload has a spec");
                    // Keep immutable identity and storage fields while converging the failed create.
                    spec.template = desired.template.clone();
                    spec.replicas = desired.replicas;
                    let updated = deployment_client
                        .update_deployment(&namespace, &container_name, &adopted)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to reconcile adopted deployment '{container_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                    self.update_generation =
                        Some(updated.metadata.generation.ok_or_else(|| {
                            AlienError::new(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Adopted deployment '{container_name}' has no generation"
                                ),
                                resource_id: Some(config.id.clone()),
                            })
                        })?);
                    info!(deployment_name=%container_name, namespace=%namespace, "Adopting existing Kubernetes Deployment");
                }
                Err(err) => {
                    return Err(err.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create deployment '{}'.", container_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: WaitingForWorkload,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForWorkload,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
        progress = "Waiting for workload readiness.",
    )]
    async fn waiting_for_workload(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.workload_failure = None;
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;

        // A gateway whose resolver does not work never becomes ready, so it never reaches the
        // Ready handler where the resolver is compared; it is compared here too.
        if config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID
            && self
                .gateway_dns_drifted(
                    crate::sandbox::kubernetes_egress::detected_cluster_domain(),
                    ctx,
                )
                .await?
        {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let workload_name = self.workload_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Workload name not set in state".to_string(),
            })
        })?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        let mut template = None;
        // Check workload status (different API for Deployment vs StatefulSet)
        let (ready_replicas, replicas, healthy) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
                    self.replace_obsolete_unready_pod(&statefulset, config, namespace, ctx)
                        .await?;
                    template = statefulset.spec.as_ref().map(|spec| spec.template.clone());

                    if let Some(status) = &statefulset.status {
                        (
                            status.ready_replicas,
                            Some(status.replicas),
                            workload_ready(
                                config.replicas.unwrap_or(1) as i32,
                                status.ready_replicas,
                                Some(status.replicas),
                                status.observed_generation,
                                statefulset.metadata.generation,
                            ) && self.update_generation.is_none_or(|expected| {
                                // All existing replicas must use the adopted template. Keep the
                                // create readiness rule for replicas the cluster could start.
                                statefulset.metadata.generation == Some(expected)
                                    && status
                                        .observed_generation
                                        .is_some_and(|observed| observed >= expected)
                                    && status.updated_replicas.unwrap_or(0) == status.replicas
                            }),
                        )
                    } else {
                        (None, None, false)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "StatefulSet not yet available, continuing to wait");
                    (None, None, false)
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get statefulset '{}'.", workload_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            match deployment_client
                .get_deployment(namespace, workload_name)
                .await
            {
                Ok(deployment) => {
                    self.workload_failure = deployment_failure_progress(&deployment);
                    template = deployment.spec.as_ref().map(|spec| spec.template.clone());
                    if let Some(status) = &deployment.status {
                        (
                            status.ready_replicas,
                            status.replicas,
                            workload_ready(
                                config.replicas.unwrap_or(1) as i32,
                                status.ready_replicas,
                                status.replicas,
                                status.observed_generation,
                                deployment.metadata.generation,
                            ) && self.update_generation.is_none_or(|expected| {
                                // All existing replicas must use the adopted template. Keep the
                                // create readiness rule for replicas the cluster could start.
                                deployment.metadata.generation == Some(expected)
                                    && status
                                        .observed_generation
                                        .is_some_and(|observed| observed >= expected)
                                    && status.updated_replicas.unwrap_or(0)
                                        == status.replicas.unwrap_or(0)
                            }),
                        )
                    } else {
                        (None, None, false)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "Deployment not yet available, continuing to wait");
                    (None, None, false)
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get deployment '{}'.", workload_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        };

        let ready_selector =
            self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
        if healthy && readiness_holds(ctx, &mut self.ready_hold, namespace, &ready_selector).await?
        {
            let workload_type = if self.is_stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            info!(workload_name=%workload_name, namespace=%namespace, workload_type=%workload_type, "Container workload is ready");
            self.ready_hold.reset();

            return Ok(HandlerAction::Continue {
                state: ReconcilePublicEndpoint,
                suggested_delay: None,
            });
        }
        if !healthy {
            self.ready_hold.reset();
        }
        debug!(workload_name=%workload_name, ready=?ready_replicas, total=?replicas, "Container workload not yet ready");
        let restricted_hint = if let Some(template) = &template {
            crate::core::fail_if_root_image_blocked(
                ctx,
                namespace,
                &self.workload_labels(ctx, &config.id, self.build_labels(workload_name)),
                template,
                &config.id,
            )
            .await?
        } else {
            None
        };
        let priority_failure = if self.is_stateful {
            crate::kubernetes_priority_events::missing_priority_class_progress(
                ctx,
                config.priority,
                namespace,
                workload_name,
            )
            .await
        } else {
            None
        };
        self.workload_failure = priority_failure
            .or(restricted_hint)
            .or(self.workload_failure.take());

        if last_poll(
            self._internal_stay_count,
            KUBERNETES_WORKLOAD_READY_MAX_POLLS,
        ) {
            self.ready_hold.reset();
            let selector = self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
            let pod_reason =
                workload_pod_reason(ctx, namespace, &selector, self.workload_failure.as_deref())
                    .await;
            return Err(rollout_budget_exhausted(AlienError::new(
                ErrorData::WorkloadNotReady {
                    resource_id: config.id.clone(),
                    message: rollout_timeout_message(
                        if self.is_stateful {
                            "StatefulSet"
                        } else {
                            "Deployment"
                        },
                        workload_name,
                        &format!(
                            "desired {}, ready {}, total {}",
                            config.replicas.unwrap_or(1),
                            ready_replicas.unwrap_or(0),
                            replicas.unwrap_or(0)
                        ),
                        pod_reason.as_deref(),
                    ),
                },
            )));
        }

        Ok(HandlerAction::Stay {
            max_times: Some(KUBERNETES_WORKLOAD_READY_MAX_POLLS),
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
        let config = ctx.desired_resource_config::<Container>()?;
        let workload_name = self.workload_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Workload name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let labels = self.build_labels(workload_name);
        let action = reconcile_kubernetes_public_endpoint(
            ctx,
            container_public_endpoint_target(
                &config.id,
                workload_name,
                namespace,
                labels,
                &config.public_endpoints,
                config
                    .health_check
                    .as_ref()
                    .map(|check| check.path.as_str()),
            )?,
            &mut self.public_endpoint,
        )
        .await?;

        match action {
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

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;

        // Heartbeat check: verify workload status
        if let (Some(workload_name), Some(namespace)) =
            (self.workload_name.clone(), self.namespace.clone())
        {
            let workload_name = &workload_name;
            let namespace = &namespace;
            crate::kubernetes_runtime_credentials::reconcile(
                ctx,
                namespace,
                workload_name,
                &config.id,
                alien_core::commands_types::CommandTargetType::Container,
            )
            .await?;

            if let ContainerCode::Image { image } = &config.code {
                crate::kubernetes_registry::refresh_pull_secret(
                    ctx,
                    namespace,
                    workload_name,
                    &config.id,
                    image,
                )
                .await?;
            }
            // Gateway configuration is read from the vault, so its changes are detected
            // here rather than by the synchronous deployment-snapshot check.
            if config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
                if self
                    .gateway_environment_drifted(config, workload_name, ctx)
                    .await?
                    || self
                        .gateway_dns_drifted(
                            crate::sandbox::kubernetes_egress::detected_cluster_domain(),
                            ctx,
                        )
                        .await?
                {
                    return Ok(HandlerAction::Continue {
                        state: UpdateStart,
                        suggested_delay: None,
                    });
                }
            }
            if config.id != alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
                self.env_secret.adopt_unkeyed(
                    &config.id,
                    workload_name,
                    &ctx.deployment_config.environment_variables.variables,
                    &direct_monitoring_auth_headers(ctx),
                )?;
            }
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            let (ready_replicas, replicas, workload) = if self.is_stateful {
                let statefulset = deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get statefulset '{}'", workload_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                if let Some(status) = statefulset.status.clone() {
                    (
                        status.ready_replicas,
                        Some(status.replicas),
                        KubernetesWorkload::StatefulSet(StatefulSet {
                            status: Some(status),
                            ..statefulset
                        }),
                    )
                } else {
                    (None, None, KubernetesWorkload::StatefulSet(statefulset))
                }
            } else {
                let deployment = deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get deployment '{}'", workload_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                if let Some(status) = deployment.status.clone() {
                    (
                        status.ready_replicas,
                        status.replicas,
                        KubernetesWorkload::Deployment(Deployment {
                            status: Some(status),
                            ..deployment
                        }),
                    )
                } else {
                    (None, None, KubernetesWorkload::Deployment(deployment))
                }
            };

            // A linked resource can change what its binding publishes without this container
            // being updated, such as a sandbox reading its broker address again on its own Ready.
            // Compare before replica health: a stale binding can be why a replica is unhealthy.
            let live_pod_spec = match &workload {
                KubernetesWorkload::Deployment(deployment) => deployment
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.template.spec.as_ref()),
                KubernetesWorkload::StatefulSet(statefulset) => statefulset
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.template.spec.as_ref()),
                _ => None,
            };
            if let Some(live) = linked_bindings::pod_template_env(
                live_pod_spec,
                &kubernetes_resource_name("", &config.id),
            ) {
                let drifted = linked_bindings::kubernetes_linked_binding_drift(
                    ctx,
                    &config.links,
                    &config.id,
                    live,
                )
                .await?;
                if !drifted.is_empty() {
                    // Names only: binding values can carry secrets.
                    info!(
                        container = %config.id,
                        variables = ?drifted,
                        reason = "linked binding drift",
                        "Refreshing the container's linked bindings"
                    );
                    return Ok(HandlerAction::Continue {
                        state: UpdateStart,
                        suggested_delay: None,
                    });
                }
            }

            let healthy = match &workload {
                KubernetesWorkload::Deployment(deployment) => {
                    deployment_has_ready_replicas(deployment)
                }
                KubernetesWorkload::StatefulSet(statefulset) => {
                    statefulset_has_ready_replicas(statefulset)
                }
                KubernetesWorkload::DaemonSet(_) => false,
            };
            let workload_kind = if self.is_stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            let assessment = assess_rollout(
                &mut self.rollout_watch,
                &workload,
                chrono::Utc::now().timestamp(),
            );
            let selector = self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
            if let Some(cause) = &assessment.stall {
                // The new version is not rolling out, even if old pods still serve.
                let pod_reason = workload_pod_reason(
                    ctx,
                    namespace,
                    &selector,
                    self.workload_failure.as_deref(),
                )
                .await;
                if healthy || ready_replicas.unwrap_or(0) > 0 {
                    return self
                        .not_ready
                        .outcome(
                            AlienError::new(ErrorData::WorkloadNotReady {
                                resource_id: config.id.clone(),
                                message: stalled_rollout_message(
                                    workload_kind,
                                    workload_name,
                                    &format!(
                                        "{cause}; {} of {} replicas ready",
                                        ready_replicas.unwrap_or(0),
                                        replicas.unwrap_or(0)
                                    ),
                                    pod_reason.as_deref(),
                                ),
                            }),
                            self.recovering,
                            chrono::Utc::now().timestamp(),
                        )
                        .map(|delay| HandlerAction::Stay {
                            max_times: None,
                            suggested_delay: Some(delay),
                        });
                }
            }
            if !healthy && matches!(assessment.wait, RolloutWait::Wait) {
                // Kubernetes is still replacing replicas. Check again without using
                // the retry budget; the bound is the rollout's own time limit.
                self.zero_ready_observed = false;
                self.recovery_checks = 0;
                self.last_recovery_check_unix = None;
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(Duration::from_secs(10)),
                });
            }
            let zero_ready = ready_replicas.unwrap_or(0) == 0;
            self.zero_ready_observed = !healthy && zero_ready;
            if !healthy {
                let pod_reason = workload_pod_reason(
                    ctx,
                    namespace,
                    &selector,
                    self.workload_failure.as_deref(),
                )
                .await;
                let cause = assessment.stall.clone().unwrap_or_default();
                return self.not_ready.outcome(AlienError::new(ErrorData::WorkloadNotReady {
                    resource_id: config.id.clone(),
                    message: format!(
                        "{workload_kind} '{workload_name}' has {} ready replicas out of {} observed{}{}",
                        ready_replicas.unwrap_or(0),
                        replicas.unwrap_or(0),
                        if cause.is_empty() { String::new() } else { format!("; {cause}") },
                        pod_reason.map(|r| format!("; {r}")).unwrap_or_default(),
                    ),
                }), self.recovering, crate::kubernetes_workload_heartbeat::not_ready_now_unix()).map(|delay| HandlerAction::Stay {
 max_times: None,
 suggested_delay: Some(delay),
 });
            }

            self.not_ready.clear();
            if self.recovering && !workload_fully_ready(&workload) {
                // Old pods serving is not recovery: the new version has not finished rolling out.
                self.zero_ready_observed = false;
                self.recovery_checks = 0;
                self.last_recovery_check_unix = None;
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(Duration::from_secs(10)),
                });
            }

            let labels = self.build_labels(workload_name);
            emit_kubernetes_workload_heartbeat(
                ctx,
                KubernetesWorkloadHeartbeatInput {
                    deployment_id: None,
                    resource_id: config.id.clone(),
                    resource_type: Container::RESOURCE_TYPE,
                    data_kind: KubernetesWorkloadDataKind::Container,
                    command_supported: false,
                    namespace: namespace.clone(),
                    workload_name: workload_name.clone(),
                    workload_kind: if self.is_stateful {
                        alien_core::KubernetesWorkloadKind::StatefulSet
                    } else {
                        alien_core::KubernetesWorkloadKind::Deployment
                    },
                    workload,
                    label_selector: label_selector(&labels),
                },
            )
            .await?;

            self.reconcile_network_policy(config, workload_name, namespace, ctx)
                .await?;
            self.reconcile_internal_service(config, workload_name, namespace, ctx)
                .await?;

            let action = reconcile_kubernetes_public_endpoint(
                ctx,
                container_public_endpoint_target(
                    &config.id,
                    workload_name,
                    namespace,
                    labels,
                    &config.public_endpoints,
                    config
                        .health_check
                        .as_ref()
                        .map(|check| check.path.as_str()),
                )?,
                &mut self.public_endpoint,
            )
            .await?;
            if let KubernetesEndpointAction::Waiting { suggested_delay } = action {
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(suggested_delay),
                });
            }

            // Dependents above are migrated first. Stamp the root last so the uninstall hook can
            // use it as durable proof for exact legacy env/registry Secret cleanup.
            let desired_labels =
                self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
            if self.is_stateful {
                let mut existing = deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to inspect StatefulSet '{workload_name}' for ownership migration"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !crate::core::kubernetes_labels_match_current_scope(
                    existing.metadata.labels.as_ref(),
                    &desired_labels,
                ) {
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        workload_name,
                        &config.id,
                    ) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Refusing to migrate foreign StatefulSet '{workload_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    existing
                        .metadata
                        .labels
                        .get_or_insert_default()
                        .extend(desired_labels.clone());
                    deployment_client
                        .update_statefulset(namespace, workload_name, &existing)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to migrate cleanup ownership for StatefulSet '{workload_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            } else {
                let mut existing = deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to inspect Deployment '{workload_name}' for ownership migration"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !crate::core::kubernetes_labels_match_current_scope(
                    existing.metadata.labels.as_ref(),
                    &desired_labels,
                ) {
                    if !self.is_managed_workload(
                        ctx,
                        existing.metadata.labels.as_ref(),
                        workload_name,
                        &config.id,
                    ) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Refusing to migrate foreign Deployment '{workload_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    existing
                        .metadata
                        .labels
                        .get_or_insert_default()
                        .extend(desired_labels);
                    deployment_client
                        .update_deployment(namespace, workload_name, &existing)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to migrate cleanup ownership for Deployment '{workload_name}'"
                            ),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            }

            debug!(workload_name=%workload_name, namespace=%namespace, "Container workload is healthy");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.zero_ready_observed = false;
        self.not_ready.clear();
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;
        // A refresh started from Ready (gateway configuration, linked binding drift) can come
        // before any planned update; the configuration in place is then the current one.
        let previous = match ctx
            .state
            .resources
            .get(ctx.desired_config.id())
            .and_then(|resource| resource.previous_config.as_ref())
        {
            Some(_) => ctx.previous_resource_config::<Container>()?,
            None => config,
        };
        if previous
            .persistent_storage
            .as_ref()
            .map(|storage| &storage.mount_path)
            != config
                .persistent_storage
                .as_ref()
                .map(|storage| &storage.mount_path)
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Adding or removing persistent storage or changing its mount path during a Kubernetes update is unsupported".into(),
                resource_id: Some(config.id.clone()),
            }));
        }
        let capacity = |container: &Container| {
            container
                .persistent_storage
                .as_ref()
                .map(|storage| {
                    alien_core::instance_catalog::parse_memory_bytes(&storage.size).map_err(
                        |message| {
                            AlienError::new(ErrorData::ResourceConfigInvalid {
                                message,
                                resource_id: Some(config.id.clone()),
                            })
                        },
                    )
                })
                .transpose()
        };
        let unchanged_spelling = previous
            .persistent_storage
            .as_ref()
            .map(|storage| &storage.size)
            == config
                .persistent_storage
                .as_ref()
                .map(|storage| &storage.size);
        // Unchanged Kubernetes quantities need no parsing. Keep supporting all
        // quantities accepted during installation, including decimal exponents.
        if !unchanged_spelling && capacity(previous)? != capacity(config)? {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Changing persistent storage on Kubernetes requires an explicit PVC expansion and StatefulSet migration; volumeClaimTemplates are immutable".into(),
                resource_id: Some(config.id.clone()),
            }));
        }
        // Equivalent quantities must retain the installed spelling in the immutable
        // StatefulSet claim template (for example, 1Ti and 1024Gi).
        let mut normalized_config = config.clone();
        if let (Some(installed), Some(desired)) = (
            &previous.persistent_storage,
            &mut normalized_config.persistent_storage,
        ) {
            desired.size.clone_from(&installed.size);
        }
        let config = &normalized_config;

        let workload_name = self.workload_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Workload name not set in state".to_string(),
            })
        })?;
        let workload_name_owned = workload_name.clone();
        let workload_name = &workload_name_owned;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let namespace_owned = namespace.clone();
        let namespace = &namespace_owned;

        // The pod spec reads every link's binding. Wait for them before the first write, so a
        // dependent stepped while its dependency updates neither calls the API nor changes its
        // Secrets and Services ahead of its workload.
        crate::core::ensure_linked_resources_running(&config.links, ctx, &config.id)?;

        let workload_type = if self.is_stateful {
            "StatefulSet"
        } else {
            "Deployment"
        };
        info!(workload_name=%workload_name, workload_type=%workload_type, "Updating Kubernetes Container workload");

        // Prove ownership of the root workload before mutating any dependent
        // Secret or Service. A same-name foreign workload must not be able to
        // authorize migration of legacy, unscoped dependents.
        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let registry_secret_name = format!("{}-registry", workload_name);
        let environment_secret_name = format!("{}-env", workload_name);
        let mut generation_before = None;
        let (resource_version, legacy_registry_owner_proven, legacy_environment_owner_proven) =
            if self.is_stateful {
                let existing = deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get statefulset '{}' before update",
                            workload_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !self.is_managed_workload(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    workload_name,
                    &config.id,
                ) {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Refusing to update StatefulSet '{workload_name}' because it is not owned by Container '{}' in this deployment",
                        config.id
                    ),
                    resource_id: Some(config.id.clone()),
                }));
                }
                generation_before = existing.metadata.generation;
                let pod_spec = existing
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.template.spec.as_ref());
                (
                    existing.metadata.resource_version.clone(),
                    crate::kubernetes_registry::pod_spec_references_image_pull_secret(
                        pod_spec,
                        &registry_secret_name,
                    ),
                    crate::core::pod_spec_references_environment_secret(
                        pod_spec,
                        &environment_secret_name,
                    ),
                )
            } else {
                let existing = deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get deployment '{}' before update",
                            workload_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !self.is_managed_workload(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    workload_name,
                    &config.id,
                ) {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Refusing to update Deployment '{workload_name}' because it is not owned by Container '{}' in this deployment",
                        config.id
                    ),
                    resource_id: Some(config.id.clone()),
                }));
                }
                generation_before = existing.metadata.generation;
                let pod_spec = existing
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.template.spec.as_ref());
                (
                    existing.metadata.resource_version.clone(),
                    crate::kubernetes_registry::pod_spec_references_image_pull_secret(
                        pod_spec,
                        &registry_secret_name,
                    ),
                    crate::core::pod_spec_references_environment_secret(
                        pod_spec,
                        &environment_secret_name,
                    ),
                )
            };

        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        crate::kubernetes_runtime_credentials::reconcile(
            ctx,
            namespace,
            workload_name,
            &config.id,
            alien_core::commands_types::CommandTargetType::Container,
        )
        .await?;
        let image_pull_secret_name = if let Some(image) = image_needing_pull_secret(
            &config.code,
            ctx.deployment_config.manager_url.as_deref(),
            ctx.deployment_config.image_registry.is_some(),
        ) {
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
        let env_secret_plan = self
            .reconcile_environment_secret(
                config,
                workload_name,
                namespace,
                legacy_environment_owner_proven,
                ctx,
            )
            .await?;
        self.refresh_gateway_dns(
            config,
            crate::sandbox::kubernetes_egress::detected_cluster_domain(),
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
        self.service_port = first_declared_container_port(config);
        self.reconcile_network_policy(config, workload_name, namespace, ctx)
            .await?;
        self.service_ports = config.ports.iter().map(|port| port.port).collect();
        self.reconcile_internal_service(config, workload_name, namespace, ctx)
            .await?;
        if self.is_stateful {
            let mut new_statefulset = self
                .build_statefulset(
                    config,
                    workload_name,
                    namespace,
                    &service_account_name,
                    image_pull_secret_name.as_deref(),
                    env_secret_plan.as_ref(),
                    ctx,
                )
                .await?;
            new_statefulset.metadata.resource_version = resource_version;

            let updated = deployment_client
                .update_statefulset(namespace, workload_name, &new_statefulset)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update statefulset '{}'.", workload_name),
                    resource_id: Some(config.id.clone()),
                })?;
            self.update_template_unchanged =
                generation_before.is_some_and(|before| Some(before) == updated.metadata.generation);
            self.update_generation = Some(updated.metadata.generation.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Updated statefulset '{}' has no generation", workload_name),
                    resource_id: Some(config.id.clone()),
                })
            })?);
        } else {
            let mut new_deployment = self
                .build_deployment(
                    config,
                    workload_name,
                    namespace,
                    &service_account_name,
                    image_pull_secret_name.as_deref(),
                    env_secret_plan.as_ref(),
                    ctx,
                )
                .await?;
            new_deployment.metadata.resource_version = resource_version;

            let updated = deployment_client
                .update_deployment(namespace, workload_name, &new_deployment)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update deployment '{}'.", workload_name),
                    resource_id: Some(config.id.clone()),
                })?;
            self.update_template_unchanged =
                generation_before.is_some_and(|before| Some(before) == updated.metadata.generation);
            self.update_generation = Some(updated.metadata.generation.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Updated deployment '{}' has no generation", workload_name),
                    resource_id: Some(config.id.clone()),
                })
            })?);
        }

        info!(workload_name=%workload_name, workload_type=%workload_type, "Workload update submitted, waiting for rollout");

        Ok(HandlerAction::Continue {
            state: WaitingForUpdate,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
        progress = "Waiting for workload readiness.",
    )]
    async fn waiting_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.workload_failure = None;
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;

        let workload_name = self.workload_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Workload name not set in state".to_string(),
            })
        })?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        let desired_replicas = config.replicas.unwrap_or(1) as i32;
        let mut template = None;
        let (ready_replicas, replicas, rollout_complete) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
                    template = statefulset.spec.as_ref().map(|spec| spec.template.clone());
                    let ready = statefulset
                        .status
                        .as_ref()
                        .and_then(|status| status.ready_replicas);
                    let replicas = statefulset.status.as_ref().map(|status| status.replicas);
                    (
                        ready,
                        replicas,
                        statefulset_rollout_complete(
                            &statefulset,
                            desired_replicas,
                            self.update_generation,
                        ),
                    )
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get statefulset '{}' during update wait",
                            workload_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            match deployment_client
                .get_deployment(namespace, workload_name)
                .await
            {
                Ok(deployment) => {
                    self.workload_failure = deployment_failure_progress(&deployment);
                    template = deployment.spec.as_ref().map(|spec| spec.template.clone());
                    let ready = deployment
                        .status
                        .as_ref()
                        .and_then(|status| status.ready_replicas);
                    let replicas = deployment
                        .status
                        .as_ref()
                        .and_then(|status| status.replicas);
                    (
                        ready,
                        replicas,
                        deployment_rollout_complete(
                            &deployment,
                            desired_replicas,
                            self.update_generation,
                        ),
                    )
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get deployment '{}' during update wait",
                            workload_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        };

        let selector = self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
        if rollout_complete
            && readiness_holds(ctx, &mut self.ready_hold, namespace, &selector).await?
        {
            let workload_type = if self.is_stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            info!(workload_name=%workload_name, workload_type=%workload_type, "Container workload rollout complete");
            self.ready_hold.reset();
            return Ok(HandlerAction::Continue {
                state: ReconcilePublicEndpointAfterUpdate,
                suggested_delay: None,
            });
        }
        if !rollout_complete {
            self.ready_hold.reset();
        }
        debug!(workload_name=%workload_name, ready=?ready_replicas, total=?replicas, desired=desired_replicas, expected_generation=?self.update_generation, "Container workload rollout in progress");
        // A re-rendered workload can start refusing an image or filesystem the old one allowed:
        // say so instead of waiting out the rollout.
        let restricted_hint = if let Some(template) = &template {
            crate::core::fail_if_root_image_blocked(
                ctx,
                namespace,
                &self.workload_labels(ctx, &config.id, self.build_labels(workload_name)),
                template,
                &config.id,
            )
            .await?
        } else {
            None
        };
        let priority_failure = if self.is_stateful {
            crate::kubernetes_priority_events::missing_priority_class_progress(
                ctx,
                config.priority,
                namespace,
                workload_name,
            )
            .await
        } else {
            None
        };
        self.workload_failure = priority_failure
            .or(restricted_hint)
            .or(self.workload_failure.take());

        let max_polls = update_wait_max_polls(self.update_template_unchanged);
        if last_poll(self._internal_stay_count, max_polls) {
            self.ready_hold.reset();
            let selector = self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
            let pod_reason =
                workload_pod_reason(ctx, namespace, &selector, self.workload_failure.as_deref())
                    .await;
            if unchanged_update_rechecks(self.update_template_unchanged, pod_reason.as_deref()) {
                // Nothing rolled and the pods are not failing for good: the refresh check looks
                // again later. An update failure is never checked again.
                return Ok(HandlerAction::Continue {
                    state: RefreshFailed,
                    suggested_delay: Some(Duration::from_secs(10)),
                });
            }
            return Err(rollout_budget_exhausted(AlienError::new(
                ErrorData::WorkloadNotReady {
                    resource_id: config.id.clone(),
                    message: rollout_timeout_message(
                        if self.is_stateful {
                            "StatefulSet"
                        } else {
                            "Deployment"
                        },
                        workload_name,
                        &format!(
                            "desired {}, ready {}, total {}",
                            desired_replicas,
                            ready_replicas.unwrap_or(0),
                            replicas.unwrap_or(0)
                        ),
                        pod_reason.as_deref(),
                    ),
                },
            )));
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
        let config = ctx.desired_resource_config::<Container>()?;
        let workload_name = self.workload_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Workload name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let labels = self.build_labels(workload_name);
        let action = reconcile_kubernetes_public_endpoint(
            ctx,
            container_public_endpoint_target(
                &config.id,
                workload_name,
                namespace,
                labels,
                &config.public_endpoints,
                config
                    .health_check
                    .as_ref()
                    .map(|check| check.path.as_str()),
            )?,
            &mut self.public_endpoint,
        )
        .await?;

        match action {
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

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Container>()?;

        // The controller records the namespace before it writes, so a state without one is one
        // whose create was refused first. Nothing is recorded to delete; do not look for more.
        let Some(namespace) = self.namespace.as_ref() else {
            warn!(resource_id=%config.id, "No namespace is recorded, so nothing was deleted; an earlier create that failed part-way may have left objects");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };

        info!(namespace=%namespace, "Initiating Kubernetes Container deletion");

        let legacy_owner_proven = if let Some(workload_name) = self.workload_name.as_deref() {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let existing_labels = if self.is_stateful {
                match deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                {
                    Ok(workload) => workload.metadata.labels,
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        None
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to inspect StatefulSet '{workload_name}' before deletion"
                            ),
                            resource_id: Some(config.id.clone()),
                        }))
                    }
                }
            } else {
                match deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                {
                    Ok(workload) => workload.metadata.labels,
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        None
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to inspect Deployment '{workload_name}' before deletion"
                            ),
                            resource_id: Some(config.id.clone()),
                        }))
                    }
                }
            };
            if let Some(labels) = existing_labels.as_ref() {
                if !self.is_managed_workload(ctx, Some(labels), workload_name, &config.id) {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Refusing to delete Kubernetes workload '{workload_name}' because it is not owned by Container '{}' in this deployment",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        delete_kubernetes_public_endpoint(
            ctx,
            &config.id,
            namespace,
            self.workload_name.as_deref().unwrap_or_default(),
            "container",
            legacy_owner_proven,
            &mut self.public_endpoint,
        )
        .await?;
        if let Some(peer) = self.peer_service_name.clone() {
            self.delete_internal_service(namespace, &peer, &config.id, ctx, false)
                .await?;
            self.peer_service_name = None;
        }
        if let Some(service_name) = &self.service_name {
            self.delete_internal_service(
                namespace,
                service_name,
                &config.id,
                ctx,
                legacy_owner_proven,
            )
            .await?;
        }
        if let Some(workload_name) = self.workload_name.as_deref() {
            delete_environment_secret(
                "container",
                &config.id,
                workload_name,
                namespace,
                ctx,
                legacy_owner_proven,
            )
            .await?;
        }

        // Delete Deployment or StatefulSet
        if let Some(workload_name) = &self.workload_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            let existing_labels = if self.is_stateful {
                match deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                {
                    Ok(workload) => workload.metadata.labels,
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        None
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to inspect StatefulSet '{workload_name}' before deletion"
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            } else {
                match deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                {
                    Ok(workload) => workload.metadata.labels,
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        None
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to inspect Deployment '{workload_name}' before deletion"
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            };
            if let Some(existing_labels) = existing_labels.as_ref() {
                let desired_labels =
                    self.workload_labels(ctx, &config.id, self.build_labels(workload_name));
                if !self.is_managed_workload(ctx, Some(existing_labels), workload_name, &config.id)
                    || !crate::core::kubernetes_labels_have_compatible_scope(
                        Some(existing_labels),
                        &desired_labels,
                    )
                {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Refusing to delete Kubernetes workload '{workload_name}' because it is not owned by Container '{}' in this deployment",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            } else {
                delete_environment_secret(
                    "container",
                    &config.id,
                    workload_name,
                    namespace,
                    ctx,
                    false,
                )
                .await?;
                if self.network_policy_name.is_some() {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForNetworkPolicyDeletion,
                        suggested_delay: Some(Duration::from_secs(2)),
                    });
                }
                self.workload_name = None;
                self.namespace = None;
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }

            let delete_result = if self.is_stateful {
                deployment_client
                    .delete_statefulset(namespace, workload_name)
                    .await
            } else {
                deployment_client
                    .delete_deployment(namespace, workload_name)
                    .await
            };

            match delete_result {
                Ok(_) => {
                    let workload_type = if self.is_stateful {
                        "StatefulSet"
                    } else {
                        "Deployment"
                    };
                    info!(workload_name=%workload_name, workload_type=%workload_type, "Workload deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(workload_name=%workload_name, "Workload already deleted");

                    delete_environment_secret(
                        "container",
                        &config.id,
                        workload_name,
                        namespace,
                        ctx,
                        false,
                    )
                    .await?;

                    if self.network_policy_name.is_some() {
                        return Ok(HandlerAction::Continue {
                            state: WaitingForNetworkPolicyDeletion,
                            suggested_delay: Some(Duration::from_secs(2)),
                        });
                    }
                    self.workload_name = None;
                    self.namespace = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete workload '{}'.", workload_name),
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
        let config = ctx.desired_resource_config::<Container>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        // Check if workload is deleted
        if let Some(workload_name) = &self.workload_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            let get_result = if self.is_stateful {
                deployment_client
                    .get_statefulset(namespace, workload_name)
                    .await
                    .map(|_| ())
            } else {
                deployment_client
                    .get_deployment(namespace, workload_name)
                    .await
                    .map(|_| ())
            };

            match get_result {
                Ok(_) => {
                    debug!(workload_name=%workload_name, "Workload still exists, continuing to wait");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(workload_name=%workload_name, "Workload successfully deleted");

                    delete_environment_secret(
                        "container",
                        &config.id,
                        workload_name,
                        namespace,
                        ctx,
                        false,
                    )
                    .await?;

                    if self.network_policy_name.is_some() {
                        return Ok(HandlerAction::Continue {
                            state: WaitingForNetworkPolicyDeletion,
                            suggested_delay: Some(Duration::from_secs(2)),
                        });
                    }
                    self.workload_name = None;
                    self.namespace = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get workload '{}'.", workload_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Stay {
            max_times: Some(60), // 60 attempts * 5 seconds = 5 minutes max wait for deletion
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(state = WaitingForNetworkPolicyDeletion, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn waiting_for_network_policy_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let namespace = self.get_kubernetes_namespace(ctx)?;
        let client = ctx
            .service_provider
            .get_kubernetes_pod_client(ctx.get_kubernetes_config()?)
            .await?;
        let pods = client
            .list_pods(
                &namespace,
                Some(label_selector(&kubernetes_branded_resource_labels(
                    ctx, &config.id,
                ))),
                None,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to inspect Container pods before removing network restrictions"
                    .into(),
                resource_id: Some(config.id.clone()),
            })?;
        if !pods.items.is_empty() {
            return Ok(HandlerAction::Stay {
                max_times: Some(120),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }
        self.delete_network_policy(&namespace, &config.id, ctx)
            .await?;
        self.workload_name = None;
        self.namespace = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    // A failed health check is checked again: the resource returns to Running when the
    // workload is healthy, and keeps reporting its error while it is not.
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
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay,
            }) => {
                // A pod that is ready for a moment on each restart is not recovered.
                let restart_free = self.restart_free(ctx).await;
                if recovery_check(
                    &mut self.recovery_checks,
                    &mut self.last_recovery_check_unix,
                    restart_free,
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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let Some(workload_name) = &self.workload_name {
            Some(ResourceOutputs::new(ContainerOutputs {
                name: workload_name.clone(),
                status: ContainerStatus::Running,
                current_replicas: 0, // Will be updated by runtime
                desired_replicas: 0, // Will be updated by runtime
                // The internal Service shares the workload's name.
                internal_dns: internal_dns_name(
                    workload_name,
                    self.namespace.as_deref(),
                    crate::sandbox::kubernetes_egress::detected_cluster_domain().as_deref(),
                ),
                internal_urls: self.internal_urls(),
                replicas: Vec::new(), // Replica details tracked separately
                // Kubernetes leaves volume snapshots to the cluster's own tooling. With no
                // volumes reported, there is nothing to restore.
                volumes: Vec::new(),
                volume_backups: None,
                public_endpoints: if self.public_endpoint.named_public_endpoints.is_empty() {
                    self.public_endpoint
                        .effective_public_url()
                        .map(|url| {
                            HashMap::from([(
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
            }))
        } else {
            None
        }
    }

    fn reapplies_after_failed_refresh(&self) -> bool {
        true
    }

    /// Secret-typed env vars never enter the resource config on Kubernetes
    /// (they're projected via secretKeyRef), so config diffing alone cannot
    /// see secret rotations. Schedule an update when the snapshot-derived
    /// env-secret checksum drifts from the one applied last; the update
    /// re-reconciles the Secret and rolls pods via the checksum annotation.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let Some(workload_name) = self.workload_name.as_ref() else {
            return Ok(false);
        };
        let config = ctx.desired_resource_config::<Container>()?;
        if config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
            // Ready checks the vault value alongside the deployment snapshot.
            return Ok(false);
        }
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
            workload_name,
            &ctx.deployment_config.environment_variables.variables,
            &monitoring_headers,
        )?)
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::{BindingValue, KubernetesContainerBinding};

        // Construct binding on-the-fly from stored fields (like other controllers)
        if let (Some(container_id), Some(service_name), Some(namespace)) =
            (&self.container_id, &self.service_name, &self.namespace)
        {
            let binding = KubernetesContainerBinding {
                internal_urls: self
                    .internal_urls()
                    .into_iter()
                    .map(|(port, url)| (port, BindingValue::value(url)))
                    .collect(),
                name: BindingValue::Value(container_id.clone()),
                namespace: BindingValue::Value(namespace.clone()),
                service_name: BindingValue::Value(service_name.clone()),
                service_port: BindingValue::Value(self.service_port.unwrap_or(80)),
                public_url: self
                    .public_endpoint
                    .effective_public_url()
                    .map(BindingValue::Value),
            };

            // Serialize to JSON
            // Bindings are tagged by backend; the SDK refuses an untagged container binding.
            let binding = alien_core::bindings::ContainerBinding::Kubernetes(binding);
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod output_tests {
    use alien_core::ContainerOutputs;

    use super::{
        EnvSecretRotationTracker, KubernetesContainerController, KubernetesContainerState,
        KubernetesPublicEndpointState,
    };

    #[test]
    fn build_outputs_includes_public_endpoint_url() {
        let public_endpoint = KubernetesPublicEndpointState {
            public_url: Some("https://container.example.test".to_string()),
            ..Default::default()
        };
        let controller = KubernetesContainerController {
            state: KubernetesContainerState::Ready,
            workload_name: Some("test-container".to_string()),
            is_stateful: false,
            namespace: Some("test-namespace".to_string()),
            service_name: Some("test-container".to_string()),
            service_ports: vec![3000, 3129],
            service_port: Some(3000),
            network_policy_name: None,
            peer_service_name: None,
            container_id: Some("container".to_string()),
            public_endpoint,
            env_secret: EnvSecretRotationTracker::default(),
            update_generation: None,
            workload_failure: None,
            applied_priority_class: None,
            applied_architecture: None,
            applied_gateway_dns: None,
            zero_ready_observed: false,
            rollout_watch: None,
            recovery_checks: 0,
            not_ready: Default::default(),
            ready_hold: Default::default(),
            update_template_unchanged: false,
            recovering: false,
            last_recovery_check_unix: None,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let container_outputs = outputs
            .downcast_ref::<ContainerOutputs>()
            .expect("container outputs");

        assert_eq!(
            container_outputs
                .internal_urls
                .get(&3129)
                .map(String::as_str),
            Some("http://test-container.test-namespace.svc:3129")
        );

        assert_eq!(
            container_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.url.as_str()),
            Some("https://container.example.test")
        );

        // Linked workloads load this through the SDK, which needs the backend tag.
        use crate::core::ResourceController as _;
        let binding: alien_core::bindings::ContainerBinding = serde_json::from_value(
            controller
                .get_binding_params()
                .expect("binding")
                .expect("binding params"),
        )
        .expect("a tagged container binding");
        let alien_core::bindings::ContainerBinding::Kubernetes(binding) = binding else {
            panic!("Kubernetes container binding")
        };
        assert_eq!(
            binding.internal_urls.get(&3129),
            Some(&alien_core::BindingValue::value(
                "http://test-container.test-namespace.svc:3129".to_string()
            ))
        );
    }

    #[test]
    fn build_outputs_derives_public_url_from_load_balancer_endpoint() {
        let public_endpoint = KubernetesPublicEndpointState {
            load_balancer_endpoint: Some(alien_core::LoadBalancerEndpoint {
                dns_name: "k8s-container.example.elb.amazonaws.com".to_string(),
                hosted_zone_id: None,
            }),
            ..Default::default()
        };
        let controller = KubernetesContainerController {
            state: KubernetesContainerState::Ready,
            workload_name: Some("test-container".to_string()),
            is_stateful: false,
            namespace: Some("test-namespace".to_string()),
            service_name: Some("test-container".to_string()),
            service_ports: Vec::new(),
            service_port: Some(3000),
            network_policy_name: None,
            peer_service_name: None,
            container_id: Some("container".to_string()),
            public_endpoint,
            env_secret: EnvSecretRotationTracker::default(),
            update_generation: None,
            workload_failure: None,
            applied_priority_class: None,
            applied_architecture: None,
            applied_gateway_dns: None,
            zero_ready_observed: false,
            rollout_watch: None,
            recovery_checks: 0,
            not_ready: Default::default(),
            ready_hold: Default::default(),
            update_template_unchanged: false,
            recovering: false,
            last_recovery_check_unix: None,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let container_outputs = outputs
            .downcast_ref::<ContainerOutputs>()
            .expect("container outputs");

        assert_eq!(
            container_outputs
                .internal_urls
                .get(&3000)
                .map(String::as_str),
            Some("http://test-container.test-namespace.svc:3000")
        );

        assert_eq!(
            container_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.url.as_str()),
            Some("http://k8s-container.example.elb.amazonaws.com")
        );
    }
}

impl KubernetesContainerController {
    /// True when no ready pod of the workload restarted since the last refresh check.
    async fn restart_free(&mut self, ctx: &ResourceControllerContext<'_>) -> bool {
        let (Some(name), Some(namespace), Ok(config)) = (
            self.workload_name.clone(),
            self.namespace.clone(),
            ctx.desired_resource_config::<Container>(),
        ) else {
            return false;
        };
        let selector = self.workload_labels(ctx, &config.id, self.build_labels(&name));
        recovery_restart_free(ctx, &mut self.ready_hold, &namespace, &selector).await
    }

    fn internal_urls(&self) -> HashMap<u16, String> {
        let (Some(service), Some(namespace)) = (&self.service_name, &self.namespace) else {
            return HashMap::new();
        };
        let ports: Vec<_> = if self.service_ports.is_empty() {
            self.service_port.into_iter().collect()
        } else {
            self.service_ports.clone()
        };
        ports
            .into_iter()
            .map(|port| {
                (
                    port,
                    // Resolved through the workload's search path, whatever the cluster
                    // DNS domain is.
                    format!("http://{service}.{namespace}.svc:{port}"),
                )
            })
            .collect()
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(container_name: &str, namespace: &str, is_stateful: bool) -> Self {
        Self {
            state: KubernetesContainerState::Ready,
            workload_name: Some(container_name.to_string()),
            is_stateful,
            namespace: Some(namespace.to_string()),
            service_name: Some(container_name.to_string()),
            service_ports: Vec::new(),
            service_port: Some(80),
            network_policy_name: None,
            peer_service_name: None,
            container_id: Some("test-container".to_string()),
            public_endpoint: KubernetesPublicEndpointState::default(),
            env_secret: EnvSecretRotationTracker::default(),
            update_generation: None,
            workload_failure: None,
            applied_priority_class: None,
            applied_architecture: None,
            applied_gateway_dns: None,
            zero_ready_observed: false,
            rollout_watch: None,
            recovery_checks: 0,
            not_ready: Default::default(),
            ready_hold: Default::default(),
            update_template_unchanged: false,
            recovering: false,
            last_recovery_check_unix: None,
            _internal_stay_count: None,
        }
    }

    async fn additional_environment_secrets(
        &self,
        config: &Container,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<BTreeMap<String, String>> {
        let mut secrets = direct_monitoring_auth_headers(ctx);
        if gateway_mounts_config(config) {
            // Its pods mount the replica secret, which must exist before they start. Written
            // once and never read here; the configuration is mounted from the vault as well.
            let vault = crate::sandbox::egress::deployment_vault(ctx).await?;
            crate::sandbox::egress::create_replica_secret(vault.as_ref()).await?;
            // A mounted configuration changes without the pods, but a running replica refuses
            // a change to its peers. Keeping them in the pod's environment Secret makes such a
            // change drift there, which restarts the pods, as any configuration change did
            // when the whole configuration was in the environment.
            let namespace = self.namespace.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "The sandbox gateway namespace is not known yet".to_string(),
                })
            })?;
            secrets.insert(
                GATEWAY_RESTART_SETTINGS_ENV.to_string(),
                serde_json::json!({
                    "peers": crate::sandbox::kubernetes_egress::gateway_peers(ctx, namespace)?
                })
                .to_string(),
            );
        } else if config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
            // An image from before mounted configuration files gets its configuration, which
            // embeds the replica secret, in its environment.
            let vault = crate::sandbox::egress::deployment_vault(ctx).await?;
            let value = vault
                .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
                .await
                .context(ErrorData::ResourceConfigInvalid {
                    message: "The sandbox gateway configuration is not ready".into(),
                    resource_id: Some(config.id.clone()),
                })?;
            secrets.insert("ALIEN_EGRESS_GATEWAY_CONFIG".into(), value);
        }
        Ok(secrets)
    }

    /// Whether the gateway's environment Secret would now hold different values than the pods
    /// started with.
    async fn gateway_environment_drifted(
        &mut self,
        config: &Container,
        workload_name: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let secrets = self.additional_environment_secrets(config, ctx).await?;
        let variables = &ctx.environment_variables_for(&config.id);
        self.env_secret
            .adopt_unkeyed(&config.id, workload_name, variables, &secrets)?;
        Ok(self.env_secret.drifted_with_additional_secrets(
            &config.id,
            workload_name,
            variables,
            &secrets,
        )?)
    }

    async fn reconcile_environment_secret(
        &self,
        config: &Container,
        workload_name: &str,
        namespace: &str,
        legacy_owner_proven: bool,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<KubernetesEnvSecretPlan>> {
        let monitoring_headers = self.additional_environment_secrets(config, ctx).await?;
        reconcile_environment_secret_with_additional_secrets(
            "container",
            &config.id,
            workload_name,
            namespace,
            &monitoring_headers,
            legacy_owner_proven,
            ctx,
        )
        .await
    }

    /// Builds a Kubernetes Deployment for stateless containers.
    async fn build_deployment(
        &self,
        config: &Container,
        container_name: &str,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Deployment> {
        let selector_labels = self.build_labels(container_name);
        let labels = self.workload_labels(ctx, &config.id, selector_labels.clone());
        let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());
        let mut pod_spec = self
            .build_pod_spec(
                config,
                namespace,
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
        if ctx.deployment_config.runtime_identity_audience.is_some() {
            crate::kubernetes_runtime_credentials::mount(
                &mut pod_spec,
                &kubernetes_resource_name("", &config.id),
                container_name,
            )?;
        }

        let pod_annotations = pod_template_annotations(env_secret_plan, &config.environment)?;

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some(container_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(config.replicas.unwrap_or(1) as i32),
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
        };

        Ok(deployment)
    }

    /// Builds a Kubernetes StatefulSet for stateful containers.
    async fn build_statefulset(
        &self,
        config: &Container,
        container_name: &str,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<StatefulSet> {
        let selector_labels = self.build_labels(container_name);
        let labels = self.workload_labels(ctx, &config.id, selector_labels.clone());
        let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());
        let mut pod_spec = self
            .build_pod_spec(
                config,
                namespace,
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
        if ctx.deployment_config.runtime_identity_audience.is_some() {
            crate::kubernetes_runtime_credentials::mount(
                &mut pod_spec,
                &kubernetes_resource_name("", &config.id),
                container_name,
            )?;
        }

        let pod_annotations = pod_template_annotations(env_secret_plan, &config.environment)?;

        // Build volume claim templates for persistent storage
        let mut volume_claim_templates = Vec::new();
        if let Some(persistent_storage) = &config.persistent_storage {
            let pvc = PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("data".to_string()),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    resources: Some(k8s_openapi::api::core::v1::VolumeResourceRequirements {
                        requests: Some({
                            let mut requests = BTreeMap::new();
                            requests.insert(
                                "storage".to_string(),
                                Quantity(persistent_storage.size.clone()),
                            );
                            requests
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            };
            volume_claim_templates.push(pvc);
        }

        let statefulset = StatefulSet {
            metadata: ObjectMeta {
                name: Some(container_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                replicas: Some(config.replicas.unwrap_or(1) as i32),
                selector: LabelSelector {
                    match_labels: Some(selector_labels),
                    ..Default::default()
                },
                service_name: Some(container_name.to_string()),
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(pod_labels),
                        annotations: pod_annotations,
                        ..Default::default()
                    }),
                    spec: Some(pod_spec),
                },
                volume_claim_templates: if volume_claim_templates.is_empty() {
                    None
                } else {
                    Some(volume_claim_templates)
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        Ok(statefulset)
    }

    /// Builds a PodSpec for the container.
    async fn build_pod_spec(
        &self,
        config: &Container,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<PodSpec> {
        // Determine the container image. Source-built containers are
        // supported: `alien build` compiles the source, `alien release`
        // pushes it, and the controller receives a registry image whose
        // compiled binary is the direct entrypoint. Reaching here with
        // unbuilt source means the build step was skipped.
        let image = match &config.code {
            ContainerCode::Image { image } => ctx.deployment_config.workload_image(image),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message:
                        "Container still has unbuilt source code. Run 'alien build' first; it compiles the source into an image the controller can deploy."
                            .to_string(),
                }));
            }
        };

        // Build environment variables
        // IMPORTANT: Start with config.environment which includes injected vars from DeploymentConfig
        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_container_runtime_env_vars(ctx, &config.id)?
            .add_direct_monitoring_auth_headers(ctx)
            // Cross-target parity with the local controller: apps read
            // ALIEN_PUBLIC_ENDPOINTS_JSON to build their own absolute URLs.
            .add_current_resource_public_endpoint(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .add_self_container_binding(&config.id, self.get_binding_params()?.as_ref())?;

        let (mut env_map, bindings) = env_builder.build_with_bindings();
        env_map.remove(ENV_ALIEN_RUNTIME_SECRETS);

        // Containers project Secret-kind env vars as secretKeyRefs and never
        // load secrets at runtime, so the ALIEN_SECRETS vault-load pointer is
        // stripped from the manifest.
        let mut env_vars = projected_env_vars(env_secret_plan, bindings, env_map)?;

        // Build volume mounts
        let mut volume_mounts = Vec::new();

        // Ephemeral storage mount
        if config.ephemeral_storage.is_some() {
            volume_mounts.push(VolumeMount {
                name: "ephemeral".to_string(),
                mount_path: "/var/ephemeral".to_string(),
                ..Default::default()
            });
        }

        // Persistent storage mount (for StatefulSets)
        if let Some(persistent_storage) = &config.persistent_storage {
            volume_mounts.push(VolumeMount {
                name: "data".to_string(),
                mount_path: persistent_storage.mount_path.clone(),
                ..Default::default()
            });
        }

        let mut mount_paths = volume_mounts
            .iter()
            .map(|mount| mount.mount_path.clone())
            .collect::<BTreeSet<_>>();
        for (index, mount) in config.kubernetes_secret_mounts.iter().enumerate() {
            if mount.secret_name.trim().is_empty()
                || !mount.mount_path.starts_with('/')
                || mount.mount_path == "/"
                || !mount_paths.insert(mount.mount_path.clone())
            {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: format!(
                        "Kubernetes Secret mount #{index} needs a nonempty Secret name and a unique absolute directory mount path"
                    ),
                }));
            }
            volume_mounts.push(VolumeMount {
                name: format!("existing-secret-{index}"),
                mount_path: mount.mount_path.clone(),
                read_only: Some(true),
                ..Default::default()
            });
        }

        // A gateway that reads mounted files gets the vault's Secrets for its configuration and
        // replica secret mounted by the kubelet, which keeps them current; it refreshes the
        // configuration from the file.
        let mut gateway_volumes = Vec::new();
        if gateway_mounts_config(config) {
            for (volume, directory, entry) in [
                (
                    "egress-gateway-config",
                    crate::sandbox::egress::KUBERNETES_CONFIG_DIR,
                    crate::sandbox::egress::GATEWAY_CONFIG,
                ),
                (
                    "egress-gateway-replica-secret",
                    crate::sandbox::egress::KUBERNETES_REPLICA_SECRET_DIR,
                    crate::sandbox::egress::REPLICA_SECRET,
                ),
            ] {
                volume_mounts.push(VolumeMount {
                    name: volume.to_string(),
                    mount_path: directory.to_string(),
                    read_only: Some(true),
                    ..Default::default()
                });
                gateway_volumes.push(Volume {
                    name: volume.to_string(),
                    secret: Some(SecretVolumeSource {
                        secret_name: Some(crate::sandbox::egress::kubernetes_vault_secret_name(
                            ctx, namespace, entry,
                        )?),
                        optional: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            }
            env_vars.push(EnvVar {
                name: "ALIEN_EGRESS_GATEWAY_CONFIG_FILE".to_string(),
                value: Some(format!(
                    "{}/{}",
                    crate::sandbox::egress::KUBERNETES_CONFIG_DIR,
                    alien_core::vault_naming::KUBERNETES_SECRET_VALUE_KEY
                )),
                value_from: None,
            });
        }

        // Parse CPU and memory from ResourceSpec
        let cpu_request = config.cpu.min.clone();
        let cpu_limit = config.cpu.desired.clone();
        let memory_request = config.memory.min.clone();
        let memory_limit = config.memory.desired.clone();

        let http_probe = |probe: &alien_core::KubernetesHttpProbe| -> Result<Probe> {
            if !probe.path.starts_with('/') || probe.port == 0 {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "Kubernetes HTTP probes need an absolute path and a nonzero port"
                        .to_string(),
                }));
            }
            Ok(Probe {
                http_get: Some(HTTPGetAction {
                    path: Some(probe.path.clone()),
                    port: IntOrString::Int(i32::from(probe.port)),
                    ..Default::default()
                }),
                ..Default::default()
            })
        };
        let liveness_probe = config
            .kubernetes_liveness_probe
            .as_ref()
            .map(http_probe)
            .transpose()?;
        let gateway = config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
        let mut readiness_probe = config
            .kubernetes_readiness_probe
            .as_ref()
            .map(http_probe)
            .transpose()?;
        if gateway {
            if let Some(probe) = readiness_probe.as_mut() {
                // A writer's DNS can lag a starting replica's address by a few seconds, so a
                // write in that window reaches the other replicas only. The replica
                // resynchronizes from every peer 30 s after its first synchronization; it
                // takes traffic only after that second pass has the write.
                probe.initial_delay_seconds = Some(GATEWAY_READY_DELAY_SECONDS);
            }
        }
        let security =
            crate::core::pod_security(&config.id, config.security.as_ref(), &volume_mounts)?;
        volume_mounts.extend(security.mounts);

        let container = K8sContainer {
            name: kubernetes_resource_name("", &config.id),
            image: Some(image),
            command: config.command.clone(),
            ports: Some(
                config
                    .ports
                    .iter()
                    .map(|p| ContainerPort {
                        container_port: p.port as i32,
                        name: Some(kubernetes_port_name(p)),
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    })
                    .collect(),
            ),
            env: Some(env_vars),
            liveness_probe,
            readiness_probe,
            security_context: security.container,
            volume_mounts: if volume_mounts.is_empty() {
                None
            } else {
                Some(volume_mounts)
            },
            resources: Some(ResourceRequirements {
                requests: Some({
                    let mut requests = BTreeMap::new();
                    requests.insert("cpu".to_string(), Quantity(cpu_request));
                    requests.insert("memory".to_string(), Quantity(memory_request));
                    if let Some(ephemeral_storage) = &config.ephemeral_storage {
                        requests.insert(
                            "ephemeral-storage".to_string(),
                            Quantity(ephemeral_storage.clone()),
                        );
                    }
                    requests
                }),
                limits: Some({
                    let mut limits = BTreeMap::new();
                    limits.insert("cpu".to_string(), Quantity(cpu_limit));
                    limits.insert("memory".to_string(), Quantity(memory_limit));
                    limits
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Build volumes
        let mut volumes = Vec::new();
        if config.ephemeral_storage.is_some() {
            volumes.push(Volume {
                name: "ephemeral".to_string(),
                empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource {
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        volumes.extend(gateway_volumes);
        for (index, mount) in config.kubernetes_secret_mounts.iter().enumerate() {
            volumes.push(Volume {
                name: format!("existing-secret-{index}"),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(mount.secret_name.clone()),
                    optional: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        volumes.extend(security.volumes);

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
            service_account_name: Some(service_account_name.to_string()),
            // The gateway never calls the Kubernetes API; its configuration is projected.
            automount_service_account_token: gateway.then_some(false),
            security_context: security.pod,
            containers: vec![container],
            image_pull_secrets: crate::kubernetes_registry::workload_pull_secrets(
                ctx.deployment_config,
                image_pull_secret_name,
            ),
            volumes: if volumes.is_empty() {
                None
            } else {
                Some(volumes)
            },
            restart_policy: Some("Always".to_string()),
            // Kubernetes would otherwise inject `<SERVICE>_SERVICE_HOST`, `<SERVICE>_PORT_<n>_TCP*`
            // and similar variables for every Service in the namespace. A workload sets its own
            // variables explicitly, which Kubernetes does not overwrite; Alien's bindings carry each
            // address, so the extra variables only add names a workload can collide with.
            enable_service_links: Some(false),
            ..Default::default()
        };
        if gateway {
            // The gateway's DNS must pass its own egress policy, which admits the cluster DNS
            // pods; a node-local cache address would not match it everywhere. The choice was
            // resolved and recorded when this create or update started.
            if let Some(GatewayDns::ClusterService { address, domain }) = &self.applied_gateway_dns
            {
                pod_spec.dns_policy = Some("None".into());
                pod_spec.dns_config = Some(cluster_dns_config(namespace, domain, address));
            }
        }
        // A workload that links a sandbox calls the session broker with a token projected for it.
        crate::sandbox::kubernetes_broker_token::project_into(&mut pod_spec, &config.links);

        Ok(pod_spec)
    }

    /// Builds standard labels for Kubernetes resources.
    pub(super) fn build_labels(&self, container_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), container_name.to_string());
        labels.insert("managed-by".to_string(), "runtime".to_string());
        labels.insert("component".to_string(), "container".to_string());
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

    async fn replace_obsolete_unready_pod(
        &self,
        statefulset: &StatefulSet,
        config: &Container,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let (Some(expected), Some(status), Some(uid), Some(name)) = (
            self.update_generation,
            statefulset.status.as_ref(),
            statefulset.metadata.uid.as_deref(),
            statefulset.metadata.name.as_deref(),
        ) else {
            return Ok(());
        };
        let Some(revision) = status.update_revision.as_deref() else {
            return Ok(());
        };
        if statefulset.metadata.generation != Some(expected)
            || status.observed_generation.unwrap_or(0) < expected
            || !self.is_managed_workload(
                ctx,
                statefulset.metadata.labels.as_ref(),
                name,
                &config.id,
            )
        {
            return Ok(());
        }
        let client = ctx
            .service_provider
            .get_kubernetes_pod_client(ctx.get_kubernetes_config()?)
            .await?;
        let pods = client
            .list_pods(
                namespace,
                Some(label_selector(&self.workload_labels(
                    ctx,
                    &config.id,
                    self.build_labels(name),
                ))),
                None,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to inspect pods for adopted statefulset '{name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        for pod in pods.items {
            let owned = pod
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|owners| {
                    owners.iter().any(|owner| {
                        owner.controller == Some(true)
                            && owner.kind == "StatefulSet"
                            && owner.uid == uid
                    })
                });
            let obsolete = pod
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("controller-revision-hash"))
                .is_some_and(|current| current != revision);
            let ready = pod
                .status
                .as_ref()
                .and_then(|status| status.conditions.as_ref())
                .is_some_and(|conditions| {
                    conditions
                        .iter()
                        .any(|condition| condition.type_ == "Ready" && condition.status == "True")
                });
            if !owned || !obsolete || ready || pod.metadata.deletion_timestamp.is_some() {
                continue;
            }
            let (Some(pod_name), Some(pod_uid)) =
                (pod.metadata.name.as_deref(), pod.metadata.uid.as_deref())
            else {
                continue;
            };
            // OrderedReady can wait forever for the old broken revision. Use normal deletion,
            // one pod per poll, and keep the StatefulSet and its persistent volumes.
            match client.delete_pod_if_uid(namespace, pod_name, pod_uid).await {
                Ok(()) => {},
                Err(error) if matches!(error.error, Some(CloudClientErrorData::RemoteResourceNotFound { .. } | CloudClientErrorData::RemoteResourceConflict { .. })) => {},
                Err(error) => return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to replace obsolete pod '{pod_name}' for adopted statefulset '{name}'"),
                    resource_id: Some(config.id.clone()),
                })),
            }
            break;
        }
        Ok(())
    }

    fn is_managed_workload(
        &self,
        ctx: &ResourceControllerContext<'_>,
        labels: Option<&BTreeMap<String, String>>,
        container_name: &str,
        resource_id: &str,
    ) -> bool {
        let desired = self.workload_labels(ctx, resource_id, self.build_labels(container_name));
        crate::core::kubernetes_labels_match_identity(
            labels,
            &desired,
            &["managed-by", "component", "app"],
        ) && crate::core::kubernetes_labels_have_compatible_scope(labels, &desired)
    }

    /// Gets the Kubernetes namespace from ClientConfig
    fn get_kubernetes_namespace(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        let k8s_config = ctx.get_kubernetes_config()?;
        match k8s_config {
            alien_core::KubernetesClientConfig::InCluster { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes".to_string(),
                        message: "Kubernetes namespace not configured in InCluster config"
                            .to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes".to_string(),
                        message: "Kubernetes namespace not configured in Kubeconfig".to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes".to_string(),
                        message: "Kubernetes namespace not configured in Manual config".to_string(),
                    })
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn internal_dns_uses_the_detected_cluster_domain() {
        use super::internal_dns_name;
        assert_eq!(
            internal_dns_name("api", Some("prod"), Some("corp.example")),
            "api.prod.svc.corp.example"
        );
        // No guessed `cluster.local`: without a domain the name stops at `.svc`.
        assert_eq!(internal_dns_name("api", Some("prod"), None), "api.prod.svc");
        assert_eq!(internal_dns_name("api", None, None), "api.svc");
    }

    #[test]
    fn only_images_on_the_manager_registry_get_a_pull_secret() {
        let image = |name: &str| ContainerCode::Image {
            image: name.to_string(),
        };
        let manager = Some("https://manager.example.test");
        assert_eq!(
            image_needing_pull_secret(&image("manager.example.test/app:1"), manager, false),
            Some("manager.example.test/app:1")
        );
        assert_eq!(
            image_needing_pull_secret(&image("ghcr.io/org/gateway:1"), manager, false),
            None
        );
        assert_eq!(
            image_needing_pull_secret(&image("ghcr.io/org/gateway:1"), None, false),
            None
        );
    }

    use super::*;
    use k8s_openapi::api::apps::v1::{DeploymentStatus, StatefulSetStatus};

    /// Writes the container's Deployment, as the controller renders it, for the Kind test that
    /// applies it to a namespace enforcing the `restricted` Pod Security profile.
    /// `ALIEN_KIND_MANIFEST_DIR` receives `container.yaml`; `ALIEN_KIND_IMAGE` is the release's
    /// image and `ALIEN_KIND_IMAGE_REGISTRY` the site registry it must be pulled from.
    #[tokio::test]
    #[ignore = "writes manifests for the Kind pod security test"]
    async fn write_the_default_deployment_for_the_kind_test() {
        let dir = std::env::var("ALIEN_KIND_MANIFEST_DIR").expect("ALIEN_KIND_MANIFEST_DIR");
        let image = std::env::var("ALIEN_KIND_IMAGE").expect("ALIEN_KIND_IMAGE");
        // ALIEN_KIND_CONFIG names a container from an example stack; the default is the
        // controller test fixture with the restricted profile written explicitly.
        let mut config = match std::env::var("ALIEN_KIND_CONFIG") {
            Ok(path) => serde_json::from_str(&std::fs::read_to_string(path).expect("config"))
                .expect("container config"),
            Err(_) => {
                let mut config = manifest_test_container(&[], false);
                config.security = Some(ContainerSecurity {
                    profile: ContainerSecurityProfile::Restricted,
                    run_as_user: None,
                    run_as_group: None,
                    read_only_root_filesystem: None,
                    writable_paths: Vec::new(),
                });
                config
            }
        };
        let name = std::env::var("ALIEN_KIND_NAME").unwrap_or_else(|_| "web".to_string());
        let out = std::env::var("ALIEN_KIND_OUT").unwrap_or_else(|_| "container.yaml".to_string());
        config.code = ContainerCode::Image { image };
        config.replicas = Some(1);
        let mut harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        if let Ok(registry) = std::env::var("ALIEN_KIND_IMAGE_REGISTRY") {
            harness = harness.with_image_registry(&registry);
        }
        let controller = manifest_test_controller();
        let mut value = if config.persistent_storage.is_some() {
            let set = controller
                .build_statefulset(
                    &config,
                    &name,
                    "default",
                    "default",
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("statefulset manifest");
            let mut value = serde_json::to_value(&set).expect("serialize");
            value["kind"] = "StatefulSet".into();
            value
        } else {
            let deployment = controller
                .build_deployment(
                    &config,
                    &name,
                    "default",
                    "default",
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("deployment manifest");
            let mut value = serde_json::to_value(&deployment).expect("serialize");
            value["kind"] = "Deployment".into();
            value
        };
        value["apiVersion"] = "apps/v1".into();
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

    #[test]
    fn a_site_mirror_never_receives_the_manager_pull_token() {
        let code = ContainerCode::Image {
            image: "manager.example.com/app@sha256:abc".to_string(),
        };
        assert!(
            image_needing_pull_secret(&code, Some("https://manager.example.com"), false).is_some()
        );
        assert!(
            image_needing_pull_secret(&code, Some("https://manager.example.com"), true).is_none()
        );
    }

    #[tokio::test]
    async fn adopted_workload_accepts_ready_created_replicas() {
        for stateful in [false, true] {
            let server = httpmock::MockServer::start_async().await;
            let mut config = manifest_test_container(&[], stateful);
            config.replicas = Some(3);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            controller.update_generation = Some(2);
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            crate::core::kubernetes_manifest_test_support::mock_empty_pod_list(&server);
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "metadata":{"name":"web","generation":2},
                    "spec":{"replicas":3,"selector":{},"template":{}},
                    "status":{"replicas":1,"readyReplicas":1,"availableReplicas":1,"updatedReplicas":1,"observedGeneration":2,"currentRevision":"old","updateRevision":"new"}
                }));
            });
            // Readiness must hold across three polls before the workload counts as ready.
            let mut action = None;
            for _ in 0..3 {
                action = Some(
                    controller
                        .waiting_for_workload(&ctx)
                        .await
                        .expect("read adopted workload"),
                );
            }
            assert!(
                matches!(
                    action,
                    Some(KubernetesContainerHandlerAction::Continue { .. })
                ),
                "all created replicas run the new template, stateful={stateful}"
            );
        }
    }

    /// A gateway whose resolver does not work never becomes ready, so it waits in
    /// `WaitingForWorkload` and never reaches the Ready handler where the resolver is compared.
    /// It must be updated from the wait.
    #[tokio::test]
    async fn a_gateway_that_never_became_ready_is_updated_when_its_resolver_changes() {
        let server = httpmock::MockServer::start_async().await;
        kube_dns_mock(&server, "10.0.0.10");
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/deployments/web");
            then.status(200).json_body(serde_json::json!({
                "metadata":{"name":"web","generation":1},
                "spec":{"replicas":2,"selector":{},"template":{}},
                "status":{"replicas":2,"readyReplicas":0,"updatedReplicas":2,"observedGeneration":1}
            }));
        });
        let gateway = gateway_container(GATEWAY_IMAGE);
        let harness = KubernetesManifestTestHarness::new(Resource::new(gateway), vec![]);
        let mut ctx = harness.ctx();
        point_at(&mut ctx, &server);
        // Built with an address the Service no longer has, and not ready.
        let mut controller = manifest_test_controller();
        controller.applied_gateway_dns = Some(GatewayDns::ClusterService {
            address: "10.0.0.99".into(),
            domain: "corp.example".into(),
        });
        let action = controller.waiting_for_workload(&ctx).await.expect("wait");
        assert!(
            matches!(
                action,
                KubernetesContainerHandlerAction::Continue {
                    state: KubernetesContainerState::UpdateStart,
                    ..
                }
            ),
            "{action:?}"
        );
    }

    #[tokio::test]
    async fn adopted_statefulset_replaces_only_its_obsolete_unready_pod() {
        for (owner, revision, ready, deletes) in [
            ("workload-uid", "old", false, 1),
            ("another-workload", "old", false, 0),
            ("workload-uid", "new", false, 0),
            ("workload-uid", "old", true, 0),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let config = manifest_test_container(&[], true);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = true;
            controller.update_generation = Some(2);
            let labels = controller.workload_labels(&ctx, "web", controller.build_labels("web"));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path("/apis/apps/v1/namespaces/test-ns/statefulsets/web");
                then.status(200).json_body(serde_json::json!({
                    "metadata":{"name":"web","uid":"workload-uid","generation":2,"labels":labels},
                    "spec":{"replicas":1,"serviceName":"web","selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":{"replicas":1,"observedGeneration":2,"currentRevision":"old","updateRevision":"new"}
                }));
            });
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path("/api/v1/namespaces/test-ns/pods");
                then.status(200).json_body(serde_json::json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":[{
                    "metadata":{"name":"web-0","uid":"old-pod-uid","labels":{"controller-revision-hash":revision},"ownerReferences":[{"apiVersion":"apps/v1","kind":"StatefulSet","name":"web","uid":owner,"controller":true}]},
                    "spec":{"containers":[{"name":"web","image":"example.test/web:broken"}]},
                    "status":{"conditions":[{"type":"Ready","status":if ready {"True"} else {"False"}}]}
                }]}));
            });
            let deletion = server.mock(|when, then| {
                when.method(httpmock::Method::DELETE).path("/api/v1/namespaces/test-ns/pods/web-0")
                    .json_body(serde_json::json!({"apiVersion":"v1","kind":"DeleteOptions","preconditions":{"uid":"old-pod-uid"}}));
                then.status(200).json_body(serde_json::json!({"kind":"Status","apiVersion":"v1","status":"Success"}));
            });
            assert!(matches!(
                controller
                    .waiting_for_workload(&ctx)
                    .await
                    .expect("wait for corrected workload"),
                KubernetesContainerHandlerAction::Stay { .. }
            ));
            assert_eq!(
                deletion.hits(),
                deletes,
                "owner={owner}, revision={revision}, ready={ready}"
            );
        }
    }

    #[tokio::test]
    async fn create_retry_converges_existing_workload_to_corrective_release() {
        for stateful in [false, true] {
            let server = httpmock::MockServer::start_async().await;
            crate::core::kubernetes_manifest_test_support::mock_empty_pod_list(&server);
            let config = manifest_test_container(&[("RELEASE", "corrected")], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut deployment_config = ctx.deployment_config.clone();
            deployment_config.deployment_token = Some("test-token".into());
            ctx.deployment_config = &deployment_config;
            server.mock(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/namespaces/test-ns/secrets");
                then.status(201)
                    .json_body(serde_json::json!({"metadata":{"name":"web-registry"}}));
            });
            let mut controller = KubernetesContainerController::default();
            let name = kubernetes_resource_name(ctx.resource_prefix, &config.id);
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            let path = format!("/apis/apps/v1/namespaces/test-ns/{collection}/{name}");
            let labels = controller.workload_labels(&ctx, "web", controller.build_labels(&name));
            let old = serde_json::json!({
                "metadata":{"name":name,"resourceVersion":"17","generation":1,"labels":labels},
                "spec":{"replicas":1,"serviceName":"retained-service","selector":{"matchLabels":{"app":name}},
                    "template":{"spec":{"containers":[{"name":"web","image":"registry.example.com/web:broken"}]}},
                    "volumeClaimTemplates":[{"metadata":{"name":"data"},"spec":{"accessModes":["ReadWriteOnce"],"resources":{"requests":{"storage":"1Gi"}}}}]},
                "status":{"replicas":1,"readyReplicas":1,"observedGeneration":1}
            });
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(&path);
                then.status(200).json_body(old.clone());
            });
            server.mock(|when, then| {
                when.method(httpmock::Method::POST).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}"));
                then.status(409).json_body(serde_json::json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"AlreadyExists","code":409}));
            });
            let mut updated = old.clone();
            updated["metadata"]["generation"] = 2.into();
            let write = server.mock(|when, then| {
                when.method(httpmock::Method::PUT).path(&path)
                    .json_body_partial(serde_json::json!({
                        "metadata":{"resourceVersion":"17"},
                        "spec":{"template":{"spec":{"containers":[{"name":"web","image":"registry.example.com/web:1"}]}}}
                    }).to_string());
                then.status(200).json_body(updated);
            });
            server.mock(|when, then| {
                when.method(httpmock::Method::GET);
                then.status(404).json_body(serde_json::json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404}));
            });
            assert!(matches!(
                controller.create_start(&ctx).await.expect("retry create"),
                KubernetesContainerHandlerAction::Continue { .. }
            ));
            assert_eq!(
                write.hits(),
                1,
                "adoption must apply the corrective image, stateful={stateful}"
            );
            assert!(
                matches!(
                    controller
                        .waiting_for_workload(&ctx)
                        .await
                        .expect("wait for new generation"),
                    KubernetesContainerHandlerAction::Stay { .. }
                ),
                "old ready pods cannot complete the corrective release"
            );
        }
    }

    #[tokio::test]
    async fn a_single_replica_restart_is_waited_for_until_the_bound() {
        for stateful in [false, true] {
            let server = httpmock::MockServer::start_async().await;
            let kind = if stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "apiVersion":"apps/v1", "kind":kind,
                    "metadata":{"name":"web", "generation":1},
                    "spec":{"replicas":1,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":{"replicas":1,"updatedReplicas":1,"observedGeneration":1}
                }));
            });
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            assert!(matches!(
                controller
                    .ready(&ctx)
                    .await
                    .expect("zero ready may be a restart"),
                KubernetesContainerHandlerAction::Stay { .. }
            ));
            controller.not_ready =
                crate::kubernetes_workload_heartbeat::NotReadyObservations::since(
                    chrono::Utc::now().timestamp()
                        - crate::kubernetes_workload_heartbeat::REFRESH_NOT_READY_BOUND_SECS,
                );
            let error = controller
                .ready(&ctx)
                .await
                .expect_err("zero ready for the whole bound is not ready");
            assert_eq!(error.code, "WORKLOAD_NOT_READY");
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn partly_ready_workload_retries_while_rolling_out_and_reports_not_ready_once_settled() {
        // (stateful, status, retried while rolling out)
        let deployment_status = |reason: &str, condition: &str| {
            serde_json::json!({
                "replicas":2,"updatedReplicas":2,"readyReplicas":1,"observedGeneration":3,
                "conditions":[{"type":"Progressing","status":condition,"reason":reason}]
            })
        };
        let cases = [
            // The new pods exist, one is not ready yet: still rolling out.
            (false, deployment_status("ReplicaSetUpdated", "True"), true),
            // Rollout finished and a replica later stopped being ready: drift.
            (
                false,
                deployment_status("NewReplicaSetAvailable", "True"),
                false,
            ),
            // Kubernetes gave up on the rollout: drift.
            (
                false,
                deployment_status("ProgressDeadlineExceeded", "False"),
                false,
            ),
            // Kubernetes gave up while new pods are still missing (a bad image): drift, even
            // though fewer replicas are updated than wanted.
            (
                false,
                serde_json::json!({"replicas":3,"updatedReplicas":1,"readyReplicas":0,
                    "observedGeneration":3,
                    "conditions":[{"type":"Progressing","status":"False","reason":"ProgressDeadlineExceeded"}]}),
                false,
            ),
            (
                true,
                serde_json::json!({"replicas":2,"updatedReplicas":2,"readyReplicas":1,
                    "observedGeneration":3,"currentRevision":"web-1","updateRevision":"web-2"}),
                true,
            ),
            (
                true,
                serde_json::json!({"replicas":2,"updatedReplicas":2,"readyReplicas":1,
                    "observedGeneration":3,"currentRevision":"web-2","updateRevision":"web-2"}),
                false,
            ),
        ];
        for (stateful, status, rolling_out) in cases {
            let server = httpmock::MockServer::start_async().await;
            let kind = if stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "apiVersion":"apps/v1", "kind":kind,
                    "metadata":{"name":"web", "generation":3},
                    "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":status
                }));
            });
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            if !rolling_out {
                // Not ready, and Kubernetes is not rolling: inside the bound it waits.
                assert!(matches!(
                    controller.ready(&ctx).await,
                    Ok(KubernetesContainerHandlerAction::Stay { .. })
                ));
                controller.not_ready =
                    crate::kubernetes_workload_heartbeat::NotReadyObservations::since(
                        chrono::Utc::now().timestamp()
                            - crate::kubernetes_workload_heartbeat::REFRESH_NOT_READY_BOUND_SECS,
                    );
            }
            let result = controller.ready(&ctx).await;
            if rolling_out {
                // Checked again without an error, so no retry is used up.
                assert!(
                    matches!(
                        result,
                        Ok(KubernetesContainerHandlerAction::Stay {
                            max_times: None,
                            ..
                        })
                    ),
                    "{status}: {result:?}"
                );
                assert!(controller.rollout_watch.is_some());
            } else {
                let error = result.expect_err("one of two replicas is not ready");
                assert_eq!(error.code, "WORKLOAD_NOT_READY", "{status}");
                assert!(!error.retryable);
            }
        }
    }

    #[tokio::test]
    async fn a_rollout_that_outlasts_its_time_bound_is_a_retryable_not_ready_with_a_clear_message()
    {
        let now = chrono::Utc::now().timestamp();
        // (stateful, status, bound in the workload spec, key the watch is kept under, seconds seen)
        let deployment_status = serde_json::json!({"replicas":2,"updatedReplicas":2,"readyReplicas":1,
            "observedGeneration":3,
            "conditions":[{"type":"Progressing","status":"True","reason":"ReplicaSetUpdated"}]});
        let statefulset_status = serde_json::json!({"replicas":2,"updatedReplicas":2,"readyReplicas":1,
            "observedGeneration":3,"currentRevision":"web-1","updateRevision":"web-2"});
        for (stateful, status, key, seen_for, expect_drift) in [
            (false, deployment_status.clone(), "generation-3", 700, true),
            // The default bound is 600 s: a rollout seen 100 s ago is still allowed.
            (false, deployment_status.clone(), "generation-3", 100, false),
            // A new revision starts its own time bound.
            (false, deployment_status.clone(), "generation-2", 700, false),
            (
                true,
                statefulset_status.clone(),
                "generation-3-web-2",
                700,
                true,
            ),
            (
                true,
                statefulset_status.clone(),
                "generation-3-web-2",
                100,
                false,
            ),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let kind = if stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "apiVersion":"apps/v1", "kind":kind,
                    "metadata":{"name":"web", "generation":3},
                    "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":status
                }));
            });
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            controller.rollout_watch = Some(RolloutWatch {
                key: key.into(),
                first_seen_unix: now - seen_for,
                complete_since_unix: None,
            });
            if expect_drift {
                controller.not_ready =
                    crate::kubernetes_workload_heartbeat::NotReadyObservations::since(
                        chrono::Utc::now().timestamp()
                            - crate::kubernetes_workload_heartbeat::REFRESH_NOT_READY_BOUND_SECS,
                    );
            }
            let result = controller.ready(&ctx).await;
            if expect_drift {
                let error = result.expect_err("the rollout outlasted its bound");
                assert_eq!(error.code, "WORKLOAD_NOT_READY");
                assert!(
                    error.message.contains("rollout has not finished"),
                    "{}",
                    error.message
                );
            } else {
                assert!(
                    matches!(result, Ok(KubernetesContainerHandlerAction::Stay { .. })),
                    "{result:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_rollout_past_its_bound_is_not_ready_even_while_the_previous_version_serves() {
        let now = chrono::Utc::now().timestamp();
        // (stateful, status, seconds the rollout has been seen, expect drift)
        let stuck_deployment = serde_json::json!({"replicas":3,"updatedReplicas":1,"readyReplicas":2,
            "availableReplicas":2,"observedGeneration":3,
            "conditions":[{"type":"Progressing","status":"False","reason":"ProgressDeadlineExceeded"}]});
        let rolling_deployment = serde_json::json!({"replicas":3,"updatedReplicas":1,"readyReplicas":2,
            "availableReplicas":2,"observedGeneration":3,
            "conditions":[{"type":"Progressing","status":"True","reason":"ReplicaSetUpdated"}]});
        let statefulset = serde_json::json!({"replicas":2,"updatedReplicas":1,"readyReplicas":2,
            "observedGeneration":3,"currentRevision":"web-1","updateRevision":"web-2"});
        for (stateful, status, seen_for, expect_drift) in [
            (false, stuck_deployment, 0, true),
            (false, rolling_deployment, 0, false),
            (true, statefulset.clone(), 700, true),
            (true, statefulset, 100, false),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let kind = if stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            let collection = if stateful {
                "statefulsets"
            } else {
                "deployments"
            };
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "apiVersion":"apps/v1", "kind":kind,
                    "metadata":{"name":"web", "generation":3},
                    "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":status
                }));
            });
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            if seen_for > 0 {
                controller.rollout_watch = Some(RolloutWatch {
                    key: "generation-3-web-2".into(),
                    first_seen_unix: now - seen_for,
                    complete_since_unix: None,
                });
            }
            if expect_drift {
                controller.not_ready =
                    crate::kubernetes_workload_heartbeat::NotReadyObservations::since(
                        chrono::Utc::now().timestamp()
                            - crate::kubernetes_workload_heartbeat::REFRESH_NOT_READY_BOUND_SECS,
                    );
                let error = controller
                    .ready(&ctx)
                    .await
                    .expect_err("the new version never rolled out");
                assert_eq!(error.code, "WORKLOAD_NOT_READY");
                assert!(
                    error.message.contains("'web' rollout is stalled")
                        && (error.message.contains("ProgressDeadlineExceeded")
                            || error.message.contains("deadline")),
                    "{}",
                    error.message
                );
            } else {
                // Inside the bound the previous version serving is healthy; the later
                // steps need a full harness, so only the absence of drift is checked.
                let result = controller.ready(&ctx).await;
                assert!(
                    result.is_ok()
                        || !result
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.message.contains("did not finish within")),
                    "{result:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_container_with_an_unfinished_rollout_does_not_recover_while_old_pods_serve() {
        for stateful in [false, true] {
            let server = httpmock::MockServer::start_async().await;
            let (kind, collection) = if stateful {
                ("StatefulSet", "statefulsets")
            } else {
                ("Deployment", "deployments")
            };
            let status = if stateful {
                serde_json::json!({"replicas":2,"updatedReplicas":1,"readyReplicas":2,
                    "observedGeneration":3,"currentRevision":"web-1","updateRevision":"web-2"})
            } else {
                serde_json::json!({"replicas":3,"updatedReplicas":1,"readyReplicas":2,
                    "availableReplicas":2,"observedGeneration":3,
                    "conditions":[{"type":"Progressing","status":"True","reason":"ReplicaSetUpdated"}]})
            };
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                then.status(200).json_body(serde_json::json!({
                    "apiVersion":"apps/v1", "kind":kind,
                    "metadata":{"name":"web", "generation":3},
                    "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                    "status":status
                }));
            });
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.is_stateful = stateful;
            controller.state = KubernetesContainerState::RefreshFailed;
            controller.recovery_checks = 2;
            let result = controller.refresh_failed(&ctx).await;
            assert!(
                matches!(
                    result,
                    Ok(KubernetesContainerHandlerAction::Stay {
                        max_times: None,
                        ..
                    })
                ),
                "{kind}: {result:?}"
            );
            assert_eq!(controller.recovery_checks, 0, "{kind}");
        }
    }

    #[tokio::test]
    async fn a_refresh_failed_container_needs_consecutive_healthy_checks_to_recover() {
        let config = manifest_test_container(&[], false);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let mut controller = manifest_test_controller();
        let labels = {
            let ctx = harness.ctx();
            controller.workload_labels(&ctx, "web", controller.build_labels("web"))
        };
        let server = httpmock::MockServer::start_async().await;
        let mut healthy = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/deployments/web");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"Deployment",
                "metadata":{"name":"web", "generation":3, "labels":labels},
                "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                "status":{"replicas":2,"updatedReplicas":2,"readyReplicas":2,"availableReplicas":2,
                    "observedGeneration":3,
                    "conditions":[{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable"}]}
            }));
        });
        for (path, kind) in [("pods", "PodList"), ("events", "EventList")] {
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path(format!("/api/v1/namespaces/test-ns/{path}"));
                then.status(200).json_body(
                    serde_json::json!({"apiVersion":"v1","kind":kind,"metadata":{},"items":[]}),
                );
            });
        }
        let mut ctx = harness.ctx();
        point_at(&mut ctx, &server);
        controller.state = KubernetesContainerState::RefreshFailed;

        // Healthy, then unhealthy again (a crash-looping pod): the count starts over.
        for _ in 0..2 {
            controller.step(&ctx).await.expect("healthy check");
            assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
        }
        healthy.delete();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/deployments/web");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"Deployment",
                "metadata":{"name":"web", "generation":3},
                "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                "status":{"replicas":2,"updatedReplicas":2,"readyReplicas":1,"observedGeneration":3,
                    "conditions":[{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable"}]}
            }));
        });
        let error = controller
            .step(&ctx)
            .await
            .expect_err("real drift keeps the failure");
        assert_eq!(error.code, "WORKLOAD_NOT_READY");
        assert_eq!(controller.recovery_checks, 0);
        assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
    }

    #[tokio::test]
    async fn a_refresh_failed_container_is_checked_again_and_recovers_when_healthy() {
        let config = manifest_test_container(&[], false);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let mut controller = manifest_test_controller();
        let labels = {
            let ctx = harness.ctx();
            controller.workload_labels(&ctx, "web", controller.build_labels("web"))
        };
        let server = httpmock::MockServer::start_async().await;
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/apis/apps/v1/namespaces/test-ns/deployments/web");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"Deployment",
                "metadata":{"name":"web", "generation":3, "labels":labels},
                "spec":{"replicas":2,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                "status":{"replicas":2,"updatedReplicas":2,"readyReplicas":2,"availableReplicas":2,
                    "observedGeneration":3,
                    "conditions":[{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable"}]}
            }));
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/v1/namespaces/test-ns/pods");
            then.status(200).json_body(
                serde_json::json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":[]}),
            );
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/v1/namespaces/test-ns/events");
            then.status(200).json_body(
                serde_json::json!({"apiVersion":"v1","kind":"EventList","metadata":{},"items":[]}),
            );
        });
        let mut ctx = harness.ctx();
        point_at(&mut ctx, &server);

        controller.state = KubernetesContainerState::RefreshFailed;
        assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
        // The next step of a refresh-failed container is its health check.
        for _ in 0..crate::kubernetes_workload_heartbeat::RECOVERY_HEALTHY_CHECKS {
            assert_eq!(controller.get_status(), ResourceStatus::RefreshFailed);
            let result = controller.step(&ctx).await;
            assert!(result.is_ok(), "{result:?}");
            // A step right after another does not count: the executor can step quickly.
            let again = controller.step(&ctx).await;
            assert!(again.is_ok(), "{again:?}");
            // Pretend the counted check was a while ago.
            controller.last_recovery_check_unix = controller
                .last_recovery_check_unix
                .map(|t| t - crate::kubernetes_workload_heartbeat::RECOVERY_CHECK_MIN_GAP_SECS);
        }
        assert_eq!(controller.get_status(), ResourceStatus::Running);
    }

    #[test]
    fn zero_replica_workload_waits_for_the_controller_to_observe_its_generation() {
        assert!(!workload_ready(0, None, None, None, Some(2)));
        assert!(!workload_ready(0, None, None, Some(1), Some(2)));
        assert!(workload_ready(0, None, None, Some(2), Some(2)));
        assert!(!workload_ready(0, Some(1), Some(1), Some(2), Some(2)));
    }

    #[tokio::test]
    async fn workload_progress_reports_current_replica_failure_and_clears_after_repair() {
        for updating in [false, true] {
            let server = httpmock::MockServer::start_async().await;
            crate::core::kubernetes_manifest_test_support::mock_empty_pod_list(&server);
            let mut response = serde_json::json!({
                "apiVersion": "apps/v1", "kind": "Deployment",
                "metadata": {"name": "web", "generation": 2},
                "status": {"replicas": 0, "readyReplicas": 0, "observedGeneration": 2,
                    "conditions": [{"type": "ReplicaFailure", "status": "True",
                        "reason": "FailedCreate", "message": "pods are forbidden: exceeded quota: site-pods"}]}
            });
            let config = manifest_test_container(&[], false);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
            let mut ctx = harness.ctx();
            point_at(&mut ctx, &server);
            let mut controller = manifest_test_controller();
            controller.state = if updating {
                KubernetesContainerState::WaitingForUpdate
            } else {
                KubernetesContainerState::WaitingForWorkload
            };
            for (observed, failed, expected) in [
                (
                    2,
                    true,
                    "FailedCreate: pods are forbidden: exceeded quota: site-pods",
                ),
                (1, true, "Waiting for workload readiness."),
                (2, false, "Waiting for workload readiness."),
            ] {
                response["status"]["observedGeneration"] = observed.into();
                response["status"]["conditions"][0]["status"] =
                    if failed { "True" } else { "False" }.into();
                let mut request = server.mock(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/apis/apps/v1/namespaces/test-ns/deployments/web");
                    then.status(200).json_body(response.clone());
                });
                let action = if updating {
                    controller.waiting_for_update(&ctx).await
                } else {
                    controller.waiting_for_workload(&ctx).await
                }
                .unwrap();
                assert!(matches!(
                    action,
                    KubernetesContainerHandlerAction::Stay { .. }
                ));
                request.assert_hits(1);
                request.delete();
                let restored: KubernetesContainerController =
                    serde_json::from_value(serde_json::to_value(&controller).unwrap()).unwrap();
                assert_eq!(ResourceController::get_progress(&restored), Some(expected));
            }
        }
    }

    #[tokio::test]
    async fn initial_workload_accepts_ready_capacity_and_waits_for_unready_pods() {
        for stateful in [false, true] {
            for (ready, total, complete) in
                [(1, 1, true), (3, 3, true), (1, 3, false), (0, 1, false)]
            {
                let server = httpmock::MockServer::start_async().await;
                crate::core::kubernetes_manifest_test_support::mock_empty_pod_list(&server);
                let kind = if stateful {
                    "StatefulSet"
                } else {
                    "Deployment"
                };
                let collection = if stateful {
                    "statefulsets"
                } else {
                    "deployments"
                };
                server.mock(|when, then| {
                    when.method(httpmock::Method::GET).path(format!("/apis/apps/v1/namespaces/test-ns/{collection}/web"));
                    then.status(200).json_body(serde_json::json!({
                        "apiVersion":"apps/v1", "kind":kind,
                        "metadata":{"name":"web", "generation":1},
                        "spec":{"replicas":3,"selector":{"matchLabels":{"app":"web"}},"template":{"spec":{"containers":[]}}},
                        "status":{"replicas":total,"readyReplicas":ready,"observedGeneration":1}
                    }));
                });
                let mut config = manifest_test_container(&[], stateful);
                config.replicas = Some(3);
                let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
                let mut ctx = harness.ctx();
                point_at(&mut ctx, &server);
                let mut controller = manifest_test_controller();
                controller.is_stateful = stateful;
                // Ready capacity completes once readiness has held for three polls.
                let mut action = controller
                    .waiting_for_workload(&ctx)
                    .await
                    .expect("workload poll");
                for _ in 0..2 {
                    action = controller
                        .waiting_for_workload(&ctx)
                        .await
                        .expect("workload poll");
                }
                assert_eq!(
                    matches!(action, KubernetesContainerHandlerAction::Continue { .. }),
                    complete,
                    "{kind}, ready={ready}, total={total}"
                );
            }
        }
    }

    #[test]
    fn deployment_update_waits_for_the_new_revision_to_be_available() {
        let mut deployment = Deployment {
            metadata: ObjectMeta {
                generation: Some(2),
                ..Default::default()
            },
            status: Some(DeploymentStatus {
                observed_generation: Some(1),
                replicas: Some(1),
                ready_replicas: Some(1),
                available_replicas: Some(1),
                updated_replicas: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));

        let status = deployment.status.as_mut().unwrap();
        status.observed_generation = Some(2);
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));

        let status = deployment.status.as_mut().unwrap();
        status.replicas = Some(2);
        status.updated_replicas = Some(1);
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));

        let status = deployment.status.as_mut().unwrap();
        status.replicas = Some(1);
        status.available_replicas = Some(0);
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));

        deployment.status.as_mut().unwrap().available_replicas = Some(1);
        assert!(deployment_rollout_complete(&deployment, 1, Some(2)));

        deployment.metadata.generation = Some(1);
        deployment.status.as_mut().unwrap().observed_generation = Some(1);
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));
    }

    #[test]
    fn zero_replica_deployment_update_waits_only_for_observed_generation() {
        let mut deployment = Deployment {
            metadata: ObjectMeta {
                generation: Some(2),
                ..Default::default()
            },
            status: Some(DeploymentStatus {
                observed_generation: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!deployment_rollout_complete(&deployment, 0, Some(2)));
        deployment.status.as_mut().unwrap().observed_generation = Some(2);
        assert!(deployment_rollout_complete(&deployment, 0, Some(2)));
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));
    }

    #[test]
    fn statefulset_update_waits_for_the_current_revision_to_advance() {
        let mut statefulset = StatefulSet {
            metadata: ObjectMeta {
                generation: Some(2),
                ..Default::default()
            },
            status: Some(StatefulSetStatus {
                observed_generation: Some(2),
                replicas: 1,
                ready_replicas: Some(1),
                updated_replicas: Some(0),
                current_revision: Some("old".to_string()),
                update_revision: Some("new".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!statefulset_rollout_complete(&statefulset, 1, Some(2)));

        statefulset.status.as_mut().unwrap().updated_replicas = Some(1);
        assert!(!statefulset_rollout_complete(&statefulset, 1, Some(2)));

        statefulset.status.as_mut().unwrap().current_revision = Some("new".to_string());
        assert!(statefulset_rollout_complete(&statefulset, 1, Some(2)));

        statefulset.metadata.generation = Some(1);
        statefulset.status.as_mut().unwrap().observed_generation = Some(1);
        assert!(!statefulset_rollout_complete(&statefulset, 1, Some(2)));
    }

    #[test]
    fn test_kubernetes_container_name() {
        // Test basic functionality
        assert_eq!(
            kubernetes_resource_name("my-stack", "my-container"),
            "my-container"
        );

        // Test character filtering and lowercasing
        assert_eq!(
            kubernetes_resource_name("My_Stack!", "Test#123"),
            "test-123"
        );

        // Test length truncation
        let long_prefix = "a".repeat(50);
        let long_id = "b".repeat(20);
        let result = kubernetes_resource_name(&long_prefix, &long_id);
        assert!(result.len() <= 63);
        assert_eq!(result, long_id);
    }

    #[test]
    fn test_kubernetes_service_account_name() {
        // Test basic functionality
        assert_eq!(
            kubernetes_service_account_name("my-app", "reader"),
            "my-app-reader-sa"
        );

        // Test character filtering
        assert_eq!(
            kubernetes_service_account_name("My_App!", "Writer#Profile"),
            "my-app-writer-profile-sa"
        );

        // Test length truncation
        let long_prefix = "a".repeat(50);
        let result = kubernetes_service_account_name(&long_prefix, "reader");
        assert!(result.len() <= 63);
    }

    #[test]
    fn deployment_secret_matching_respects_exact_and_wildcard_targets() {
        let vars = vec![
            alien_core::EnvironmentVariable {
                name: "APP_SECRET".to_string(),
                value: "secret".to_string(),
                var_type: alien_core::EnvironmentVariableType::Secret,
                target_resources: Some(vec!["api".to_string()]),
            },
            alien_core::EnvironmentVariable {
                name: "WORKER_SECRET".to_string(),
                value: "secret".to_string(),
                var_type: alien_core::EnvironmentVariableType::Secret,
                target_resources: Some(vec!["worker*".to_string()]),
            },
            alien_core::EnvironmentVariable {
                name: "PLAIN".to_string(),
                value: "not-secret".to_string(),
                var_type: alien_core::EnvironmentVariableType::Plain,
                target_resources: None,
            },
        ];

        let api_vars = applicable_secret_environment_variables("api", &vars);
        assert_eq!(api_vars.len(), 1);
        assert_eq!(api_vars[0].name, "APP_SECRET");

        let worker_vars = applicable_secret_environment_variables("worker-main", &vars);
        assert_eq!(worker_vars.len(), 1);
        assert_eq!(worker_vars[0].name, "WORKER_SECRET");

        let dashboard_vars = applicable_secret_environment_variables("dashboard", &vars);
        assert!(dashboard_vars.is_empty());
    }

    #[test]
    fn command_receiver_token_flows_through_secret_plan_scoped_per_resource() {
        // The receiver's `ALIEN_COMMANDS_TOKEN` is a Secret-kind env
        // var scoped per Container/Daemon. `applicable_secret_environment_variables`
        // is what drives `KubernetesEnvSecretPlan.keys` → each key rendered as a
        // `secretKeyRef` in the container/daemon manifests. This asserts the token
        // reaches exactly its own resource and never a sibling — the same
        // machinery for both container and daemon controllers.
        let vars = vec![
            alien_core::EnvironmentVariable {
                name: alien_core::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                value: "tok-container".to_string(),
                var_type: alien_core::EnvironmentVariableType::Secret,
                target_resources: Some(vec!["api-container".to_string()]),
            },
            alien_core::EnvironmentVariable {
                name: alien_core::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                value: "tok-daemon".to_string(),
                var_type: alien_core::EnvironmentVariableType::Secret,
                target_resources: Some(vec!["log-daemon".to_string()]),
            },
            // Plain receiver vars must NOT be selected as secrets.
            alien_core::EnvironmentVariable {
                name: alien_core::ENV_ALIEN_COMMANDS_URL.to_string(),
                value: "https://cmd.example.test/v1".to_string(),
                var_type: alien_core::EnvironmentVariableType::Plain,
                target_resources: Some(vec!["api-container".to_string()]),
            },
        ];

        let container_secrets = applicable_secret_environment_variables("api-container", &vars);
        assert_eq!(container_secrets.len(), 1);
        assert_eq!(
            container_secrets[0].name,
            alien_core::ENV_ALIEN_COMMANDS_TOKEN
        );
        assert_eq!(container_secrets[0].value, "tok-container");

        let daemon_secrets = applicable_secret_environment_variables("log-daemon", &vars);
        assert_eq!(daemon_secrets.len(), 1);
        assert_eq!(daemon_secrets[0].value, "tok-daemon");

        // A resource with no scoped receiver token gets no secret keys — the
        // commands_enabled=false / non-receiver case yields an empty plan.
        let other = applicable_secret_environment_variables("worker-main", &vars);
        assert!(other.is_empty());
    }

    #[test]
    fn internal_service_uses_declared_container_ports() {
        let config = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "100m".to_string(),
                desired: "500m".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "128Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .port(3000)
            .permissions("runtime".to_string())
            .build();
        let controller = KubernetesContainerController {
            state: KubernetesContainerState::Ready,
            workload_name: Some("api".to_string()),
            is_stateful: false,
            namespace: Some("test-ns".to_string()),
            service_name: Some("api".to_string()),
            service_ports: Vec::new(),
            service_port: Some(3000),
            network_policy_name: None,
            peer_service_name: None,
            container_id: Some("api".to_string()),
            public_endpoint: KubernetesPublicEndpointState::default(),
            env_secret: EnvSecretRotationTracker::default(),
            update_generation: None,
            workload_failure: None,
            applied_priority_class: None,
            applied_architecture: None,
            applied_gateway_dns: None,
            zero_ready_observed: false,
            rollout_watch: None,
            recovery_checks: 0,
            not_ready: Default::default(),
            ready_hold: Default::default(),
            update_template_unchanged: false,
            recovering: false,
            last_recovery_check_unix: None,
            _internal_stay_count: None,
        };
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![]);
        let ctx = harness.ctx();

        let service = controller
            .build_internal_service(&config, "api", "test-ns", &ctx)
            .expect("internal service");
        let spec = service.spec.expect("service spec");

        assert_eq!(service.metadata.name.as_deref(), Some("api"));
        assert_eq!(
            service
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("alien.dev/deployment"))
                .map(String::as_str),
            Some("test")
        );
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));
        let ports = spec.ports.expect("service ports");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 3000);
        assert_eq!(ports[0].target_port, Some(IntOrString::Int(3000)));
    }

    // ── Typed manifest tests ─────────────────────────────────────────
    //
    // These build the actual k8s-openapi objects the controller submits and
    // assert on the typed fields — no string snapshots.

    use crate::core::kubernetes_manifest_test_support::{
        assert_secret_key_ref, deployment_env, pod_template_checksum_annotation, pod_template_env,
        secret_env_var, KubernetesManifestTestHarness,
    };
    use crate::core::{
        direct_monitoring_auth_headers, environment_secret_plan,
        environment_secret_plan_with_additional_secrets, OTEL_EXPORTER_OTLP_HEADERS,
        OTEL_EXPORTER_OTLP_METRICS_HEADERS,
    };
    use alien_core::{
        ContainerSecurity, ContainerSecurityProfile, KubernetesHttpProbe, KubernetesSecretMount,
        OtlpConfig, Resource, ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_DEPLOYER_SECRETS,
        ENV_ALIEN_LAMBDA_MODE, ENV_ALIEN_RUNTIME_SECRETS, ENV_ALIEN_RUNTIME_SEND_OTLP,
        ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT, ENV_ALIEN_WORKER_GRPC_ADDRESS,
    };
    fn manifest_test_container(environment: &[(&str, &str)], stateful: bool) -> Container {
        let mut config = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: "registry.example.com/web:1".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "100m".to_string(),
                desired: "500m".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "128Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .stateful(stateful)
            .permissions("default".to_string())
            .build();
        for (name, value) in environment {
            config
                .environment
                .insert(name.to_string(), value.to_string());
        }
        config
    }

    #[test]
    fn controller_state_written_before_rollout_tracking_still_loads() {
        let mut controller = manifest_test_controller();
        controller.rollout_watch = Some(RolloutWatch {
            key: "generation-3".into(),
            first_seen_unix: 1000,
            complete_since_unix: None,
        });
        controller.recovery_checks = 2;
        controller.last_recovery_check_unix = Some(1000);
        controller.zero_ready_observed = true;
        let mut state = serde_json::to_value(controller).unwrap();
        for field in [
            "rolloutWatch",
            "recoveryChecks",
            "lastRecoveryCheckUnix",
            "zeroReadyObserved",
        ] {
            assert!(
                state.as_object_mut().unwrap().remove(field).is_some(),
                "{field} was not in the written state"
            );
        }
        let controller: KubernetesContainerController = serde_json::from_value(state).unwrap();
        assert!(controller.rollout_watch.is_none());
        assert_eq!(controller.recovery_checks, 0);
        assert_eq!(controller.last_recovery_check_unix, None);
        assert!(!controller.zero_ready_observed);
    }

    /// A container refused before its controller recorded a namespace has nothing recorded: deleting
    /// it has nothing to delete and completes instead of failing the whole delete.
    #[tokio::test]
    async fn a_container_that_never_got_as_far_as_a_namespace_deletes_cleanly() {
        let config = manifest_test_container(&[], false);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        let ctx = harness.ctx();
        let mut controller = KubernetesContainerController::default();
        assert!(controller.namespace.is_none() && controller.workload_name.is_none());

        let action = controller
            .delete_start(&ctx)
            .await
            .expect("nothing is recorded, so there is nothing to fail on");
        assert!(
            matches!(
                action,
                KubernetesContainerHandlerAction::Continue {
                    state: KubernetesContainerState::Deleted,
                    ..
                }
            ),
            "{action:?}"
        );
    }

    fn manifest_test_controller() -> KubernetesContainerController {
        KubernetesContainerController {
            workload_name: Some("web".to_string()),
            namespace: Some("test-ns".to_string()),
            service_name: Some("web".to_string()),
            container_id: Some("web".to_string()),
            ..Default::default()
        }
    }

    /// A deployment whose secrets vault is Kubernetes Secrets in `test-ns`.
    fn secrets_vault_state() -> alien_core::StackState {
        use crate::core::state_utils::StackResourceStateExt;
        let mut state = alien_core::StackState::new(alien_core::Platform::Kubernetes);
        let mut vault = alien_core::StackResourceState::new_pending(
            "vault".into(),
            Resource::new(alien_core::Vault::new(alien_core::SECRETS_VAULT_ID.into()).build()),
            None,
            vec![],
        );
        vault
            .set_internal_controller(Some(Box::new(crate::vault::KubernetesVaultController {
                namespace: Some("test-ns".into()),
                vault_prefix: Some("test-secrets".into()),
                ..Default::default()
            })))
            .unwrap();
        state
            .resources
            .insert(alien_core::SECRETS_VAULT_ID.into(), vault);
        state
    }

    fn point_at(ctx: &mut ResourceControllerContext<'_>, server: &httpmock::MockServer) {
        let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
    }

    fn gateway_container(image: &str) -> Container {
        let mut gateway = manifest_test_container(&[], false);
        gateway.id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string();
        gateway.code = ContainerCode::Image {
            image: image.to_string(),
        };
        gateway
    }

    /// A `--secret` with no target is for the stack's own workloads. The gateway Alien adds does
    /// not get it in its environment Secret, so it does not count as drift there: drift
    /// restarts the gateway pods.
    #[tokio::test]
    async fn an_untargeted_secret_does_not_reach_the_managed_gateway() {
        use crate::core::kubernetes_manifest_test_support::secret_env_var;
        use httpmock::Method::POST;
        unsafe {
            std::env::set_var(
                crate::sandbox::kubernetes_egress::CLUSTER_DOMAIN_ENV,
                "corp.example",
            )
        };
        let server = httpmock::MockServer::start_async().await;
        fn secret_named(
            request: &httpmock::prelude::HttpMockRequest,
            name: &str,
        ) -> Option<serde_json::Value> {
            let secret: serde_json::Value =
                serde_json::from_slice(request.body.as_deref()?).ok()?;
            (secret["metadata"]["name"] == name).then_some(secret)
        }
        let replica_secret = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/namespaces/test-ns/secrets")
                    .matches(|request| {
                        secret_named(request, "test-secrets-sandbox-egress-replica-secret")
                            .is_some()
                    });
                then.status(201).json_body(serde_json::json!({
                    "apiVersion":"v1","kind":"Secret","metadata":{"name":"test-secrets-sandbox-egress-replica-secret"}
                }));
            })
            .await;
        // The stack's own secret must not be written to the gateway's environment Secret.
        let leaked = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/namespaces/test-ns/secrets")
                    .matches(|request| {
                        secret_named(request, "gateway-env")
                            .is_some_and(|secret| secret["data"]["FOR_STACK"].is_string())
                    });
                then.status(201).json_body(serde_json::json!({
                    "apiVersion":"v1","kind":"Secret","metadata":{"name":"gateway-env"}
                }));
            })
            .await;
        // Only the targeted secret and the restart settings may be written.
        let environment_secret = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/namespaces/test-ns/secrets")
                    .matches(|request| {
                        secret_named(request, "gateway-env").is_some_and(|secret| {
                            let mut keys: Vec<_> = secret["data"]
                                .as_object()
                                .map(|data| data.keys().cloned().collect())
                                .unwrap_or_default();
                            keys.sort();
                            keys == ["ALIEN_EGRESS_GATEWAY_RESTART_SETTINGS"]
                        })
                    });
                then.status(201).json_body(serde_json::json!({
                    "apiVersion":"v1","kind":"Secret","metadata":{"name":"gateway-env"}
                }));
            })
            .await;

        let gateway = gateway_container(
            "example.invalid/gateway@sha256:1111111111111111111111111111111111111111111111111111111111111111",
        );
        let variables = |stack_secret: &str| {
            vec![
                secret_env_var("FOR_STACK", stack_secret, None),
                secret_env_var(
                    "FOR_GATEWAY",
                    "gateway-value",
                    Some(vec!["sandbox-egress-gateway"]),
                ),
            ]
        };
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(gateway.clone()), variables("before"));
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "s3://acme/agents/bundle.zip".to_string(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let stack = alien_core::Stack::new("test-stack".to_string())
            .add(sandbox, alien_core::ResourceLifecycle::Frozen)
            .build();
        let state = secrets_vault_state();
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        point_at(&mut ctx, &server);
        let mut controller = manifest_test_controller();
        controller.workload_name = Some("gateway".to_string());
        assert!(alien_core::sandbox_egress::is_managed_gateway(
            ctx.desired_stack,
            &gateway.id
        ));

        let plan = controller
            .reconcile_environment_secret(&gateway, "gateway", "test-ns", false, &ctx)
            .await
            .expect("the environment Secret holds the targeted secret and the settings")
            .expect("a plan");
        assert_eq!(
            leaked.hits_async().await,
            0,
            "an untargeted secret reached the gateway's environment Secret"
        );
        assert_eq!(environment_secret.hits_async().await, 1);
        assert!(replica_secret.hits_async().await >= 1);
        controller.env_secret.record(Some(&plan));

        // Rotating the stack's secret leaves the gateway alone.
        let mut rotated = harness.ctx();
        rotated.state = &state;
        rotated.desired_stack = &stack;
        point_at(&mut rotated, &server);
        let rotated_variables = variables("after");
        let mut config = rotated.deployment_config.clone();
        config.environment_variables.variables = rotated_variables;
        rotated.deployment_config = &config;
        assert!(!controller
            .gateway_environment_drifted(&gateway, "gateway", &rotated)
            .await
            .expect("drift check"));
    }

    const GATEWAY_IMAGE: &str = "example.invalid/gateway@sha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn kube_dns_mock<'a>(server: &'a httpmock::MockServer, address: &'a str) -> httpmock::Mock<'a> {
        use httpmock::Method::GET;
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/namespaces/kube-system/services/kube-dns");
            then.status(200).json_body(serde_json::json!({
                "apiVersion":"v1","kind":"Service",
                "metadata":{"name":"kube-dns","namespace":"kube-system"},
                "spec":{"clusterIP":address}
            }));
        })
    }

    async fn gateway_pod(
        controller: &KubernetesContainerController,
        gateway: &Container,
        ctx: &ResourceControllerContext<'_>,
    ) -> k8s_openapi::api::core::v1::PodSpec {
        controller
            .build_deployment(gateway, "gateway", "test-ns", "gateway-sa", None, None, ctx)
            .await
            .expect("deployment manifest")
            .spec
            .expect("deployment spec")
            .template
            .spec
            .expect("pod spec")
    }

    /// The gateway's lookups go to the cluster DNS Service address, which its egress policy
    /// admits through the `kube-dns` pod selector, not to a link-local address a node-local
    /// cache may have put in its resolver file.
    #[tokio::test]
    async fn the_gateway_resolves_through_the_cluster_dns_service() {
        let server = httpmock::MockServer::start_async().await;
        let dns = kube_dns_mock(&server, "10.0.0.10");
        let gateway = gateway_container(GATEWAY_IMAGE);
        let harness = KubernetesManifestTestHarness::new(Resource::new(gateway.clone()), vec![]);
        let state = secrets_vault_state();
        let mut ctx = harness.ctx();
        ctx.state = &state;
        point_at(&mut ctx, &server);
        let mut controller = manifest_test_controller();
        controller
            .refresh_gateway_dns(&gateway, Some("corp.example".into()), &ctx)
            .await
            .expect("resolver choice");
        let pod = gateway_pod(&controller, &gateway, &ctx).await;
        assert_eq!(dns.hits(), 1);
        assert_eq!(pod.dns_policy.as_deref(), Some("None"));
        let config = pod.dns_config.expect("resolver settings");
        assert_eq!(config.nameservers, Some(vec!["10.0.0.10".to_string()]));
        assert_eq!(
            config.searches,
            Some(vec![
                "test-ns.svc.corp.example".to_string(),
                "svc.corp.example".to_string(),
                "corp.example".to_string()
            ])
        );
        assert_eq!(config.options.unwrap()[0].value.as_deref(), Some("5"));

        // A workload that is not the gateway keeps the cluster's resolver and reads nothing.
        let other = manifest_test_container(&[], false);
        let harness = KubernetesManifestTestHarness::new(Resource::new(other.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        point_at(&mut ctx, &server);
        controller
            .refresh_gateway_dns(&other, Some("corp.example".into()), &ctx)
            .await
            .expect("no choice");
        let pod = controller
            .build_deployment(&other, "app", "test-ns", "app-sa", None, None, &ctx)
            .await
            .expect("deployment manifest")
            .spec
            .expect("deployment spec")
            .template
            .spec
            .expect("pod spec");
        assert!(pod.dns_policy.is_none() && pod.dns_config.is_none());
        assert_eq!(dns.hits(), 1);
    }

    /// An existing gateway (built with the cluster resolver, nothing recorded) rolls exactly when
    /// the resolver now chosen differs, and not again on the next reconcile.
    #[tokio::test]
    async fn the_gateway_rolls_once_when_its_resolved_dns_changes() {
        let server = httpmock::MockServer::start_async().await;
        let mut dns = kube_dns_mock(&server, "10.0.0.10");
        let gateway = gateway_container(GATEWAY_IMAGE);
        let harness = KubernetesManifestTestHarness::new(Resource::new(gateway.clone()), vec![]);
        let state = secrets_vault_state();
        let mut ctx = harness.ctx();
        ctx.state = &state;
        point_at(&mut ctx, &server);
        let domain = || Some("corp.example".to_string());
        let mut controller = manifest_test_controller();

        // The cluster default was applied before the choice was recorded: the Service is now
        // readable, so the gateway rolls once ...
        assert!(controller
            .gateway_dns_drifted(domain(), &ctx)
            .await
            .unwrap());
        controller
            .refresh_gateway_dns(&gateway, domain(), &ctx)
            .await
            .unwrap();
        // ... and neither the next reconcile nor the one after rolls it again.
        for _ in 0..2 {
            assert!(!controller
                .gateway_dns_drifted(domain(), &ctx)
                .await
                .unwrap());
        }

        // The address moves: one more roll.
        dns.delete();
        let _moved = kube_dns_mock(&server, "10.0.0.11");
        assert!(controller
            .gateway_dns_drifted(domain(), &ctx)
            .await
            .unwrap());
        controller
            .refresh_gateway_dns(&gateway, domain(), &ctx)
            .await
            .unwrap();
        assert!(!controller
            .gateway_dns_drifted(domain(), &ctx)
            .await
            .unwrap());

        // A cluster that has no cluster domain, or a Service that cannot be read, is no change.
        assert!(controller.gateway_dns_drifted(None, &ctx).await.unwrap());
        let mut unrecorded = manifest_test_controller();
        assert!(!unrecorded.gateway_dns_drifted(None, &ctx).await.unwrap());
        assert!(unrecorded.applied_gateway_dns.is_some());
    }

    /// The cluster DNS Service read decides only which resolver the gateway uses, never whether
    /// it deploys. A refused read (an install made before the Operator could read it, or a cluster
    /// that does not let the grant be created), an absent Service, a server error and an API server
    /// that does not answer all leave the gateway the cluster's resolver, and a running gateway is
    /// not rolled by them.
    #[tokio::test]
    async fn a_gateway_keeps_the_cluster_resolver_when_the_dns_service_cannot_be_read() {
        use httpmock::Method::GET;
        for status in [Some(403u16), Some(404), Some(500), Some(503), None] {
            let server = httpmock::MockServer::start_async().await;
            if let Some(status) = status {
                server
                    .mock_async(|when, then| {
                        when.method(GET)
                            .path("/api/v1/namespaces/kube-system/services/kube-dns");
                        then.status(status).json_body(serde_json::json!({
                            "kind":"Status","apiVersion":"v1","status":"Failure","code":status
                        }));
                    })
                    .await;
            }
            let gateway = gateway_container(GATEWAY_IMAGE);
            let harness =
                KubernetesManifestTestHarness::new(Resource::new(gateway.clone()), vec![]);
            let state = secrets_vault_state();
            let mut ctx = harness.ctx();
            ctx.state = &state;
            point_at(&mut ctx, &server);
            if status.is_none() {
                // Nothing listens at the address: the request gets no answer.
                drop(server);
                let alien_core::ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
                    panic!("Kubernetes config")
                };
                let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut()
                else {
                    panic!("manual config")
                };
                *server_url = "http://127.0.0.1:9".to_string();
            }
            let domain = || Some("corp.example".to_string());
            let mut controller = manifest_test_controller();
            controller
                .refresh_gateway_dns(&gateway, domain(), &ctx)
                .await
                .unwrap_or_else(|error| {
                    panic!("{status:?}: a failed read must leave the default resolver: {error}")
                });
            let pod = gateway_pod(&controller, &gateway, &ctx).await;
            assert!(
                pod.dns_policy.is_none() && pod.dns_config.is_none(),
                "{status:?}"
            );
            assert!(!controller
                .gateway_dns_drifted(domain(), &ctx)
                .await
                .unwrap());

            // A gateway already on the Service address keeps it through the failed read.
            let applied = GatewayDns::ClusterService {
                address: "10.0.0.10".into(),
                domain: "corp.example".into(),
            };
            controller.applied_gateway_dns = Some(applied.clone());
            assert!(!controller
                .gateway_dns_drifted(domain(), &ctx)
                .await
                .unwrap());
            controller
                .refresh_gateway_dns(&gateway, domain(), &ctx)
                .await
                .unwrap();
            assert_eq!(controller.applied_gateway_dns, Some(applied), "{status:?}");
        }
    }

    /// A gateway on an image that reads mounted files gets the vault's configuration Secret and
    /// replica-secret Secret mounted by the kubelet, and the controller reads neither: it only
    /// creates the replica secret before the pods need it. An earlier image still gets the
    /// configuration, read by the controller, in its environment.
    #[tokio::test]
    async fn a_mounting_gateway_gets_its_secrets_from_the_kubelet_not_the_controller() {
        // Outside a cluster the controller is told the cluster DNS domain.
        unsafe {
            std::env::set_var(
                crate::sandbox::kubernetes_egress::CLUSTER_DOMAIN_ENV,
                "corp.example",
            )
        };
        use httpmock::Method::{GET, POST};
        let server = httpmock::MockServer::start_async().await;
        let reads = server
            .mock_async(|when, then| {
                when.method(GET).path_contains("/api/v1/namespaces/test-ns/secrets/");
                then.status(403).json_body(serde_json::json!({
                    "kind":"Status","apiVersion":"v1","status":"Failure","reason":"Forbidden","code":403
                }));
            })
            .await;
        fn is_replica_secret(request: &httpmock::prelude::HttpMockRequest) -> bool {
            request
                .body
                .as_deref()
                .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
                .is_some_and(|secret| {
                    secret["metadata"]["name"] == "test-secrets-sandbox-egress-replica-secret"
                })
        }
        let replica_secret = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/namespaces/test-ns/secrets")
                    .matches(is_replica_secret);
                then.status(201).json_body(serde_json::json!({
                    "apiVersion":"v1","kind":"Secret","metadata":{"name":"test-secrets-sandbox-egress-replica-secret"}
                }));
            })
            .await;
        let gateway = gateway_container(
            "example.invalid/gateway@sha256:1111111111111111111111111111111111111111111111111111111111111111",
        );
        let harness = KubernetesManifestTestHarness::new(Resource::new(gateway.clone()), vec![]);
        let state = secrets_vault_state();
        let mut ctx = harness.ctx();
        ctx.state = &state;
        point_at(&mut ctx, &server);
        let controller = manifest_test_controller();

        let environment = controller
            .additional_environment_secrets(&gateway, &ctx)
            .await
            .expect("the replica secret is created without a read");
        assert!(!environment.contains_key("ALIEN_EGRESS_GATEWAY_CONFIG"));
        // The settings a running replica cannot refresh stay in its environment Secret, whose
        // drift restarts the pods.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&environment[GATEWAY_RESTART_SETTINGS_ENV])
                .unwrap(),
            serde_json::json!({"peers": {"dns": "sandbox-egress-gateway-peers.test-ns.svc.corp.example", "port": 3129}})
        );
        assert_eq!(replica_secret.hits_async().await, 1);
        assert_eq!(reads.hits_async().await, 0);
        // The Ready pass compares them with what the pods got: a new peer address (a changed
        // cluster domain) is drift, and drift restarts the pods.
        let mut applied = EnvSecretRotationTracker::default();
        applied.record(
            environment_secret_plan_with_additional_secrets(
                "sandbox-egress-gateway",
                "gateway",
                &[],
                &environment,
            )
            .expect("checksum key")
            .as_ref(),
        );
        assert!(!applied
            .drifted_with_additional_secrets("sandbox-egress-gateway", "gateway", &[], &environment)
            .expect("checksum key"));
        let mut moved = environment.clone();
        moved.insert(
            GATEWAY_RESTART_SETTINGS_ENV.to_string(),
            serde_json::json!({"peers": {
                "dns": "sandbox-egress-gateway-peers.test-ns.svc.other.example",
                "port": 3129
            }})
            .to_string(),
        );
        assert!(applied
            .drifted_with_additional_secrets("sandbox-egress-gateway", "gateway", &[], &moved)
            .expect("checksum key"));

        let pod = controller
            .build_deployment(
                &gateway,
                "gateway",
                "test-ns",
                "gateway-sa",
                None,
                None,
                &ctx,
            )
            .await
            .expect("deployment manifest")
            .spec
            .expect("deployment spec")
            .template
            .spec
            .expect("pod spec");
        assert_eq!(pod.automount_service_account_token, Some(false));
        let mounted: BTreeMap<_, _> = pod
            .volumes
            .iter()
            .flatten()
            .filter_map(|volume| {
                let secret = volume.secret.as_ref()?;
                Some((
                    volume.name.clone(),
                    (secret.secret_name.clone()?, secret.optional),
                ))
            })
            .collect();
        assert_eq!(
            mounted,
            BTreeMap::from([
                (
                    "egress-gateway-config".to_string(),
                    (
                        "test-secrets-sandbox-egress-gateway-config".to_string(),
                        Some(false)
                    )
                ),
                (
                    "egress-gateway-replica-secret".to_string(),
                    (
                        "test-secrets-sandbox-egress-replica-secret".to_string(),
                        Some(false)
                    )
                ),
            ])
        );
        let container = &pod.containers[0];
        let mounts: BTreeMap<_, _> = container
            .volume_mounts
            .iter()
            .flatten()
            .map(|mount| {
                (
                    mount.name.clone(),
                    (mount.mount_path.clone(), mount.read_only),
                )
            })
            .collect();
        assert_eq!(
            mounts["egress-gateway-config"],
            ("/etc/alien/egress-gateway/config".to_string(), Some(true))
        );
        assert_eq!(
            mounts["egress-gateway-replica-secret"],
            (
                "/etc/alien/egress-gateway/replica-secret".to_string(),
                Some(true)
            )
        );
        let environment = container.env.as_ref().expect("env");
        assert!(environment.iter().any(|variable| variable.name
            == "ALIEN_EGRESS_GATEWAY_CONFIG_FILE"
            && variable.value.as_deref() == Some("/etc/alien/egress-gateway/config/value")));
        assert!(!environment
            .iter()
            .any(|variable| variable.name == "ALIEN_EGRESS_GATEWAY_CONFIG"));
        // The pinned image from before: configuration in the environment, read by the controller.
        let earlier = gateway_container(
            "ghcr.io/alienplatform/alien-egress-gateway@sha256:8538880f99141888e54bedc7d290194dfee5bfe11910266b309dde53de2601b0",
        );
        let pod = controller
            .build_deployment(
                &earlier,
                "gateway",
                "test-ns",
                "gateway-sa",
                None,
                None,
                &ctx,
            )
            .await
            .unwrap()
            .spec
            .unwrap()
            .template
            .spec
            .unwrap();
        assert!(pod
            .volumes
            .iter()
            .flatten()
            .all(|volume| volume.secret.is_none()));
        let error = controller
            .additional_environment_secrets(&earlier, &ctx)
            .await
            .expect_err("the earlier image needs the controller to read the configuration");
        assert!(error.message.contains("not ready"), "{}", error.message);
        assert!(reads.hits_async().await >= 1);
    }

    #[tokio::test]
    async fn gateway_replicas_wait_out_a_resynchronization_and_get_no_api_token() {
        // Outside a cluster the controller is told the cluster DNS domain.
        unsafe {
            std::env::set_var(
                crate::sandbox::kubernetes_egress::CLUSTER_DOMAIN_ENV,
                "corp.example",
            )
        };
        let mut gateway = manifest_test_container(&[], false);
        gateway.id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string();
        let mut ordinary = manifest_test_container(&[], false);
        for config in [&mut gateway, &mut ordinary] {
            config.kubernetes_readiness_probe = Some(KubernetesHttpProbe {
                path: "/health".to_string(),
                port: 8080,
            });
        }
        let server = httpmock::MockServer::start_async().await;
        for (config, delay, automount) in [
            (&gateway, Some(GATEWAY_READY_DELAY_SECONDS), Some(false)),
            (&ordinary, None, None),
        ] {
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let state = secrets_vault_state();
            let mut ctx = harness.ctx();
            ctx.state = &state;
            point_at(&mut ctx, &server);
            let pod = manifest_test_controller()
                .build_deployment(config, "web", "test-ns", "web-sa", None, None, &ctx)
                .await
                .expect("deployment manifest")
                .spec
                .expect("deployment spec")
                .template
                .spec
                .expect("pod spec");
            assert_eq!(
                pod.automount_service_account_token, automount,
                "{}",
                config.id
            );
            assert_eq!(
                pod.containers[0]
                    .readiness_probe
                    .as_ref()
                    .and_then(|probe| probe.initial_delay_seconds),
                delay,
                "{}",
                config.id
            );
        }
        assert!(GATEWAY_READY_DELAY_SECONDS > 30);
    }

    /// Containers carry the PriorityClass the administrator mapped to their level; a container
    /// without a level (or with a level nobody mapped) gets a pod spec identical to before.
    #[tokio::test]
    async fn pod_spec_carries_the_class_mapped_to_the_declared_priority() {
        unsafe {
            std::env::set_var(
                alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES,
                r#"{"critical":"vendor-core","low":"vendor-batch"}"#,
            )
        };
        let render = |priority: Option<alien_core::WorkloadPriority>| async move {
            let mut config = manifest_test_container(&[], false);
            config.priority = priority;
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let state = secrets_vault_state();
            let mut ctx = harness.ctx();
            ctx.state = &state;
            let pod = manifest_test_controller()
                .build_deployment(&config, "web", "test-ns", "web-sa", None, None, &ctx)
                .await
                .expect("deployment manifest")
                .spec
                .expect("deployment spec")
                .template
                .spec
                .expect("pod spec");
            (
                pod.priority_class_name.clone(),
                serde_json::to_string(&pod).unwrap(),
            )
        };
        use alien_core::WorkloadPriority::*;
        assert_eq!(
            render(Some(Critical)).await.0.as_deref(),
            Some("vendor-core")
        );
        assert_eq!(render(Some(Low)).await.0.as_deref(), Some("vendor-batch"));
        // Declared but unmapped, and undeclared: no class, same bytes as a container
        // that never heard of priorities.
        let (unmapped, unmapped_json) = render(Some(High)).await;
        let (none, none_json) = render(None).await;
        assert_eq!((unmapped, none), (None, None));
        assert_eq!(unmapped_json, none_json);
        assert!(!none_json.contains("priorityClassName"));
    }

    #[test]
    fn replica_failure_for_a_missing_priority_class_names_the_values_key() {
        unsafe {
            std::env::set_var(
                alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES,
                r#"{"critical":"vendor-core","low":"vendor-batch"}"#,
            )
        };
        let deployment: Deployment = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 1},
            "status": {"observedGeneration": 1, "conditions": [{
                "type": "ReplicaFailure", "status": "True", "reason": "FailedCreate",
                "message": "pods \"web-1\" is forbidden: no PriorityClass with name vendor-core was found"}]}
        }))
        .unwrap();
        let message = deployment_failure_progress(&deployment).unwrap();
        assert!(message.contains("'vendor-core'"), "{message}");
        assert!(
            message.contains("runtime.workloadPriority.classes.critical"),
            "{message}"
        );
    }

    /// Renders the Deployment the controller would submit for a fresh copy of
    /// the same container config, as a reconcile after a new release does. The
    /// container serves several ports, so its own binding carries several
    /// internal URLs.
    async fn render_deployment_for_replicas(replicas: u32) -> Deployment {
        let names = (0..24)
            .map(|index| (format!("APP_VAR_{index:02}"), format!("value-{index}")))
            .collect::<Vec<_>>();
        let environment = names
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let mut config = manifest_test_container(&environment, false);
        config.replicas = Some(replicas);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let mut controller = manifest_test_controller();
        controller.service_ports = vec![8080, 3000, 9090, 3129, 8443, 5000, 7000, 6000];
        controller
            .build_deployment(
                &config,
                "web",
                "test-ns",
                "web-sa",
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("deployment manifest")
    }

    #[tokio::test]
    async fn scale_only_change_keeps_the_pod_template_identical() {
        let before = render_deployment_for_replicas(1)
            .await
            .spec
            .expect("deployment spec");
        let after = render_deployment_for_replicas(3)
            .await
            .spec
            .expect("deployment spec");

        assert_eq!(before.replicas, Some(1));
        assert_eq!(after.replicas, Some(3));
        // Kubernetes starts a new rollout whenever the pod template changes,
        // so a replica-count change must leave it byte-for-byte equal.
        assert_eq!(before.template, after.template);

        let app_vars = pod_template_env(&after.template)
            .into_iter()
            .map(|variable| variable.name)
            .filter(|name| name.starts_with("APP_VAR_"))
            .collect::<Vec<_>>();
        let expected = (0..24)
            .map(|index| format!("APP_VAR_{index:02}"))
            .collect::<Vec<_>>();
        assert_eq!(app_vars, expected, "plain env vars render sorted by name");

        let binding = pod_template_env(&after.template)
            .into_iter()
            .find(|variable| variable.name == "ALIEN_WEB_BINDING")
            .and_then(|variable| variable.value)
            .expect("the container's own binding");
        let binding: serde_json::Value = serde_json::from_str(&binding).expect("binding JSON");
        let ports = binding["internalUrls"]
            .as_object()
            .expect("internal URLs")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ports,
            ["3000", "3129", "5000", "6000", "7000", "8080", "8443", "9090"],
            "internal URLs render sorted by port"
        );
    }

    /// A release without a `security` field renders no pod security; only an
    /// explicit `restricted` profile restricts the Deployment's pod.
    #[tokio::test]
    async fn container_pod_is_restricted_only_when_the_release_says_so() {
        let render = |security: Option<ContainerSecurity>| async move {
            let mut config = manifest_test_container(&[], false);
            config.security = security;
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            manifest_test_controller()
                .build_deployment(
                    &config,
                    "web",
                    "test-ns",
                    "web-sa",
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("deployment manifest")
                .spec
                .expect("deployment spec")
                .template
                .spec
                .expect("pod spec")
        };
        let old = render(None).await;
        assert!(old.security_context.is_none());
        assert!(old.volumes.is_none());
        assert!(old.containers[0].security_context.is_none());
        assert!(old.containers[0].volume_mounts.is_none());

        let restricted = render(Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: Vec::new(),
        }))
        .await;
        assert_eq!(
            restricted
                .security_context
                .as_ref()
                .unwrap()
                .run_as_non_root,
            Some(true)
        );
        assert!(restricted.containers[0].security_context.is_some());
        assert!(restricted.volumes.as_ref().is_some_and(|v| !v.is_empty()));

        // The explicit opt-out renders no pod security, like an absent setting.
        let unrestricted = render(Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Unrestricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: Vec::new(),
        }))
        .await;
        assert!(unrestricted.security_context.is_none());
        assert!(unrestricted.volumes.is_none());
        assert!(unrestricted.containers[0].security_context.is_none());
        assert!(unrestricted.containers[0].volume_mounts.is_none());
    }

    #[tokio::test]
    async fn deployment_mounts_existing_secret_as_read_only_files() {
        let mut config = manifest_test_container(&[], false);
        config.kubernetes_secret_mounts.push(KubernetesSecretMount {
            secret_name: "enrollment-token".to_string(),
            mount_path: "/var/run/enrollment".to_string(),
        });
        config.kubernetes_liveness_probe = Some(KubernetesHttpProbe {
            path: "/healthz".to_string(),
            port: 8080,
        });
        config.kubernetes_readiness_probe = Some(KubernetesHttpProbe {
            path: "/readyz".to_string(),
            port: 8080,
        });
        config.security = Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: Some(65532),
            run_as_group: Some(65532),
            read_only_root_filesystem: Some(true),
            writable_paths: Vec::new(),
        });
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let deployment = manifest_test_controller()
            .build_deployment(
                &config,
                "web",
                "test-ns",
                "web-sa",
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("deployment manifest");
        let pod = deployment
            .spec
            .expect("deployment spec")
            .template
            .spec
            .expect("pod spec");
        assert_eq!(pod.containers[0].name, "web");
        let volume = pod
            .volumes
            .expect("volumes")
            .into_iter()
            .next()
            .expect("Secret volume");
        assert_eq!(volume.name, "existing-secret-0");
        let source = volume.secret.expect("Secret source");
        assert_eq!(source.secret_name.as_deref(), Some("enrollment-token"));
        assert_eq!(source.optional, Some(false));
        let mount = pod.containers[0]
            .volume_mounts
            .as_ref()
            .expect("volume mounts")
            .first()
            .expect("Secret mount");
        assert_eq!(mount.name, "existing-secret-0");
        assert_eq!(mount.mount_path, "/var/run/enrollment");
        assert_eq!(mount.read_only, Some(true));
        let pod_security = pod.security_context.as_ref().expect("pod security");
        assert_eq!(pod_security.run_as_non_root, Some(true));
        assert_eq!(pod_security.run_as_user, Some(65532));
        assert_eq!(pod_security.run_as_group, Some(65532));
        assert_eq!(pod_security.fs_group, Some(65532));
        assert_eq!(
            pod_security.fs_group_change_policy.as_deref(),
            Some("OnRootMismatch")
        );
        assert_eq!(
            pod_security
                .seccomp_profile
                .as_ref()
                .map(|profile| profile.type_.as_str()),
            Some("RuntimeDefault")
        );
        let container_security = pod.containers[0]
            .security_context
            .as_ref()
            .expect("container security");
        assert_eq!(container_security.allow_privilege_escalation, Some(false));
        assert_eq!(container_security.read_only_root_filesystem, Some(true));
        assert_eq!(
            container_security
                .capabilities
                .as_ref()
                .and_then(|caps| caps.drop.as_ref()),
            Some(&vec!["ALL".to_string()])
        );
        let liveness = pod.containers[0]
            .liveness_probe
            .as_ref()
            .and_then(|probe| probe.http_get.as_ref())
            .expect("liveness HTTP probe");
        assert_eq!(liveness.path.as_deref(), Some("/healthz"));
        assert_eq!(liveness.port, IntOrString::Int(8080));
        let readiness = pod.containers[0]
            .readiness_probe
            .as_ref()
            .and_then(|probe| probe.http_get.as_ref())
            .expect("readiness HTTP probe");
        assert_eq!(readiness.path.as_deref(), Some("/readyz"));
        assert_eq!(readiness.port, IntOrString::Int(8080));
    }

    #[tokio::test]
    async fn deployment_manifest_projects_secrets_and_never_carries_alien_secrets() {
        let variables = vec![
            secret_env_var("APP_SECRET", "s3cret", None),
            secret_env_var(
                ENV_ALIEN_COMMANDS_TOKEN,
                "receiver-token",
                Some(vec!["web"]),
            ),
        ];
        // Simulate a config injected by an older manager that still collapsed
        // secrets: the pointer must be stripped from the manifest regardless.
        let config = manifest_test_container(
            &[
                ("APP_ENV", "prod"),
                (
                    ENV_ALIEN_SECRETS,
                    "{\"keys\":[\"APP_SECRET\"],\"hash\":\"legacy\"}",
                ),
                (ENV_ALIEN_RUNTIME_SECRETS, "vault://legacy-runtime"),
            ],
            false,
        );
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables.clone())
                .with_monitoring(OtlpConfig {
                    logs_endpoint: "https://manager.test/v1/logs".to_string(),
                    logs_auth_header: "authorization=Bearer logs-token".to_string(),
                    metrics_endpoint: Some("https://manager.test/v1/metrics".to_string()),
                    metrics_auth_header: Some("authorization=Bearer metrics-token".to_string()),
                    resource_attributes: HashMap::new(),
                });
        let monitoring_headers = direct_monitoring_auth_headers(&harness.ctx());
        let plan = environment_secret_plan_with_additional_secrets(
            "web",
            "web",
            &variables,
            &monitoring_headers,
        )
        .expect("checksum key")
        .expect("plan");
        let controller = manifest_test_controller();

        let deployment = controller
            .build_deployment(
                &config,
                "web",
                "test-ns",
                "web-sa",
                None,
                Some(&plan),
                &harness.ctx(),
            )
            .await
            .expect("deployment manifest");

        let env = deployment_env(&deployment);

        // App secret and the command receiver token are native projections.
        assert_secret_key_ref(&env, "APP_SECRET", "web-env");
        assert_secret_key_ref(&env, ENV_ALIEN_COMMANDS_TOKEN, "web-env");
        assert_secret_key_ref(&env, OTEL_EXPORTER_OTLP_HEADERS, "web-env");
        assert_secret_key_ref(&env, OTEL_EXPORTER_OTLP_METRICS_HEADERS, "web-env");

        // The runtime vault-load pointer never reaches the manifest.
        assert!(
            !env.iter().any(|var| matches!(
                var.name.as_str(),
                ENV_ALIEN_SECRETS | ENV_ALIEN_RUNTIME_SECRETS
            )),
            "vault pointers must not appear in a Kubernetes Container manifest"
        );

        // Plain vars still flow through.
        let app_env = env
            .iter()
            .find(|var| var.name == "APP_ENV")
            .expect("plain APP_ENV");
        assert_eq!(app_env.value.as_deref(), Some("prod"));

        // No worker-era runtime env leaks into Container manifests.
        for name in [
            ENV_ALIEN_TRANSPORT,
            ENV_ALIEN_WORKER_GRPC_ADDRESS,
            ENV_ALIEN_RUNTIME_SEND_OTLP,
            ENV_ALIEN_LAMBDA_MODE,
        ] {
            assert!(
                !env.iter().any(|var| var.name == name),
                "worker-era env var '{name}' must not appear in a Container manifest"
            );
        }

        // The pod template carries the checksum that rolls pods on rotation.
        assert_eq!(
            pod_template_checksum_annotation(&deployment.spec.expect("spec").template),
            Some(plan.checksum.clone())
        );
    }

    /// Service-link variables of the namespace's Services must not reach the container.
    #[tokio::test]
    async fn container_pods_do_not_get_service_link_variables() {
        for stateful in [false, true] {
            let config = manifest_test_container(&[], stateful);
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let controller = manifest_test_controller();
            let pod = if stateful {
                controller
                    .build_statefulset(
                        &config,
                        "web",
                        "test-ns",
                        "web-sa",
                        None,
                        None,
                        &harness.ctx(),
                    )
                    .await
                    .expect("statefulset manifest")
                    .spec
                    .expect("spec")
                    .template
                    .spec
                    .expect("pod spec")
            } else {
                controller
                    .build_deployment(
                        &config,
                        "web",
                        "test-ns",
                        "web-sa",
                        None,
                        None,
                        &harness.ctx(),
                    )
                    .await
                    .expect("deployment manifest")
                    .spec
                    .expect("spec")
                    .template
                    .spec
                    .expect("pod spec")
            };
            assert_eq!(pod.enable_service_links, Some(false), "stateful={stateful}");
        }
    }

    #[tokio::test]
    async fn statefulset_manifest_projects_secrets_and_stamps_checksum() {
        let variables = vec![secret_env_var("APP_SECRET", "s3cret", None)];
        let config = manifest_test_container(&[], true);
        let plan = environment_secret_plan("web", "web", &variables)
            .expect("checksum key")
            .expect("plan");
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
        let controller = manifest_test_controller();

        let statefulset = controller
            .build_statefulset(
                &config,
                "web",
                "test-ns",
                "web-sa",
                None,
                Some(&plan),
                &harness.ctx(),
            )
            .await
            .expect("statefulset manifest");

        let spec = statefulset.spec.expect("statefulset spec");
        let env = spec.template.spec.as_ref().expect("pod spec").containers[0]
            .env
            .clone()
            .expect("container env");
        assert_secret_key_ref(&env, "APP_SECRET", "web-env");
        assert!(!env.iter().any(|var| var.name == ENV_ALIEN_SECRETS));
        assert_eq!(
            pod_template_checksum_annotation(&spec.template),
            Some(plan.checksum.clone())
        );
    }

    #[tokio::test]
    async fn secret_rotation_changes_the_rendered_pod_template() {
        let config = manifest_test_container(&[], false);
        let controller = manifest_test_controller();

        let mut templates = Vec::new();
        for value in ["v1", "v1", "v2"] {
            let variables = vec![secret_env_var("APP_SECRET", value, None)];
            let plan = environment_secret_plan("web", "web", &variables)
                .expect("checksum key")
                .expect("plan");
            let harness =
                KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
            let deployment = controller
                .build_deployment(
                    &config,
                    "web",
                    "test-ns",
                    "web-sa",
                    None,
                    Some(&plan),
                    &harness.ctx(),
                )
                .await
                .expect("deployment manifest");
            templates.push(deployment.spec.expect("spec").template);
        }

        assert_eq!(
            templates[0], templates[1],
            "identical secret values must render an identical pod template"
        );
        assert_ne!(
            pod_template_checksum_annotation(&templates[0]),
            pod_template_checksum_annotation(&templates[2]),
            "rotating a secret value must change the pod template (rollout)"
        );
    }

    #[tokio::test]
    async fn deployer_secret_overwrite_changes_the_rendered_pod_template() {
        let controller = manifest_test_controller();

        let mut templates = Vec::new();
        for version in ["7", "7", "8"] {
            let deployer_secrets = serde_json::json!([{
                "name": "DATABASE_PASSWORD",
                "vaultKey": "database-password",
                "secretName": "stack-deployer-secrets-database-password",
                "label": "Database password",
                "required": true,
                "version": version,
            }])
            .to_string();
            let config = manifest_test_container(
                &[(ENV_ALIEN_DEPLOYER_SECRETS, deployer_secrets.as_str())],
                false,
            );
            let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let deployment = controller
                .build_deployment(
                    &config,
                    "web",
                    "test-ns",
                    "web-sa",
                    None,
                    None,
                    &harness.ctx(),
                )
                .await
                .expect("deployment manifest");
            let template = deployment.spec.expect("spec").template;
            let env = template.spec.as_ref().expect("pod spec").containers[0]
                .env
                .clone()
                .expect("container env");
            let reference = env
                .iter()
                .find(|var| var.name == "DATABASE_PASSWORD")
                .and_then(|var| var.value_from.as_ref()?.secret_key_ref.as_ref())
                .expect("DATABASE_PASSWORD is a secretKeyRef");
            assert_eq!(reference.name, "stack-deployer-secrets-database-password");
            templates.push(template);
        }

        let sorted_env = |template: &PodTemplateSpec| {
            let mut env = template.spec.as_ref().expect("pod spec").containers[0]
                .env
                .clone()
                .expect("container env");
            env.sort_by(|left, right| left.name.cmp(&right.name));
            env
        };
        assert_eq!(
            templates[0].metadata, templates[1].metadata,
            "the same secret version must render the same pod template metadata"
        );
        assert_eq!(
            sorted_env(&templates[0]),
            sorted_env(&templates[2]),
            "the containers only reference the Secret, so their env cannot change"
        );
        assert_ne!(
            templates[0].metadata, templates[2].metadata,
            "a new secret version must change the pod template (rollout)"
        );
    }

    /// A Helm upgrade that remaps a level changes the pod spec without touching the stack,
    /// so the controller compares the class it applied with the one now mapped.
    #[test]
    fn needs_update_detects_a_changed_priority_class_mapping() {
        unsafe {
            std::env::set_var(
                alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES,
                r#"{"critical":"vendor-core","low":"vendor-batch"}"#,
            )
        };
        let mut config = manifest_test_container(&[], false);
        config.priority = Some(alien_core::WorkloadPriority::Critical);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);

        let mut controller = manifest_test_controller();
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));
        controller.applied_priority_class = Some("old-class".to_string());
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));
        controller.applied_priority_class = Some("vendor-core".to_string());
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));
    }

    #[test]
    fn needs_update_follows_the_built_architecture_a_release_records() {
        use alien_core::instance_catalog::Architecture::{Arm64, X86_64};
        let config = manifest_test_container(&[], false);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let mut controller = manifest_test_controller();
        controller.applied_priority_class = None;
        let mut stack = harness.ctx().desired_stack.clone();
        // The stack also pins a node selector of its own (the pool architecture): it must
        // not make a workload with no record, or a multi-architecture one, roll.
        crate::core::kubernetes_manifest_test_support::add_x86_64_pool(&mut stack);
        let id = config.id.clone();
        let wants_update = |controller: &KubernetesContainerController,
                            stack: &alien_core::Stack| {
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
        let config = manifest_test_container(&[], false);
        let original = vec![secret_env_var("APP_SECRET", "v1", None)];
        let original_plan = environment_secret_plan("web", "web", &original)
            .expect("checksum key")
            .expect("plan");

        let mut controller = manifest_test_controller();
        controller.env_secret.record(Some(&original_plan));

        // Same snapshot: no drift.
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(config.clone()), original.clone());
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        // Rotated value: drift → update.
        let rotated = vec![secret_env_var("APP_SECRET", "v2", None)];
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), rotated);
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        // All secrets removed: drift → update (Secret refs must go away).
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        // Never had secrets, still none: no drift.
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
        let config = Container::new("web".into())
            .code(ContainerCode::Image {
                image: "nginx:latest".into(),
            })
            .permissions("web".into())
            .cpu(alien_core::ResourceSpec {
                min: "0.5".into(),
                desired: "0.5".into(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".into(),
                desired: "512Mi".into(),
            })
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
                KubernetesContainerState::ReconcilePublicEndpoint,
                ResourceStatus::Provisioning,
            ),
            (
                KubernetesContainerState::ReconcilePublicEndpointAfterUpdate,
                ResourceStatus::Updating,
            ),
        ] {
            let controller = KubernetesContainerController {
                state,
                workload_name: Some("web".into()),
                namespace: Some("test-ns".into()),
                ..Default::default()
            };
            check_certificate_wait(&mut harness, controller, "web", status, |c, at| {
                c.public_endpoint.wait_started_unix.insert(
                    crate::kubernetes_public_endpoint::EndpointWait::Certificate,
                    at,
                );
            })
            .await;
        }
    }

    /// A refresh-failed container is retried by applying its current spec again: running the failed
    /// readiness check again reads the same pods and fails the same way.
    #[test]
    fn a_retry_of_a_refresh_failed_container_applies_the_spec_again() {
        use crate::core::state_utils::StackResourceStateExt;
        let mut record = alien_core::StackResourceState::new_pending(
            Container::RESOURCE_TYPE.to_string(),
            alien_core::Resource::new(manifest_test_container(&[], false)),
            Some(alien_core::ResourceLifecycle::Live),
            vec![],
        );
        let mut controller = manifest_test_controller();
        controller.state = KubernetesContainerState::RefreshFailed;
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
            .downcast_ref::<KubernetesContainerController>()
            .unwrap();
        assert_eq!(resumed.state, KubernetesContainerState::UpdateStart);
    }

    /// Real drift (an edited spec, a missing object) is not a failed readiness check: a retry
    /// does not apply the spec over it, it resumes the check, which reports it again.
    #[test]
    fn a_retry_of_a_drift_refresh_failure_does_not_apply_the_spec() {
        use crate::core::state_utils::StackResourceStateExt;
        let mut record = alien_core::StackResourceState::new_pending(
            Container::RESOURCE_TYPE.to_string(),
            alien_core::Resource::new(manifest_test_container(&[], false)),
            Some(alien_core::ResourceLifecycle::Live),
            vec![],
        );
        let mut controller = manifest_test_controller();
        controller.state = KubernetesContainerState::RefreshFailed;
        record.status = ResourceStatus::RefreshFailed;
        record.error = Some(crate::core::state_utils::saved_refresh_error("x", false));
        record
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();

        assert!(record.retry_failed().unwrap());

        let resumed = record.get_internal_controller().unwrap().unwrap();
        let resumed = resumed
            .as_any()
            .downcast_ref::<KubernetesContainerController>()
            .unwrap();
        assert_eq!(resumed.state, KubernetesContainerState::RefreshFailed);
    }
}

#[cfg(test)]
#[path = "kubernetes_update_tests.rs"]
mod update_tests;
