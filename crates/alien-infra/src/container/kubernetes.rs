use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_runtime_pod_labels,
    reconcile_environment_secret, EnvironmentVariableBuilder, KubernetesEnvSecretPlan,
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
    branded_tag_key, kubernetes_resource_name, kubernetes_service_account_name, public_url_host,
    Container, ContainerCode, ContainerOutputs, ContainerStatus, PublicEndpointOutput,
    ResourceOutputs, ResourceStatus, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE,
    DEFAULT_ALIEN_LABEL_DOMAIN, ENV_ALIEN_SECRETS,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    Container as K8sContainer, ContainerPort, EnvVar, LocalObjectReference, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, PodSpec, PodTemplateSpec, ResourceRequirements, Service,
    ServicePort, ServiceSpec, Volume, VolumeMount,
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

async fn create_registry_pull_secret(
    secrets_client: &std::sync::Arc<dyn alien_k8s_clients::SecretsApi>,
    namespace: &str,
    secret_name: &str,
    proxy_host: &str,
    deployment_token: &str,
) -> Result<()> {
    crate::kubernetes_registry::ensure_registry_pull_secret(
        secrets_client,
        namespace,
        secret_name,
        proxy_host,
        deployment_token,
    )
    .await
}

fn first_declared_container_port(config: &Container) -> Option<u16> {
    config.ports.first().map(|port| port.port)
}

fn kubernetes_port_name(port: &alien_core::ContainerPort) -> String {
    if false {
        "http".to_string()
    } else {
        format!("tcp-{}", port.port)
    }
}

fn is_already_exists(error: &alien_client_core::Error) -> bool {
    let text = error.to_string();
    text.contains("AlreadyExists") || text.contains("409")
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
    /// The first declared service port for binding construction.
    pub(crate) service_port: Option<u16>,
    /// The container ID (for binding construction)
    pub(crate) container_id: Option<String>,
    /// Public endpoint route/certificate state.
    pub(crate) public_endpoint: KubernetesPublicEndpointState,
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

        // Store data needed for binding construction
        self.container_id = Some(config.id.clone());
        self.service_name = Some(container_name.clone());
        self.service_port = first_declared_container_port(config);
        self.namespace = Some(namespace.clone());
        // Generate ServiceAccount name following Helm naming convention
        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        let image_pull_secret_name = if let ContainerCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                })
            })?;
            let secret_name = format!("{}-registry", container_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, &namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };
        let env_secret_plan = self
            .reconcile_environment_secret(config, &container_name, &namespace, ctx)
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
                Err(err) if is_already_exists(&err) => {
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
                Err(err) if is_already_exists(&err) => {
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
        let (ready_replicas, replicas) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
                    if let Some(status) = &statefulset.status {
                        (status.ready_replicas, Some(status.replicas))
                    } else {
                        (None, None)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "StatefulSet not yet available, continuing to wait");
                    (None, None)
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
                        (status.ready_replicas, status.replicas)
                    } else {
                        (None, None)
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    debug!(workload_name=%workload_name, "Deployment not yet available, continuing to wait");
                    (None, None)
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
        if let (Some(ready_replicas), Some(replicas)) = (ready_replicas, replicas) {
            let desired_replicas = config.replicas.unwrap_or(1) as i32;
            if ready_replicas >= desired_replicas.min(replicas) && replicas > 0 {
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
            } else {
                debug!(workload_name=%workload_name, ready=%ready_replicas, total=%replicas, "Container workload not yet ready");
            }
        }

        Ok(HandlerAction::Stay {
            max_times: KUBERNETES_WORKLOAD_READY_MAX_POLLS,
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
                max_times: 60,
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
        if let (Some(workload_name), Some(namespace)) = (&self.workload_name, &self.namespace) {
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
                    max_times: 60,
                    suggested_delay: Some(suggested_delay),
                });
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

        let workload_type = if self.is_stateful {
            "StatefulSet"
        } else {
            "Deployment"
        };
        info!(workload_name=%workload_name, workload_type=%workload_type, "Updating Kubernetes Container workload");

        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        let image_pull_secret_name = if let ContainerCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                })
            })?;
            let secret_name = format!("{}-registry", workload_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };
        let env_secret_plan = self
            .reconcile_environment_secret(config, workload_name, namespace, ctx)
            .await?;
        self.service_port = first_declared_container_port(config);
        self.reconcile_internal_service(config, workload_name, namespace, ctx)
            .await?;
        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        if self.is_stateful {
            // Get existing StatefulSet to carry over resourceVersion
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

            let resource_version = existing.metadata.resource_version.clone();
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

            deployment_client
                .update_statefulset(namespace, workload_name, &new_statefulset)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update statefulset '{}'.", workload_name),
                    resource_id: Some(config.id.clone()),
                })?;
        } else {
            // Get existing Deployment to carry over resourceVersion
            let existing = deployment_client
                .get_deployment(namespace, workload_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get deployment '{}' before update", workload_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let resource_version = existing.metadata.resource_version.clone();
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

            deployment_client
                .update_deployment(namespace, workload_name, &new_deployment)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update deployment '{}'.", workload_name),
                    resource_id: Some(config.id.clone()),
                })?;
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

        let (ready_replicas, replicas) = if self.is_stateful {
            match deployment_client
                .get_statefulset(namespace, workload_name)
                .await
            {
                Ok(statefulset) => {
                    if let Some(status) = &statefulset.status {
                        (status.ready_replicas, Some(status.replicas))
                    } else {
                        (None, None)
                    }
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
                    if let Some(status) = &deployment.status {
                        (status.ready_replicas, status.replicas)
                    } else {
                        (None, None)
                    }
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

        if let (Some(ready_replicas), Some(replicas)) = (ready_replicas, replicas) {
            let desired_replicas = config.replicas.unwrap_or(1) as i32;
            if ready_replicas >= desired_replicas.min(replicas) && replicas > 0 {
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
            } else {
                debug!(workload_name=%workload_name, ready=%ready_replicas, total=%replicas, "Container workload rollout in progress");
            }
        }

        Ok(HandlerAction::Stay {
            max_times: KUBERNETES_WORKLOAD_READY_MAX_POLLS,
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
                max_times: 60,
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

        delete_kubernetes_public_endpoint(ctx, &config.id, namespace, &mut self.public_endpoint)
            .await?;
        if let Some(service_name) = &self.service_name {
            self.delete_internal_service(namespace, service_name, ctx)
                .await?;
        }

        // Delete Deployment or StatefulSet
        if let Some(workload_name) = &self.workload_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

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
            max_times: 60, // 60 attempts * 5 seconds = 5 minutes max wait for deletion
            suggested_delay: Some(Duration::from_secs(5)),
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
                internal_dns: format!("{}.svc.cluster.local", workload_name),
                replicas: Vec::new(), // Replica details tracked separately
                public_endpoints: self
                    .public_endpoint
                    .effective_public_url()
                    .map(|url| {
                        HashMap::from([(
                            "default".to_string(),
                            PublicEndpointOutput {
                                host: public_url_host(&url).unwrap_or_default(),
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

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::{BindingValue, KubernetesContainerBinding};

        // Construct binding on-the-fly from stored fields (like other controllers)
        if let (Some(container_id), Some(service_name), Some(namespace)) =
            (&self.container_id, &self.service_name, &self.namespace)
        {
            let binding = KubernetesContainerBinding {
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
        KubernetesContainerController, KubernetesContainerState, KubernetesPublicEndpointState,
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
            service_port: Some(3000),
            container_id: Some("container".to_string()),
            public_endpoint,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let container_outputs = outputs
            .downcast_ref::<ContainerOutputs>()
            .expect("container outputs");

        assert_eq!(
            container_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.url.as_str()),
            Some("https://container.example.test")
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
            service_port: Some(3000),
            container_id: Some("container".to_string()),
            public_endpoint,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let container_outputs = outputs
            .downcast_ref::<ContainerOutputs>()
            .expect("container outputs");

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
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(container_name: &str, namespace: &str, is_stateful: bool) -> Self {
        Self {
            state: KubernetesContainerState::Ready,
            workload_name: Some(container_name.to_string()),
            is_stateful,
            namespace: Some(namespace.to_string()),
            service_name: Some(container_name.to_string()),
            service_port: Some(80),
            container_id: Some("test-container".to_string()),
            public_endpoint: KubernetesPublicEndpointState::default(),
            _internal_stay_count: None,
        }
    }

    async fn reconcile_environment_secret(
        &self,
        config: &Container,
        workload_name: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<KubernetesEnvSecretPlan>> {
        reconcile_environment_secret("container", &config.id, workload_name, namespace, ctx).await
    }

    async fn reconcile_internal_service(
        &self,
        config: &Container,
        service_name: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let service_client = ctx
            .service_provider
            .get_kubernetes_service_client(kubernetes_config)
            .await?;

        let Some(mut service) = self.build_internal_service(config, service_name, namespace) else {
            self.delete_internal_service(namespace, service_name, ctx)
                .await?;
            return Ok(());
        };

        match service_client.create_service(namespace, &service).await {
            Ok(_) => Ok(()),
            Err(e) if is_already_exists(&e) => {
                let existing = service_client
                    .get_service(namespace, service_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get internal Service '{}' before update",
                            service_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                service.metadata.resource_version = existing.metadata.resource_version;
                service_client
                    .update_service(namespace, service_name, &service)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to update internal Service '{}'", service_name),
                        resource_id: Some(config.id.clone()),
                    })?;
                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create internal Service '{}'", service_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    async fn delete_internal_service(
        &self,
        namespace: &str,
        service_name: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let service_client = ctx
            .service_provider
            .get_kubernetes_service_client(kubernetes_config)
            .await?;

        match service_client.delete_service(namespace, service_name).await {
            Ok(()) => Ok(()),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete internal Service '{}'", service_name),
                resource_id: Some(service_name.to_string()),
            })),
        }
    }

    fn build_internal_service(
        &self,
        config: &Container,
        service_name: &str,
        namespace: &str,
    ) -> Option<Service> {
        if config.ports.is_empty() {
            return None;
        }

        let labels = self.build_labels(service_name);
        Some(Service {
            metadata: ObjectMeta {
                name: Some(service_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("ClusterIP".to_string()),
                selector: Some(labels),
                ports: Some(
                    config
                        .ports
                        .iter()
                        .map(|port| ServicePort {
                            name: Some(kubernetes_port_name(port)),
                            port: port.port as i32,
                            protocol: Some("TCP".to_string()),
                            target_port: Some(IntOrString::Int(port.port as i32)),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        })
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
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
        let pod_annotations = env_secret_plan.map(|plan| {
            BTreeMap::from([("env-secret-checksum".to_string(), plan.checksum.clone())])
        });

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
                service_account_name,
                image_pull_secret_name,
                env_secret_plan,
                ctx,
            )
            .await?;
        let pod_annotations = env_secret_plan.map(|plan| {
            BTreeMap::from([("env-secret-checksum".to_string(), plan.checksum.clone())])
        });

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
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<PodSpec> {
        // Determine the container image
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "Source-based containers not yet supported in Kubernetes platform"
                        .to_string(),
                }));
            }
        };

        // Build environment variables
        // IMPORTANT: Start with config.environment which includes injected vars from DeploymentConfig
        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_container_runtime_env_vars(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .add_self_container_binding(&config.id, self.get_binding_params()?.as_ref())?;

        let (env_map, bindings) = env_builder.build_with_bindings();

        let mut env_vars = Vec::new();

        if let Some(plan) = env_secret_plan {
            for key in &plan.keys {
                env_vars.push(EnvVar {
                    name: key.clone(),
                    value: None,
                    value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                        secret_key_ref: Some(k8s_openapi::api::core::v1::SecretKeySelector {
                            name: plan.secret_name.clone(),
                            key: key.clone(),
                            optional: Some(false),
                        }),
                        ..Default::default()
                    }),
                });
            }
        }

        // Process bindings for Kubernetes SecretRefs
        for (binding_name, binding_json) in bindings {
            if let Ok(extraction) = crate::core::k8s_secret_bindings::extract_binding_secrets(
                &binding_name,
                &binding_json,
            ) {
                // Add individual secret env vars with valueFrom.secretKeyRef
                for (env_name, secret_name, secret_key) in extraction.secret_env_vars {
                    env_vars.push(EnvVar {
                        name: env_name,
                        value: None,
                        value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                            secret_key_ref: Some(k8s_openapi::api::core::v1::SecretKeySelector {
                                name: secret_name,
                                key: secret_key,
                                optional: Some(false),
                            }),
                            ..Default::default()
                        }),
                    });
                }

                // Add the binding JSON with $(VAR) placeholders
                let env_key = format!(
                    "ALIEN_{}_BINDING",
                    binding_name.to_uppercase().replace('-', "_")
                );
                env_vars.push(EnvVar {
                    name: env_key,
                    value: Some(extraction.resolved_binding_json),
                    value_from: None,
                });
            }
        }

        // Add all remaining env vars from the builder (includes user vars + injected vars)
        for (key, value) in env_map {
            if key == ENV_ALIEN_SECRETS && env_secret_plan.is_some() {
                continue;
            }
            // Skip if already added as a secret ref
            if !env_vars.iter().any(|ev| ev.name == key) {
                env_vars.push(EnvVar {
                    name: key,
                    value: Some(value),
                    value_from: None,
                });
            }
        }

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

        // Parse CPU and memory from ResourceSpec
        let cpu_request = config.cpu.min.clone();
        let cpu_limit = config.cpu.desired.clone();
        let memory_request = config.memory.min.clone();
        let memory_limit = config.memory.desired.clone();

        let container = K8sContainer {
            name: "container".to_string(),
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

        let pod_spec = PodSpec {
            service_account_name: Some(service_account_name.to_string()),
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
    fn build_labels(&self, container_name: &str) -> BTreeMap<String, String> {
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
    ) -> bool {
        let label_domain = ctx
            .deployment_config
            .label_domain
            .as_deref()
            .unwrap_or(DEFAULT_ALIEN_LABEL_DOMAIN);
        let managed_by_key = branded_tag_key(label_domain, ALIEN_MANAGED_BY_TAG_KEY);
        let default_managed_by_key =
            branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY);
        labels.is_some_and(|labels| {
            labels.get(&managed_by_key).map(String::as_str) == Some(ALIEN_MANAGED_BY_TAG_VALUE)
                || labels.get(&default_managed_by_key).map(String::as_str)
                    == Some(ALIEN_MANAGED_BY_TAG_VALUE)
                || (labels.get("managed-by").map(String::as_str) == Some("runtime")
                    && labels.get("component").map(String::as_str) == Some("container")
                    && labels.get("app").map(String::as_str) == Some(container_name))
        })
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
            service_port: Some(3000),
            container_id: Some("api".to_string()),
            public_endpoint: KubernetesPublicEndpointState::default(),
            _internal_stay_count: None,
        };

        let service = controller
            .build_internal_service(&config, "api", "test-ns")
            .expect("internal service");
        let spec = service.spec.expect("service spec");

        assert_eq!(service.metadata.name.as_deref(), Some("api"));
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));
        let ports = spec.ports.expect("service ports");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 3000);
        assert_eq!(ports[0].target_port, Some(IntOrString::Int(3000)));
    }
}
