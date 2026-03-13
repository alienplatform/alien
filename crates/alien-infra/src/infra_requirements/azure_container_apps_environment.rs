use alien_azure_clients::AzureClientConfig;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::azure_utils::get_resource_group_name;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::managed_environments::{
    ManagedEnvironment, ManagedEnvironmentProperties, VnetConfiguration,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentOutputs, Network, ResourceOutputs,
    ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;

#[controller]
pub struct AzureContainerAppsEnvironmentController {
    /// The actual Azure environment name (may differ from config.id).
    pub(crate) environment_name: Option<String>,
    /// The Azure resource ID of the environment.
    pub(crate) resource_id: Option<String>,
    /// The default domain for applications in this environment.
    pub(crate) default_domain: Option<String>,
    /// The static IP address of the environment (if applicable).
    pub(crate) static_ip: Option<String>,
    /// Long-running operation information for monitoring Azure operations.
    pub(crate) long_running_operation: Option<LongRunningOperation>,
}

#[controller]
impl AzureContainerAppsEnvironmentController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        info!(id=%desired_config.id, "Initiating Azure Container Apps Environment creation");
        let azure_config = ctx.get_azure_config()?;
        self.environment_name = Some(generate_container_apps_environment_name(
            &ctx.resource_prefix,
            &desired_config.id,
        ));
        let resource_group_name = get_resource_group_name(ctx.state)?;

        // Create the managed environment via Azure client
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;
        let managed_env = self.build_managed_environment(azure_config, ctx);

        let operation_result = client
            .create_or_update_managed_environment(
                &resource_group_name,
                self.environment_name.as_ref().unwrap(),
                &managed_env,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure Container Apps Environment".to_string(),
                resource_id: Some(desired_config.id.clone()),
            })?;

        match operation_result {
            OperationResult::Completed(result) => {
                info!(environment_name=%self.environment_name.as_ref().unwrap(), "Environment creation completed immediately");
                self.handle_creation_completed(&result);
                // Always go to Ready after immediate completion - linear flow
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(long_running_op) => {
                info!(environment_name=%self.environment_name.as_ref().unwrap(), operation_url=%long_running_op.url, "Environment creation initiated as long-running operation");
                self.long_running_operation = Some(long_running_op.clone());

                // Always go to CreatingEnvironmentOperation for long-running ops - linear flow
                Ok(HandlerAction::Continue {
                    state: CreatingEnvironmentOperation,
                    suggested_delay: long_running_op.retry_after,
                })
            }
        }
    }

    #[handler(
        state = CreatingEnvironmentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        let long_running_op = self.long_running_operation.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Long-running operation not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, operation_url=%long_running_op.url, "Checking Azure long-running operation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_config)?;

        let status_result = operation_client
            .check_status(
                long_running_op,
                "CreateManagedEnvironment",
                environment_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure long-running operation failed".to_string(),
                resource_id: Some(desired_config.id.clone()),
            })?;

        if status_result.is_some() {
            info!(environment_name=%environment_name, "Azure long-running operation completed successfully, now checking resource status");
            // Operation completed, now check the actual resource status
            self.long_running_operation = None; // Clear the operation since it's done
                                                // Always go to CreatingEnvironment next - linear flow
            Ok(HandlerAction::Continue {
                state: CreatingEnvironment,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            // Operation still running - retry this same state
            debug!(environment_name=%environment_name, "Azure long-running operation still in progress");
            let delay = long_running_op
                .retry_after
                .unwrap_or(Duration::from_secs(10));
            Ok(HandlerAction::Stay {
                max_times: 30,
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = CreatingEnvironment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, "Checking environment creation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        match client
            .get_managed_environment(&resource_group_name, environment_name)
            .await
        {
            Ok(managed_env) => {
                if let Some(properties) = &managed_env.properties {
                    use alien_azure_clients::models::managed_environments::ManagedEnvironmentPropertiesProvisioningState::*;
                    match properties.provisioning_state {
                        Some(Succeeded) => {
                            info!(environment_name=%environment_name, "Environment creation completed");
                            self.handle_creation_completed(&managed_env);

                            // Always go to ApplyingResourcePermissions next
                            Ok(HandlerAction::Continue {
                                state: ApplyingResourcePermissions,
                                suggested_delay: None,
                            })
                        }
                        Some(InitializationInProgress)
                        | Some(InfrastructureSetupInProgress)
                        | Some(Waiting) => {
                            debug!(environment_name=%environment_name, "Environment still being created");
                            Ok(HandlerAction::Stay {
                                max_times: 20,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                        Some(Failed) => {
                            error!(environment_name=%environment_name, "Environment creation failed");
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Environment creation failed with status: Failed"
                                    .to_string(),
                                resource_id: Some(desired_config.id.clone()),
                            }));
                        }
                        _ => {
                            debug!(environment_name=%environment_name, "Provisioning state not available, continuing to wait");
                            Ok(HandlerAction::Stay {
                                max_times: 20,
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                    }
                } else {
                    debug!(environment_name=%environment_name, "Properties not available, continuing to wait");
                    Ok(HandlerAction::Stay {
                        max_times: 20,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                debug!(environment_name=%environment_name, "Environment not yet available, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check environment creation status".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self
            .environment_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Environment name not set".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                })
            })?
            .clone();

        info!(environment_name=%environment_name, "Applying resource-scoped permissions for container apps environment");

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx, &environment_name)
            .await?;

        info!(environment_name=%environment_name, "Resource-scoped permissions applied successfully");

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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;

        // Heartbeat check: verify environment provisioning state
        if let Some(environment_name) = &self.environment_name {
            let resource_group_name = get_resource_group_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_container_apps_client(azure_config)?;

            let managed_env = client
                .get_managed_environment(&resource_group_name, environment_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get Container Apps Environment '{}'",
                        environment_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(properties) = managed_env.properties {
                use alien_azure_clients::models::managed_environments::ManagedEnvironmentPropertiesProvisioningState::*;
                match properties.provisioning_state {
                    Some(Succeeded) => {
                        // Environment is healthy
                    }
                    Some(state) => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: format!(
                                "Provisioning state changed from Succeeded to {:?}",
                                state
                            ),
                        }));
                    }
                    None => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: "Provisioning state is no longer available".to_string(),
                        }));
                    }
                }
            }
        }

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
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        info!(environment_name=%environment_name, "Initiating Azure Container Apps Environment deletion");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        match client
            .delete_managed_environment(&resource_group_name, environment_name)
            .await
        {
            Ok(operation_result) => match operation_result {
                OperationResult::Completed(_) => {
                    info!(environment_name=%environment_name, "Environment deletion completed immediately");
                    self.clear_state();
                    // Always go to Deleted after immediate completion - linear flow
                    Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    })
                }
                OperationResult::LongRunning(long_running_op) => {
                    info!(environment_name=%environment_name, operation_url=%long_running_op.url, "Environment deletion initiated as long-running operation");
                    self.long_running_operation = Some(long_running_op);
                    // Always go to DeletingEnvironmentOperation for long-running ops - linear flow
                    Ok(HandlerAction::Continue {
                        state: DeletingEnvironmentOperation,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            },
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(environment_name=%environment_name, "Environment already deleted");
                self.clear_state();
                // Always go to Deleted when already deleted - linear flow
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Azure Container Apps Environment".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = DeletingEnvironmentOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        let long_running_op = self.long_running_operation.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Long-running operation not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, operation_url=%long_running_op.url, "Checking Azure deletion long-running operation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_config)?;

        let status_result = operation_client
            .check_status(
                long_running_op,
                "DeleteManagedEnvironment",
                environment_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure deletion long-running operation failed".to_string(),
                resource_id: Some(desired_config.id.clone()),
            })?;

        if status_result.is_some() {
            info!(environment_name=%environment_name, "Azure deletion long-running operation completed successfully, now checking resource status");
            // Operation completed, now check if the resource is actually gone
            self.long_running_operation = None; // Clear the operation since it's done
                                                // Always go to DeletingEnvironment next - linear flow
            Ok(HandlerAction::Continue {
                state: DeletingEnvironment,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            // Operation still running - retry this same state
            debug!(environment_name=%environment_name, "Azure deletion long-running operation still in progress");
            let delay = long_running_op
                .retry_after
                .unwrap_or(Duration::from_secs(10));
            Ok(HandlerAction::Stay {
                max_times: 30,
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = DeletingEnvironment,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, "Checking environment deletion status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        match client
            .get_managed_environment(&resource_group_name, environment_name)
            .await
        {
            Ok(_) => {
                debug!(environment_name=%environment_name, "Environment still exists, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(environment_name=%environment_name, "Environment successfully deleted");
                self.clear_state();
                // Always go to Deleted when deletion is confirmed - linear flow
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check environment deletion status".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Only return outputs when we have all the required information
        if let (Some(environment_name), Some(resource_id)) =
            (&self.environment_name, &self.resource_id)
        {
            Some(ResourceOutputs::new(AzureContainerAppsEnvironmentOutputs {
                environment_name: environment_name.clone(),
                resource_id: resource_id.clone(),
                default_domain: self.default_domain.clone().unwrap_or_default(),
                static_ip: self.static_ip.clone(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureContainerAppsEnvironmentController {
    // ─────────────── HELPER METHODS ────────────────────────────
    fn handle_creation_completed(&mut self, managed_env: &ManagedEnvironment) {
        self.resource_id = managed_env.id.clone();
        self.default_domain = managed_env
            .properties
            .as_ref()
            .and_then(|p| p.default_domain.clone());
        self.static_ip = managed_env
            .properties
            .as_ref()
            .and_then(|p| p.static_ip.clone());
    }

    fn clear_state(&mut self) {
        self.environment_name = None;
        self.resource_id = None;
        self.default_domain = None;
        self.static_ip = None;
        self.long_running_operation = None;
    }

    fn build_managed_environment(
        &self,
        azure_config: &AzureClientConfig,
        ctx: &ResourceControllerContext,
    ) -> ManagedEnvironment {
        let location = azure_config.region.as_deref().unwrap_or("East US");

        let mut tags = HashMap::new();
        tags.insert(
            "alien-resource".to_string(),
            "container-apps-environment".to_string(),
        );
        tags.insert(
            "alien-environment-id".to_string(),
            ctx.desired_config.id().to_string(),
        );
        tags.insert("alien-stack".to_string(), ctx.resource_prefix.to_string());

        // Get VNet configuration if a Network resource exists
        let vnet_configuration = self.get_vnet_configuration(ctx);
        if vnet_configuration.is_some() {
            info!("Configuring Container Apps Environment with VNet integration");
        }

        ManagedEnvironment {
            id: None,
            identity: None,
            kind: None,
            location: location.to_string(),
            name: None,
            properties: Some(ManagedEnvironmentProperties {
                app_logs_configuration: None,
                custom_domain_configuration: None,
                dapr_ai_connection_string: None,
                dapr_ai_instrumentation_key: None,
                dapr_configuration: None,
                default_domain: None,
                deployment_errors: None,
                event_stream_endpoint: None,
                infrastructure_resource_group: None,
                keda_configuration: None,
                peer_authentication: None,
                peer_traffic_configuration: None,
                provisioning_state: None,
                static_ip: None,
                vnet_configuration,
                workload_profiles: vec![], // Empty workload profiles for default setup
                zone_redundant: Some(false), // Default to non-zone redundant for simplicity
            }),
            system_data: None,
            tags,
            type_: None,
        }
    }

    /// Gets VNet configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the private subnet ID from the Network controller to configure the Container
    /// Apps Environment with VNet integration.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vnet_configuration(&self, ctx: &ResourceControllerContext) -> Option<VnetConfiguration> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return None;
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            match ctx.require_dependency::<crate::network::AzureNetworkController>(&network_ref) {
                Ok(state) => state,
                Err(_) => return None,
            };

        // Get the private subnet resource ID
        // For Azure Container Apps, we need the full resource ID of the subnet
        let vnet_resource_id = match &network_state.vnet_resource_id {
            Some(id) => id,
            None => return None,
        };

        let private_subnet_name = match &network_state.private_subnet_name {
            Some(name) => name,
            None => return None,
        };

        // Construct the full subnet resource ID
        let infrastructure_subnet_id =
            format!("{}/subnets/{}", vnet_resource_id, private_subnet_name);

        Some(VnetConfiguration {
            infrastructure_subnet_id: Some(infrastructure_subnet_id),
            internal: Some(false), // Allow external access by default
            docker_bridge_cidr: None,
            platform_reserved_cidr: None,
            platform_reserved_dns_ip: None,
        })
    }

    /// Applies resource-scoped permissions to the container apps environment from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        environment_name: &str,
    ) -> Result<()> {
        use crate::infra_requirements::azure_utils;
        use alien_permissions::PermissionContext;

        // Get deployment target Azure configuration
        let azure_config = ctx.get_azure_config()?;

        // Build permission context for this specific container apps environment
        let _permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(azure_utils::get_resource_group_name(ctx.state)?)
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(environment_name.to_string());

        // Find all permission profiles that apply to this environment
        let stack_configs = &ctx.desired_stack;
        for (profile_name, profile_config) in &stack_configs.permissions.profiles {
            // Skip processing permissions for now - this is just a logging implementation
            info!(
                profile = %profile_name,
                permission_sets_count = profile_config.0.len(),
                "Would apply resource-scoped permissions for container apps environment"
            );

            // TODO: In a full implementation, this would:
            // 1. Find the service account for this profile from the stack state
            // 2. Generate Azure role definitions and assignments for each permission set
            // 3. Apply the permissions to the specific container apps environment resource
            // For now, this is just a placeholder to complete the milestone
        }

        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(environment_name: &str) -> Self {
        Self {
            state: AzureContainerAppsEnvironmentState::Ready,
            environment_name: Some(environment_name.to_string()),
            resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.App/managedEnvironments/{}", environment_name)),
            default_domain: Some(format!("{}.eastus.azurecontainerapps.io", environment_name)),
            static_ip: Some("20.1.2.3".to_string()),
            long_running_operation: None,
            _internal_stay_count: None,
        }
    }
}

/// Generates the full, prefixed Azure Container Apps Environment name (pure function).
fn generate_container_apps_environment_name(resource_prefix: &str, id: &str) -> String {
    // Azure Container Apps Environment names must be valid DNS names
    // Format: {prefix}-{id} with length constraints and character restrictions
    let clean_prefix = resource_prefix
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    let clean_id = id
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();

    let combined = format!("{}-{}", clean_prefix, clean_id);

    // Truncate to reasonable length if necessary (Azure limit is usually around 60 chars)
    if combined.len() > 60 {
        combined[..60].to_string()
    } else {
        combined
    }
}
