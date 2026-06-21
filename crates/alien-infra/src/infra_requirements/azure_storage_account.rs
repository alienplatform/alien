use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::azure_utils::{azure_storage_account_resource_id, get_resource_group_name};
use crate::core::{
    AzureStorageAccountArmResource, AzureStorageAccountEndpoints as AzureStorageArmEndpoints,
    AzureStorageAccountProperties, AzureStorageSku, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    AzureClientConfig, AzureStorageAccount, AzureStorageAccountEndpoints,
    AzureStorageAccountHeartbeatData, AzureStorageAccountOutputs, HeartbeatBackend, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, StorageHeartbeatStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AzureStorageAccountController {
    /// The actual Azure storage account name (may differ from config.id).
    pub(crate) account_name: Option<String>,
    /// The Azure resource ID of the storage account.
    pub(crate) resource_id: Option<String>,
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
                if let Some(provisioning_state) = properties
                    .as_ref()
                    .and_then(|p| p.provisioning_state.as_deref())
                {
                    match provisioning_state {
                        "Succeeded" => {
                            info!(account_name=%account_name, "Storage account creation completed, retrieving details");

                            // Extract details from the response
                            self.resource_id = account_info.id.clone().or_else(|| {
                                Some(azure_storage_account_resource_id(
                                    &azure_config.subscription_id,
                                    &resource_group_name,
                                    account_name,
                                ))
                            });
                            let primary_endpoints = properties.and_then(|p| p.primary_endpoints);
                            self.primary_blob_endpoint = primary_endpoints
                                .as_ref()
                                .and_then(|e| e.blob.clone())
                                .or_else(|| Some(azure_storage_endpoint(account_name, "blob")));
                            self.primary_file_endpoint = primary_endpoints
                                .as_ref()
                                .and_then(|e| e.file.clone())
                                .or_else(|| Some(azure_storage_endpoint(account_name, "file")));
                            self.primary_queue_endpoint = primary_endpoints
                                .as_ref()
                                .and_then(|e| e.queue.clone())
                                .or_else(|| Some(azure_storage_endpoint(account_name, "queue")));
                            self.primary_table_endpoint = primary_endpoints
                                .as_ref()
                                .and_then(|e| e.table.clone())
                                .or_else(|| Some(azure_storage_endpoint(account_name, "table")));

                            info!(account_name=%account_name, "Successfully retrieved storage account details");

                            Ok(HandlerAction::Continue {
                                state: ApplyingPermissions,
                                suggested_delay: None,
                            })
                        }
                        "Creating" | "ResolvingDns" => {
                            debug!(account_name=%account_name, "Storage account still being created");
                            Ok(HandlerAction::Continue {
                                state: CreatingStorageAccount,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                        "Failed" => Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: "Storage account creation failed with status: Failed"
                                .to_string(),
                            resource_id: Some(config.id.clone()),
                        })),
                        other => {
                            debug!(account_name=%account_name, state=%other, "Storage account is not ready yet");
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

            emit_azure_storage_account_heartbeat(
                ctx,
                &config.id,
                &resource_group_name,
                &storage_account,
            );

            if let Some(properties) = &storage_account.properties {
                if properties.provisioning_state.as_deref() != Some("Succeeded") {
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
            Some(primary_blob_endpoint),
            Some(primary_file_endpoint),
            Some(primary_queue_endpoint),
            Some(primary_table_endpoint),
        ) = (
            &self.account_name,
            &self.resource_id,
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
            }))
        } else {
            None
        }
    }
}

fn azure_storage_endpoint(account_name: &str, service: &str) -> String {
    format!("https://{account_name}.{service}.core.windows.net/")
}

fn emit_azure_storage_account_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    account: &AzureStorageAccountArmResource,
) {
    let properties = account.properties.as_ref();
    let provisioning_state = properties.and_then(|p| p.provisioning_state.as_deref());
    let (health, lifecycle) = match provisioning_state {
        Some("Succeeded") => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
        Some(_) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Failed),
        None => (ObservedHealth::Unknown, ProviderLifecycleState::Unknown),
    };
    let name = account
        .name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: AzureStorageAccount::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::AzureStorageAccount(AzureStorageAccountHeartbeatData {
            status: StorageHeartbeatStatus {
                health,
                lifecycle,
                message: Some(format!(
                    "Azure storage account '{}' provisioning state is {}",
                    name,
                    provisioning_state
                        .map(ToString::to_string)
                        .as_deref()
                        .unwrap_or("unknown")
                )),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name,
            resource_id: account.id.clone(),
            resource_group: Some(resource_group_name.to_string()),
            location: Some(account.location.clone()),
            kind: account.kind.clone(),
            sku_name: account.sku.as_ref().map(|sku| sku.name.clone()),
            sku_tier: account.sku.as_ref().and_then(|sku| sku.tier.clone()),
            provisioning_state: provisioning_state.map(str::to_string),
            primary_endpoints: storage_account_endpoints(
                properties.and_then(|p| p.primary_endpoints.as_ref()),
            ),
            secondary_endpoints: storage_account_endpoints(
                properties.and_then(|p| p.secondary_endpoints.as_ref()),
            ),
            public_network_access: properties.and_then(|p| p.public_network_access.clone()),
            allow_blob_public_access: properties.and_then(|p| p.allow_blob_public_access),
            allow_shared_key_access: properties.and_then(|p| p.allow_shared_key_access),
            minimum_tls_version: properties.and_then(|p| p.minimum_tls_version.clone()),
            supports_https_traffic_only: properties.and_then(|p| p.supports_https_traffic_only),
            encryption_key_source: properties
                .and_then(|p| p.encryption.as_ref())
                .and_then(|encryption| encryption.key_source.clone()),
            require_infrastructure_encryption: properties
                .and_then(|p| p.encryption.as_ref())
                .and_then(|encryption| encryption.require_infrastructure_encryption),
            network_default_action: properties
                .and_then(|p| p.network_acls.as_ref())
                .and_then(|rules| rules.default_action.clone()),
            network_bypass: properties
                .and_then(|p| p.network_acls.as_ref())
                .and_then(|rules| rules.bypass.clone()),
            network_ip_rule_count: properties
                .and_then(|p| p.network_acls.as_ref())
                .map(|rules| rules.ip_rules.len() as u32),
            network_virtual_network_rule_count: properties
                .and_then(|p| p.network_acls.as_ref())
                .map(|rules| rules.virtual_network_rules.len() as u32),
            network_resource_access_rule_count: properties
                .and_then(|p| p.network_acls.as_ref())
                .map(|rules| rules.resource_access_rules.len() as u32),
        }),
        raw: vec![],
    });
}

fn storage_account_endpoints(
    endpoints: Option<&AzureStorageArmEndpoints>,
) -> AzureStorageAccountEndpoints {
    endpoints
        .map(|endpoints| AzureStorageAccountEndpoints {
            blob: endpoints.blob.clone(),
            dfs: endpoints.dfs.clone(),
            file: endpoints.file.clone(),
            queue: endpoints.queue.clone(),
            table: endpoints.table.clone(),
            web: endpoints.web.clone(),
        })
        .unwrap_or_default()
}

// Separate impl block for helper methods
impl AzureStorageAccountController {
    fn build_storage_account_params(
        &self,
        azure_config: &AzureClientConfig,
        ctx: &ResourceControllerContext,
    ) -> AzureStorageAccountArmResource {
        let location = azure_config.region.as_deref().unwrap_or("East US");

        let mut tags = HashMap::new();
        tags.insert("resource-type".to_string(), "storage".to_string());
        tags.insert("resource".to_string(), ctx.desired_config.id().to_string());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

        AzureStorageAccountArmResource {
            id: None,
            kind: Some("StorageV2".to_string()),
            location: location.to_string(),
            name: None,
            properties: Some(AzureStorageAccountProperties {
                supports_https_traffic_only: Some(true),
                ..Default::default()
            }),
            sku: Some(AzureStorageSku {
                name: "Standard_LRS".to_string(),
                tier: None,
            }),
            tags,
            type_: None,
        }
    }

    fn clear_state(&mut self) {
        self.account_name = None;
        self.resource_id = None;
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
        use crate::core::Scope;
        use alien_permissions::PermissionContext;

        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        let azure_config = ctx.get_azure_config()?;

        // Build permission context for this specific storage account resource
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let mut permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group)
            .with_storage_account_name(account_name.to_string())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(account_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }

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
        Self {
                state: AzureStorageAccountState::Ready,
                account_name: Some(account_name.to_string()),
                resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.Storage/storageAccounts/{}", account_name)),
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
