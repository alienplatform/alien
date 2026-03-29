use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::EnvironmentVariableBuilder;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Function, FunctionCode, FunctionOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

/// Generates a Kubernetes resource name from the stack prefix and function ID.
fn generate_kubernetes_function_name(resource_prefix: &str, id: &str) -> String {
    // Kubernetes names must be lowercase and follow DNS-1123 label requirements
    let clean_prefix = resource_prefix
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();
    let clean_id = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();

    let combined = format!("{}-{}", clean_prefix, clean_id);

    // Truncate to 63 characters if necessary (Kubernetes limit)
    if combined.len() > 63 {
        combined[..63].to_string()
    } else {
        combined
    }
}

/// Generates the ServiceAccount name following Helm naming convention.
/// Format: {release-name}-{permission-profile}-sa
fn generate_service_account_name(resource_prefix: &str, permission_profile: &str) -> String {
    let clean_prefix = resource_prefix
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();
    let clean_profile = permission_profile
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();

    let combined = format!("{}-{}-sa", clean_prefix, clean_profile);

    // Truncate to 63 characters if necessary
    if combined.len() > 63 {
        combined[..63].to_string()
    } else {
        combined
    }
}

#[controller]
pub struct KubernetesFunctionController {
    /// The name of the created Kubernetes Deployment.
    pub(crate) deployment_name: Option<String>,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
    /// The service name for the function (for binding construction)
    pub(crate) service_name: Option<String>,
    /// The public URL if available (from Helm pre-computed map)
    pub(crate) public_url: Option<String>,
    /// The function ID (for binding construction)
    pub(crate) function_id: Option<String>,
}

#[controller]
impl KubernetesFunctionController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Function>()?;

        info!(id=%config.id, "Initiating Kubernetes Function creation");

        let function_name = generate_kubernetes_function_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;

        // Store data needed for binding construction
        self.function_id = Some(config.id.clone());
        self.service_name = Some(function_name.clone());
        self.namespace = Some(namespace.clone());
        self.public_url = ctx
            .deployment_config
            .public_urls
            .as_ref()
            .and_then(|urls| urls.get(&config.id))
            .cloned();

        // Generate ServiceAccount name following Helm naming convention
        let service_account_name =
            generate_service_account_name(&ctx.resource_prefix, config.get_permissions());

        // Create the Deployment
        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let deployment = self
            .build_deployment(
                config,
                &function_name,
                &namespace,
                &service_account_name,
                ctx,
            )
            .await?;

        let _created_deployment = deployment_client
            .create_deployment(&namespace, &deployment)
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
        let config = ctx.desired_resource_config::<Function>()?;

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
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        match deployment_client
            .get_deployment(namespace, deployment_name)
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
                                state: Ready,
                                suggested_delay: Some(Duration::from_secs(30)),
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

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Function>()?;

        // Heartbeat check: verify deployment status
        if let (Some(deployment_name), Some(namespace)) = (&self.deployment_name, &self.namespace) {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            let deployment = deployment_client
                .get_deployment(namespace, deployment_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get deployment '{}'", deployment_name),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(status) = deployment.status {
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

            debug!(deployment_name=%deployment_name, namespace=%namespace, "Function deployment is healthy");
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
        let config = ctx.desired_resource_config::<Function>()?;

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

        info!(deployment_name=%deployment_name, "Updating Kubernetes Function deployment");

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        // Get the existing deployment to carry over resourceVersion (required for PUT)
        let existing = deployment_client
            .get_deployment(namespace, deployment_name)
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
            generate_service_account_name(&ctx.resource_prefix, config.get_permissions());
        let mut new_deployment = self
            .build_deployment(
                config,
                deployment_name,
                namespace,
                &service_account_name,
                ctx,
            )
            .await?;
        new_deployment.metadata.resource_version = resource_version;

        deployment_client
            .update_deployment(namespace, deployment_name, &new_deployment)
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
        let config = ctx.desired_resource_config::<Function>()?;

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
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        match deployment_client
            .get_deployment(namespace, deployment_name)
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
                                state: Ready,
                                suggested_delay: Some(Duration::from_secs(30)),
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

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Function>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        info!(namespace=%namespace, "Initiating Kubernetes Function deletion");

        // Delete Deployment
        if let Some(deployment_name) = &self.deployment_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            match deployment_client
                .delete_deployment(namespace, deployment_name)
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
        let config = ctx.desired_resource_config::<Function>()?;

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
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;

            match deployment_client
                .get_deployment(namespace, deployment_name)
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
            Some(ResourceOutputs::new(FunctionOutputs {
                function_name: deployment_name.clone(),
                url: None, // URL comes from Helm-created Service/Ingress, not managed here
                identifier: Some(format!("deployment/{}", deployment_name)),
                load_balancer_endpoint: None,
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::{BindingValue, KubernetesFunctionBinding};

        // Construct binding on-the-fly from stored fields (like other controllers)
        if let (Some(function_id), Some(service_name), Some(namespace)) =
            (&self.function_id, &self.service_name, &self.namespace)
        {
            let binding = KubernetesFunctionBinding {
                name: BindingValue::Value(function_id.clone()),
                namespace: BindingValue::Value(namespace.clone()),
                service_name: BindingValue::Value(service_name.clone()),
                service_port: BindingValue::Value(80),
                public_url: self
                    .public_url
                    .as_ref()
                    .map(|url| BindingValue::Value(url.clone())),
            };

            // Serialize to JSON
            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
}

impl KubernetesFunctionController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str, namespace: &str) -> Self {
        Self {
            state: KubernetesFunctionState::Ready,
            deployment_name: Some(function_name.to_string()),
            namespace: Some(namespace.to_string()),
            service_name: Some(function_name.to_string()),
            public_url: None,
            function_id: Some("test-function".to_string()),
            _internal_stay_count: None,
        }
    }

    /// Builds a Kubernetes Deployment for the function.
    async fn build_deployment(
        &self,
        config: &Function,
        function_name: &str,
        namespace: &str,
        service_account_name: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Deployment> {
        let labels = self.build_labels(function_name);

        // Determine the container image
        let image = match &config.code {
            FunctionCode::Image { image } => image.clone(),
            FunctionCode::Source { .. } => {
                // For source code, we would need to get the built image from Build resource
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "Source-based functions not yet supported in Kubernetes platform"
                        .to_string(),
                }));
            }
        };

        // Build environment variables
        // IMPORTANT: Start with config.environment which includes injected vars from DeploymentConfig
        use crate::core::ResourceController;
        let env_builder = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_function_transport_env_vars(ctx.platform)
            .add_env_var("ALIEN_RUNTIME_SEND_OTLP".to_string(), "true".to_string())
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .add_self_function_binding(&config.id, self.get_binding_params()?.as_ref())?;

        let (env_map, bindings) = env_builder.build_with_bindings();

        let mut env_vars = Vec::new();

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
            name: "function".to_string(),
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

        let pod_spec = PodSpec {
            service_account_name: Some(service_account_name.to_string()),
            containers: vec![container],
            restart_policy: Some("Always".to_string()),
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
                        labels: Some(labels),
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
        labels.insert("managed-by".to_string(), "alien".to_string());
        labels.insert("component".to_string(), "function".to_string());
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
    fn test_generate_kubernetes_function_name() {
        // Test basic functionality
        assert_eq!(
            generate_kubernetes_function_name("my-stack", "my-func"),
            "my-stack-my-func"
        );

        // Test character filtering and lowercasing
        assert_eq!(
            generate_kubernetes_function_name("My_Stack!", "Test#123"),
            "mystack-test123"
        );

        // Test length truncation
        let long_prefix = "a".repeat(50);
        let long_id = "b".repeat(20);
        let result = generate_kubernetes_function_name(&long_prefix, &long_id);
        assert!(result.len() <= 63);
        assert!(result.starts_with("aaa"));
    }

    #[test]
    fn test_generate_service_account_name() {
        // Test basic functionality
        assert_eq!(
            generate_service_account_name("my-app", "reader"),
            "my-app-reader-sa"
        );

        // Test character filtering
        assert_eq!(
            generate_service_account_name("My_App!", "Writer#Profile"),
            "myapp-writerprofile-sa"
        );

        // Test length truncation
        let long_prefix = "a".repeat(50);
        let result = generate_service_account_name(&long_prefix, "reader");
        assert!(result.len() <= 63);
    }
}
