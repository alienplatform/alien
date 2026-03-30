use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_aws_clients::codebuild::{
    BatchGetProjectsRequest, CloudWatchLogsConfig, CreateProjectRequest, DeleteProjectRequest,
    EnvironmentVariable, LogsConfig, ProjectArtifacts, ProjectEnvironment, ProjectSource,
    S3LogsConfig,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Build, BuildOutputs, ResourceOutputs, ResourceRef, ResourceStatus};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

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
            .description(format!("Alien build project: {}", cfg.id))
            .build();

        let response =
            client
                .create_project(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create CodeBuild project".to_string(),
                    resource_id: Some(cfg.id.clone()),
                })?;

        self.project_arn = response.project.arn.clone();
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
            .description(format!("Alien build project: {}", current_config.id))
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
        let aws_project_name = get_aws_project_name(ctx.resource_prefix, &build_config.id);

        info!(name=%aws_project_name, "Deleting CodeBuild project");

        let request = DeleteProjectRequest::builder()
            .name(aws_project_name.clone())
            .build();

        match client.delete_project(request).await {
            Ok(_) => {
                info!(name=%aws_project_name, "CodeBuild project deleted successfully");
                self.project_arn = None;
                self.project_name = None;
                self.build_env_vars = None;
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                warn!(name=%aws_project_name, "CodeBuild project was already deleted");
                self.project_arn = None;
                self.project_name = None;
                self.build_env_vars = None;
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete CodeBuild project".to_string(),
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
        EnvironmentVariableBuilder::new(initial_env)
            .add_standard_alien_env_vars(ctx)
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

#[cfg(test)]
mod tests {
    //! # AWS Build Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_aws_clients::codebuild::{
        CreateProjectRequest, CreateProjectResponse, MockCodeBuildApi, Project,
        UpdateProjectRequest, UpdateProjectResponse,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{Build, BuildOutputs, Platform, ResourceStatus};
    use alien_error::AlienError;
    use rstest::rstest;

    use crate::build::{fixtures::*, AwsBuildController};
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
                .description(format!("Alien build project: {}", project_name))
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
                .description(format!("Alien build project: {}", project_name))
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
}
