use alien_azure_clients::AzureClientConfig;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::resources::ResourceGroup;
use alien_azure_clients::resources::{AzureResourcesClient, ResourcesApi};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureResourceGroup, AzureResourceGroupOutputs, Resource, ResourceDefinition, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct AzureResourceGroupController {
    /// The name of the created Azure Resource Group.
    pub(crate) resource_group_name: Option<String>,
    /// The Azure resource ID of the created Resource Group.
    pub(crate) resource_id: Option<String>,
    /// The location/region where the Resource Group was created.
    pub(crate) location: Option<String>,
}

#[controller]
impl AzureResourceGroupController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        info!(id=%config.id, "Initiating Azure Resource Group creation");

        let group_name = generate_azure_resource_group_name(&ctx.resource_prefix, &config.id);
        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        let resource_group = ResourceGroup {
            id: None,
            location: azure_config
                .region
                .clone()
                .unwrap_or_else(|| "East US".to_string()),
            managed_by: None,
            name: Some(group_name.clone()),
            properties: None,
            tags: HashMap::new(),
            type_: None,
        };

        let rg = client
            .create_or_update_resource_group(&group_name, &resource_group)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create or update resource group '{}'.",
                    group_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.resource_group_name = Some(group_name.clone());
        self.location = Some(rg.location.clone());
        self.resource_id = rg.id.clone();

        info!(group_name=%group_name, "Resource group creation initiated");

        Ok(HandlerAction::Continue {
            state: CreatingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = CreatingResourceGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_resource_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        let group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Resource group name not set in state".to_string(),
            })
        })?;

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        match client.get_resource_group(group_name).await {
            Ok(rg) => {
                if let Some(props) = &rg.properties {
                    if let Some(state) = &props.provisioning_state {
                        if state == "Succeeded" {
                            info!(group_name=%group_name, "Resource group creation completed");

                            self.resource_id = rg.id.clone();
                            self.location = Some(rg.location.clone());

                            return Ok(HandlerAction::Continue {
                                state: Ready,
                                suggested_delay: None,
                            });
                        } else if state == "Creating" {
                            debug!(group_name=%group_name, "Resource group still being created");

                            return Ok(HandlerAction::Continue {
                                state: CreatingResourceGroup,
                                suggested_delay: Some(Duration::from_secs(5)),
                            });
                        } else {
                            error!(group_name=%group_name, state=?state, "Resource group creation failed");
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Resource group creation failed with state: {}",
                                    state
                                ),
                                resource_id: Some(config.id.clone()),
                            }));
                        }
                    }
                }
                debug!(group_name=%group_name, "Provisioning state not available, continuing to wait");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(group_name=%group_name, "Resource group not yet available, continuing to wait");
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Error checking resource group status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: CreatingResourceGroup,
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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        // Heartbeat check: verify resource group provisioning state
        if let Some(group_name) = &self.resource_group_name {
            let client = ctx
                .service_provider
                .get_azure_resources_client(azure_config)?;

            let rg = client.get_resource_group(group_name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'", group_name),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(properties) = rg.properties {
                if properties.provisioning_state != Some("Succeeded".to_string()) {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Provisioning state changed from Succeeded to {:?}",
                            properties.provisioning_state
                        ),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(60)),
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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        let group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Resource group name not set in state".to_string(),
            })
        })?;

        info!(group_name=%group_name, "Initiating Azure Resource Group deletion");

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        match client.delete_resource_group(group_name).await {
            Ok(_) => {
                info!(group_name=%group_name, "Resource group deletion initiated");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(group_name=%group_name, "Resource group already deleted");

                self.resource_group_name = None;
                self.resource_id = None;
                self.location = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Failed to initiate resource group deletion");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = DeletingResourceGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_resource_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        let group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Resource group name not set in state".to_string(),
            })
        })?;

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        match client.get_resource_group(group_name).await {
            Ok(_) => {
                debug!(group_name=%group_name, "Resource group still exists, continuing to wait");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(group_name=%group_name, "Resource group successfully deleted");

                self.resource_group_name = None;
                self.resource_id = None;
                self.location = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Error checking resource group deletion status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
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
        if let (Some(resource_group_name), Some(resource_id), Some(location)) =
            (&self.resource_group_name, &self.resource_id, &self.location)
        {
            Some(ResourceOutputs::new(AzureResourceGroupOutputs {
                name: resource_group_name.clone(),
                resource_id: resource_id.clone(),
                location: location.clone(),
            }))
        } else {
            None
        }
    }
}

impl AzureResourceGroupController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(resource_group_name: &str) -> Self {
        Self {
            state: AzureResourceGroupState::Ready,
            resource_group_name: Some(resource_group_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/{}",
                resource_group_name
            )),
            location: Some("East US".to_string()),
            _internal_stay_count: None,
        }
    }
}

/// Generates the full, prefixed Azure resource group name (pure function).
fn generate_azure_resource_group_name(resource_prefix: &str, id: &str) -> String {
    // Azure resource group names must be 1-90 characters, case insensitive
    // Format: {prefix}-{id} with length constraints and character restrictions
    let clean_prefix = resource_prefix
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == '(' || *c == ')'
        })
        .collect::<String>();
    let clean_id = id
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == '(' || *c == ')'
        })
        .collect::<String>();

    let combined = format!("{}-{}", clean_prefix, clean_id);

    // Truncate to 90 characters if necessary (Azure limit)
    if combined.len() > 90 {
        combined[..90].to_string()
    } else {
        combined
    }
}
