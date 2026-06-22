use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use aws_sdk_codebuild::{
    operation::{create_project::CreateProjectInput, update_project::UpdateProjectInput},
    types::{
        ArtifactsType, CloudWatchLogsConfig, ComputeType as AwsCodeBuildComputeType,
        EnvironmentType, EnvironmentVariable as AwsCodeBuildEnvironmentVariable,
        ImagePullCredentialsType, LogsConfig, LogsConfigStatusType, Project as CodeBuildProject,
        ProjectArtifacts, ProjectEnvironment, ProjectSource, S3LogsConfig, SourceType,
        Tag as CodeBuildTag,
    },
    Client as CodeBuildClient,
};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    standard_resource_tags, AwsCodeBuildHeartbeatData, Build, BuildHeartbeatData,
    BuildHeartbeatStatus, BuildOutputs, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Generates the full, prefixed AWS CodeBuild project name.
fn get_aws_project_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

/// Maps compute type from Alien to AWS CodeBuild values
/// Uses LINUX_CONTAINER environment type with appropriate memory allocations
fn map_compute_type(compute_type: &alien_core::ComputeType) -> String {
    match compute_type {
        alien_core::ComputeType::Small => "BUILD_GENERAL1_SMALL".to_string(), // 1 GB memory
        alien_core::ComputeType::Medium => "BUILD_GENERAL1_MEDIUM".to_string(), // 2 GB memory
        alien_core::ComputeType::Large => "BUILD_GENERAL1_LARGE".to_string(), // 4 GB memory
        alien_core::ComputeType::XLarge => "BUILD_GENERAL1_2XLARGE".to_string(), // 8 GB memory
    }
}

/// Maps compute type from Alien to AWS CodeBuild environment type values
fn map_environment_type(_compute_type: &alien_core::ComputeType) -> String {
    "LINUX_CONTAINER".to_string()
}

#[controller]
pub struct AwsBuildController {
    /// The ARN of the created CodeBuild project.
    pub(crate) project_arn: Option<String>,
    /// The logical AWS CodeBuild project name (stack prefix + id). Stored to expose in outputs.
    pub(crate) project_name: Option<String>,
    /// The computed environment variables for this build.
    pub(crate) build_env_vars: Option<HashMap<String, String>>,
}

#[controller]
impl AwsBuildController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_codebuild_client(aws_cfg)
            .await?;
        let cfg = ctx.desired_resource_config::<Build>()?;

        info!(name=%cfg.id, "Initiating CodeBuild project creation");

        // Get the ServiceAccount for this build's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let role_arn = self
            .get_service_account_arn(ctx, &service_account_ref)
            .await?;

        let aws_project_name = get_aws_project_name(ctx.resource_prefix, &cfg.id);

        // Prepare and store environment variables for the build
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, &aws_project_name)
            .await?;

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars.clone());

        let project_config = build_codebuild_create_project_request(
            ctx.resource_prefix,
            cfg,
            aws_project_name.clone(),
            role_arn,
            env_vars,
        )?;

        let response = create_codebuild_project(&client, project_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create CodeBuild project".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        self.project_arn = response.arn().map(ToString::to_string);
        self.project_name = Some(aws_project_name.clone());

        info!(name=%aws_project_name, arn=%self.project_arn.as_deref().unwrap_or("unknown"), "CodeBuild project created successfully");

        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Build>()?;

        info!(name=%config.id, "Applying resource-scoped permissions for CodeBuild project");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(project_name) = &self.project_name {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                project_name,
                "build",
            )
            .await?;
        }

        info!(name=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_codebuild_client(aws_cfg)
            .await?;
        let config = ctx.desired_resource_config::<Build>()?;

        // Heartbeat check: verify CodeBuild project still exists and check basic properties
        if let Some(project_name) = &self.project_name {
            let project = get_codebuild_project(&client, project_name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to check CodeBuild project status".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            let project = match project {
                Some(project) => project,
                None => {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: "CodeBuild project no longer exists".to_string(),
                    }));
                }
            };

            // Check basic drift detection - compare compute type
            let expected_compute_type = map_compute_type(&config.compute_type);
            if let Some(actual_compute_type) = project
                .environment()
                .map(|environment| environment.compute_type().as_str().to_string())
            {
                if actual_compute_type != expected_compute_type {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "CodeBuild project compute type has drifted from '{}' to '{}'",
                            expected_compute_type, actual_compute_type
                        ),
                    }));
                }
            }

            emit_aws_build_heartbeat(ctx, &config.id, &project);
        }

        debug!(name = %config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_codebuild_client(aws_cfg)
            .await?;
        let current_config = ctx.desired_resource_config::<Build>()?;
        let aws_project_name = get_aws_project_name(ctx.resource_prefix, &current_config.id);

        info!(name=%aws_project_name, "Updating CodeBuild project configuration");

        // Get the ServiceAccount for this build's permission profile
        let service_account_id = format!("{}-sa", current_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let role_arn = self
            .get_service_account_arn(ctx, &service_account_ref)
            .await?;

        // Prepare and store environment variables for the build
        let env_vars = self
            .prepare_environment_variables(
                &current_config.environment,
                &current_config.links,
                ctx,
                &aws_project_name,
            )
            .await?;

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars.clone());

        let project_config = build_codebuild_update_project_request(
            ctx.resource_prefix,
            current_config,
            aws_project_name.clone(),
            role_arn,
            env_vars,
        )?;

        let response = update_codebuild_project(&client, project_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update CodeBuild project".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

        self.project_arn = response.arn().map(ToString::to_string);
        self.project_name = Some(aws_project_name.clone());

        info!(name=%aws_project_name, "CodeBuild project updated successfully");

        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_codebuild_client(aws_cfg)
            .await?;
        let build_config = ctx.desired_resource_config::<Build>()?;
        let aws_project_name = get_aws_project_name(ctx.resource_prefix, &build_config.id);

        info!(name=%aws_project_name, "Deleting CodeBuild project");

        match get_codebuild_project(&client, &aws_project_name).await {
            Ok(None) => {
                warn!(name=%aws_project_name, "CodeBuild project was already deleted");
                self.project_arn = None;
                self.project_name = None;
                self.build_env_vars = None;
            }
            Ok(Some(_)) => {
                client
                    .delete_project()
                    .name(&aws_project_name)
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to delete CodeBuild project".to_string(),
                        resource_id: Some(build_config.id.clone()),
                    })?;
                info!(name=%aws_project_name, "CodeBuild project deleted successfully");
                self.project_arn = None;
                self.project_name = None;
                self.build_env_vars = None;
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check CodeBuild project before deletion".to_string(),
                    resource_id: Some(build_config.id.clone()),
                }));
            }
        }

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
        self.project_arn.as_ref().map(|arn| {
            ResourceOutputs::new(BuildOutputs {
                identifier: arn.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::BuildBinding;

        if let (Some(project_name), Some(build_env_vars)) =
            (&self.project_name, &self.build_env_vars)
        {
            let binding =
                BuildBinding::codebuild(project_name.clone(), build_env_vars.clone(), None);
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

// Separate impl block for helper methods
impl AwsBuildController {
    // ─────────────── HELPER METHODS ────────────────────────────
    async fn get_service_account_arn(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_account_ref: &ResourceRef,
    ) -> Result<String> {
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                service_account_ref,
            )?;

        service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "build".to_string(),
                    dependency_id: service_account_ref.id().to_string(),
                })
            })
            .map(|s| s.to_string())
    }

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        project_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        EnvironmentVariableBuilder::try_new(initial_env)?
            .add_standard_alien_env_vars(ctx)?
            .add_linked_resources(links, ctx, project_name_for_error_logging)
            .await
            .map(|builder| builder.build())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(project_name: &str) -> Self {
        Self {
            state: AwsBuildState::Ready,
            project_arn: Some(format!(
                "arn:aws:codebuild:us-east-1:123456789012:project/{}",
                project_name
            )),
            project_name: Some(project_name.to_string()),
            build_env_vars: Some(HashMap::new()),
            _internal_stay_count: None,
        }
    }
}

fn build_codebuild_create_project_request(
    resource_prefix: &str,
    build: &Build,
    project_name: String,
    service_role: String,
    environment_variables: HashMap<String, String>,
) -> Result<CreateProjectInput> {
    let description = format!("Runtime build project: {}", build.id);
    let tags = codebuild_tags(standard_resource_tags(resource_prefix, &build.id));
    let (source, artifacts, environment, logs_config) =
        codebuild_project_components(&project_name, build, environment_variables)?;

    CreateProjectInput::builder()
        .name(project_name)
        .description(description)
        .source(source)
        .artifacts(artifacts)
        .environment(environment)
        .service_role(service_role)
        .set_tags(Some(tags))
        .logs_config(logs_config)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild create request for '{}'",
                build.id
            ),
            resource_id: Some(build.id.clone()),
        })
}

fn build_codebuild_update_project_request(
    resource_prefix: &str,
    build: &Build,
    project_name: String,
    service_role: String,
    environment_variables: HashMap<String, String>,
) -> Result<UpdateProjectInput> {
    let description = format!("Runtime build project: {}", build.id);
    let tags = codebuild_tags(standard_resource_tags(resource_prefix, &build.id));
    let (source, artifacts, environment, logs_config) =
        codebuild_project_components(&project_name, build, environment_variables)?;

    UpdateProjectInput::builder()
        .name(project_name)
        .description(description)
        .source(source)
        .artifacts(artifacts)
        .environment(environment)
        .service_role(service_role)
        .set_tags(Some(tags))
        .logs_config(logs_config)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild update request for '{}'",
                build.id
            ),
            resource_id: Some(build.id.clone()),
        })
}

fn codebuild_project_components(
    project_name: &str,
    build: &Build,
    environment_variables: HashMap<String, String>,
) -> Result<(
    ProjectSource,
    ProjectArtifacts,
    ProjectEnvironment,
    LogsConfig,
)> {
    let source = ProjectSource::builder()
        .r#type(SourceType::NoSource)
        .buildspec(
            r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build script will be provided at runtime"
"#
            .to_string(),
        )
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build CodeBuild source for project '{project_name}'"),
            resource_id: Some(build.id.clone()),
        })?;

    let artifacts = ProjectArtifacts::builder()
        .r#type(ArtifactsType::NoArtifacts)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build CodeBuild artifacts for project '{project_name}'"),
            resource_id: Some(build.id.clone()),
        })?;

    let environment_variables = environment_variables
        .into_iter()
        .map(|(name, value)| {
            AwsCodeBuildEnvironmentVariable::builder()
                .name(name)
                .value(value)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to build CodeBuild environment variable for project '{project_name}'"
                    ),
                    resource_id: Some(build.id.clone()),
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let environment = ProjectEnvironment::builder()
        .r#type(EnvironmentType::from(
            map_environment_type(&build.compute_type).as_str(),
        ))
        .image("ghcr.io/alienplatform/alien-builder:latest")
        .compute_type(AwsCodeBuildComputeType::from(
            map_compute_type(&build.compute_type).as_str(),
        ))
        .image_pull_credentials_type(ImagePullCredentialsType::ServiceRole)
        .set_environment_variables(Some(environment_variables))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build CodeBuild environment for project '{project_name}'"),
            resource_id: Some(build.id.clone()),
        })?;

    let cloud_watch_logs = CloudWatchLogsConfig::builder()
        .status(LogsConfigStatusType::Enabled)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild CloudWatch logs config for project '{project_name}'"
            ),
            resource_id: Some(build.id.clone()),
        })?;

    let s3_logs = S3LogsConfig::builder()
        .status(LogsConfigStatusType::Disabled)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild S3 logs config for project '{project_name}'"
            ),
            resource_id: Some(build.id.clone()),
        })?;

    let logs_config = LogsConfig::builder()
        .cloud_watch_logs(cloud_watch_logs)
        .s3_logs(s3_logs)
        .build();

    Ok((source, artifacts, environment, logs_config))
}

fn codebuild_tags(tags: HashMap<String, String>) -> Vec<CodeBuildTag> {
    tags.into_iter()
        .map(|(key, value)| CodeBuildTag::builder().key(key).value(value).build())
        .collect()
}

async fn create_codebuild_project(
    client: &CodeBuildClient,
    request: CreateProjectInput,
) -> Result<CodeBuildProject> {
    let project_name = request.name().unwrap_or("<unknown>").to_string();

    let response = client
        .create_project()
        .set_name(request.name)
        .set_description(request.description)
        .set_source(request.source)
        .set_secondary_sources(request.secondary_sources)
        .set_source_version(request.source_version)
        .set_secondary_source_versions(request.secondary_source_versions)
        .set_artifacts(request.artifacts)
        .set_secondary_artifacts(request.secondary_artifacts)
        .set_cache(request.cache)
        .set_environment(request.environment)
        .set_service_role(request.service_role)
        .set_timeout_in_minutes(request.timeout_in_minutes)
        .set_queued_timeout_in_minutes(request.queued_timeout_in_minutes)
        .set_encryption_key(request.encryption_key)
        .set_tags(request.tags)
        .set_vpc_config(request.vpc_config)
        .set_badge_enabled(request.badge_enabled)
        .set_logs_config(request.logs_config)
        .set_file_system_locations(request.file_system_locations)
        .set_build_batch_config(request.build_batch_config)
        .set_concurrent_build_limit(request.concurrent_build_limit)
        .set_auto_retry_limit(request.auto_retry_limit)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("CodeBuild CreateProject API failed for project '{project_name}'"),
            resource_id: None,
        })?;

    response.project().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "CodeBuild CreateProject response for '{project_name}' did not include a project"
            ),
            resource_id: None,
        })
    })
}

async fn update_codebuild_project(
    client: &CodeBuildClient,
    request: UpdateProjectInput,
) -> Result<CodeBuildProject> {
    let project_name = request.name().unwrap_or("<unknown>").to_string();

    let response = client
        .update_project()
        .set_name(request.name)
        .set_description(request.description)
        .set_source(request.source)
        .set_secondary_sources(request.secondary_sources)
        .set_source_version(request.source_version)
        .set_secondary_source_versions(request.secondary_source_versions)
        .set_artifacts(request.artifacts)
        .set_secondary_artifacts(request.secondary_artifacts)
        .set_cache(request.cache)
        .set_environment(request.environment)
        .set_service_role(request.service_role)
        .set_timeout_in_minutes(request.timeout_in_minutes)
        .set_queued_timeout_in_minutes(request.queued_timeout_in_minutes)
        .set_encryption_key(request.encryption_key)
        .set_tags(request.tags)
        .set_vpc_config(request.vpc_config)
        .set_badge_enabled(request.badge_enabled)
        .set_logs_config(request.logs_config)
        .set_file_system_locations(request.file_system_locations)
        .set_build_batch_config(request.build_batch_config)
        .set_concurrent_build_limit(request.concurrent_build_limit)
        .set_auto_retry_limit(request.auto_retry_limit)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("CodeBuild UpdateProject API failed for project '{project_name}'"),
            resource_id: None,
        })?;

    response.project().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "CodeBuild UpdateProject response for '{project_name}' did not include a project"
            ),
            resource_id: None,
        })
    })
}

async fn get_codebuild_project(
    client: &CodeBuildClient,
    project_name: &str,
) -> Result<Option<CodeBuildProject>> {
    let response = client
        .batch_get_projects()
        .names(project_name)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("CodeBuild BatchGetProjects API failed for project '{project_name}'"),
            resource_id: None,
        })?;

    Ok(response.projects().first().cloned())
}

fn emit_aws_build_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project: &CodeBuildProject,
) {
    let project_name = project.name().unwrap_or_default().to_string();
    let source = project.source();
    let artifacts = project.artifacts();
    let environment = project.environment();
    let logs_config = project.logs_config();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Build::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Build(BuildHeartbeatData::AwsCodeBuild(
            AwsCodeBuildHeartbeatData {
                status: BuildHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "AWS CodeBuild project '{}' is reachable",
                        project_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                project_name,
                project_arn: project.arn().map(ToString::to_string),
                description: project.description().map(ToString::to_string),
                source_type: source.map(|source| source.r#type().as_str().to_string()),
                artifacts_type: artifacts.map(|artifacts| artifacts.r#type().as_str().to_string()),
                artifacts_encryption_disabled: artifacts
                    .and_then(|artifacts| artifacts.encryption_disabled()),
                environment_type: environment
                    .map(|environment| environment.r#type().as_str().to_string()),
                environment_image: environment.map(|environment| environment.image().to_string()),
                compute_type: environment
                    .map(|environment| environment.compute_type().as_str().to_string()),
                image_pull_credentials_type: environment
                    .and_then(|environment| environment.image_pull_credentials_type())
                    .map(|credentials_type| credentials_type.as_str().to_string()),
                privileged_mode: environment.and_then(|environment| environment.privileged_mode()),
                environment_variable_count: environment
                    .and_then(|environment| {
                        u32::try_from(environment.environment_variables().len()).ok()
                    })
                    .unwrap_or(0),
                service_role_present: project.service_role().is_some(),
                encryption_key_present: project.encryption_key().is_some(),
                cloud_watch_logs_status: logs_config
                    .and_then(|logs_config| logs_config.cloud_watch_logs())
                    .map(|logs| logs.status().as_str().to_string()),
                s3_logs_status: logs_config
                    .and_then(|logs_config| logs_config.s3_logs())
                    .map(|logs| logs.status().as_str().to_string()),
                timeout_in_minutes: project.timeout_in_minutes(),
                queued_timeout_in_minutes: project.queued_timeout_in_minutes(),
                created: project.created().map(|created| created.as_secs_f64()),
                last_modified: project
                    .last_modified()
                    .map(|last_modified| last_modified.as_secs_f64()),
            },
        )),
        raw: vec![],
    });
}

#[cfg(test)]
mod tests {
    //! # AWS Build Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_core::{Build, BuildOutputs, Platform, ResourceStatus};
    use aws_sdk_codebuild::{
        operation::{
            batch_get_projects::BatchGetProjectsOutput, create_project::CreateProjectOutput,
            delete_project::DeleteProjectOutput, update_project::UpdateProjectOutput,
        },
        types::{
            ArtifactsType, CloudWatchLogsConfig, ComputeType as AwsCodeBuildComputeType,
            EnvironmentType, ImagePullCredentialsType, LogsConfig, LogsConfigStatusType,
            Project as CodeBuildProject, ProjectArtifacts, ProjectEnvironment, ProjectSource,
            S3LogsConfig, SourceType,
        },
        Client,
    };
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use aws_smithy_mocks::{mock, mock_client, RuleMode};
    use rstest::rstest;

    use crate::build::aws::map_compute_type;
    use crate::build::{fixtures::*, AwsBuildController};
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

    fn project_description(project_name: &str, compute_type: Option<String>) -> CodeBuildProject {
        let source = ProjectSource::builder()
            .r#type(SourceType::NoSource)
            .build()
            .expect("source should build");
        let artifacts = ProjectArtifacts::builder()
            .r#type(ArtifactsType::NoArtifacts)
            .build()
            .expect("artifacts should build");
        let environment = ProjectEnvironment::builder()
            .r#type(EnvironmentType::LinuxContainer)
            .image("ghcr.io/alienplatform/alien-builder:latest")
            .compute_type(AwsCodeBuildComputeType::from(
                compute_type.as_deref().unwrap_or("BUILD_GENERAL1_SMALL"),
            ))
            .image_pull_credentials_type(ImagePullCredentialsType::ServiceRole)
            .build()
            .expect("environment should build");
        let cloud_watch_logs = CloudWatchLogsConfig::builder()
            .status(LogsConfigStatusType::Enabled)
            .build()
            .expect("cloudwatch logs should build");
        let s3_logs = S3LogsConfig::builder()
            .status(LogsConfigStatusType::Disabled)
            .build()
            .expect("s3 logs should build");
        let logs_config = LogsConfig::builder()
            .cloud_watch_logs(cloud_watch_logs)
            .s3_logs(s3_logs)
            .build();

        CodeBuildProject::builder()
            .name(project_name)
            .arn(format!(
                "arn:aws:codebuild:us-east-1:123456789012:project/{}",
                project_name
            ))
            .description(format!("Runtime build project: {}", project_name))
            .source(source)
            .artifacts(artifacts)
            .environment(environment)
            .service_role("arn:aws:iam::123456789012:role/codebuild")
            .logs_config(logs_config)
            .build()
    }

    fn setup_mock_service_provider(codebuild_client: Client) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_codebuild_client()
            .returning(move |_| Ok(codebuild_client.clone()));

        Arc::new(mock_provider)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_build())]
    #[case::with_env(build_with_env_vars())]
    #[case::large_compute(build_medium_compute())]
    #[case::custom_image(build_custom_image())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] build: Build) {
        let project_name = format!("test-{}", build.id);
        let create_project_name = project_name.clone();
        let create_project_output_name = project_name.clone();
        let create_compute_type = map_compute_type(&build.compute_type);
        let create_output_compute_type = create_compute_type.clone();
        let create_rule = mock!(Client::create_project)
            .match_requests(move |request| {
                request.name() == Some(create_project_name.as_str())
                    && request
                        .environment()
                        .map(|environment| environment.compute_type().as_str())
                        == Some(create_compute_type.as_str())
                    && request.service_role().is_some()
            })
            .then_output(move || {
                CreateProjectOutput::builder()
                    .project(project_description(
                        &create_project_output_name,
                        Some(create_output_compute_type.clone()),
                    ))
                    .build()
            });
        let get_project_name = project_name.clone();
        let get_project_output_name = project_name.clone();
        let get_rule = mock!(Client::batch_get_projects)
            .match_requests(move |request| {
                request.names().iter().any(|name| name == &get_project_name)
            })
            .then_output(move || {
                BatchGetProjectsOutput::builder()
                    .projects(project_description(&get_project_output_name, None))
                    .build()
            });
        let delete_project_name = project_name.clone();
        let delete_rule = mock!(Client::delete_project)
            .match_requests(move |request| request.name() == Some(delete_project_name.as_str()))
            .then_output(|| DeleteProjectOutput::builder().build());
        let codebuild_client = mock_client!(
            aws_sdk_codebuild,
            RuleMode::Sequential,
            [&create_rule, &get_rule, &delete_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(codebuild_client);

        let mut executor = SingleControllerExecutor::builder()
            .resource(build)
            .controller(AwsBuildController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let build_outputs = outputs.downcast_ref::<BuildOutputs>().unwrap();
        assert!(build_outputs.identifier.contains("arn:aws:codebuild"));

        // Delete the build
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
        assert_eq!(create_rule.num_calls(), 1);
        assert_eq!(get_rule.num_calls(), 1);
        assert_eq!(delete_rule.num_calls(), 1);
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_env(basic_build(), build_with_env_vars())]
    #[case::env_to_complex(build_with_env_vars(), build_medium_compute())]
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_build: Build, #[case] to_build: Build) {
        // Ensure both builds have the same ID for valid updates
        let build_id = "test-update-build".to_string();
        let mut from_build = from_build;
        from_build.id = build_id.clone();

        let mut to_build = to_build;
        to_build.id = build_id.clone();

        let project_name = format!("test-{}", build_id);
        let update_project_name = project_name.clone();
        let update_project_output_name = project_name.clone();
        let update_compute_type = map_compute_type(&to_build.compute_type);
        let update_output_compute_type = update_compute_type.clone();
        let update_rule = mock!(Client::update_project)
            .match_requests(move |request| {
                request.name() == Some(update_project_name.as_str())
                    && request
                        .environment()
                        .map(|environment| environment.compute_type().as_str())
                        == Some(update_compute_type.as_str())
                    && request.service_role().is_some()
            })
            .then_output(move || {
                UpdateProjectOutput::builder()
                    .project(project_description(
                        &update_project_output_name,
                        Some(update_output_compute_type.clone()),
                    ))
                    .build()
            });
        let codebuild_client = mock_client!(
            aws_sdk_codebuild,
            RuleMode::Sequential,
            [&update_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(codebuild_client);

        // Start with the "from" build in Ready state
        let ready_controller = AwsBuildController::mock_ready(&project_name);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_build)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new build
        executor.update(to_build).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(update_rule.num_calls(), 1);
    }
}
