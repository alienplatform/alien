use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Build, BuildOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;

use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

/// Generates a Kubernetes Job name from the stack prefix and build ID.
fn generate_kubernetes_build_name(resource_prefix: &str, id: &str) -> String {
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

    let combined = format!("{}-build-{}", clean_prefix, clean_id);

    // Truncate to 63 characters if necessary (Kubernetes limit)
    if combined.len() > 63 {
        combined[..63].to_string()
    } else {
        combined
    }
}

/// Generates the ServiceAccount name for builds following Helm naming convention.
/// Format: {release-name}-build-sa
fn generate_build_service_account_name(resource_prefix: &str) -> String {
    let clean_prefix = resource_prefix
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();

    let combined = format!("{}-build-sa", clean_prefix);

    // Truncate to 63 characters if necessary
    if combined.len() > 63 {
        combined[..63].to_string()
    } else {
        combined
    }
}

/// Kubernetes Build controller that creates Jobs for executing builds.
///
/// In the new Kubernetes platform design:
/// - Helm creates the ServiceAccount with build permissions (push to registries, etc.)
/// - Helm creates the Role/RoleBinding for namespace-scoped permissions
/// - Helm creates NetworkPolicy for build sandboxing
/// - The controller only creates Job resources for executing builds
#[controller]
pub struct KubernetesBuildController {
    /// The name of the created Kubernetes Job
    pub(crate) job_name: Option<String>,
    /// The namespace where the job is deployed
    pub(crate) namespace: Option<String>,
    /// The image digest of the built image (from job output)
    pub(crate) image_digest: Option<String>,
}

#[controller]
impl KubernetesBuildController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        info!(id=%config.id, "Initiating Kubernetes Build Job creation");

        let job_name = generate_kubernetes_build_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;

        // Generate ServiceAccount name following Helm naming convention
        let service_account_name = generate_build_service_account_name(&ctx.resource_prefix);

        // Create the Job
        let job_client = ctx
            .service_provider
            .get_kubernetes_job_client(kubernetes_config)
            .await?;
        let job = self
            .build_job(config, &job_name, &namespace, &service_account_name, ctx)
            .await?;

        let _created_job = job_client.create_job(&namespace, &job).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to create build job '{}'.", job_name),
                resource_id: Some(config.id.clone()),
            },
        )?;

        self.job_name = Some(job_name.clone());
        self.namespace = Some(namespace.clone());

        info!(job_name=%job_name, namespace=%namespace, "Build Job creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForJob,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForJob,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        let job_name = self.job_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Job name not set in state".to_string(),
            })
        })?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let job_client = ctx
            .service_provider
            .get_kubernetes_job_client(kubernetes_config)
            .await?;

        match job_client.get_job(namespace, job_name).await {
            Ok(job) => {
                if let Some(status) = &job.status {
                    // Check if job completed successfully
                    if let Some(succeeded) = status.succeeded {
                        if succeeded > 0 {
                            info!(job_name=%job_name, namespace=%namespace, "Build Job completed successfully");

                            // TODO: Extract image digest from job output/logs
                            // For now, use "latest" tag — the image was pushed by the build job
                            // and will be pulled by tag, not digest.
                            self.image_digest = Some("latest".to_string());

                            return Ok(HandlerAction::Continue {
                                state: Ready,
                                suggested_delay: Some(Duration::from_secs(30)),
                            });
                        }
                    }

                    // Check if job failed
                    if let Some(failed) = status.failed {
                        if failed > 0 {
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: format!("Build job '{}' failed", job_name),
                                resource_id: Some(config.id.clone()),
                            }));
                        }
                    }

                    // Still running
                    debug!(job_name=%job_name, "Build Job still running");
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(job_name=%job_name, "Job not yet available, continuing to wait");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get job '{}'.", job_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Stay {
            max_times: 120, // 120 attempts * 10 seconds = 20 minutes max wait for build
            suggested_delay: Some(Duration::from_secs(10)),
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
        let config = ctx.desired_resource_config::<Build>()?;

        // Heartbeat check: verify job still exists and completed successfully
        if let (Some(job_name), Some(namespace)) = (&self.job_name, &self.namespace) {
            let job_client = ctx
                .service_provider
                .get_kubernetes_job_client(kubernetes_config)
                .await?;

            let job = job_client.get_job(namespace, job_name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get job '{}'", job_name),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(status) = job.status {
                if let Some(succeeded) = status.succeeded {
                    if succeeded == 0 {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: "Build job no longer shows successful completion".to_string(),
                        }));
                    }
                }
            }

            debug!(job_name=%job_name, namespace=%namespace, "Build Job is healthy");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Build Jobs are immutable in Kubernetes — update by deleting the old job and creating a new one.
    // All states use on_failure = UpdateFailed to keep failure recovery within the Update flow.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = DeletingOldJob,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn deleting_old_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        info!(namespace=%namespace, "Deleting old Build Job to apply updated config");

        if let Some(job_name) = &self.job_name {
            let job_client = ctx
                .service_provider
                .get_kubernetes_job_client(kubernetes_config)
                .await?;

            match job_client.delete_job(namespace, job_name).await {
                Ok(_) => {
                    info!(job_name=%job_name, "Old Job deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(job_name=%job_name, "Old Job already gone");
                    self.job_name = None;
                    self.image_digest = None;
                    return Ok(HandlerAction::Continue {
                        state: RecreatingJob,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete old job '{}'.", job_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: WaitingForOldJobDeletion,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForOldJobDeletion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_old_job_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        if let Some(job_name) = &self.job_name {
            let job_client = ctx
                .service_provider
                .get_kubernetes_job_client(kubernetes_config)
                .await?;

            match job_client.get_job(namespace, job_name).await {
                Ok(_) => {
                    debug!(job_name=%job_name, "Old Job still exists, waiting for deletion");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(job_name=%job_name, "Old Job deleted, proceeding to recreate");
                    self.job_name = None;
                    self.image_digest = None;
                    return Ok(HandlerAction::Continue {
                        state: RecreatingJob,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get old job '{}' while waiting for deletion",
                            job_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            return Ok(HandlerAction::Continue {
                state: RecreatingJob,
                suggested_delay: None,
            });
        }

        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = RecreatingJob,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn recreating_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        let job_name = generate_kubernetes_build_name(&ctx.resource_prefix, &config.id);
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let service_account_name = generate_build_service_account_name(&ctx.resource_prefix);

        info!(job_name=%job_name, namespace=%namespace, "Recreating Build Job with updated config");

        let job_client = ctx
            .service_provider
            .get_kubernetes_job_client(kubernetes_config)
            .await?;
        let job = self
            .build_job(config, &job_name, namespace, &service_account_name, ctx)
            .await?;

        job_client
            .create_job(namespace, &job)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create updated build job '{}'.", job_name),
                resource_id: Some(config.id.clone()),
            })?;

        self.job_name = Some(job_name.clone());

        info!(job_name=%job_name, "Updated Build Job creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForRecreatedJob,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForRecreatedJob,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_recreated_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Build>()?;

        let job_name = self.job_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Job name not set in state".to_string(),
            })
        })?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let job_client = ctx
            .service_provider
            .get_kubernetes_job_client(kubernetes_config)
            .await?;

        match job_client.get_job(namespace, job_name).await {
            Ok(job) => {
                if let Some(status) = &job.status {
                    if let Some(succeeded) = status.succeeded {
                        if succeeded > 0 {
                            info!(job_name=%job_name, "Updated Build Job completed successfully");
                            // TODO: Extract image digest from job output/logs
                            // For now, use "latest" tag — the image was pushed by the build job
                            // and will be pulled by tag, not digest.
                            self.image_digest = Some("latest".to_string());
                            return Ok(HandlerAction::Continue {
                                state: Ready,
                                suggested_delay: Some(Duration::from_secs(30)),
                            });
                        }
                    }

                    if let Some(failed) = status.failed {
                        if failed > 0 {
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: format!("Updated build job '{}' failed", job_name),
                                resource_id: Some(config.id.clone()),
                            }));
                        }
                    }

                    debug!(job_name=%job_name, "Updated Build Job still running");
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(job_name=%job_name, "Updated Job not yet available, continuing to wait");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get updated job '{}'.", job_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Stay {
            max_times: 120,
            suggested_delay: Some(Duration::from_secs(10)),
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
        let config = ctx.desired_resource_config::<Build>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        info!(namespace=%namespace, "Initiating Kubernetes Build Job deletion");

        // Delete Job
        if let Some(job_name) = &self.job_name {
            let job_client = ctx
                .service_provider
                .get_kubernetes_job_client(kubernetes_config)
                .await?;

            match job_client.delete_job(namespace, job_name).await {
                Ok(_) => {
                    info!(job_name=%job_name, "Job deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(job_name=%job_name, "Job already deleted");

                    self.job_name = None;
                    self.namespace = None;
                    self.image_digest = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete job '{}'.", job_name),
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
        let config = ctx.desired_resource_config::<Build>()?;

        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        // Check if job is deleted
        if let Some(job_name) = &self.job_name {
            let job_client = ctx
                .service_provider
                .get_kubernetes_job_client(kubernetes_config)
                .await?;

            match job_client.get_job(namespace, job_name).await {
                Ok(_) => {
                    debug!(job_name=%job_name, "Job still exists, continuing to wait");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(job_name=%job_name, "Job successfully deleted");

                    self.job_name = None;
                    self.namespace = None;
                    self.image_digest = None;

                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get job '{}'.", job_name),
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
        if let Some(job_name) = &self.job_name {
            Some(ResourceOutputs::new(BuildOutputs {
                identifier: format!("job/{}", job_name),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Build bindings are constructed from namespace and ServiceAccount info
        // No controller-specific binding params needed
        Ok(None)
    }
}

impl KubernetesBuildController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(job_name: &str, namespace: &str) -> Self {
        Self {
            state: KubernetesBuildState::Ready,
            job_name: Some(job_name.to_string()),
            namespace: Some(namespace.to_string()),
            image_digest: Some("sha256:mock".to_string()),
            _internal_stay_count: None,
        }
    }

    /// Builds a Kubernetes Job for the build.
    async fn build_job(
        &self,
        config: &Build,
        job_name: &str,
        namespace: &str,
        service_account_name: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Job> {
        let labels = self.build_labels(job_name);

        let env_builder = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        let (env_map, bindings) = env_builder.build_with_bindings();

        let mut env_vars = Vec::new();

        // Process bindings for Kubernetes SecretRefs
        for (binding_name, binding_json) in bindings {
            if let Ok(extraction) = crate::core::k8s_secret_bindings::extract_binding_secrets(
                &binding_name,
                &binding_json,
            ) {
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

        for (key, value) in env_map {
            if !env_vars.iter().any(|ev| ev.name == key) {
                env_vars.push(EnvVar {
                    name: key,
                    value: Some(value),
                    value_from: None,
                });
            }
        }

        // TODO: Determine build image and arguments from config.source
        // For now, use a placeholder
        let build_image = "gcr.io/kaniko-project/executor:latest".to_string();
        let build_args = vec![
            "--dockerfile=Dockerfile".to_string(),
            "--context=.".to_string(),
        ];

        let container = Container {
            name: "build".to_string(),
            image: Some(build_image),
            args: Some(build_args),
            env: Some(env_vars),
            ..Default::default()
        };

        let pod_spec = PodSpec {
            service_account_name: Some(service_account_name.to_string()),
            containers: vec![container],
            restart_policy: Some("Never".to_string()),
            ..Default::default()
        };

        let job = Job {
            metadata: ObjectMeta {
                name: Some(job_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(JobSpec {
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(labels),
                        ..Default::default()
                    }),
                    spec: Some(pod_spec),
                },
                backoff_limit: Some(3),
                ..Default::default()
            }),
            ..Default::default()
        };

        Ok(job)
    }

    /// Builds standard labels for Kubernetes resources.
    fn build_labels(&self, job_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), job_name.to_string());
        labels.insert("managed-by".to_string(), "alien".to_string());
        labels.insert("component".to_string(), "build".to_string());
        labels
    }

    /// Gets the Kubernetes namespace from KubernetesClientConfig
    fn get_kubernetes_namespace(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        let k8s_config = ctx.get_kubernetes_config()?;
        match k8s_config {
            alien_core::KubernetesClientConfig::InCluster { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-build".to_string(),
                        message: "Kubernetes namespace not configured in InCluster config"
                            .to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-build".to_string(),
                        message: "Kubernetes namespace not configured in Kubeconfig".to_string(),
                    })
                })
            }
            alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes-build".to_string(),
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
    fn test_generate_kubernetes_build_name() {
        // Test basic functionality
        assert_eq!(
            generate_kubernetes_build_name("my-stack", "my-build"),
            "my-stack-build-my-build"
        );

        // Test character filtering and lowercasing
        assert_eq!(
            generate_kubernetes_build_name("My_Stack!", "Test#123"),
            "mystack-build-test123"
        );

        // Test length truncation
        let long_prefix = "a".repeat(50);
        let long_id = "b".repeat(20);
        let result = generate_kubernetes_build_name(&long_prefix, &long_id);
        assert!(result.len() <= 63);
    }

    #[test]
    fn test_generate_build_service_account_name() {
        // Test basic functionality
        assert_eq!(
            generate_build_service_account_name("my-app"),
            "my-app-build-sa"
        );

        // Test character filtering
        assert_eq!(
            generate_build_service_account_name("My_App!"),
            "myapp-build-sa"
        );

        // Test length truncation
        let long_prefix = "a".repeat(60);
        let result = generate_build_service_account_name(&long_prefix);
        assert!(result.len() <= 63);
    }
}
