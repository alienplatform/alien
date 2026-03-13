use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::keyvault::KeyVaultManagementApi;
use alien_azure_clients::models::keyvault::{
    AccessPolicyEntry, Permissions, PermissionsSecretsItem, Sku, SkuFamily, SkuName,
    VaultCreateOrUpdateParameters, VaultProperties,
};
use alien_core::{AzureManagementConfig, ResourceOutputs, ResourceStatus, Vault, VaultOutputs};

/// Azure Vault controller.
///
/// Azure Key Vault is an actual Azure resource that needs to be created.
/// This controller manages the full lifecycle of the Key Vault resource.
#[controller]
pub struct AzureVaultController {
    /// The name of the Azure Key Vault
    pub(crate) vault_name: Option<String>,
    /// The resource group name where the vault is created
    pub(crate) resource_group_name: Option<String>,
    /// The vault URI
    pub(crate) vault_uri: Option<String>,
    /// Key Vault management client for Azure operations
    #[serde(skip)]
    pub(crate) vault_client: Option<Arc<dyn KeyVaultManagementApi>>,
}

#[controller]
impl AzureVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;
        let azure_config = ctx.get_azure_config()?;

        // Initialize the Key Vault management client
        self.vault_client = Some(
            ctx.service_provider
                .get_azure_key_vault_management_client(azure_config)?,
        );

        // Generate vault name and resource group
        self.vault_name = Some(format!("{}-{}", ctx.resource_prefix, config.id));
        self.resource_group_name = Some(format!("{}-rg", ctx.resource_prefix));
        self.vault_uri = Some(format!(
            "https://{}.vault.azure.net",
            self.vault_name.as_ref().unwrap()
        ));

        info!(
            vault_id = %config.id,
            vault_name = %self.vault_name.as_deref().unwrap_or("unknown"),
            resource_group = %self.resource_group_name.as_deref().unwrap_or("unknown"),
            "Starting Azure Key Vault creation"
        );

        // Create the Azure Key Vault
        self.create_azure_key_vault(ctx, azure_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Key Vault '{}'",
                    self.vault_name.as_deref().unwrap_or("unknown")
                ),
                resource_id: self.vault_name.clone(),
            })?;

        info!(
            vault_id = %config.id,
            vault_name = %self.vault_name.as_deref().unwrap_or("unknown"),
            vault_uri = %self.vault_uri.as_deref().unwrap_or("unknown"),
            "Azure Key Vault created successfully"
        );

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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let (Some(vault_name), Some(resource_group_name)) =
            (&self.vault_name, &self.resource_group_name)
        {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            // Build Azure resource scope for the Key Vault
            let resource_scope = Scope::Resource {
                resource_group_name: resource_group_name.clone(),
                resource_provider: "Microsoft.KeyVault".to_string(),
                parent_resource_path: None,
                resource_type: "vaults".to_string(),
                resource_name: vault_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                vault_name,
                resource_scope,
                "Vault",
            )
            .await?;
        }

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "Azure Key Vault update (no-op for now)"
        );

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
        let config = ctx.desired_resource_config::<Vault>()?;
        let azure_config = ctx.get_azure_config()?;

        info!(
            vault_id = %config.id,
            vault_name = %self.vault_name.as_deref().unwrap_or("unknown"),
            "Deleting Azure Key Vault"
        );

        // Initialize client if not already done
        if self.vault_client.is_none() {
            self.vault_client = Some(
                ctx.service_provider
                    .get_azure_key_vault_management_client(azure_config)?,
            );
        }

        // Delete the Azure Key Vault if it exists
        if let (Some(vault_name), Some(resource_group_name), Some(_vault_uri)) =
            (&self.vault_name, &self.resource_group_name, &self.vault_uri)
        {
            if let Some(client) = &self.vault_client {
                client
                    .delete_vault(resource_group_name.clone(), vault_name.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete Azure Key Vault '{}'", vault_name),
                        resource_id: Some(vault_name.clone()),
                    })?;

                info!(
                    vault_id = %config.id,
                    vault_name = %vault_name,
                    "Azure Key Vault deleted successfully"
                );
            }
        }

        self.vault_name = None;
        self.resource_group_name = None;
        self.vault_uri = None;
        self.vault_client = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        debug!(vault_id = %config.id, "Azure Key Vault ready (placeholder)");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
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
        if let (Some(vault_name), Some(resource_group_name), Some(_vault_uri)) =
            (&self.vault_name, &self.resource_group_name, &self.vault_uri)
        {
            let vault_id = format!("/subscriptions/{{subscriptionId}}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}", resource_group_name, vault_name);
            Some(ResourceOutputs::new(VaultOutputs { vault_id }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_name) = &self.vault_name {
            let binding = VaultBinding::key_vault(vault_name.clone());

            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

impl AzureVaultController {
    /// Create an Azure Key Vault with proper access policies
    async fn create_azure_key_vault(
        &self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &alien_azure_clients::AzureClientConfig,
    ) -> Result<()> {
        let vault_name = self.vault_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Vault name not set".to_string(),
                operation: Some("create_azure_key_vault".to_string()),
                resource_id: None,
            })
        })?;

        let resource_group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Resource group name not set".to_string(),
                operation: Some("create_azure_key_vault".to_string()),
                resource_id: None,
            })
        })?;

        let client = self.vault_client.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Key Vault client not initialized".to_string(),
                operation: Some("create_azure_key_vault".to_string()),
                resource_id: None,
            })
        })?;

        // Parse tenant ID from Azure config
        let tenant_id = Uuid::parse_str(&azure_config.tenant_id)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: format!("Invalid tenant ID format: {}", azure_config.tenant_id),
                operation: Some("create_azure_key_vault".to_string()),
                resource_id: Some(vault_name.clone()),
            })?;

        // Get the region, defaulting to East US if not specified
        let location = azure_config.region.as_deref().unwrap_or("East US");

        // Create access policy for the service principal with secret permissions
        // For production, this should be configured with proper identity management
        let access_policies = if let Some(azure_management) = ctx.get_azure_management_config()? {
            let principal_id = &azure_management.management_principal_id;
            let principal_uuid = Uuid::parse_str(principal_id).into_alien_error().context(
                ErrorData::InfrastructureError {
                    message: format!("Invalid management principal ID format: {}", principal_id),
                    operation: Some("create_azure_key_vault".to_string()),
                    resource_id: Some(vault_name.clone()),
                },
            )?;

            vec![AccessPolicyEntry {
                application_id: None,
                object_id: principal_uuid.to_string(),
                permissions: Permissions {
                    certificates: vec![],
                    keys: vec![],
                    secrets: vec![
                        PermissionsSecretsItem::Get,
                        PermissionsSecretsItem::Set,
                        PermissionsSecretsItem::List,
                        PermissionsSecretsItem::Delete,
                    ],
                    storage: vec![],
                },
                tenant_id,
            }]
        } else {
            // No management config - create vault without management access policies
            vec![]
        };

        let vault_properties = VaultProperties {
            access_policies,
            create_mode: None,
            enable_purge_protection: None,
            enable_rbac_authorization: false, // Use access policies for now
            enable_soft_delete: true,
            enabled_for_deployment: false,
            enabled_for_disk_encryption: false,
            enabled_for_template_deployment: false,
            hsm_pool_resource_id: None,
            network_acls: None,
            private_endpoint_connections: vec![],
            provisioning_state: None,
            public_network_access: "Enabled".to_string(),
            sku: Sku {
                name: SkuName::Standard,
                family: SkuFamily::A,
            },
            soft_delete_retention_in_days: 7, // Minimum retention period
            tenant_id,
            vault_uri: None,
        };

        let mut tags = HashMap::new();
        tags.insert("ManagedBy".to_string(), "Alien".to_string());
        tags.insert("Environment".to_string(), "Production".to_string());

        let vault_params = VaultCreateOrUpdateParameters {
            location: location.to_string(),
            properties: vault_properties,
            tags,
        };

        info!(
            vault_name = %vault_name,
            resource_group = %resource_group_name,
            location = %location,
            "Creating Azure Key Vault with parameters"
        );

        client
            .create_or_update_vault(
                resource_group_name.clone(),
                vault_name.clone(),
                vault_params,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Azure Key Vault creation failed for vault '{}'", vault_name),
                resource_id: Some(vault_name.clone()),
            })?;

        Ok(())
    }
}
