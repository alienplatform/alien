use std::{collections::HashMap, time::Duration};
use tracing::{debug, info};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    Build, BuildOutputs, ResourceOutputs, ResourceRef, ResourceStatus, ServiceAccount,
};
use alien_error::{AlienError, Context};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct GcpBuildController {
    /// The GCP project ID where builds run
    pub(crate) project_id: Option<String>,
    /// The GCP project location (region) for builds
    pub(crate) location: Option<String>,
    /// Build configuration ID for outputs
    pub(crate) build_config_id: Option<String>,
    /// The computed environment variables for this build.
    pub(crate) build_env_vars: Option<HashMap<String, String>>,
    /// The service account email for Cloud Build operations
    pub(crate) service_account: Option<String>,
}

#[controller]
impl GcpBuildController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let cfg = ctx.desired_resource_config::<Build>()?;

        info!(name=%cfg.id, "Initializing GCP Cloud Build configuration");

        // Cloud Build doesn't need advance project creation like CodeBuild
        // We just store the configuration, project ID, and the location
        self.project_id = Some(gcp_config.project_id.clone());
        self.location = Some(gcp_config.region.clone());
        self.build_config_id = Some(cfg.id.clone());

        // Get service account from the linked ServiceAccount
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account = self.get_service_account(ctx, &service_account_ref).await?;
        self.service_account = Some(service_account);

        // Prepare and store environment variables for the build
        let env_vars = EnvironmentVariableBuilder::new(&cfg.environment)
            .add_standard_alien_env_vars(ctx)
            .add_linked_resources(&cfg.links, ctx, &cfg.id)
            .await?
            .build();

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars);

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx).await?;

        info!(name=%cfg.id, location=%gcp_config.region, "GCP Cloud Build configuration ready");

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
        let config = ctx.desired_resource_config::<Build>()?;

        // Heartbeat check: GCP Cloud Build doesn't create persistent projects like AWS CodeBuild.
        // Builds are created on-demand, so heartbeat is essentially a no-op.
        // We just verify our basic configuration is still valid.
        if self.location.is_none() || self.build_config_id.is_none() {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "GCP Cloud Build configuration is missing".to_string(),
            }));
        }

        debug!(name = %config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)), // Check again in 60 seconds
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
        let current_config = ctx.desired_resource_config::<Build>()?;

        info!(name=%current_config.id, "Updating GCP Cloud Build configuration");

        // For Cloud Build, we don't need to pre-create projects
        // All configuration is applied when builds are triggered
        // So we just update our stored configuration
        self.build_config_id = Some(current_config.id.clone());

        // Get service account from the linked ServiceAccount
        let service_account_id = format!("{}-sa", current_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account = self.get_service_account(ctx, &service_account_ref).await?;
        self.service_account = Some(service_account);

        // Prepare and store environment variables for the build
        let env_vars = EnvironmentVariableBuilder::new(&current_config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_linked_resources(&current_config.links, ctx, &current_config.id)
            .await?
            .build();

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars);

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx).await?;

        info!(name=%current_config.id, "GCP Cloud Build configuration updated");

        Ok(HandlerAction::Continue {
            state: Ready,
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
        let build_config = ctx.desired_resource_config::<Build>()?;

        info!(name=%build_config.id, "Deleting GCP Cloud Build configuration");

        // For Cloud Build, there's no persistent project to delete
        // We just clear our configuration
        self.location = None;
        self.build_config_id = None;
        self.build_env_vars = None;

        info!(name=%build_config.id, "GCP Cloud Build configuration deleted");

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
        self.build_config_id.as_ref().map(|id| {
            ResourceOutputs::new(BuildOutputs {
                identifier: id.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, BuildBinding};

        if let (Some(build_env_vars), Some(service_account)) =
            (&self.build_env_vars, &self.service_account)
        {
            let binding =
                BuildBinding::cloudbuild(build_env_vars.clone(), service_account.clone(), None);

            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl GcpBuildController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(build_id: &str) -> Self {
        Self {
            state: GcpBuildState::Ready,
            project_id: Some("test-project-id".to_string()),
            location: Some("us-central1".to_string()),
            build_config_id: Some(build_id.to_string()),
            build_env_vars: Some(HashMap::new()),
            service_account: Some(
                "test-build-sa@test-project-id.iam.gserviceaccount.com".to_string(),
            ),
            _internal_stay_count: None,
        }
    }

    /// Extracts the service account email from a linked ServiceAccount
    async fn get_service_account(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_account_ref: &ResourceRef,
    ) -> Result<String> {
        use crate::service_account::GcpServiceAccountController;

        // Ensure it's the correct type
        if service_account_ref.resource_type() != &ServiceAccount::RESOURCE_TYPE {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Expected ServiceAccount reference, but found {:?}",
                    service_account_ref.resource_type()
                ),
                resource_id: Some(service_account_ref.id().to_string()),
            }));
        }

        // Get the ServiceAccount's internal state
        let service_account_controller =
            ctx.require_dependency::<GcpServiceAccountController>(service_account_ref)?;

        // Get the service account email from the resolved state
        service_account_controller
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: ctx.desired_config.id().to_string(),
                    dependency_id: service_account_ref.id().to_string(),
                })
            })
            .map(|s| s.to_string())
    }

    /// Applies resource-scoped permissions to the build configuration from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        use alien_permissions::{generators::GcpRuntimePermissionsGenerator, PermissionContext};

        let config = ctx.desired_resource_config::<Build>()?;
        let gcp_config = ctx.get_gcp_config()?;

        // Build permission context for this specific build resource
        let permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(config.id.clone());

        let generator = GcpRuntimePermissionsGenerator::new();

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            if let Some(permission_set_refs) = profile.0.get(&config.id) {
                info!(
                    build_id = %config.id,
                    profile = %profile_name,
                    permission_sets = ?permission_set_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing resource-scoped permissions for build"
                );

                self.process_profile_permissions(
                    ctx,
                    profile_name,
                    permission_set_refs,
                    &generator,
                    &permission_context,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to process permissions for profile '{}' on build '{}'",
                        profile_name, config.id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        Ok(())
    }

    /// Process permissions for a specific profile (logging only for now)
    async fn process_profile_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &alien_permissions::generators::GcpRuntimePermissionsGenerator,
        permission_context: &alien_permissions::PermissionContext,
    ) -> Result<()> {
        use alien_permissions::BindingTarget;

        // Get the service account email for this profile
        let service_account_email =
            self.get_service_account_email_for_profile(ctx, profile_name)?;

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            // Generate IAM bindings for resource-scoped permissions
            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            info!(
                profile = %profile_name,
                service_account = %service_account_email,
                permission_set = %permission_set.id,
                bindings_count = bindings_result.bindings.len(),
                "Generated IAM bindings for build permissions (project-level application required)"
            );

            // Note: For GCP Cloud Build, permissions are typically applied at the project level
            // with conditions to restrict access to specific build triggers and builds.
            // This would require project-level IAM policy management which is more complex
            // and typically handled by the project setup rather than individual resources.
            // For now, we log the intention but don't apply the bindings directly.
        }

        Ok(())
    }

    /// Get the service account email for a permission profile
    fn get_service_account_email_for_profile(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "build".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }
}

#[cfg(test)]
mod tests {
    //! # GCP Build Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{Build, BuildOutputs, Platform, ResourceStatus};
    use alien_error::AlienError;
    use alien_gcp_clients::cloudbuild::{
        Build as CloudBuild, BuildStep, CloudBuildApi, MockCloudBuildApi,
    };
    use alien_gcp_clients::longrunning::Operation;
    use rstest::rstest;

    use crate::build::{fixtures::*, GcpBuildController};
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };

    fn setup_mock_service_provider() -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        // For GCP Cloud Build, we don't need to mock any client calls during the controller lifecycle
        // since builds are created on-demand, not as persistent projects
        mock_provider
            .expect_get_gcp_cloudbuild_client()
            .returning(|_| {
                let mock_client = MockCloudBuildApi::new();
                Ok(Arc::new(mock_client))
            });

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
        let mock_provider = setup_mock_service_provider();

        let mut executor = SingleControllerExecutor::builder()
            .resource(build)
            .controller(GcpBuildController::default())
            .platform(Platform::Gcp)
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
        assert!(!build_outputs.identifier.is_empty());

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

        let mock_provider = setup_mock_service_provider();

        // Start with the "from" build in Ready state
        let ready_controller = GcpBuildController::mock_ready(&build_id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_build)
            .controller(ready_controller)
            .platform(Platform::Gcp)
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
