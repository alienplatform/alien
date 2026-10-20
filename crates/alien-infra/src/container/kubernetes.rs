use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use tracing::{debug, info};

use crate::core::kubernetes_errors::is_remote_resource_conflict;
use crate::core::{
    delete_environment_secret, direct_monitoring_auth_headers, kubernetes_branded_resource_labels,
    kubernetes_cleanup_resource_labels, kubernetes_runtime_pod_labels, linked_bindings,
    pod_template_annotations,
    projected_env_vars, reconcile_environment_secret_with_additional_secrets,
    EnvSecretRotationTracker, EnvironmentVariableBuilder, KubernetesEnvSecretPlan,
    ResourceController, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_public_endpoint::{
    container_public_endpoint_target, delete_kubernetes_public_endpoint,
    reconcile_kubernetes_public_endpoint, KubernetesEndpointAction, KubernetesPublicEndpointState,
};
use crate::kubernetes_workload_heartbeat::{
    emit_kubernetes_workload_heartbeat, label_selector, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput,
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
const KUBERNETES_WORKLOAD_READY_MAX_POLLS: u32 = 360; // 360 * 5s = 30 minutes

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
    matches!((ready_replicas, replicas), (Some(ready), Some(total)) if total > 0 && ready >= desired_replicas.min(total))
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

/// The container's image when the deployment token must become pull
/// credentials for it. With a token (every connected deployment), that is
/// every image. Without one (an air-gapped deployment), images from another
/// registry, such as the environment's own, are pulled with the access the
/// cluster already has; an image still pointing at the manager is returned
/// so the missing token fails loudly.
fn image_needing_pull_secret<'a>(
    code: &'a ContainerCode,
    has_deployment_token: bool,
    manager_url: Option<&str>,
) -> Option<&'a str> {
    let ContainerCode::Image { image } = code else {
        return None;
    };
    if has_deployment_token {
        return Some(image);
    }
    let Some(manager_url) = manager_url else {
        return Some(image);
    };
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

fn first_declared_container_port(config: &Container) -> Option<u16> {
    config.ports.first().map(|port| port.port)
}

fn deployment_rollout_complete(
    deployment: &Deployment,
    desired_replicas: i32,
    expected_generation: Option<i64>,
) -> bool {
    let (Some(generation), Some(status)) = (deployment.metadata.generation, &deployment.status)
    else {
        return false;
    };

    // Ready replicas can still belong to the old ReplicaSet after an update.
    Some(generation) == expected_generation
        && status
            .observed_generation
            .is_some_and(|observed| observed >= generation)
        // Kubernetes omits replica counters when a Deployment is scaled to
        // zero. An observed generation is enough to complete that rollout.
        && status.updated_replicas.unwrap_or(0) == desired_replicas
        && status.replicas.unwrap_or(0) == desired_replicas
        && status.available_replicas.unwrap_or(0) >= desired_replicas
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
}

#[controller]
impl KubernetesContainerController {
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
        let image_pull_secret_name = if let Some(image) = image_needing_pull_secret(
            &config.code,
            ctx.deployment_config.deployment_token.is_some(),
            ctx.deployment_config.manager_url.as_deref(),
        ) {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                })
            })?;
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
                token,
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
        self.env_secret.record(env_secret_plan.as_ref());
        self.reconcile_network_policy(config, &container_name, &namespace, ctx)
            .await?;
        self.reconcile_internal_service(config, &container_name, &namespace, ctx)
            .await?;

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
    )]
    async fn waiting_for_workload(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
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

        // Check workload status (different API for Deployment vs StatefulSet)
        let (ready_replicas, replicas, observed_generation, generation) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
                    if let Some(status) = &statefulset.status {
                        (
                            status.ready_replicas,
                            Some(status.replicas),
                            status.observed_generation,
                            statefulset.metadata.generation,
                        )
                    } else {
                        (None, None, None, None)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "StatefulSet not yet available, continuing to wait");
                    (None, None, None, None)
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
                    if let Some(status) = &deployment.status {
                        (
                            status.ready_replicas,
                            status.replicas,
                            status.observed_generation,
                            deployment.metadata.generation,
                        )
                    } else {
                        (None, None, None, None)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "Deployment not yet available, continuing to wait");
                    (None, None, None, None)
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get deployment '{}'.", workload_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        };

        // Check if ready
        if workload_ready(
            config.replicas.unwrap_or(1) as i32,
            ready_replicas,
            replicas,
            observed_generation,
            generation,
        ) {
            let workload_type = if self.is_stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            info!(workload_name=%workload_name, namespace=%namespace, workload_type=%workload_type, "Container workload is ready");

            return Ok(HandlerAction::Continue {
                state: ReconcilePublicEndpoint,
                suggested_delay: None,
            });
        }
        debug!(workload_name=%workload_name, ready=?ready_replicas, total=?replicas, "Container workload not yet ready");

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
                max_times: Some(60),
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
            // Gateway configuration is read from the vault, so its changes are detected
            // here rather than by the synchronous deployment-snapshot check.
            if config.id == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
                let secrets = self.additional_environment_secrets(config, ctx).await?;
                if self.env_secret.drifted_with_additional_secrets(
                    &config.id,
                    workload_name,
                    &ctx.deployment_config.environment_variables.variables,
                    &secrets,
                ) {
                    return Ok(HandlerAction::Continue {
                        state: UpdateStart,
                        suggested_delay: None,
                    });
                }
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

            if let (Some(ready_replicas), Some(replicas)) = (ready_replicas, replicas) {
                if ready_replicas < replicas {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Container workload has {} ready replicas out of {} total",
                            ready_replicas, replicas
                        ),
                    }));
                }
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
                    max_times: Some(60),
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
        let image_pull_secret_name = if let Some(image) = image_needing_pull_secret(
            &config.code,
            ctx.deployment_config.deployment_token.is_some(),
            ctx.deployment_config.manager_url.as_deref(),
        ) {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                })
            })?;
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
                token,
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
        self.env_secret.record(env_secret_plan.as_ref());
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
    )]
    async fn waiting_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
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
        let (ready_replicas, replicas, rollout_complete) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
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

        if rollout_complete {
            let workload_type = if self.is_stateful {
                "StatefulSet"
            } else {
                "Deployment"
            };
            info!(workload_name=%workload_name, workload_type=%workload_type, "Container workload rollout complete");
            return Ok(HandlerAction::Continue {
                state: ReconcilePublicEndpointAfterUpdate,
                suggested_delay: None,
            });
        }
        debug!(workload_name=%workload_name, ready=?ready_replicas, total=?replicas, desired=desired_replicas, expected_generation=?self.update_generation, "Container workload rollout in progress");

        Ok(HandlerAction::Stay {
            max_times: Some(KUBERNETES_WORKLOAD_READY_MAX_POLLS),
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
                max_times: Some(60),
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

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

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

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let Some(workload_name) = &self.workload_name {
            Some(ResourceOutputs::new(ContainerOutputs {
                name: workload_name.clone(),
                status: ContainerStatus::Running,
                current_replicas: 0, // Will be updated by runtime
                desired_replicas: 0, // Will be updated by runtime
                // The internal Service shares the workload's name.
                internal_dns: match &self.namespace {
                    Some(namespace) => format!("{workload_name}.{namespace}.svc.cluster.local"),
                    None => format!("{workload_name}.svc.cluster.local"),
                },
                internal_urls: self.internal_urls(),
                replicas: Vec::new(), // Replica details tracked separately
                // Kubernetes leaves volume snapshots to the cluster's own tooling. With no
                // volumes reported, there is nothing to restore.
                volumes: Vec::new(),
                volume_backups: None,
                public_endpoints: self
                    .public_endpoint
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
                    .unwrap_or_default(),
            }))
        } else {
            None
        }
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
        let monitoring_headers = direct_monitoring_auth_headers(ctx);
        Ok(self.env_secret.drifted_with_additional_secrets(
            &config.id,
            workload_name,
            &ctx.deployment_config.environment_variables.variables,
            &monitoring_headers,
        ))
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
        let pod_spec = self
            .build_pod_spec(
                config,
                namespace,
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
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
        let pod_spec = self
            .build_pod_spec(
                config,
                namespace,
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
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
            ContainerCode::Image { image } => image.clone(),
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
        let security = config.security.as_ref();
        if security.is_some_and(|settings| settings.run_as_user <= 0 || settings.run_as_group <= 0)
        {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Restricted container security needs positive user and group IDs"
                    .to_string(),
            }));
        }

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
            security_context: security.map(|settings| SecurityContext {
                allow_privilege_escalation: Some(false),
                read_only_root_filesystem: Some(settings.read_only_root_filesystem),
                capabilities: Some(Capabilities {
                    drop: Some(vec!["ALL".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
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

        let pool = alien_core::kubernetes_container_pool(ctx.desired_stack, config).map_err(
            |message| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message,
                })
            },
        )?;
        let node_selector = alien_core::kubernetes_compute_node_selector(ctx.desired_stack, pool)
            .map_err(|message| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message,
            })
        })?;
        let pod_spec = PodSpec {
            node_selector,
            service_account_name: Some(service_account_name.to_string()),
            // The gateway never calls the Kubernetes API; its configuration is projected.
            automount_service_account_token: gateway.then_some(false),
            security_context: security.map(|settings| PodSecurityContext {
                run_as_non_root: Some(true),
                run_as_user: Some(settings.run_as_user),
                run_as_group: Some(settings.run_as_group),
                fs_group: Some(settings.run_as_group),
                seccomp_profile: Some(SeccompProfile {
                    type_: "RuntimeDefault".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            containers: vec![container],
            image_pull_secrets: image_pull_secret_name.map(|name| {
                vec![LocalObjectReference {
                    name: name.to_string(),
                }]
            }),
            volumes: if volumes.is_empty() {
                None
            } else {
                Some(volumes)
            },
            restart_policy: Some("Always".to_string()),
            ..Default::default()
        };

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
    use super::*;
    use k8s_openapi::api::apps::v1::{DeploymentStatus, StatefulSetStatus};

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
    fn zero_replica_workload_waits_for_the_controller_to_observe_its_generation() {
        assert!(!workload_ready(0, None, None, None, Some(1)));
        assert!(!workload_ready(0, None, None, Some(1), Some(2)));
        assert!(!workload_ready(0, Some(1), Some(1), Some(2), Some(2)));
        assert!(workload_ready(0, None, None, Some(2), Some(2)));
        assert!(workload_ready(1, Some(1), Some(1), Some(2), Some(2)));
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
        assert_secret_key_ref, deployment_env, pod_template_checksum_annotation, secret_env_var,
        KubernetesManifestTestHarness,
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

    /// A gateway on an image that reads mounted files gets the vault's configuration Secret and
    /// replica-secret Secret mounted by the kubelet, and the controller reads neither: it only
    /// creates the replica secret before the pods need it. An earlier image still gets the
    /// configuration, read by the controller, in its environment.
    #[tokio::test]
    async fn a_mounting_gateway_gets_its_secrets_from_the_kubelet_not_the_controller() {
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
        assert_eq!(replica_secret.hits_async().await, 1);
        assert_eq!(reads.hits_async().await, 0);

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
        assert!(pod.volumes.is_none());
        let error = controller
            .additional_environment_secrets(&earlier, &ctx)
            .await
            .expect_err("the earlier image needs the controller to read the configuration");
        assert!(error.message.contains("not ready"), "{}", error.message);
        assert!(reads.hits_async().await >= 1);
    }

    #[tokio::test]
    async fn gateway_replicas_wait_out_a_resynchronization_and_get_no_api_token() {
        let mut gateway = manifest_test_container(&[], false);
        gateway.id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string();
        let mut ordinary = manifest_test_container(&[], false);
        for config in [&mut gateway, &mut ordinary] {
            config.kubernetes_readiness_probe = Some(KubernetesHttpProbe {
                path: "/health".to_string(),
                port: 8080,
            });
        }
        for (config, delay, automount) in [
            (&gateway, Some(GATEWAY_READY_DELAY_SECONDS), Some(false)),
            (&ordinary, None, None),
        ] {
            let harness =
                KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
            let state = secrets_vault_state();
            let mut ctx = harness.ctx();
            ctx.state = &state;
            let pod = manifest_test_controller()
                .build_deployment(config, "web", "test-ns", "web-sa", None, None, &ctx)
                .await
                .expect("deployment manifest")
                .spec
                .expect("deployment spec")
                .template
                .spec
                .expect("pod spec");
            assert_eq!(pod.automount_service_account_token, automount, "{}", config.id);
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
            run_as_user: 65532,
            run_as_group: 65532,
            read_only_root_filesystem: true,
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

    #[tokio::test]
    async fn statefulset_manifest_projects_secrets_and_stamps_checksum() {
        let variables = vec![secret_env_var("APP_SECRET", "s3cret", None)];
        let config = manifest_test_container(&[], true);
        let plan = environment_secret_plan("web", "web", &variables).expect("plan");
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
            let plan = environment_secret_plan("web", "web", &variables).expect("plan");
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

    #[test]
    fn needs_update_detects_env_secret_rotation() {
        let config = manifest_test_container(&[], false);
        let original = vec![secret_env_var("APP_SECRET", "v1", None)];
        let original_plan = environment_secret_plan("web", "web", &original).expect("plan");

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
}
