use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{
    kubernetes_runtime_pod_labels, reconcile_environment_secret, EnvironmentVariableBuilder,
    KubernetesEnvSecretPlan, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_client::{create, delete, get, replace};
use crate::kubernetes_public_endpoint::{
    delete_kubernetes_public_endpoint, reconcile_kubernetes_public_endpoint,
    worker_public_endpoint_target, KubernetesEndpointAction, KubernetesPublicEndpointState,
};
use crate::kubernetes_workload_heartbeat::{
    emit_kubernetes_workload_heartbeat, label_selector, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_resource_name, kubernetes_service_account_name, ResourceOutputs, ResourceStatus,
    Worker, WorkerCode, WorkerOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, LocalObjectReference, PodSpec, PodTemplateSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

#[controller]
pub struct KubernetesWorkerController {
    /// The name of the created Kubernetes Deployment.
    pub(crate) deployment_name: Option<String>,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
    /// The service name for the worker (for binding construction)
    pub(crate) service_name: Option<String>,
    /// The worker ID (for binding construction)
    pub(crate) worker_id: Option<String>,
    /// Public endpoint route/certificate state.
    pub(crate) public_endpoint: KubernetesPublicEndpointState,
}

#[controller]
impl KubernetesWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Worker>()?;

        info!(id=%config.id, "Initiating Kubernetes Worker creation");

        let function_name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;

        // Store data needed for binding construction
        self.worker_id = Some(config.id.clone());
        self.service_name = Some(function_name.clone());
        self.namespace = Some(namespace.clone());
        // Generate ServiceAccount name following Helm naming convention
        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());

        // Create a Docker config Secret so kubelet can authenticate when pulling
        // from the manager's registry. Required for image-based deployments.
        let image_pull_secret_name = if let WorkerCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let secret_name = format!("{}-registry", function_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, &namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };
        let env_secret_plan =
            reconcile_environment_secret("worker", &config.id, &function_name, &namespace, ctx)
                .await?;

        // Create the Deployment
        let deployment_client = ctx
            .service_provider
            .get_kubernetes_client(kubernetes_config)
            .await?;
        let deployment = self
            .build_deployment(
                config,
                &function_name,
                &namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;

        let _created_deployment = create(
            kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), &namespace),
            &deployment,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create deployment '{}'.", function_name),
            resource_id: Some(config.id.clone()),
        })?;

        self.deployment_name = Some(function_name.clone());
        self.namespace = Some(namespace.clone());

        info!(deployment_name=%function_name, namespace=%namespace, "Deployment creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForDeployment,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDeployment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_deployment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Worker>()?;

        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
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
            .get_kubernetes_client(kubernetes_config)
            .await?;

        match get(
            kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
            deployment_name,
        )
        .await
        {
            Ok(deployment) => {
                if let Some(status) = &deployment.status {
                    if let (Some(ready_replicas), Some(replicas)) =
                        (status.ready_replicas, status.replicas)
                    {
                        if ready_replicas == replicas && replicas > 0 {
                            info!(deployment_name=%deployment_name, namespace=%namespace, "Deployment is ready");

                            return Ok(HandlerAction::Continue {
                                state: ReconcilePublicEndpoint,
                                suggested_delay: None,
                            });
                        } else {
                            debug!(deployment_name=%deployment_name, ready=%ready_replicas, total=%replicas, "Deployment not yet ready");
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(deployment_name=%deployment_name, "Deployment not yet available, continuing to wait");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get deployment '{}'.", deployment_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Stay {
            max_times: 60, // 60 attempts * 5 seconds = 5 minutes max wait
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
        let config = ctx.desired_resource_config::<Worker>()?;
        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let labels = self.build_labels(deployment_name);
        let action = reconcile_kubernetes_public_endpoint(
            ctx,
            worker_public_endpoint_target(
                &config.id,
                deployment_name,
                namespace,
                labels,
                &config.ingress,
                config
                    .readiness_probe
                    .as_ref()
                    .map(|probe| probe.path.as_str()),
            ),
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
        let config = ctx.desired_resource_config::<Worker>()?;

        // Heartbeat check: verify deployment status
        if let (Some(deployment_name), Some(namespace)) = (&self.deployment_name, &self.namespace) {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_client(kubernetes_config)
                .await?;

            let deployment = get(
                kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
                deployment_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get deployment '{}'", deployment_name),
                resource_id: Some(config.id.clone()),
            })?;

            if let Some(status) = deployment.status.clone() {
                if let (Some(ready_replicas), Some(replicas)) =
                    (status.ready_replicas, status.replicas)
                {
                    if ready_replicas < replicas {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: format!(
                                "Deployment has {} ready replicas out of {} total",
                                ready_replicas, replicas
                            ),
                        }));
                    }
                }
            }

            let labels = self.build_labels(deployment_name);
            emit_kubernetes_workload_heartbeat(
                ctx,
                KubernetesWorkloadHeartbeatInput {
                    deployment_id: None,
                    resource_id: config.id.clone(),
                    resource_type: Worker::RESOURCE_TYPE,
                    data_kind: KubernetesWorkloadDataKind::Worker,
                    command_supported: false,
                    namespace: namespace.clone(),
                    workload_name: deployment_name.clone(),
                    workload_kind: alien_core::KubernetesWorkloadKind::Deployment,
                    workload: KubernetesWorkload::Deployment(deployment),
                    label_selector: label_selector(&labels)?,
                },
            )
            .await?;

            let action = reconcile_kubernetes_public_endpoint(
                ctx,
                worker_public_endpoint_target(
                    &config.id,
                    deployment_name,
                    namespace,
                    labels,
                    &config.ingress,
                    config
                        .readiness_probe
                        .as_ref()
                        .map(|probe| probe.path.as_str()),
                ),
                &mut self.public_endpoint,
            )
            .await?;
            if let KubernetesEndpointAction::Waiting { suggested_delay } = action {
                return Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(suggested_delay),
                });
            }

            debug!(deployment_name=%deployment_name, namespace=%namespace, "Worker deployment is healthy");
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
        let config = ctx.desired_resource_config::<Worker>()?;

        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
            })
        })?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        info!(deployment_name=%deployment_name, "Updating Kubernetes Worker deployment");

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_client(kubernetes_config)
            .await?;

        // Get the existing deployment to carry over resourceVersion (required for PUT)
        let existing = get(
            kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
            deployment_name,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to get deployment '{}' before update",
                deployment_name
            ),
            resource_id: Some(config.id.clone()),
        })?;

        let resource_version = existing.metadata.resource_version.clone();

        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        // Registry pull secret is required for image-based deployments.
        let image_pull_secret_name = if let WorkerCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let secret_name = format!("{}-registry", deployment_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };
        let env_secret_plan =
            reconcile_environment_secret("worker", &config.id, deployment_name, namespace, ctx)
                .await?;

        let mut new_deployment = self
            .build_deployment(
                config,
                deployment_name,
                namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;
        new_deployment.metadata.resource_version = resource_version;

        replace(
            kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
            deployment_name,
            &new_deployment,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to update deployment '{}'.", deployment_name),
            resource_id: Some(config.id.clone()),
        })?;

        info!(deployment_name=%deployment_name, "Deployment update submitted, waiting for rollout");

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
        let config = ctx.desired_resource_config::<Worker>()?;

        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
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
            .get_kubernetes_client(kubernetes_config)
            .await?;

        match get(
            kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
            deployment_name,
        )
        .await
        {
            Ok(deployment) => {
                if let Some(status) = &deployment.status {
                    if let (Some(ready_replicas), Some(replicas)) =
                        (status.ready_replicas, status.replicas)
                    {
                        if ready_replicas >= replicas && replicas > 0 {
                            info!(deployment_name=%deployment_name, "Deployment rollout complete");
                            return Ok(HandlerAction::Continue {
                                state: ReconcilePublicEndpointAfterUpdate,
                                suggested_delay: None,
                            });
                        } else {
                            debug!(deployment_name=%deployment_name, ready=%ready_replicas, total=%replicas, "Deployment rollout in progress");
                        }
                    }
                }
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get deployment '{}' during update wait",
                        deployment_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Stay {
            max_times: 60,
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
        let config = ctx.desired_resource_config::<Worker>()?;
        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;
        let labels = self.build_labels(deployment_name);
        let action = reconcile_kubernetes_public_endpoint(
            ctx,
            worker_public_endpoint_target(
                &config.id,
                deployment_name,
                namespace,
                labels,
                &config.ingress,
                config
                    .readiness_probe
                    .as_ref()
                    .map(|probe| probe.path.as_str()),
            ),
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
        let config = ctx.desired_resource_config::<Worker>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        info!(namespace=%namespace, "Initiating Kubernetes Worker deletion");

        delete_kubernetes_public_endpoint(ctx, &config.id, namespace, &mut self.public_endpoint)
            .await?;

        // Delete Deployment
        if let Some(deployment_name) = &self.deployment_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_client(kubernetes_config)
                .await?;

            match delete::<Deployment>(
                kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
                deployment_name,
            )
            .await
            {
                Ok(_) => {
                    info!(deployment_name=%deployment_name, "Deployment deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(deployment_name=%deployment_name, "Deployment already deleted");

                    self.deployment_name = None;
                    self.namespace = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete deployment '{}'.", deployment_name),
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
        let config = ctx.desired_resource_config::<Worker>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        // Check if deployment is deleted
        if let Some(deployment_name) = &self.deployment_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_client(kubernetes_config)
                .await?;

            match get(
                kube::Api::<Deployment>::namespaced(deployment_client.as_ref().clone(), namespace),
                deployment_name,
            )
            .await
            {
                Ok(_) => {
                    debug!(deployment_name=%deployment_name, "Deployment still exists, continuing to wait");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(deployment_name=%deployment_name, "Deployment successfully deleted");

                    self.deployment_name = None;
                    self.namespace = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get deployment '{}'.", deployment_name),
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
        if let Some(deployment_name) = &self.deployment_name {
            Some(ResourceOutputs::new(WorkerOutputs {
                worker_name: deployment_name.clone(),
                url: self.public_endpoint.effective_public_url(),
                identifier: Some(format!("deployment/{}", deployment_name)),
                load_balancer_endpoint: self.public_endpoint.load_balancer_endpoint.clone(),
                commands_push_target: None, // Kubernetes uses polling
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::{BindingValue, KubernetesWorkerBinding};

        // Construct binding on-the-fly from stored fields (like other controllers)
        if let (Some(worker_id), Some(service_name), Some(namespace)) =
            (&self.worker_id, &self.service_name, &self.namespace)
        {
            let binding = KubernetesWorkerBinding {
                name: BindingValue::Value(worker_id.clone()),
                namespace: BindingValue::Value(namespace.clone()),
                service_name: BindingValue::Value(service_name.clone()),
                service_port: BindingValue::Value(80),
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
    use alien_core::WorkerOutputs;

    use super::{KubernetesPublicEndpointState, KubernetesWorkerController, KubernetesWorkerState};

    #[test]
    fn build_outputs_includes_public_endpoint_url() {
        let public_endpoint = KubernetesPublicEndpointState {
            public_url: Some("https://worker.example.test".to_string()),
            ..Default::default()
        };
        let controller = KubernetesWorkerController {
            state: KubernetesWorkerState::Ready,
            deployment_name: Some("test-worker".to_string()),
            namespace: Some("test-namespace".to_string()),
            service_name: Some("test-worker".to_string()),
            worker_id: Some("worker".to_string()),
            public_endpoint,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let worker_outputs = outputs
            .downcast_ref::<WorkerOutputs>()
            .expect("worker outputs");

        assert_eq!(
            worker_outputs.url.as_deref(),
            Some("https://worker.example.test")
        );
    }

    #[test]
    fn build_outputs_derives_public_url_from_load_balancer_endpoint() {
        let public_endpoint = KubernetesPublicEndpointState {
            load_balancer_endpoint: Some(alien_core::LoadBalancerEndpoint {
                dns_name: "k8s-worker.example.elb.amazonaws.com".to_string(),
                hosted_zone_id: None,
            }),
            ..Default::default()
        };
        let controller = KubernetesWorkerController {
            state: KubernetesWorkerState::Ready,
            deployment_name: Some("test-worker".to_string()),
            namespace: Some("test-namespace".to_string()),
            service_name: Some("test-worker".to_string()),
            worker_id: Some("worker".to_string()),
            public_endpoint,
            _internal_stay_count: None,
        };

        let outputs = controller.build_outputs().expect("outputs");
        let worker_outputs = outputs
            .downcast_ref::<WorkerOutputs>()
            .expect("worker outputs");

        assert_eq!(
            worker_outputs.url.as_deref(),
            Some("http://k8s-worker.example.elb.amazonaws.com")
        );
    }
}

impl KubernetesWorkerController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str, namespace: &str) -> Self {
        Self {
            state: KubernetesWorkerState::Ready,
            deployment_name: Some(function_name.to_string()),
            namespace: Some(namespace.to_string()),
            service_name: Some(function_name.to_string()),
            worker_id: Some("test-worker".to_string()),
            public_endpoint: KubernetesPublicEndpointState::default(),
            _internal_stay_count: None,
        }
    }

    /// Builds a Kubernetes Deployment for the worker.
    async fn build_deployment(
        &self,
        config: &Worker,
        function_name: &str,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Deployment> {
        let labels = self.build_labels(function_name);
        let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());

        // Determine the container image
        let image = match &config.code {
            WorkerCode::Image { image } => image.clone(),
            WorkerCode::Source { .. } => {
                // For source code, we would need to get the built image from Build resource
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "Source-based workers not yet supported in Kubernetes platform"
                        .to_string(),
                }));
            }
        };

        // Build environment variables
        // IMPORTANT: Start with config.environment which includes injected vars from DeploymentConfig
        use crate::core::ResourceController;
        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_worker_runtime_env_vars(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .add_self_worker_binding(&config.id, self.get_binding_params()?.as_ref())?;

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
            // Skip if already added as a secret ref
            if !env_vars.iter().any(|ev| ev.name == key) {
                env_vars.push(EnvVar {
                    name: key,
                    value: Some(value),
                    value_from: None,
                });
            }
        }

        let container = Container {
            name: "worker".to_string(),
            image: Some(image),
            ports: Some(vec![ContainerPort {
                container_port: 8080,
                name: Some("http".to_string()),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            env: Some(env_vars),
            resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
                requests: Some({
                    let mut requests = BTreeMap::new();
                    requests.insert(
                        "memory".to_string(),
                        k8s_openapi::apimachinery::pkg::api::resource::Quantity(format!(
                            "{}Mi",
                            config.memory_mb
                        )),
                    );
                    requests.insert(
                        "cpu".to_string(),
                        k8s_openapi::apimachinery::pkg::api::resource::Quantity("100m".to_string()),
                    );
                    requests
                }),
                limits: Some({
                    let mut limits = BTreeMap::new();
                    limits.insert(
                        "memory".to_string(),
                        k8s_openapi::apimachinery::pkg::api::resource::Quantity(format!(
                            "{}Mi",
                            config.memory_mb
                        )),
                    );
                    limits.insert(
                        "cpu".to_string(),
                        k8s_openapi::apimachinery::pkg::api::resource::Quantity("1".to_string()),
                    );
                    limits
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let image_pull_secrets = image_pull_secret_name.map(|name| {
            vec![LocalObjectReference {
                name: name.to_string(),
            }]
        });
        let pod_annotations = env_secret_plan.map(|plan| {
            BTreeMap::from([("env-secret-checksum".to_string(), plan.checksum.clone())])
        });

        let pod_spec = PodSpec {
            service_account_name: Some(service_account_name.to_string()),
            containers: vec![container],
            restart_policy: Some("Always".to_string()),
            image_pull_secrets,
            ..Default::default()
        };

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some(function_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(1),
                selector: LabelSelector {
                    match_labels: Some(labels.clone()),
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

    /// Builds standard labels for Kubernetes resources.
    fn build_labels(&self, function_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), function_name.to_string());
        labels.insert("managed-by".to_string(), "runtime".to_string());
        labels.insert("component".to_string(), "worker".to_string());
        labels
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
    fn test_kubernetes_function_name() {
        // Test basic functionality
        assert_eq!(kubernetes_resource_name("my-stack", "my-func"), "my-func");

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
}

// ---------------------------------------------------------------------------
// Registry proxy helpers
// ---------------------------------------------------------------------------

/// Create a Kubernetes Docker config Secret for authenticating with the
/// manager's registry.
async fn create_registry_pull_secret(
    secrets_client: &std::sync::Arc<kube::Client>,
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
