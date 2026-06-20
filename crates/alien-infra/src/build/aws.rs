use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::aws_sdk::{CodeBuildProjectConfig, CodeBuildProjectDescription};
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

        let project_config = build_codebuild_project_config(
            ctx.resource_prefix,
            cfg,
            aws_project_name.clone(),
            role_arn,
            env_vars,
        );

        let response =
            client
                .create_project(project_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create CodeBuild project".to_string(),
                    resource_id: Some(cfg.id.clone()),
                })?;

        self.project_arn = response.arn.clone();
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
            let project =
                client
                    .get_project(project_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to check CodeBuild project status".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

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
            if let Some(actual_compute_type) = &project.compute_type {
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

        let project_config = build_codebuild_project_config(
            ctx.resource_prefix,
            current_config,
            aws_project_name.clone(),
            role_arn,
            env_vars,
        );

        let response =
            client
                .update_project(project_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update CodeBuild project".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })?;

        self.project_arn = response.arn.clone();
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

        match client.get_project(&aws_project_name).await {
            Ok(None) => {
                warn!(name=%aws_project_name, "CodeBuild project was already deleted");
                self.project_arn = None;
                self.project_name = None;
                self.build_env_vars = None;
            }
            Ok(Some(_)) => {
                client.delete_project(&aws_project_name).await.context(
                    ErrorData::CloudPlatformError {
                        message: "Failed to delete CodeBuild project".to_string(),
                        resource_id: Some(build_config.id.clone()),
                    },
                )?;
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

fn build_codebuild_project_config(
    resource_prefix: &str,
    build: &Build,
    project_name: String,
    service_role: String,
    environment_variables: HashMap<String, String>,
) -> CodeBuildProjectConfig {
    CodeBuildProjectConfig {
        name: project_name,
        buildspec: r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build script will be provided at runtime"
"#
        .to_string(),
        environment_type: map_environment_type(&build.compute_type),
        image: "ghcr.io/alienplatform/alien-builder:latest".to_string(),
        compute_type: map_compute_type(&build.compute_type),
        image_pull_credentials_type: "SERVICE_ROLE".to_string(),
        environment_variables: environment_variables.into_iter().collect(),
        service_role,
        description: format!("Runtime build project: {}", build.id),
        tags: standard_resource_tags(resource_prefix, &build.id),
    }
}

fn emit_aws_build_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project: &CodeBuildProjectDescription,
) {
    let project_name = project.name.clone();

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
                source_type: project.source_type.clone(),
                artifacts_type: project.artifacts_type.clone(),
                artifacts_encryption_disabled: project.artifacts_encryption_disabled,
                environment_type: project.environment_type.clone(),
                environment_image: project.environment_image.clone(),
                compute_type: project.compute_type.clone(),
                image_pull_credentials_type: project.image_pull_credentials_type.clone(),
                privileged_mode: project.privileged_mode,
                environment_variable_count: project.environment_variable_count,
                service_role_present: project.service_role_present,
                encryption_key_present: project.encryption_key_present,
                cloud_watch_logs_status: project.cloud_watch_logs_status.clone(),
                s3_logs_status: project.s3_logs_status.clone(),
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

    use std::sync::Arc;

    use crate::aws_sdk::{CodeBuildApi, CodeBuildProjectConfig, CodeBuildProjectDescription};
    use crate::error::Result;
    use alien_core::{Build, BuildOutputs, Platform, ResourceStatus};
    use rstest::rstest;

    use crate::build::{fixtures::*, AwsBuildController};
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

    struct TestCodeBuildClient {
        project_name: String,
    }

    #[async_trait::async_trait]
    impl CodeBuildApi for TestCodeBuildClient {
        async fn create_project(
            &self,
            config: CodeBuildProjectConfig,
        ) -> Result<CodeBuildProjectDescription> {
            Ok(project_description(&config.name, Some(config.compute_type)))
        }

        async fn update_project(
            &self,
            config: CodeBuildProjectConfig,
        ) -> Result<CodeBuildProjectDescription> {
            Ok(project_description(&config.name, Some(config.compute_type)))
        }

        async fn get_project(
            &self,
            _project_name: &str,
        ) -> Result<Option<CodeBuildProjectDescription>> {
            Ok(Some(project_description(&self.project_name, None)))
        }

        async fn delete_project(&self, _project_name: &str) -> Result<()> {
            Ok(())
        }
    }

    fn project_description(
        project_name: &str,
        compute_type: Option<String>,
    ) -> CodeBuildProjectDescription {
        CodeBuildProjectDescription {
            name: project_name.to_string(),
            arn: Some(format!(
                "arn:aws:codebuild:us-east-1:123456789012:project/{}",
                project_name
            )),
            description: Some(format!("Runtime build project: {}", project_name)),
            source_type: Some("NO_SOURCE".to_string()),
            artifacts_type: Some("NO_ARTIFACTS".to_string()),
            artifacts_encryption_disabled: None,
            environment_type: Some("LINUX_CONTAINER".to_string()),
            environment_image: Some("ghcr.io/alienplatform/alien-builder:latest".to_string()),
            compute_type: compute_type.or_else(|| Some("BUILD_GENERAL1_SMALL".to_string())),
            image_pull_credentials_type: Some("SERVICE_ROLE".to_string()),
            privileged_mode: None,
            environment_variable_count: 0,
            service_role_present: true,
            encryption_key_present: false,
            cloud_watch_logs_status: Some("ENABLED".to_string()),
            s3_logs_status: Some("DISABLED".to_string()),
            timeout_in_minutes: None,
            queued_timeout_in_minutes: None,
            created: None,
            last_modified: None,
        }
    }

    fn setup_mock_client(project_name: &str) -> Arc<dyn CodeBuildApi> {
        Arc::new(TestCodeBuildClient {
            project_name: project_name.to_string(),
        })
    }

    fn setup_mock_service_provider(
        mock_codebuild: Arc<dyn CodeBuildApi>,
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
        let mock_codebuild = setup_mock_client(&project_name);
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
        let mock_codebuild = setup_mock_client(&project_name);
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
