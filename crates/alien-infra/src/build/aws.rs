use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::{
    aws_ownership::is_tagged_as_ours, EnvironmentVariableBuilder, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_aws_clients::codebuild::{
    BatchGetProjectsRequest, CloudWatchLogsConfig, CodeBuildApi, CreateProjectRequest,
    DeleteProjectRequest, EnvironmentVariable, LogsConfig, Project, ProjectArtifacts,
    ProjectEnvironment, ProjectSource, S3LogsConfig, Tag,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsCodeBuildHeartbeatData, Build, BuildHeartbeatData,
    BuildHeartbeatStatus, BuildOutputs, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use chrono::Utc;

/// Generates the full, prefixed AWS CodeBuild project name.
fn get_aws_project_name(prefix: &str, name: &str) -> String {
    aws_prefixed_resource_name(prefix, name)
}

/// What holds a CodeBuild project name, as far as this deployment is concerned.
enum ExistingProject {
    Missing,
    /// Carries this deployment's tags for the resource.
    Ours {
        arn: Option<String>,
    },
    /// Exists without them: someone else made it.
    Foreign,
}

/// Finds the project named `project_name` and tells whether this deployment created it. The
/// name is derived from the deployment prefix and resource ID, so the account can already hold
/// a project with it that someone else made; only our tags show it is ours.
async fn inspect_project(
    client: &dyn CodeBuildApi,
    project_name: &str,
    our_tags: &HashMap<String, String>,
    resource_id: &str,
) -> Result<ExistingProject> {
    let response = client
        .batch_get_projects(
            BatchGetProjectsRequest::builder()
                .names(vec![project_name.to_string()])
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read CodeBuild project '{}'", project_name),
            resource_id: Some(resource_id.to_string()),
        })?;
    let Some(project) = response
        .projects
        .unwrap_or_default()
        .into_iter()
        .find(|project| project.name.as_deref() == Some(project_name))
    else {
        return Ok(ExistingProject::Missing);
    };
    let tags: HashMap<String, String> = project
        .tags
        .unwrap_or_default()
        .into_iter()
        .map(|tag| (tag.key, tag.value))
        .collect();
    if is_tagged_as_ours(&tags, our_tags) {
        Ok(ExistingProject::Ours { arn: project.arn })
    } else {
        Ok(ExistingProject::Foreign)
    }
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
    /// The AWS CodeBuild project name (stack prefix + id). Set only once this controller
    /// created the project, or found it carrying this deployment's tags; deletion acts on this
    /// project only.
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

        // Create generic buildspec - actual script will be provided at runtime via bindings
        let buildspec = r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build script will be provided at runtime"
"#
        .to_string();

        // Create project source configuration
        let source = ProjectSource::builder()
            .r#type("NO_SOURCE".to_string())
            .buildspec(buildspec)
            .build();

        // Create project artifacts configuration (no artifacts needed)
        let artifacts = ProjectArtifacts::builder()
            .r#type("NO_ARTIFACTS".to_string())
            .build();

        // Convert environment variables
        let environment_variables: Vec<EnvironmentVariable> = env_vars
            .into_iter()
            .map(|(name, value)| {
                EnvironmentVariable::builder()
                    .name(name)
                    .value(value)
                    .build()
            })
            .collect();

        // Create project environment configuration
        let environment = ProjectEnvironment::builder()
            .r#type(map_environment_type(&cfg.compute_type))
            .image("ghcr.io/alienplatform/alien-builder:latest".to_string())
            .compute_type(map_compute_type(&cfg.compute_type))
            .image_pull_credentials_type("SERVICE_ROLE".to_string())
            .environment_variables(environment_variables)
            .build();

        let request = CreateProjectRequest::builder()
            .name(aws_project_name.clone())
            .source(source)
            .artifacts(artifacts)
            .environment(environment)
            .logs_config(LogsConfig {
                cloud_watch_logs: Some(CloudWatchLogsConfig {
                    status: "ENABLED".to_string(),
                    group_name: None,
                    stream_name: None,
                }),
                s3_logs: Some(S3LogsConfig {
                    status: "DISABLED".to_string(),
                    location: None,
                    encryption_disabled: None,
                    bucket_owner_access: None,
                }),
            })
            .service_role(role_arn)
            .description(format!("Runtime build project: {}", cfg.id))
            .tags(
                standard_resource_tags(ctx.resource_prefix, &cfg.id)
                    .into_iter()
                    .map(|(key, value)| Tag::builder().key(key).value(value).build())
                    .collect(),
            )
            .build();

        match client.create_project(request).await {
            Ok(response) => {
                self.project_arn = response.project.arn;
                self.project_name = Some(aws_project_name.clone());
                info!(name=%aws_project_name, arn=%self.project_arn.as_deref().unwrap_or("unknown"), "CodeBuild project created successfully");
            }
            // A project with the name already exists. It is ours only if it carries our tags:
            // an earlier attempt created it and its response was lost. Never adopt another.
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                let our_tags = standard_resource_tags(ctx.resource_prefix, &cfg.id);
                match inspect_project(&*client, &aws_project_name, &our_tags, &cfg.id).await? {
                    ExistingProject::Ours { arn } => {
                        info!(name=%aws_project_name, "CodeBuild project was created by an earlier attempt; resuming");
                        self.project_arn = arn;
                        self.project_name = Some(aws_project_name.clone());
                    }
                    ExistingProject::Foreign => {
                        return Err(AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
                            resource_id: cfg.id.clone(),
                            object: format!("CodeBuild project '{}'", aws_project_name),
                        }));
                    }
                    // Deleted since CreateProject answered; the retry creates it.
                    ExistingProject::Missing => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: "Failed to create CodeBuild project".to_string(),
                            resource_id: Some(cfg.id.clone()),
                        }));
                    }
                }
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create CodeBuild project".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        }

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
            let request = BatchGetProjectsRequest::builder()
                .names(vec![project_name.clone()])
                .build();

            let result = client.batch_get_projects(request).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to check CodeBuild project status".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if result.projects.as_ref().map_or(true, |p| p.is_empty()) {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: "CodeBuild project no longer exists".to_string(),
                }));
            }

            let project = &result.projects.as_ref().unwrap()[0];

            // Check basic drift detection - compare compute type
            let expected_compute_type = map_compute_type(&config.compute_type);
            if let Some(environment) = &project.environment {
                let actual_compute_type = &environment.compute_type;
                if actual_compute_type != &expected_compute_type {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "CodeBuild project compute type has drifted from '{}' to '{}'",
                            expected_compute_type, actual_compute_type
                        ),
                    }));
                }
            }

            emit_aws_build_heartbeat(ctx, &config.id, project);
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

        // Create updated generic buildspec - actual script will be provided at runtime via bindings
        let buildspec = r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build script will be provided at runtime"
"#
        .to_string();

        // Create updated project source
        let source = ProjectSource::builder()
            .r#type("NO_SOURCE".to_string())
            .buildspec(buildspec)
            .build();

        // Convert environment variables
        let environment_variables: Vec<EnvironmentVariable> = env_vars
            .into_iter()
            .map(|(name, value)| {
                EnvironmentVariable::builder()
                    .name(name)
                    .value(value)
                    .build()
            })
            .collect();

        // Create updated environment
        let environment = ProjectEnvironment::builder()
            .r#type(map_environment_type(&current_config.compute_type))
            .image("ghcr.io/alienplatform/alien-builder:latest".to_string())
            .compute_type(map_compute_type(&current_config.compute_type))
            .image_pull_credentials_type("SERVICE_ROLE".to_string())
            .environment_variables(environment_variables)
            .build();

        // Create updated artifacts
        let artifacts = ProjectArtifacts::builder()
            .r#type("NO_ARTIFACTS".to_string())
            .build();

        let update_request = alien_aws_clients::codebuild::UpdateProjectRequest::builder()
            .name(aws_project_name.clone())
            .source(source)
            .artifacts(artifacts)
            .environment(environment)
            .logs_config(LogsConfig {
                cloud_watch_logs: Some(CloudWatchLogsConfig {
                    status: "ENABLED".to_string(),
                    group_name: None,
                    stream_name: None,
                }),
                s3_logs: Some(S3LogsConfig {
                    status: "DISABLED".to_string(),
                    location: None,
                    encryption_disabled: None,
                    bucket_owner_access: None,
                }),
            })
            .service_role(role_arn)
            .description(format!("Runtime build project: {}", current_config.id))
            .build();

        client
            .update_project(update_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update CodeBuild project".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

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

        // A project is recorded only once CreateProject succeeded, in this and every earlier
        // version. Without one, the project with the derived name is deleted only if it carries
        // our tags (a create whose response was lost); otherwise someone else made it.
        let aws_project_name = match &self.project_name {
            Some(name) => name.clone(),
            None => {
                let name = get_aws_project_name(ctx.resource_prefix, &build_config.id);
                let our_tags = standard_resource_tags(ctx.resource_prefix, &build_config.id);
                match inspect_project(&*client, &name, &our_tags, &build_config.id).await? {
                    ExistingProject::Ours { .. } => name,
                    ExistingProject::Missing => {
                        info!(resource_id=%build_config.id, "No CodeBuild project to delete");
                        return Ok(self.finish_delete());
                    }
                    ExistingProject::Foreign => {
                        warn!(
                            resource_id=%build_config.id,
                            name=%name,
                            "Keeping CodeBuild project: it is not tagged as created by this deployment"
                        );
                        return Ok(self.finish_delete());
                    }
                }
            }
        };

        info!(name=%aws_project_name, "Deleting CodeBuild project");

        let request = DeleteProjectRequest::builder()
            .name(aws_project_name.clone())
            .build();

        match client.delete_project(request).await {
            Ok(_) => {
                info!(name=%aws_project_name, "CodeBuild project deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                warn!(name=%aws_project_name, "CodeBuild project was already deleted");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete CodeBuild project".to_string(),
                    resource_id: Some(build_config.id.clone()),
                }));
            }
        }

        Ok(self.finish_delete())
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

    fn finish_delete(&mut self) -> AwsBuildHandlerAction {
        self.project_arn = None;
        self.project_name = None;
        self.build_env_vars = None;
        AwsBuildHandlerAction::Continue {
            state: AwsBuildState::Deleted,
            suggested_delay: None,
        }
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

fn emit_aws_build_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project: &Project,
) {
    let environment = project.environment.as_ref();
    let artifacts = project.artifacts.as_ref();
    let source = project.source.as_ref();
    let logs_config = project.logs_config.as_ref();
    let project_name = project
        .name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());

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
                project_arn: project.arn.clone(),
                description: project.description.clone(),
                source_type: source.map(|source| source.r#type.clone()),
                artifacts_type: artifacts.map(|artifacts| artifacts.r#type.clone()),
                artifacts_encryption_disabled: artifacts
                    .and_then(|artifacts| artifacts.encryption_disabled),
                environment_type: environment.map(|environment| environment.r#type.clone()),
                environment_image: environment.map(|environment| environment.image.clone()),
                compute_type: environment.map(|environment| environment.compute_type.clone()),
                image_pull_credentials_type: environment
                    .and_then(|environment| environment.image_pull_credentials_type.clone()),
                privileged_mode: environment.and_then(|environment| environment.privileged_mode),
                environment_variable_count: environment
                    .and_then(|environment| environment.environment_variables.as_ref())
                    .map(|environment_variables| environment_variables.len() as u32)
                    .unwrap_or(0),
                service_role_present: project.service_role.is_some(),
                encryption_key_present: project.encryption_key.is_some(),
                cloud_watch_logs_status: logs_config
                    .and_then(|logs_config| logs_config.cloud_watch_logs.as_ref())
                    .map(|logs| logs.status.clone()),
                s3_logs_status: logs_config
                    .and_then(|logs_config| logs_config.s3_logs.as_ref())
                    .map(|logs| logs.status.clone()),
                timeout_in_minutes: project.timeout_in_minutes,
                queued_timeout_in_minutes: project.queued_timeout_in_minutes,
                created: project.created,
                last_modified: project.last_modified,
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

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::codebuild::{
        BatchGetProjectsResponse, CreateProjectRequest, CreateProjectResponse,
        DeleteProjectResponse, MockCodeBuildApi, Project, Tag, UpdateProjectRequest,
        UpdateProjectResponse,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{standard_resource_tags, Build, BuildOutputs, Platform, ResourceStatus};
    use alien_error::AlienError;
    use rstest::rstest;

    use crate::build::{fixtures::*, AwsBuildController, AwsBuildState};
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };

    fn create_successful_project_response(project_name: &str) -> CreateProjectResponse {
        CreateProjectResponse {
            project: Project::builder()
                .name(project_name.to_string())
                .arn(format!(
                    "arn:aws:codebuild:us-east-1:123456789012:project/{}",
                    project_name
                ))
                .description(format!("Runtime build project: {}", project_name))
                .build(),
        }
    }

    fn create_successful_update_response(project_name: &str) -> UpdateProjectResponse {
        UpdateProjectResponse {
            project: Project::builder()
                .name(project_name.to_string())
                .arn(format!(
                    "arn:aws:codebuild:us-east-1:123456789012:project/{}",
                    project_name
                ))
                .description(format!("Runtime build project: {}", project_name))
                .build(),
        }
    }

    fn setup_mock_client_for_creation_and_deletion(project_name: &str) -> Arc<MockCodeBuildApi> {
        let mut mock_codebuild = MockCodeBuildApi::new();

        // Mock successful project creation
        let project_name = project_name.to_string();
        let project_name_for_create = project_name.clone();
        mock_codebuild
            .expect_create_project()
            .returning(move |_| Ok(create_successful_project_response(&project_name_for_create)));

        // Mock successful project deletion
        mock_codebuild
            .expect_delete_project()
            .returning(|_| Ok(alien_aws_clients::codebuild::DeleteProjectResponse {}));

        Arc::new(mock_codebuild)
    }

    fn setup_mock_client_for_creation_and_update(project_name: &str) -> Arc<MockCodeBuildApi> {
        let mut mock_codebuild = MockCodeBuildApi::new();

        // Mock successful project creation
        let project_name = project_name.to_string();
        let project_name_for_create = project_name.clone();
        mock_codebuild
            .expect_create_project()
            .returning(move |_| Ok(create_successful_project_response(&project_name_for_create)));

        // Mock successful project update
        let project_name_for_update = project_name.clone();
        mock_codebuild
            .expect_update_project()
            .returning(move |_| Ok(create_successful_update_response(&project_name_for_update)));

        Arc::new(mock_codebuild)
    }

    fn setup_mock_service_provider(
        mock_codebuild: Arc<MockCodeBuildApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_codebuild_client()
            .returning(move |_| Ok(mock_codebuild.clone()));

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
        let mock_codebuild = setup_mock_client_for_creation_and_deletion(&project_name);
        let mock_provider = setup_mock_service_provider(mock_codebuild);

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
        let mock_codebuild = setup_mock_client_for_creation_and_update(&project_name);
        let mock_provider = setup_mock_service_provider(mock_codebuild);

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
    }

    // ─────────────── OWNERSHIP TESTS ──────────────────────────────────

    /// The CodeBuild projects in the account: name to tags.
    type FakeProjects = Arc<Mutex<HashMap<String, HashMap<String, String>>>>;

    fn project_arn(name: &str) -> String {
        format!("arn:aws:codebuild:us-east-1:123456789012:project/{name}")
    }

    /// A CodeBuild mock that keeps the account's projects and their tags. CreateProject fails
    /// with ResourceAlreadyExistsException on a taken name, like AWS. With `lose_create_response`
    /// the first CreateProject creates the project but its response is lost.
    fn fake_codebuild(projects: FakeProjects, lose_create_response: bool) -> MockCodeBuildApi {
        let mut mock = MockCodeBuildApi::new();

        let state = projects.clone();
        let lose = Arc::new(Mutex::new(lose_create_response));
        mock.expect_create_project().returning(move |request| {
            let mut projects = state.lock().unwrap();
            if projects.contains_key(&request.name) {
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: format!("Project already exists: {}", request.name),
                        resource_type: "Project".to_string(),
                        resource_name: request.name.clone(),
                    },
                ));
            }
            let tags = request
                .tags
                .unwrap_or_default()
                .into_iter()
                .map(|tag| (tag.key, tag.value))
                .collect();
            projects.insert(request.name.clone(), tags);
            if std::mem::take(&mut *lose.lock().unwrap()) {
                return Err(AlienError::new(CloudClientErrorData::HttpRequestFailed {
                    message: "connection reset before the response arrived".to_string(),
                }));
            }
            Ok(create_successful_project_response(&request.name))
        });

        let state = projects.clone();
        mock.expect_batch_get_projects().returning(move |request| {
            let projects = state.lock().unwrap();
            let (found, not_found): (Vec<_>, Vec<_>) = request
                .names
                .iter()
                .partition(|name| projects.contains_key(*name));
            Ok(BatchGetProjectsResponse {
                projects: Some(
                    found
                        .into_iter()
                        .map(|name| {
                            Project::builder()
                                .name(name.clone())
                                .arn(project_arn(name))
                                .tags(
                                    projects[name]
                                        .iter()
                                        .map(|(key, value)| Tag {
                                            key: key.clone(),
                                            value: value.clone(),
                                        })
                                        .collect(),
                                )
                                .build()
                        })
                        .collect(),
                ),
                projects_not_found: Some(not_found.into_iter().cloned().collect()),
            })
        });

        let state = projects.clone();
        mock.expect_delete_project().returning(move |request| {
            // DeleteProject succeeds for a name with no project, like AWS.
            state.lock().unwrap().remove(&request.name);
            Ok(DeleteProjectResponse {})
        });

        mock
    }

    fn projects_with(existing: &[(&str, HashMap<String, String>)]) -> FakeProjects {
        Arc::new(Mutex::new(
            existing
                .iter()
                .map(|(name, tags)| (name.to_string(), tags.clone()))
                .collect(),
        ))
    }

    /// Tags of a project someone made by hand in the same account.
    fn handmade_tags() -> HashMap<String, String> {
        HashMap::from([("owner".to_string(), "handmade".to_string())])
    }

    async fn executor_with(
        build: Build,
        controller: AwsBuildController,
        projects: &FakeProjects,
        lose_create_response: bool,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(build)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(fake_codebuild(
                projects.clone(),
                lose_create_response,
            ))))
            .with_test_dependencies()
            .build()
            .await
            .unwrap()
    }

    /// Normal create and delete: the project this deployment created is tagged, then deleted.
    #[tokio::test]
    async fn create_then_delete_removes_our_project() {
        let projects = projects_with(&[]);
        let mut executor = executor_with(
            basic_build(),
            AwsBuildController::default(),
            &projects,
            false,
        )
        .await;

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            projects.lock().unwrap().get("test-basic-build"),
            Some(&standard_resource_tags("test", "basic-build"))
        );

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(projects.lock().unwrap().is_empty());
    }

    /// A same-account project that already has the build's name was made by someone else.
    /// Creating refuses it, and removing the resource afterwards leaves it in place.
    #[rstest]
    #[case::other_tags(handmade_tags())]
    #[case::no_tags(HashMap::new())]
    #[case::other_deployment(standard_resource_tags("other", "basic-build"))]
    #[tokio::test]
    async fn existing_project_without_our_tags_is_never_adopted_or_deleted(
        #[case] tags: HashMap<String, String>,
    ) {
        let projects = projects_with(&[("test-basic-build", tags.clone())]);
        let mut executor = executor_with(
            basic_build(),
            AwsBuildController::default(),
            &projects,
            false,
        )
        .await;

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a project this deployment did not create is refused");
        assert!(
            matches!(
                &error.error,
                Some(crate::error::ErrorData::ExistingCloudResourceNotOwned { resource_id, object })
                    if resource_id == "basic-build"
                        && object == "CodeBuild project 'test-basic-build'"
            ),
            "{error:?}"
        );
        assert!(!error.retryable, "retrying cannot make the project ours");
        let controller = executor.internal_state::<AwsBuildController>().unwrap();
        assert_eq!(controller.project_name, None);
        assert!(executor.outputs().is_none());

        // The release that drops the resource deletes nothing.
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *projects.lock().unwrap(),
            HashMap::from([("test-basic-build".to_string(), tags)])
        );
    }

    /// State written by earlier versions after a create that failed on the name: nothing
    /// recorded. Deleting it must neither get stuck nor delete the foreign project.
    #[tokio::test]
    async fn delete_after_a_failed_create_keeps_a_foreign_project() {
        let projects = projects_with(&[("test-basic-build", handmade_tags())]);
        let controller = AwsBuildController {
            state: AwsBuildState::CreateFailed,
            project_arn: None,
            project_name: None,
            build_env_vars: Some(HashMap::new()),
            _internal_stay_count: None,
        };
        let mut executor = executor_with(basic_build(), controller, &projects, false).await;

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            projects.lock().unwrap().get("test-basic-build"),
            Some(&handmade_tags())
        );
    }

    /// A create whose response was lost left our tagged project unrecorded. A retry resumes
    /// it; a delete instead removes it.
    #[tokio::test]
    async fn retry_after_a_lost_create_response_resumes_our_project() {
        let projects = projects_with(&[]);
        let mut executor = executor_with(
            basic_build(),
            AwsBuildController::default(),
            &projects,
            true,
        )
        .await;

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the first attempt loses its response");
        assert!(error.retryable, "{error:?}");
        assert!(projects.lock().unwrap().contains_key("test-basic-build"));

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let outputs = executor.outputs().unwrap();
        assert_eq!(
            outputs.downcast_ref::<BuildOutputs>().unwrap().identifier,
            project_arn("test-basic-build")
        );
    }

    #[tokio::test]
    async fn delete_after_an_interrupted_create_removes_our_tagged_project() {
        let projects = projects_with(&[(
            "test-basic-build",
            standard_resource_tags("test", "basic-build"),
        )]);
        let controller = AwsBuildController {
            state: AwsBuildState::CreateFailed,
            project_arn: None,
            project_name: None,
            build_env_vars: None,
            _internal_stay_count: None,
        };
        let mut executor = executor_with(basic_build(), controller, &projects, false).await;

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(projects.lock().unwrap().is_empty());
    }

    /// Deletion acts on the project the controller recorded (for example, by an import), not
    /// on a name recomputed from the prefix.
    #[tokio::test]
    async fn delete_removes_the_recorded_project_only() {
        let projects = projects_with(&[
            ("imported-project", HashMap::new()),
            ("test-basic-build", handmade_tags()),
        ]);
        let mut executor = executor_with(
            basic_build(),
            AwsBuildController::mock_ready("imported-project"),
            &projects,
            false,
        )
        .await;

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *projects.lock().unwrap(),
            HashMap::from([("test-basic-build".to_string(), handmade_tags())])
        );
    }
}
