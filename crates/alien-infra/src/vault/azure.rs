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
    Sku, SkuFamily, SkuName, VaultCreateOrUpdateParameters, VaultProperties,
};
use alien_core::{ResourceOutputs, ResourceStatus, Vault, VaultOutputs};

/// Azure built-in role ID for "Key Vault Secrets Officer"
/// Grants full access to manage Key Vault secrets.
const KEY_VAULT_SECRETS_OFFICER_ROLE_ID: &str = "b86a8fe4-44ce-4948-aee5-eccb2c155cd7";

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

        // Generate vault name and look up resource group from infra requirements
        self.vault_name = Some(format!("{}-{}", ctx.resource_prefix, config.id));
        self.resource_group_name = Some(
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?,
        );
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

        // Assign Key Vault Secrets Officer to the controller's identity so it can
        // manage secrets (e.g., sync deployment env vars) in this vault.
        self.assign_controller_vault_role(ctx, azure_config).await?;

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

        // Use RBAC authorization — permissions are managed via Azure role assignments
        // created by the service account controller, not vault access policies.
        let vault_properties = VaultProperties {
            access_policies: vec![],
            create_mode: None,
            enable_purge_protection: None,
            enable_rbac_authorization: true,
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

    /// Assign the Key Vault Secrets Officer built-in role to the controller's
    /// own service principal so it can sync secrets to this vault.
    async fn assign_controller_vault_role(
        &self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &alien_azure_clients::AzureClientConfig,
    ) -> Result<()> {
        use alien_azure_clients::authorization::{AuthorizationApi, Scope};
        use alien_azure_clients::extract_oid_from_token;
        use alien_azure_clients::AzureClientConfigExt;
        use alien_azure_clients::models::authorization_role_assignments::{
            RoleAssignment, RoleAssignmentProperties,
            RoleAssignmentPropertiesPrincipalType,
        };

        let vault_name = self.vault_name.as_deref().unwrap_or("unknown");
        let resource_group_name = self.resource_group_name.as_deref().unwrap_or("unknown");

        // Get a management token and extract the caller's object ID
        let token = azure_config
            .get_bearer_token()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get bearer token for controller identity".to_string(),
                resource_id: Some(vault_name.to_string()),
            })?;

        let controller_oid = extract_oid_from_token(&token).context(ErrorData::CloudPlatformError {
            message: "Failed to extract controller object ID from token".to_string(),
            resource_id: Some(vault_name.to_string()),
        })?;

        info!(
            vault_name = %vault_name,
            controller_oid = %controller_oid,
            "Assigning Key Vault Secrets Officer role to controller"
        );

        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;

        let scope = Scope::Resource {
            resource_group_name: resource_group_name.to_string(),
            resource_provider: "Microsoft.KeyVault".to_string(),
            parent_resource_path: None,
            resource_type: "vaults".to_string(),
            resource_name: vault_name.to_string(),
        };

        let full_role_definition_id = format!(
            "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            azure_config.subscription_id, KEY_VAULT_SECRETS_OFFICER_ROLE_ID
        );

        let assignment_name = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "alien:azure:vault-controller-role:{}:{}",
                vault_name, controller_oid
            )
            .as_bytes(),
        )
        .to_string();

        let role_assignment_id = auth_client.build_role_assignment_id(&scope, assignment_name);

        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            properties: Some(RoleAssignmentProperties {
                condition: None,
                condition_version: None,
                created_by: None,
                created_on: None,
                delegated_managed_identity_resource_id: None,
                description: Some(format!(
                    "Alien controller Key Vault Secrets Officer for vault {}",
                    vault_name
                )),
                principal_id: controller_oid.clone(),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                role_definition_id: full_role_definition_id,
                scope: Some(format!(
                    "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                    azure_config.subscription_id, resource_group_name, vault_name
                )),
                updated_by: None,
                updated_on: None,
            }),
            type_: None,
        };

        auth_client
            .create_or_update_role_assignment_by_id(role_assignment_id, &role_assignment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to assign Key Vault Secrets Officer to controller on vault '{}'",
                    vault_name
                ),
                resource_id: Some(vault_name.to_string()),
            })?;

        info!(
            vault_name = %vault_name,
            "Controller Key Vault Secrets Officer role assigned successfully"
        );

        Ok(())
    }
}
