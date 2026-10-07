use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};
use uuid::Uuid;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::keyvault::{
    Sku, SkuFamily, SkuName, Vault as AzureVaultModel, VaultCreateOrUpdateParameters,
    VaultProperties,
};
use alien_azure_clients::{authorization::Scope, keyvault::KeyVaultManagementApi};
use alien_core::{
    AzureKeyVaultHeartbeatData, HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, Vault,
    VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use chrono::Utc;

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

impl AzureVaultController {
    async fn apply_permissions(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        let vault_name = self.vault_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Vault name is missing from saved controller state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Vault resource group is missing from saved controller state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
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
            "vault",
        )
        .await?;

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

        Ok(())
    }

    /// Creates a ready Azure Key Vault controller for tests.
    pub fn mock_ready(vault_id: &str) -> Self {
        Self {
            state: AzureVaultState::Ready,
            vault_name: Some(format!("test-{vault_id}")),
            resource_group_name: Some("test-rg".to_string()),
            vault_uri: Some(format!("https://test-{vault_id}.vault.azure.net")),
            vault_client: None,
            _internal_stay_count: None,
        }
    }
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
        self.resource_group_name =
            Some(crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?);
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
        self.create_azure_key_vault(azure_config)
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
        self.apply_permissions(ctx).await?;

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
            "Reconciling Azure Key Vault permissions"
        );

        self.apply_permissions(ctx).await?;

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
        let azure_config = ctx.get_azure_config()?;

        if self.vault_client.is_none() {
            self.vault_client = Some(
                ctx.service_provider
                    .get_azure_key_vault_management_client(azure_config)?,
            );
        }

        if let (Some(vault_name), Some(resource_group_name), Some(client)) = (
            &self.vault_name,
            &self.resource_group_name,
            &self.vault_client,
        ) {
            let vault = client
                .get_vault(resource_group_name.clone(), vault_name.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get Azure Key Vault '{}'", vault_name),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_azure_key_vault_heartbeat(ctx, &config.id, resource_group_name, vault);
        }

        debug!(vault_id = %config.id, "Azure Key Vault heartbeat check passed");

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
            let vault_id = format!(
                "/subscriptions/{{subscriptionId}}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}",
                resource_group_name, vault_name
            );
            Some(ResourceOutputs::new(VaultOutputs { vault_id }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_name) = &self.vault_name {
            let binding = VaultBinding::key_vault(vault_name.clone());

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

fn emit_azure_key_vault_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    vault: AzureVaultModel,
) {
    let provisioning_state = vault
        .properties
        .provisioning_state
        .as_ref()
        .map(ToString::to_string);
    let health = match provisioning_state.as_deref() {
        Some("Succeeded") => ObservedHealth::Healthy,
        Some(_) => ObservedHealth::Degraded,
        None => ObservedHealth::Unknown,
    };
    let lifecycle = match provisioning_state.as_deref() {
        Some("Succeeded") => ProviderLifecycleState::Running,
        Some(_) => ProviderLifecycleState::Updating,
        None => ProviderLifecycleState::Unknown,
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Vault::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Vault(VaultHeartbeatData::AzureKeyVault(
            AzureKeyVaultHeartbeatData {
                status: VaultHeartbeatStatus {
                    health,
                    lifecycle,
                    message: Some(format!(
                        "Azure Key Vault management metadata is reachable; provisioning state is {}",
                        provisioning_state.as_deref().unwrap_or("unknown")
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: vault.name.unwrap_or_else(|| resource_id.to_string()),
                resource_group: Some(resource_group_name.to_string()),
                resource_id: vault.id,
                location: vault.location,
                vault_uri: vault.properties.vault_uri,
                provisioning_state,
                sku_family: Some(vault.properties.sku.family.to_string()),
                sku_name: Some(vault.properties.sku.name.to_string()),
                soft_delete_enabled: vault.properties.enable_soft_delete,
                soft_delete_retention_days: vault.properties.soft_delete_retention_in_days,
                purge_protection_enabled: vault.properties.enable_purge_protection,
                rbac_authorization_enabled: vault.properties.enable_rbac_authorization,
                public_network_access: vault.properties.public_network_access,
                access_policy_count: vault.properties.access_policies.len() as u32,
                private_endpoint_connection_count: vault
                    .properties
                    .private_endpoint_connections
                    .len() as u32,
                secret_metadata_listed: false,
            },
        )),
        raw: vec![],
    });
}

impl AzureVaultController {
    /// Create an Azure Key Vault with proper access policies
    async fn create_azure_key_vault(
        &self,
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
}

#[cfg(test)]
mod permission_update_tests {
    use super::*;
    use crate::core::{
        MockPlatformServiceProvider, ResourceController, StackExecutor, StackResourceStateExt,
    };
    use crate::infra_requirements::AzureResourceGroupController;
    use crate::service_account::AzureServiceAccountController;
    use alien_azure_clients::{AzureClientConfigExt as _, authorization::MockAuthorizationApi};
    use alien_client_core::ErrorData as CloudError;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{
        AzureClientConfig, AzureResourceGroup, ClientConfig, DeploymentConfig,
        EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority, Resource,
        ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackResourceState, StackSettings,
        StackState,
    };
    use std::sync::{Arc, Mutex};

    // Simulate role assignment upserts, including a lost response.
    // Unexpected provider calls fail the mock.
    fn fixture(
        lifecycle: ResourceLifecycle,
        authority: InitialSetupAuthority,
        writes: usize,
        lose_first_response: bool,
    ) -> (StackExecutor, StackState, Arc<Mutex<Vec<String>>>) {
        let policies = Arc::new(Mutex::new(Vec::new()));
        let saved = policies.clone();
        let mut authorization = MockAuthorizationApi::new();
        authorization.expect_create_or_update_role_assignment_by_id()
            .times(writes)
            .returning(move |id, assignment| {
                let properties = assignment.properties.as_ref().unwrap();
                assert!(id.contains("/providers/Microsoft.KeyVault/vaults/test-secrets/providers/Microsoft.Authorization/roleAssignments/"));
                assert_eq!(properties.principal_id, "87654321-4321-4321-4321-210987654321");
                assert!(properties.role_definition_id.ends_with("4633458b-17de-408a-b874-0445c86b69e6"));
                let mut saved = saved.lock().unwrap();
                saved.push(id.to_string());
                if lose_first_response && saved.len() == 1 {
                    return Err(AlienError::new(CloudError::HttpRequestFailed {
                        message: "Connection closed after the role assignment".to_string(),
                    }));
                }
                Ok(assignment.clone())
            });
        let authorization = Arc::new(authorization);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_authorization_client()
            .times(writes)
            .returning(move |_| Ok(authorization.clone()));
        let vault = Vault::new("secrets".to_string()).build();
        let account = ServiceAccount::new("consumer-sa".to_string()).build();
        let stack = Stack::new("test".to_string())
            .add_with_dependencies(
                vault.clone(),
                lifecycle,
                vec![ResourceRef::new(
                    ServiceAccount::RESOURCE_TYPE,
                    "consumer-sa",
                )],
            )
            .add(account.clone(), ResourceLifecycle::Frozen)
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .permission(
                "consumer",
                PermissionProfile::new().resource("secrets", ["vault/data-read"]),
            )
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .build();
        let executor = StackExecutor::builder(
            &stack,
            ClientConfig::Azure(Box::new(AzureClientConfig::mock())),
        )
        .deployment_config(&config)
        .service_provider(Arc::new(provider))
        .initial_setup_authority(authority)
        .step_running_resources(false)
        .build()
        .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Azure, "test".to_string());
        let controller = AzureVaultController::mock_ready("secrets");
        let mut vault_state = StackResourceState::new_pending(
            Vault::RESOURCE_TYPE.to_string(),
            Resource::new(vault),
            Some(lifecycle),
            vec![],
        );
        vault_state.status = ResourceStatus::Running;
        vault_state.outputs = controller.get_outputs();
        vault_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("secrets".to_string(), vault_state);
        // The newly created role is ready, but the existing vault has not yet
        // recorded its new dependency or assigned vault read access.
        let controller = AzureServiceAccountController::mock_ready("test-consumer-sa");
        let mut account_state = StackResourceState::new_pending(
            ServiceAccount::RESOURCE_TYPE.to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        account_state.status = ResourceStatus::Running;
        account_state.outputs = controller.get_outputs();
        account_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state
            .resources
            .insert("consumer-sa".to_string(), account_state);
        let group = AzureResourceGroup::new("default-resource-group".to_string()).build();
        let controller = AzureResourceGroupController::mock_ready("test-rg");
        let mut group_state = StackResourceState::new_pending(
            AzureResourceGroup::RESOURCE_TYPE.to_string(),
            Resource::new(group),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        group_state.status = ResourceStatus::Running;
        group_state.outputs = controller.get_outputs();
        group_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state
            .resources
            .insert("default-resource-group".to_string(), group_state);
        (executor, state, policies)
    }

    #[tokio::test]
    async fn setup_update_grants_existing_vault_access_to_consumer() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            1,
            false,
        );
        assert!(
            executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("secrets")
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(policies.lock().unwrap().len(), 1);
        // Repeating setup does not schedule another update after convergence.
        assert!(
            !executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("secrets")
        );
    }

    #[tokio::test]
    async fn imported_vault_update_refuses_permission_writes() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::ImportedHandoff,
            0,
            false,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(
            state.resources["secrets"]
                .error
                .as_ref()
                .unwrap()
                .to_string()
                .contains("rerun setup")
        );
        assert!(policies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn live_vault_update_assigns_access_without_recreating_the_vault() {
        let (executor, state, assignments) = fixture(
            ResourceLifecycle::Live,
            InitialSetupAuthority::ImportedHandoff,
            1,
            false,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(assignments.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn update_refuses_missing_saved_vault_identifiers() {
        for missing_name in [true, false] {
            let (executor, mut state, assignments) = fixture(
                ResourceLifecycle::Frozen,
                InitialSetupAuthority::DirectSetup,
                0,
                false,
            );
            let resource = state.resources.get_mut("secrets").unwrap();
            let mut controller = resource
                .get_internal_controller_typed::<AzureVaultController>()
                .unwrap();
            if missing_name {
                controller.vault_name = None;
            } else {
                controller.resource_group_name = None;
            }
            resource
                .set_internal_controller(Some(Box::new(controller)))
                .unwrap();
            let state = executor.step(state).await.unwrap().next_state;
            assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
            assert!(
                state.resources["secrets"]
                    .error
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("missing from saved controller state")
            );
            assert!(assignments.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn lost_response_resumes_the_saved_update_and_upserts_the_same_policy() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            2,
            true,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"].error.is_some());
        // Reload the durable checkpoint and drive the executor's actual retry.
        let state: StackState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        let policies = policies.lock().unwrap();
        assert_eq!(policies.len(), 2);
        assert_eq!(policies[0], policies[1]);
    }
}
