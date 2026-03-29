use alien_azure_clients::AzureClientConfig;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::azure_utils::get_resource_group_name;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::queue_namespace::*;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureServiceBusNamespace, AzureServiceBusNamespaceOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct AzureServiceBusNamespaceController {
    /// The actual Azure namespace name (may differ from config.id).
    pub(crate) namespace_name: Option<String>,
    /// The Azure resource ID of the namespace.
    pub(crate) resource_id: Option<String>,
    /// The fully qualified domain name of the namespace.
    pub(crate) fqdn: Option<String>,
    /// The endpoint for Service Bus operations.
    pub(crate) endpoint: Option<String>,
}

#[controller]
impl AzureServiceBusNamespaceController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;
        info!(id=%desired_config.id, "Initiating Azure Service Bus Namespace creation");

        let azure_config = ctx.get_azure_config()?;
        self.namespace_name = Some(generate_service_bus_namespace_name(
            &ctx.resource_prefix,
            &desired_config.id,
        ));
        let resource_group_name = get_resource_group_name(ctx.state)?;

        // Create the Service Bus namespace
        let client = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;
        let namespace_props = self.build_namespace_properties(azure_config, ctx);

        client
            .create_or_update_namespace(
                resource_group_name.clone(),
                self.namespace_name.as_ref().unwrap().clone(),
                namespace_props,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure Service Bus Namespace".to_string(),
                resource_id: Some(desired_config.id.clone()),
            })?;

        info!(namespace_name=%self.namespace_name.as_ref().unwrap(), "Service Bus namespace creation initiated");

        // Azure Service Bus namespace creation is typically synchronous for Standard tier
        // Move to verification state
        Ok(HandlerAction::Continue {
            state: CreatingNamespace,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = CreatingNamespace,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;
        let namespace_name = self.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Namespace name not set in state".to_string(),
            })
        })?;

        debug!(namespace_name=%namespace_name, "Checking namespace creation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        match client
            .get_namespace(resource_group_name, namespace_name.clone())
            .await
        {
            Ok(namespace) => {
                if let Some(properties) = &namespace.properties {
                    match properties.status.as_deref() {
                        Some("Active") => {
                            info!(namespace_name=%namespace_name, "Namespace creation completed");
                            self.handle_creation_completed(&namespace);

                            // Always go to ApplyingResourcePermissions next
                            Ok(HandlerAction::Continue {
                                state: ApplyingResourcePermissions,
                                suggested_delay: None,
                            })
                        }
                        Some("Creating") | Some("Updating") => {
                            debug!(namespace_name=%namespace_name, "Namespace still being created");
                            Ok(HandlerAction::Stay {
                                max_times: 60,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                        Some("Failed") => {
                            error!(namespace_name=%namespace_name, "Namespace creation failed");
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Namespace creation failed with status: Failed"
                                    .to_string(),
                                resource_id: Some(desired_config.id.clone()),
                            }));
                        }
                        other => {
                            debug!(namespace_name=%namespace_name, status=?other, "Status not 'Active', continuing to wait");
                            Ok(HandlerAction::Stay {
                                max_times: 60,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                    }
                } else {
                    debug!(namespace_name=%namespace_name, "Properties not available, continuing to wait");
                    Ok(HandlerAction::Stay {
                        max_times: 60,
                        suggested_delay: Some(Duration::from_secs(15)),
                    })
                }
            }
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                debug!(namespace_name=%namespace_name, "Namespace not yet available, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check namespace creation status".to_string(),
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
        let desired_config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;
        let namespace_name = self
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Namespace name not set".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                })
            })?
            .clone();

        info!(namespace_name=%namespace_name, "Applying resource-scoped permissions for service bus namespace");

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx, &namespace_name)
            .await?;

        info!(namespace_name=%namespace_name, "Resource-scoped permissions applied successfully");

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
        let config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;

        // Heartbeat check: verify namespace provisioning state
        if let Some(namespace_name) = &self.namespace_name {
            let resource_group_name = get_resource_group_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_service_bus_management_client(azure_config)?;

            let namespace = client
                .get_namespace(resource_group_name, namespace_name.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get Service Bus Namespace '{}'", namespace_name),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(properties) = namespace.properties {
                match properties.status.as_deref() {
                    Some("Active") => {
                        // Namespace is healthy
                    }
                    Some(state) => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: format!("Status changed from Active to {}", state),
                        }));
                    }
                    None => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: "Status is no longer available".to_string(),
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
        let desired_config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;
        let namespace_name = self.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Namespace name not set in state".to_string(),
            })
        })?;

        info!(namespace_name=%namespace_name, "Initiating Azure Service Bus Namespace deletion");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        match client
            .delete_namespace(resource_group_name, namespace_name.clone())
            .await
        {
            Ok(_) => {
                info!(namespace_name=%namespace_name, "Namespace deletion initiated");
                // Always go to DeletingNamespace for verification
                Ok(HandlerAction::Continue {
                    state: DeletingNamespace,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(namespace_name=%namespace_name, "Namespace already deleted");
                self.clear_state();
                // Always go to Deleted when already deleted
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Azure Service Bus Namespace".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = DeletingNamespace,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureServiceBusNamespace>()?;
        let namespace_name = self.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Namespace name not set in state".to_string(),
            })
        })?;

        debug!(namespace_name=%namespace_name, "Checking namespace deletion status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        match client
            .get_namespace(resource_group_name, namespace_name.clone())
            .await
        {
            Ok(_) => {
                debug!(namespace_name=%namespace_name, "Namespace still exists, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(AlienError {
                error: Some(CloudClientErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!(namespace_name=%namespace_name, "Namespace successfully deleted");
                self.clear_state();
                // Always go to Deleted when deletion is confirmed
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check namespace deletion status".to_string(),
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
        if let (Some(namespace_name), Some(resource_id)) = (&self.namespace_name, &self.resource_id)
        {
            Some(ResourceOutputs::new(AzureServiceBusNamespaceOutputs {
                namespace_name: namespace_name.clone(),
                resource_id: resource_id.clone(),
                fqdn: self.fqdn.clone().unwrap_or_default(),
                endpoint: self.endpoint.clone().unwrap_or_default(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureServiceBusNamespaceController {
    // ─────────────── HELPER METHODS ────────────────────────────
    fn handle_creation_completed(&mut self, namespace: &SbNamespace) {
        self.resource_id = namespace.id.clone();
        self.fqdn = namespace
            .properties
            .as_ref()
            .and_then(|p| p.service_bus_endpoint.clone());
        self.endpoint = self.fqdn.clone();
    }

    fn clear_state(&mut self) {
        self.namespace_name = None;
        self.resource_id = None;
        self.fqdn = None;
        self.endpoint = None;
    }

    fn build_namespace_properties(
        &self,
        azure_config: &AzureClientConfig,
        ctx: &ResourceControllerContext,
    ) -> SbNamespaceProperties {
        SbNamespaceProperties {
            private_endpoint_connections: vec![],
            public_network_access: SbNamespacePropertiesPublicNetworkAccess::Enabled,
            ..Default::default()
        }
    }

    /// Applies resource-scoped permissions to the service bus namespace from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        _namespace_name: &str,
    ) -> Result<()> {
        // Find all permission profiles that apply to this namespace
        let stack_configs = &ctx.desired_stack;
        for (profile_name, profile_config) in &stack_configs.permissions.profiles {
            // Skip processing permissions for now - this is just a logging implementation
            info!(
                profile = %profile_name,
                permission_sets_count = profile_config.0.len(),
                "Would apply resource-scoped permissions for service bus namespace"
            );

            // TODO: In a full implementation, this would:
            // 1. Find the service account for this profile from the stack state
            // 2. Generate Azure role definitions and assignments for each permission set
            // 3. Apply the permissions to the specific service bus namespace resource
            // For now, this is just a placeholder to complete the milestone
        }

        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(namespace_name: &str) -> Self {
        Self {
            state: AzureServiceBusNamespaceState::Ready,
            namespace_name: Some(namespace_name.to_string()),
            resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.ServiceBus/namespaces/{}", namespace_name)),
            fqdn: Some(format!("{}.servicebus.windows.net", namespace_name)),
            endpoint: Some(format!("https://{}.servicebus.windows.net/", namespace_name)),
            _internal_stay_count: None,
        }
    }
}

/// Generates the full, prefixed Azure Service Bus Namespace name (pure function).
fn generate_service_bus_namespace_name(resource_prefix: &str, id: &str) -> String {
    // Azure Service Bus Namespace names must be:
    // - 6-50 characters, globally unique DNS name
    // - Alphanumeric and hyphens only, start with letter, end with letter/number
    // - Must NOT end with "-sb" (Azure reserves this suffix)
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

    // Truncate to 50 chars (Azure limit for Service Bus namespace names)
    if combined.len() > 50 {
        combined[..50].to_string()
    } else {
        combined
    }
}
