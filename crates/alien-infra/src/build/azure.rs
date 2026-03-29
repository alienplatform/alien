use std::{collections::HashMap, time::Duration};
use tracing::{debug, info};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{Build, BuildOutputs, ResourceOutputs, ResourceRef, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct AzureBuildController {
    /// The managed environment ID for Container Apps.
    /// This is used by alien-bindings to create actual jobs.
    pub(crate) managed_environment_id: Option<String>,
    /// The resource group name where the Container Apps environment is located.
    pub(crate) resource_group_name: Option<String>,
    /// The computed environment variables for this build.
    pub(crate) build_env_vars: Option<HashMap<String, String>>,
    /// The managed identity ID for authentication
    pub(crate) managed_identity_id: Option<String>,
    /// The resource prefix for generating unique job names
    pub(crate) resource_prefix: Option<String>,
}

#[controller]
impl AzureBuildController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Build>()?;

        info!(name=%cfg.id, "Setting up Azure Build environment configuration");

        // Store the resource prefix for use in binding parameters
        self.resource_prefix = Some(ctx.resource_prefix.to_string());

        // Get the managed environment ID from the stack state
        let managed_environment_id = self.get_managed_environment_id(ctx)?;
        self.managed_environment_id = Some(managed_environment_id.clone());

        // Get and store the resource group name
        let resource_group_name = self.get_resource_group_name(ctx)?;
        self.resource_group_name = Some(resource_group_name);

        // Get managed identity ID from the linked ServiceAccount
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let managed_identity_id = self
            .get_managed_identity_id(ctx, &service_account_ref)
            .await?;
        self.managed_identity_id = Some(managed_identity_id);

        // Prepare and store environment variables for the build
        let env_vars = EnvironmentVariableBuilder::new(&cfg.environment)
            .add_standard_alien_env_vars(ctx)
            .add_linked_resources(&cfg.links, ctx, &cfg.id)
            .await?
            .build();

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars);

        info!(name=%cfg.id, environment_id=%managed_environment_id, "Azure Build environment configured");

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

        info!(name = %config.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx, &config.id)
            .await?;

        info!(name = %config.id, "Successfully applied resource-scoped permissions");

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

        // Heartbeat check: Azure Build doesn't create persistent jobs like AWS CodeBuild.
        // Jobs are created on-demand, so heartbeat is essentially a no-op.
        // We just verify our managed environment configuration is still valid.
        if self.managed_environment_id.is_none() || self.managed_identity_id.is_none() {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Azure Build configuration is missing managed environment or identity"
                    .to_string(),
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

        info!(name=%current_config.id, "Updating Azure Build environment configuration");

        // Store the resource prefix for use in binding parameters
        self.resource_prefix = Some(ctx.resource_prefix.to_string());

        // Refresh the managed environment ID in case it changed
        let managed_environment_id = self.get_managed_environment_id(ctx)?;
        self.managed_environment_id = Some(managed_environment_id.clone());

        // Get and store the resource group name
        let resource_group_name = self.get_resource_group_name(ctx)?;
        self.resource_group_name = Some(resource_group_name);

        // Get managed identity ID from the linked ServiceAccount
        let service_account_id = format!("{}-sa", current_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let managed_identity_id = self
            .get_managed_identity_id(ctx, &service_account_ref)
            .await?;
        self.managed_identity_id = Some(managed_identity_id);

        // Prepare and store environment variables for the build
        let env_vars = EnvironmentVariableBuilder::new(&current_config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_linked_resources(&current_config.links, ctx, &current_config.id)
            .await?
            .build();

        // Store the computed environment variables in controller state
        self.build_env_vars = Some(env_vars);

        info!(name=%current_config.id, environment_id=%managed_environment_id, "Azure Build environment updated");

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

        info!(name=%build_config.id, "Cleaning up Azure Build environment configuration");

        // No actual resources to delete since we don't create jobs in alien-infra
        self.managed_environment_id = None;
        self.resource_group_name = None;
        self.build_env_vars = None;
        self.managed_identity_id = None;

        info!(name=%build_config.id, "Azure Build environment cleanup completed");

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
        self.managed_environment_id.as_ref().map(|id| {
            ResourceOutputs::new(BuildOutputs {
                identifier: id.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, BuildBinding};

        if let (
            Some(managed_environment_id),
            Some(resource_group_name),
            Some(build_env_vars),
            Some(managed_identity_id),
            Some(resource_prefix),
        ) = (
            &self.managed_environment_id,
            &self.resource_group_name,
            &self.build_env_vars,
            &self.managed_identity_id,
            &self.resource_prefix,
        ) {
            let binding = BuildBinding::aca(
                managed_environment_id.clone(),
                resource_group_name.clone(),
                build_env_vars.clone(),
                Some(managed_identity_id.clone()),
                resource_prefix.clone(),
                None,
            );

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

// Separate impl block for helper methods
impl AzureBuildController {
    // ─────────────── HELPER METHODS ────────────────────────────

    fn get_resource_group_name(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        // In Azure, we need to get the resource group name from the stack state
        // This should be available from the Azure infrastructure requirements
        match crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state) {
            Ok(name) => Ok(name),
            Err(_) => {
                // Fallback to a constructed name if not found
                Ok(format!("{}-rg", ctx.resource_prefix))
            }
        }
    }

    fn get_managed_environment_id(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        // Get the managed environment name from the stack state and construct the full resource ID
        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = self.get_resource_group_name(ctx)?;
        let environment_name =
            crate::infra_requirements::azure_utils::get_container_apps_environment_name(ctx.state)?;

        let environment_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}",
            azure_config.subscription_id, resource_group_name, environment_name
        );

        Ok(environment_id)
    }

    /// Extracts the managed identity ID from a linked ServiceAccount
    async fn get_managed_identity_id(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_account_ref: &ResourceRef,
    ) -> Result<String> {
        use crate::service_account::AzureServiceAccountController;
        use alien_core::ServiceAccount;

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
            ctx.require_dependency::<AzureServiceAccountController>(service_account_ref)?;

        // Get the managed identity ID from the resolved state
        service_account_controller
            .identity_resource_id
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
        build_id: &str,
    ) -> Result<()> {
        use crate::core::AzurePermissionsHelper;
        use alien_azure_clients::authorization::Scope;
        use alien_permissions::PermissionContext;

        let config = ctx.desired_resource_config::<Build>()?;
        let azure_config = ctx.get_azure_config()?;

        // Build permission context for this specific build resource
        let permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(self.resource_group_name.as_ref().unwrap().clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(build_id.to_string());

        // Build Azure resource scope for the Container Apps environment (build execution environment)
        let environment_id = self.get_managed_environment_id(ctx)?;
        let environment_name = environment_id.split('/').last().unwrap();
        let resource_scope = Scope::Resource {
            resource_group_name: self.resource_group_name.as_ref().unwrap().clone(),
            resource_provider: "Microsoft.App".to_string(),
            parent_resource_path: None,
            resource_type: "managedEnvironments".to_string(),
            resource_name: environment_name.to_string(),
        };

        AzurePermissionsHelper::apply_resource_scoped_permissions(
            ctx,
            &config.id,
            "build",
            resource_scope,
            &permission_context,
        )
        .await
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(environment_name: &str) -> Self {
        Self {
            state: AzureBuildState::Ready,
            managed_environment_id: Some(format!("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.App/managedEnvironments/{}", environment_name)),
            resource_group_name: Some("test-rg".to_string()),
            build_env_vars: Some(HashMap::new()),
            managed_identity_id: Some("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-build-identity".to_string()),
            resource_prefix: Some("test-prefix".to_string()),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Build Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_core::{Build, BuildOutputs, Platform, ResourceStatus};
    use rstest::rstest;

    use crate::build::{fixtures::*, AzureBuildController};
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider,
    };

    #[rstest]
    #[case::basic(basic_build())]
    #[case::with_env(build_with_env_vars())]
    #[case::large_compute(build_medium_compute())]
    #[case::custom_image(build_custom_image())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] build: Build) {
        let mut mock_provider = MockPlatformServiceProvider::new();

        // Mock Azure authorization client for resource-scoped permissions
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(|_| {
                use alien_azure_clients::authorization::MockAuthorizationApi;
                Ok(Arc::new(MockAuthorizationApi::new()))
            });

        let mock_provider = Arc::new(mock_provider);

        let mut executor = SingleControllerExecutor::builder()
            .resource(build)
            .controller(AzureBuildController::default())
            .platform(Platform::Azure)
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
        assert!(build_outputs.identifier.contains("managedEnvironments"));

        // Delete the build
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

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

        let environment_name = "test-env";
        let ready_controller = AzureBuildController::mock_ready(environment_name);
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_build)
            .controller(ready_controller)
            .platform(Platform::Azure)
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
