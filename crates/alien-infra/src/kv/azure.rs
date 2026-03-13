use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info};

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};
use alien_azure_clients::tables::{
    AzureTableManagementClient, AzureTableStorageClient, TableManagementApi, TableStorageApi,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureStorageAccountOutputs, Kv, KvOutputs, Resource, ResourceDefinition, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

/// Generates the Azure Table Storage table name
fn get_azure_table_name(prefix: &str, name: &str) -> String {
    // Azure table names must be 3-63 characters, alphanumeric only, cannot start with a number
    let clean_prefix = prefix
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();
    let clean_name = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();

    let combined = format!("{}{}", clean_prefix, clean_name);

    // Ensure it starts with a letter and is within length limits
    let result = if combined.chars().next().map_or(false, |c| c.is_numeric()) {
        format!("kv{}", combined)
    } else {
        combined
    };

    if result.len() > 63 {
        result[..63].to_string()
    } else if result.len() < 3 {
        format!("{}000", result)[..3].to_string()
    } else {
        result
    }
}

#[controller]
pub struct AzureKvController {
    /// The name of the created Azure Table Storage table
    pub(crate) table_name: Option<String>,
    /// The Azure Storage Account outputs (from infrastructure dependencies)
    pub(crate) storage_account_outputs: Option<AzureStorageAccountOutputs>,
    /// The resource group name (needed for binding parameters)
    pub(crate) resource_group_name: Option<String>,
}

#[controller]
impl AzureKvController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = get_azure_table_name(&ctx.resource_prefix, &config.id);

        info!(id=%config.id, table_name=%table_name, "Creating Azure Table Storage table for KV store");

        // Get the storage account outputs from infrastructure dependencies
        let storage_account_dependency = ctx
            .state
            .get_resource_outputs::<AzureStorageAccountOutputs>("default-storage-account")
            .context(ErrorData::InfrastructureError {
                message: "Azure Storage Account dependency not found".to_string(),
                operation: Some("create_start".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        let storage_account_outputs = &storage_account_dependency;

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(&ctx.state)?;
        let management_client = ctx
            .service_provider
            .get_azure_table_management_client(azure_config)?;

        // Create the table using the management client
        management_client
            .create_table(
                &resource_group_name,
                &storage_account_outputs.account_name,
                &table_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Table Storage table '{}'",
                    table_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.table_name = Some(table_name.clone());
        self.storage_account_outputs = Some((*storage_account_outputs).clone());
        self.resource_group_name = Some(resource_group_name);

        info!(table_name=%table_name, "Azure Table Storage table created successfully");

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
        let config = ctx.desired_resource_config::<Kv>()?;

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let (Some(table_name), Some(storage_account_outputs), Some(resource_group_name)) = (
            &self.table_name,
            &self.storage_account_outputs,
            &self.resource_group_name,
        ) {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            // Build Azure resource scope for the Table Storage table
            let resource_scope = Scope::Resource {
                resource_group_name: resource_group_name.clone(),
                resource_provider: "Microsoft.Storage".to_string(),
                parent_resource_path: Some(format!(
                    "storageAccounts/{}/tableServices/default",
                    storage_account_outputs.account_name
                )),
                resource_type: "tables".to_string(),
                resource_name: table_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                table_name,
                resource_scope,
                "KV",
            )
            .await?;
        }

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

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
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(&ctx.state)?;
        let management_client = ctx
            .service_provider
            .get_azure_table_management_client(azure_config)?;

        // Heartbeat check: verify table still exists by trying to get ACL
        match management_client
            .get_table_acl(
                &resource_group_name,
                &self.storage_account_outputs.as_ref().unwrap().account_name,
                table_name,
            )
            .await
        {
            Ok(_) => {
                // Table exists and is accessible
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: "Table no longer exists".to_string(),
                }))
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to check table '{}' during heartbeat", table_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        info!(id=%config.id, "Azure KV update (no-op — no mutable fields)");
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
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        info!(table_name=%table_name, "Deleting Azure Table Storage table");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(&ctx.state)?;
        let management_client = ctx
            .service_provider
            .get_azure_table_management_client(azure_config)?;

        match management_client
            .delete_table(
                &resource_group_name,
                &self.storage_account_outputs.as_ref().unwrap().account_name,
                table_name,
            )
            .await
        {
            Ok(_) => {
                info!(table_name=%table_name, "Azure Table Storage table deleted successfully");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(table_name=%table_name, "Azure Table Storage table already deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete Azure Table Storage table '{}'",
                    table_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
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
        if let (Some(table_name), Some(storage_outputs)) =
            (&self.table_name, &self.storage_account_outputs)
        {
            Some(ResourceOutputs::new(KvOutputs {
                store_name: table_name.clone(),
                identifier: Some(format!("{}/{}", storage_outputs.account_name, table_name)),
                endpoint: Some(storage_outputs.primary_table_endpoint.clone()),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, KvBinding};

        if let (Some(table_name), Some(storage_outputs), Some(resource_group_name)) = (
            &self.table_name,
            &self.storage_account_outputs,
            &self.resource_group_name,
        ) {
            let binding = KvBinding::table_storage(
                BindingValue::value(resource_group_name.clone()),
                BindingValue::value(storage_outputs.account_name.clone()),
                BindingValue::value(table_name.clone()),
            );
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureKvController {
    fn clear_state(&mut self) {
        self.table_name = None;
        self.storage_account_outputs = None;
        self.resource_group_name = None;
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(table_name: &str, account_name: &str) -> Self {
        let mock_key = "YWJjZGVmZ2hpams="; // Mock base64 key
        let storage_outputs = AzureStorageAccountOutputs {
            account_name: account_name.to_string(),
            resource_id: format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.Storage/storageAccounts/{}", account_name),
            primary_blob_endpoint: format!("https://{}.blob.core.windows.net/", account_name),
            primary_file_endpoint: format!("https://{}.file.core.windows.net/", account_name),
            primary_queue_endpoint: format!("https://{}.queue.core.windows.net/", account_name),
            primary_table_endpoint: format!("https://{}.table.core.windows.net/", account_name),
            primary_access_key: mock_key.to_string(),
            connection_string: format!("DefaultEndpointsProtocol=https;AccountName={};AccountKey={};EndpointSuffix=core.windows.net", account_name, mock_key),
        };

        Self {
            state: AzureKvState::Ready,
            table_name: Some(table_name.to_string()),
            storage_account_outputs: Some(storage_outputs),
            resource_group_name: Some("mock-rg".to_string()),
            _internal_stay_count: None,
        }
    }
}
