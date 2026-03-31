use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::storage::{
    StorageAccount, StorageAccountCreateParameters, StorageAccountPropertiesProvisioningState,
};
use alien_azure_clients::storage_accounts::{AzureStorageAccountsClient, StorageAccountsApi};
use alien_azure_clients::AzureClientConfig;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureStorageAccount, AzureStorageAccountOutputs, Resource, ResourceDefinition, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};

#[controller]
pub struct AzureStorageAccountController {
    /// The actual Azure storage account name (may differ from config.id).
    pub(crate) account_name: Option<String>,
    /// The Azure resource ID of the storage account.
    pub(crate) resource_id: Option<String>,
    /// The primary access key for the storage account.
    pub(crate) primary_access_key: Option<String>,
    /// The connection string for the storage account.
    pub(crate) connection_string: Option<String>,
    /// The primary blob endpoint.
    pub(crate) primary_blob_endpoint: Option<String>,
    /// The primary file endpoint.
    pub(crate) primary_file_endpoint: Option<String>,
    /// The primary queue endpoint.
    pub(crate) primary_queue_endpoint: Option<String>,
    /// The primary table endpoint.
    pub(crate) primary_table_endpoint: Option<String>,
}

#[controller]
impl AzureStorageAccountController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        info!(id=%config.id, "Initiating Azure Storage Account creation");

        let azure_config = ctx.get_azure_config()?;
        let account_name = generate_storage_account_name(&ctx.resource_prefix, &config.id);
        let resource_group_name = get_resource_group_name(ctx.state)?;

        // Create the storage account via Azure client
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;
        let params = self.build_storage_account_params(azure_config, ctx);

        client
            .create_storage_account(&resource_group_name, &account_name, &params)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create Azure Storage Account '{}'.", account_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(account_name=%account_name, "Storage account creation initiated");
        self.account_name = Some(account_name);

        Ok(HandlerAction::Continue {
            state: CreatingStorageAccount,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = CreatingStorageAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Storage account name not set in state".to_string(),
            })
        })?;

        debug!(account_name=%account_name, "Checking storage account creation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;

        match client
            .get_storage_account_properties(&resource_group_name, account_name)
            .await
        {
            Ok(account_info) => {
                // Extract properties once to avoid multiple moves
                let properties = account_info.properties.clone();
                if let Some(provisioning_state) =
                    properties.as_ref().and_then(|p| p.provisioning_state)
                {
                    use alien_azure_clients::models::storage::StorageAccountPropertiesProvisioningState;
                    match provisioning_state {
                        StorageAccountPropertiesProvisioningState::Succeeded => {
                            info!(account_name=%account_name, "Storage account creation completed, retrieving details");

                            // Get storage account keys and details now that creation is complete
                            let keys = client
                                .list_storage_account_keys(&resource_group_name, account_name)
                                .await
                                .context(ErrorData::CloudPlatformError {
                                    message: format!(
                                        "Failed to list storage account keys for '{}'.",
                                        account_name
                                    ),
                                    resource_id: Some(config.id.clone()),
                                })?;

                            // Extract details from the response
                            self.resource_id = account_info.id;
                            let primary_endpoints = properties.and_then(|p| p.primary_endpoints);
                            self.primary_blob_endpoint =
                                primary_endpoints.as_ref().and_then(|e| e.blob.clone());
                            self.primary_file_endpoint =
                                primary_endpoints.as_ref().and_then(|e| e.file.clone());
                            self.primary_queue_endpoint =
                                primary_endpoints.as_ref().and_then(|e| e.queue.clone());
                            self.primary_table_endpoint =
                                primary_endpoints.as_ref().and_then(|e| e.table.clone());

                            // Extract primary key
                            self.primary_access_key =
                                keys.keys.first().and_then(|key| key.value.clone());

                            // Generate connection string
                            self.connection_string = if let (Some(key), Some(_blob_endpoint)) =
                                (&self.primary_access_key, &self.primary_blob_endpoint)
                            {
                                Some(format!(
                                    "DefaultEndpointsProtocol=https;AccountName={};AccountKey={};EndpointSuffix=core.windows.net",
                                    account_name, key
                                ))
                            } else {
                                None
                            };

                            info!(account_name=%account_name, "Successfully retrieved storage account details");

                            Ok(HandlerAction::Continue {
                                state: ApplyingPermissions,
                                suggested_delay: None,
                            })
                        }
                        StorageAccountPropertiesProvisioningState::Creating
                        | StorageAccountPropertiesProvisioningState::ResolvingDns => {
                            debug!(account_name=%account_name, "Storage account still being created");
                            Ok(HandlerAction::Continue {
                                state: CreatingStorageAccount,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                    }
                } else {
                    debug!(account_name=%account_name, "Provisioning state not available, continuing to wait");
                    Ok(HandlerAction::Continue {
                        state: CreatingStorageAccount,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(account_name=%account_name, "Storage account not yet available, continuing to wait");
                Ok(HandlerAction::Continue {
                    state: CreatingStorageAccount,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => {
                error!(account_name=%account_name, error=%e, "Error checking storage account status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get storage account properties for '{}'.",
                        account_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }
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
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;

        info!(id = %config.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(account_name) = &self.account_name {
            self.apply_resource_scoped_permissions(ctx, account_name)
                .await?;
        }

        info!(id = %config.id, "Successfully applied resource-scoped permissions");

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
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;

        // Heartbeat check: verify storage account provisioning state
        if let Some(account_name) = &self.account_name {
            let resource_group_name = get_resource_group_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_storage_accounts_client(azure_config)?;

            let storage_account = client
                .get_storage_account_properties(&resource_group_name, account_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get storage account properties for '{}'",
                        account_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(properties) = storage_account.properties {
                if properties.provisioning_state
                    != Some(StorageAccountPropertiesProvisioningState::Succeeded)
                {
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
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Storage account name not set in state".to_string(),
            })
        })?;

        info!(account_name=%account_name, "Initiating Azure Storage Account deletion");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;

        match client
            .delete_storage_account(&resource_group_name, account_name)
            .await
        {
            Ok(_) => {
                info!(account_name=%account_name, "Storage account deletion initiated");
                Ok(HandlerAction::Continue {
                    state: DeletingStorageAccount,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(account_name=%account_name, "Storage account already deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                error!(account_name=%account_name, error=%e, "Failed to initiate storage account deletion");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete storage account '{}'.", account_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }
    }

    #[handler(
        state = DeletingStorageAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Storage account name not set in state".to_string(),
            })
        })?;

        debug!(account_name=%account_name, "Checking storage account deletion status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;

        match client
            .get_storage_account_properties(&resource_group_name, account_name)
            .await
        {
            Ok(_) => {
                debug!(account_name=%account_name, "Storage account still exists, continuing to wait");
                Ok(HandlerAction::Continue {
                    state: DeletingStorageAccount,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(account_name=%account_name, "Storage account successfully deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                error!(account_name=%account_name, error=%e, "Error checking storage account deletion status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get storage account properties for '{}'.",
                        account_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
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
        if let (
            Some(account_name),
            Some(resource_id),
            Some(primary_access_key),
            Some(connection_string),
            Some(primary_blob_endpoint),
            Some(primary_file_endpoint),
            Some(primary_queue_endpoint),
            Some(primary_table_endpoint),
        ) = (
            &self.account_name,
            &self.resource_id,
            &self.primary_access_key,
            &self.connection_string,
            &self.primary_blob_endpoint,
            &self.primary_file_endpoint,
            &self.primary_queue_endpoint,
            &self.primary_table_endpoint,
        ) {
            Some(ResourceOutputs::new(AzureStorageAccountOutputs {
                account_name: account_name.clone(),
                resource_id: resource_id.clone(),
                primary_blob_endpoint: primary_blob_endpoint.clone(),
                primary_file_endpoint: primary_file_endpoint.clone(),
                primary_queue_endpoint: primary_queue_endpoint.clone(),
                primary_table_endpoint: primary_table_endpoint.clone(),
                primary_access_key: primary_access_key.clone(),
                connection_string: connection_string.clone(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureStorageAccountController {
    fn build_storage_account_params(
        &self,
        azure_config: &AzureClientConfig,
        ctx: &ResourceControllerContext,
    ) -> StorageAccountCreateParameters {
        use alien_azure_clients::models::storage::*;

        let location = azure_config.region.as_deref().unwrap_or("East US");

        let mut tags = HashMap::new();
        tags.insert("alien-resource".to_string(), "storage".to_string());
        tags.insert(
            "alien-storage-id".to_string(),
            ctx.desired_config.id().to_string(),
        );
        tags.insert("alien-stack".to_string(), ctx.resource_prefix.to_string());

        StorageAccountCreateParameters {
            extended_location: None,
            identity: None,
            kind: StorageAccountCreateParametersKind::StorageV2,
            location: location.to_string(),
            properties: Some(StorageAccountPropertiesCreateParameters {
                access_tier: None,
                allow_blob_public_access: None,
                allow_cross_tenant_replication: None,
                allow_shared_key_access: None,
                allowed_copy_scope: None,
                azure_files_identity_based_authentication: None,
                custom_domain: None,
                default_to_o_auth_authentication: None,
                dns_endpoint_type: None,
                enable_extended_groups: None,
                encryption: None,
                immutable_storage_with_versioning: None,
                is_hns_enabled: None,
                is_local_user_enabled: None,
                is_nfs_v3_enabled: None,
                is_sftp_enabled: None,
                key_policy: None,
                large_file_shares_state: None,
                minimum_tls_version: None,
                public_network_access: None,
                routing_preference: None,
                sas_policy: None,
                supports_https_traffic_only: Some(true),
                network_acls: None,
            }),
            sku: Sku {
                name: SkuName::StandardLrs,
                tier: None,
            },
            tags,
        }
    }

    fn clear_state(&mut self) {
        self.account_name = None;
        self.resource_id = None;
        self.primary_access_key = None;
        self.connection_string = None;
        self.primary_blob_endpoint = None;
        self.primary_file_endpoint = None;
        self.primary_queue_endpoint = None;
        self.primary_table_endpoint = None;
    }

    /// Applies resource-scoped permissions to the storage account from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        account_name: &str,
    ) -> Result<()> {
        use crate::core::AzurePermissionsHelper;
        use alien_azure_clients::authorization::Scope;
        use alien_permissions::PermissionContext;

        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        let azure_config = ctx.get_azure_config()?;

        // Build permission context for this specific storage account resource
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group)
            .with_storage_account_name(account_name.to_string())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(account_name.to_string());

        // Build Azure resource scope for the storage account
        let resource_scope = Scope::Resource {
            resource_group_name: crate::infra_requirements::azure_utils::get_resource_group_name(
                ctx.state,
            )?,
            resource_provider: "Microsoft.Storage".to_string(),
            parent_resource_path: None,
            resource_type: "storageAccounts".to_string(),
            resource_name: account_name.to_string(),
        };

        AzurePermissionsHelper::apply_resource_scoped_permissions(
            ctx,
            &config.id,
            "storage",
            resource_scope,
            &permission_context,
        )
        .await
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(account_name: &str) -> Self {
        let mock_key = "YWJjZGVmZ2hpams="; // Mock base64 key
        Self {
                state: AzureStorageAccountState::Ready,
                account_name: Some(account_name.to_string()),
                resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.Storage/storageAccounts/{}", account_name)),
                primary_access_key: Some(mock_key.to_string()),
                connection_string: Some(format!("DefaultEndpointsProtocol=https;AccountName={};AccountKey={};EndpointSuffix=core.windows.net", account_name, mock_key)),
                primary_blob_endpoint: Some(format!("https://{}.blob.core.windows.net/", account_name)),
                primary_file_endpoint: Some(format!("https://{}.file.core.windows.net/", account_name)),
                primary_queue_endpoint: Some(format!("https://{}.queue.core.windows.net/", account_name)),
                primary_table_endpoint: Some(format!("https://{}.table.core.windows.net/", account_name)),
                _internal_stay_count: None,
            }
    }
}

/// Generates the full, prefixed Azure storage account name (pure function).
pub fn generate_storage_account_name(resource_prefix: &str, id: &str) -> String {
    // Azure storage account names must be 3-24 characters, lowercase, and globally unique
    // Format: {prefix}{id} with length constraints and character restrictions
    let clean_prefix = resource_prefix
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();
    let clean_id = id
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();

    let combined = format!("{}{}", clean_prefix, clean_id);

    // Truncate to 24 characters if necessary
    if combined.len() > 24 {
        combined[..24].to_string()
    } else if combined.len() < 3 {
        // Pad with numbers if too short
        format!("{}{}", combined, "001"[..3 - combined.len()].to_string())
    } else {
        combined
    }
}
