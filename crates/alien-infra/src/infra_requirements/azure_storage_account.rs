use std::collections::{BTreeSet, HashMap};
use std::time::Duration;
use tracing::{debug, error, info};

use crate::azure_utils::{azure_storage_account_resource_id, get_resource_group_name};
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::blob::{
    BlobServiceProperties, CorsRule, CorsRuleAllowedMethodsItem, CorsRules,
};
use alien_azure_clients::models::storage::{
    Endpoints, StorageAccount, StorageAccountCreateParameters,
    StorageAccountPropertiesProvisioningState, StorageAccountPropertiesUpdateParameters,
    StorageAccountUpdateParameters,
};
use alien_azure_clients::AzureClientConfig;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureStorageAccount, AzureStorageAccountEndpoints, AzureStorageAccountHeartbeatData,
    AzureStorageAccountOutputs, HeartbeatBackend, InitialSetupAuthority, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

mod lifecycle;

#[controller]
pub struct AzureStorageAccountController {
    /// Rule names recorded before a direct-setup policy write; imports cannot adopt rules.
    #[serde(default)]
    pub(crate) lifecycle_rule_names: BTreeSet<String>,
    /// Last shared capability configuration verified by direct setup. Imports remain unconfirmed.
    #[serde(default)]
    pub(crate) capabilities_revision: Option<String>,
    /// Aggregate configuration being applied; changed declarations restart the owning flow.
    #[serde(default)]
    pub(crate) pending_capabilities_revision: Option<String>,
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
        require_direct_setup(ctx)?;
        let config = ctx.desired_resource_config::<AzureStorageAccount>()?;
        info!(id=%config.id, "Initiating Azure Storage Account creation");

        let azure_config = ctx.get_azure_config()?;
        let account_name = generate_storage_account_name(&ctx.resource_prefix, &config.id);
        let resource_group_name = get_resource_group_name(ctx.state)?;

        // Create the storage account via Azure client
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;
        let mut params = self.build_storage_account_params(azure_config, ctx);
        if let Some(properties) = params.properties.as_mut() {
            properties.allow_blob_public_access = Some(shared_capabilities(ctx)?.public);
        }

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

                            // Extract details from the response
                            self.resource_id = account_info.id.or_else(|| {
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
                                state: ConfiguringAccount,
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

    #[handler(state = ConfiguringAccount, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn configuring_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        self.capabilities_revision = None;
        self.pending_capabilities_revision = Some(shared_capabilities(ctx)?.revision());
        self.set_public_capability(ctx).await?;
        Ok(HandlerAction::Continue {
            state: ConfiguringBlobService,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(state = ConfiguringBlobService, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn configuring_blob_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: ConfiguringAccount,
                suggested_delay: None,
            });
        }
        self.set_shared_blob_properties(ctx).await?;
        Ok(HandlerAction::Continue {
            state: VerifyingBlobService,
            suggested_delay: None,
        })
    }

    #[handler(state = VerifyingBlobService, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn verifying_blob_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: ConfiguringAccount,
                suggested_delay: None,
            });
        }
        self.verify_shared_capabilities(ctx).await?;
        Ok(HandlerAction::Continue {
            state: ConfiguringLifecycle,
            suggested_delay: None,
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        self.capabilities_revision = None;
        self.pending_capabilities_revision = Some(shared_capabilities(ctx)?.revision());
        self.set_public_capability(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdatingBlobService,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(state = UpdatingBlobService, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn updating_blob_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        self.set_shared_blob_properties(ctx).await?;
        Ok(HandlerAction::Continue {
            state: VerifyingUpdatedBlobService,
            suggested_delay: None,
        })
    }

    #[handler(state = VerifyingUpdatedBlobService, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn verifying_updated_blob_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        self.verify_shared_capabilities(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdatingLifecycle,
            suggested_delay: None,
        })
    }

    #[handler(state = ConfiguringLifecycle, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn configuring_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: ConfiguringAccount,
                suggested_delay: None,
            });
        }
        if !self.reconcile_lifecycle(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: ConfiguringLifecycle,
                suggested_delay: None,
            });
        }
        Ok(HandlerAction::Continue {
            state: VerifyingLifecycle,
            suggested_delay: None,
        })
    }

    #[handler(state = VerifyingLifecycle, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn verifying_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        let desired = shared_capabilities(ctx)?;
        if self.pending_capabilities_revision.as_deref() != Some(desired.revision().as_str()) {
            return Ok(HandlerAction::Continue {
                state: ConfiguringAccount,
                suggested_delay: None,
            });
        }
        self.verify_lifecycle(ctx).await?;
        self.capabilities_revision = Some(desired.revision());
        self.pending_capabilities_revision = None;
        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
    }

    #[handler(state = UpdatingLifecycle, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn updating_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        if self.pending_capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        if !self.reconcile_lifecycle(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: UpdatingLifecycle,
                suggested_delay: None,
            });
        }
        Ok(HandlerAction::Continue {
            state: VerifyingUpdatedLifecycle,
            suggested_delay: None,
        })
    }

    #[handler(state = VerifyingUpdatedLifecycle, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn verifying_updated_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        require_direct_setup(ctx)?;
        let desired = shared_capabilities(ctx)?;
        if self.pending_capabilities_revision.as_deref() != Some(desired.revision().as_str()) {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        self.verify_lifecycle(ctx).await?;
        self.capabilities_revision = Some(desired.revision());
        self.pending_capabilities_revision = None;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup {
            return Ok(false);
        }
        Ok(self.capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str()))
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
        require_direct_setup(ctx)?;
        // An old checkpoint may enter here without having configured shared settings.
        if self.capabilities_revision.as_deref()
            != Some(shared_capabilities(ctx)?.revision().as_str())
        {
            return Ok(HandlerAction::Continue {
                state: ConfiguringAccount,
                suggested_delay: None,
            });
        }
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
        // A matching declaration revision does not prove that setup-owned settings
        // still match Azure. Privileged setup must inspect unchanged installations
        // before it can report success; imported runtime never gains write authority.
        if ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup {
            self.capabilities_revision = None;
            let verification = match self.verify_shared_capabilities(ctx).await {
                Ok(()) => self.verify_lifecycle(ctx).await,
                Err(error) => Err(error),
            };
            if let Err(error) = verification {
                if matches!(error.error, Some(ErrorData::ResourceDrift { .. })) {
                    return Ok(HandlerAction::Continue {
                        state: UpdateStart,
                        suggested_delay: None,
                    });
                }
                return Err(error);
            }
            self.capabilities_revision = Some(shared_capabilities(ctx)?.revision());
        }
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
        // Creation can fail before the provider returns an account. Its name
        // is deterministic, so teardown can still delete it or accept a 404.
        let account_name = self
            .account_name
            .clone()
            .unwrap_or_else(|| generate_storage_account_name(&ctx.resource_prefix, &config.id));

        self.account_name = Some(account_name.clone());

        info!(account_name=%account_name, "Initiating Azure Storage Account deletion");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;

        match client
            .delete_storage_account(&resource_group_name, &account_name)
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

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

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
    account: &StorageAccount,
) {
    let properties = account.properties.as_ref();
    let provisioning_state = properties.and_then(|p| p.provisioning_state.as_ref());
    let (health, lifecycle) = match provisioning_state {
        Some(StorageAccountPropertiesProvisioningState::Succeeded) => {
            (ObservedHealth::Healthy, ProviderLifecycleState::Running)
        }
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
            kind: account.kind.as_ref().map(ToString::to_string),
            sku_name: account.sku.as_ref().map(|sku| sku.name.to_string()),
            sku_tier: account
                .sku
                .as_ref()
                .and_then(|sku| sku.tier.as_ref().map(ToString::to_string)),
            provisioning_state: provisioning_state.map(ToString::to_string),
            primary_endpoints: storage_account_endpoints(
                properties.and_then(|p| p.primary_endpoints.as_ref()),
            ),
            secondary_endpoints: storage_account_endpoints(
                properties.and_then(|p| p.secondary_endpoints.as_ref()),
            ),
            public_network_access: properties
                .and_then(|p| p.public_network_access.as_ref())
                .map(ToString::to_string),
            allow_blob_public_access: properties.and_then(|p| p.allow_blob_public_access),
            allow_shared_key_access: properties.and_then(|p| p.allow_shared_key_access),
            minimum_tls_version: properties
                .and_then(|p| p.minimum_tls_version.as_ref())
                .map(ToString::to_string),
            supports_https_traffic_only: properties.and_then(|p| p.supports_https_traffic_only),
            encryption_key_source: properties
                .and_then(|p| p.encryption.as_ref())
                .map(|encryption| encryption.key_source.to_string()),
            require_infrastructure_encryption: properties
                .and_then(|p| p.encryption.as_ref())
                .and_then(|encryption| encryption.require_infrastructure_encryption),
            network_default_action: properties
                .and_then(|p| p.network_acls.as_ref())
                .map(|rules| rules.default_action.to_string()),
            network_bypass: properties
                .and_then(|p| p.network_acls.as_ref())
                .map(|rules| rules.bypass.to_string()),
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

fn storage_account_endpoints(endpoints: Option<&Endpoints>) -> AzureStorageAccountEndpoints {
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
    ) -> StorageAccountCreateParameters {
        use alien_azure_clients::models::storage::*;

        let location = azure_config.region.as_deref().unwrap_or("East US");

        let mut tags = HashMap::new();
        tags.insert("resource-type".to_string(), "storage".to_string());
        tags.insert("resource".to_string(), ctx.desired_config.id().to_string());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

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
        self.capabilities_revision = None;
        self.pending_capabilities_revision = None;
        self.lifecycle_rule_names.clear();
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
        use alien_azure_clients::authorization::Scope;
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
            lifecycle_rule_names: BTreeSet::new(),
            capabilities_revision: None,
            pending_capabilities_revision: None,
            account_name: Some(account_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.Storage/storageAccounts/{}",
                account_name
            )),
            primary_blob_endpoint: Some(format!("https://{}.blob.core.windows.net/", account_name)),
            primary_file_endpoint: Some(format!("https://{}.file.core.windows.net/", account_name)),
            primary_queue_endpoint: Some(format!(
                "https://{}.queue.core.windows.net/",
                account_name
            )),
            primary_table_endpoint: Some(format!(
                "https://{}.table.core.windows.net/",
                account_name
            )),
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

#[derive(Debug, Default)]
struct SharedCapabilities {
    public: bool,
    versioning: bool,
    origins: BTreeSet<String>,
    lifecycle: BTreeSet<(String, String, u32)>,
}

impl SharedCapabilities {
    fn revision(&self) -> String {
        // BTreeSet makes declaration order and duplicate contributors irrelevant.
        format!(
            "{}:{}:{:?}:{:?}",
            self.public, self.versioning, self.origins, self.lifecycle
        )
    }

    fn cors(&self) -> CorsRules {
        CorsRules {
            cors_rules: if self.origins.is_empty() {
                vec![]
            } else {
                vec![CorsRule {
                    allowed_origins: self.origins.iter().cloned().collect(),
                    allowed_methods: vec![
                        CorsRuleAllowedMethodsItem::Get,
                        CorsRuleAllowedMethodsItem::Head,
                    ],
                    allowed_headers: vec!["*".into()],
                    exposed_headers: vec![
                        "ETag".into(),
                        "Content-Range".into(),
                        "Accept-Ranges".into(),
                    ],
                    max_age_in_seconds: 3600,
                }]
            },
        }
    }
}

fn cloud_error(ctx: &ResourceControllerContext<'_>, operation: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(ctx.desired_config.id().to_owned()),
        message: operation.to_owned(),
    }
}

fn require_direct_setup(ctx: &ResourceControllerContext<'_>) -> Result<()> {
    if ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup {
        return Err(AlienError::new(ErrorData::SetupRerunRequired {
            resource_id: ctx.desired_config.id().to_owned(),
            message: "Shared Azure storage settings require an explicit direct setup run; imported runtime credentials cannot write them".into(),
        }));
    }
    Ok(())
}

fn shared_capabilities(ctx: &ResourceControllerContext<'_>) -> Result<SharedCapabilities> {
    let mut result = SharedCapabilities::default();
    // Deployment gate resolution removes declined resources before the executor.
    // Do not re-evaluate defaults here: persisted Frozen gate answers may override them.
    for (_, entry) in ctx.desired_stack.resources() {
        let Some(storage) = entry.config.downcast_ref::<Storage>() else {
            continue;
        };
        for rule in &storage.lifecycle_rules {
            result.lifecycle.insert((
                crate::storage::azure::get_azure_container_name(&ctx.resource_prefix, &storage.id),
                rule.prefix.clone().unwrap_or_default(),
                rule.days,
            ));
        }
        result.public |= storage.public_read;
        result.versioning |= storage.versioning;
        result
            .origins
            .extend(storage.cors_allowed_origins.iter().cloned());
    }
    if result.lifecycle.len() > 100 {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: ctx.desired_config.id().to_owned(),
            message: "Combined Azure lifecycle declarations exceed 100 rules".into(),
        }));
    }
    if result.origins.iter().any(|origin| {
        origin.is_empty() || origin.chars().count() > 256 || origin.contains([',', '\r', '\n'])
    }) {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: ctx.desired_config.id().to_owned(), message: "Azure CORS origins must be nonempty, at most 256 characters, and contain no commas or line breaks".into(),
        }));
    }
    let cors = result.cors();
    // Azure's 2 KiB limit covers origins, methods and headers in the rule.
    let size: usize = cors
        .cors_rules
        .iter()
        .map(|rule| {
            rule.allowed_origins.join(",").len()
                + "GET,HEAD*ETag,Content-Range,Accept-Ranges3600".len()
        })
        .sum();
    if size > 2048 {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: ctx.desired_config.id().to_owned(),
            message:
                "Combined Azure CORS settings exceed 2 KiB; reduce the enabled containers' origins"
                    .into(),
        }));
    }
    Ok(result)
}

impl AzureStorageAccountController {
    fn required_account<'a>(&'a self, ctx: &ResourceControllerContext<'_>) -> Result<&'a str> {
        self.account_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_owned(),
                message: "Storage account name is missing".into(),
            })
        })
    }

    async fn set_public_capability(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        require_direct_setup(ctx)?;
        let desired = shared_capabilities(ctx)?;
        let account = self.required_account(ctx)?;
        let group = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_storage_accounts_client(ctx.get_azure_config()?)?;
        let actual = client
            .get_storage_account_properties(&group, account)
            .await
            .context(cloud_error(ctx, "Read account public capability"))?;
        if actual.properties.and_then(|p| p.allow_blob_public_access) != Some(desired.public) {
            client
                .update_storage_account(
                    &group,
                    account,
                    &StorageAccountUpdateParameters {
                        properties: Some(StorageAccountPropertiesUpdateParameters {
                            allow_blob_public_access: Some(desired.public),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )
                .await
                .context(cloud_error(ctx, "Update account public capability"))?;
        }
        Ok(())
    }

    async fn set_shared_blob_properties(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        require_direct_setup(ctx)?;
        let desired = shared_capabilities(ctx)?;
        let account = self.required_account(ctx)?;
        let group = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_blob_container_client(ctx.get_azure_config()?)?;
        let actual = client
            .get_blob_service_properties(&group, account)
            .await
            .context(cloud_error(ctx, "Read shared blob-service settings"))?;
        // Preserve soft-delete, restore, change-feed and unrelated service settings.
        let mut properties = actual.properties.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceDrift {
                resource_id: ctx.desired_config.id().to_owned(),
                message: "Azure returned no blob-service properties".into(),
            })
        })?;
        let cors = desired.cors();
        let same_cors = serde_json::to_value(&properties.cors)
            .into_alien_error()
            .context(cloud_error(ctx, "Encode observed CORS"))?
            == serde_json::to_value(&cors)
                .into_alien_error()
                .context(cloud_error(ctx, "Encode desired CORS"))?;
        if properties.is_versioning_enabled == Some(desired.versioning) && same_cors {
            return Ok(());
        }
        properties.is_versioning_enabled = Some(desired.versioning);
        properties.cors = Some(cors);
        client
            .set_blob_service_properties(
                &group,
                account,
                &BlobServiceProperties {
                    properties: Some(properties),
                    ..Default::default()
                },
            )
            .await
            .context(cloud_error(ctx, "Set shared blob-service settings"))?;
        Ok(())
    }

    async fn verify_shared_capabilities(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        require_direct_setup(ctx)?;
        let desired = shared_capabilities(ctx)?;
        let account = self.required_account(ctx)?;
        let group = get_resource_group_name(ctx.state)?;
        let accounts = ctx
            .service_provider
            .get_azure_storage_accounts_client(ctx.get_azure_config()?)?;
        let actual = accounts
            .get_storage_account_properties(&group, account)
            .await
            .context(cloud_error(ctx, "Verify shared public capability"))?;
        let blob = ctx
            .service_provider
            .get_azure_blob_container_client(ctx.get_azure_config()?)?;
        let service = blob
            .get_blob_service_properties(&group, account)
            .await
            .context(cloud_error(ctx, "Verify shared blob-service settings"))?;
        let verified = if let Some(properties) = service.properties {
            properties.is_versioning_enabled == Some(desired.versioning)
                && serde_json::to_value(properties.cors)
                    .into_alien_error()
                    .context(cloud_error(ctx, "Encode observed CORS"))?
                    == serde_json::to_value(desired.cors())
                        .into_alien_error()
                        .context(cloud_error(ctx, "Encode desired CORS"))?
        } else {
            false
        };
        if !verified
            || actual.properties.and_then(|p| p.allow_blob_public_access) != Some(desired.public)
        {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: ctx.desired_config.id().to_owned(), message: "Azure has not confirmed the requested shared public, versioning and CORS settings".into(),
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod shared_settings_tests {
    use super::*;
    use crate::core::{
        controller_test::SingleControllerExecutor, MockPlatformServiceProvider, ResourceController,
    };
    use crate::core::{StackExecutor, StackResourceStateExt};
    use alien_azure_clients::{
        blob_containers::MockBlobContainerApi, long_running_operation::OperationResult,
        storage_accounts::MockStorageAccountsApi,
    };
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, ResourceLifecycle,
        StackSettings, StackState,
    };
    use std::sync::{Arc, Mutex};

    fn account(public: bool) -> StorageAccount {
        serde_json::from_value(serde_json::json!({"location":"eastus","properties":{"allowBlobPublicAccess":public,"provisioningState":"Succeeded"}})).unwrap()
    }

    #[tokio::test]
    async fn setup_update_preserves_other_properties_and_recovers_lost_responses() {
        for lose_account in [false, true] {
            let public = Arc::new(Mutex::new(false));
            let service = Arc::new(Mutex::new(
                serde_json::from_value::<BlobServiceProperties>(serde_json::json!({"properties":{
                    "isVersioningEnabled":false,"cors":{"corsRules":[]},
                    "deleteRetentionPolicy":{"enabled":true,"days":17},
                    "containerDeleteRetentionPolicy":{"enabled":true,"days":11},
                    "changeFeed":{"enabled":true,"retentionInDays":7}
                }}))
                .unwrap(),
            ));
            let account_writes = Arc::new(Mutex::new(0));
            let service_writes = Arc::new(Mutex::new(0));
            let mut accounts = MockStorageAccountsApi::new();
            let value = public.clone();
            accounts
                .expect_get_storage_account_properties()
                .returning(move |_, _| Ok(account(*value.lock().unwrap())));
            let value = public.clone();
            let writes = account_writes.clone();
            accounts
                .expect_update_storage_account()
                .returning(move |_, _, params| {
                    let next = params
                        .properties
                        .as_ref()
                        .unwrap()
                        .allow_blob_public_access
                        .unwrap();
                    *value.lock().unwrap() = next;
                    *writes.lock().unwrap() += 1;
                    if lose_account {
                        return Err(AlienError::new(CloudClientErrorData::Timeout {
                            message: "lost account PATCH response".into(),
                        }));
                    }
                    Ok(OperationResult::Completed(account(next)))
                });
            let mut blob = MockBlobContainerApi::new();
            blob.expect_get_management_policy().returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "policy".into(),
                        resource_name: "default".into(),
                    },
                ))
            });
            let value = service.clone();
            blob.expect_get_blob_service_properties()
                .returning(move |_, _| Ok(value.lock().unwrap().clone()));
            let value = service.clone();
            let writes = service_writes.clone();
            blob.expect_set_blob_service_properties()
                .returning(move |_, _, params| {
                    let json = serde_json::to_value(params).unwrap();
                    assert_eq!(json["properties"]["deleteRetentionPolicy"]["days"], 17);
                    assert_eq!(
                        json["properties"]["containerDeleteRetentionPolicy"]["days"],
                        11
                    );
                    assert_eq!(json["properties"]["changeFeed"]["retentionInDays"], 7);
                    *value.lock().unwrap() = params.clone();
                    *writes.lock().unwrap() += 1;
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "lost-response".into(),
                            resource_name: "blob-service".into(),
                        },
                    ))
                });
            let mut provider = MockPlatformServiceProvider::new();
            let accounts = Arc::new(accounts);
            let blob = Arc::new(blob);
            provider
                .expect_get_azure_storage_accounts_client()
                .returning(move |_| Ok(accounts.clone()));
            provider
                .expect_get_azure_blob_container_client()
                .returning(move |_| Ok(blob.clone()));
            let provider = Arc::new(provider);
            let storage = Storage::new("public".into())
                .public_read(true)
                .cors_allowed_origins(vec!["https://one.example".into()])
                .build();
            let sibling = Storage::new("versioned".into())
                .versioning(true)
                .cors_allowed_origins(vec![
                    "https://two.example".into(),
                    "https://one.example".into(),
                ])
                .build();
            let config = AzureStorageAccount::new("default-storage-account".into()).build();
            let mut controller = AzureStorageAccountController::mock_ready("teststorage");
            controller.state = AzureStorageAccountState::UpdateStart;
            // A failed mutation resumes its serialized handler, whose read adopts the committed result.
            for _ in 0..12 {
                let mut run = SingleControllerExecutor::builder()
                    .resource(config.clone())
                    .controller(
                        serde_json::from_value::<AzureStorageAccountController>(
                            serde_json::to_value(&controller).unwrap(),
                        )
                        .unwrap(),
                    )
                    .platform(Platform::Azure)
                    .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                    .with_stack_resource(storage.clone(), ResourceLifecycle::Live)
                    .with_stack_resource(sibling.clone(), ResourceLifecycle::Frozen)
                    .service_provider(provider.clone())
                    .with_dependency(
                        alien_core::AzureResourceGroup::new("default-resource-group".into())
                            .build(),
                        crate::infra_requirements::AzureResourceGroupController::mock_ready(
                            "default-resource-group",
                        ),
                    )
                    .build()
                    .await
                    .unwrap();
                let step = run.step().await;
                if step.is_ok() {
                    controller = run
                        .internal_state::<AzureStorageAccountController>()
                        .unwrap()
                        .clone();
                }
                if controller.state == AzureStorageAccountState::Ready {
                    break;
                }
            }
            assert_eq!(controller.state, AzureStorageAccountState::Ready);
            assert!(controller.capabilities_revision.is_some());
            assert_eq!(*account_writes.lock().unwrap(), 1);
            assert_eq!(*service_writes.lock().unwrap(), 1);
            let json = serde_json::to_value(service.lock().unwrap().clone()).unwrap();
            assert_eq!(json["properties"]["isVersioningEnabled"], true);
            assert_eq!(
                json["properties"]["cors"]["corsRules"][0]["allowedOrigins"],
                serde_json::json!(["https://one.example", "https://two.example"])
            );
            assert_eq!(
                json["properties"]["cors"]["corsRules"][0]["allowedMethods"],
                serde_json::json!(["GET", "HEAD"])
            );
        }
    }

    #[tokio::test]
    async fn unchanged_legacy_executor_repairs_then_removes_shared_settings() {
        let service = Arc::new(Mutex::new(serde_json::from_value::<BlobServiceProperties>(serde_json::json!({"properties":{
            "isVersioningEnabled":false,"cors":{"corsRules":[]},"deleteRetentionPolicy":{"enabled":true,"days":17}
        }})).unwrap()));
        let writes = Arc::new(Mutex::new(0));
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        let value = service.clone();
        blob.expect_get_blob_service_properties()
            .returning(move |_, _| Ok(value.lock().unwrap().clone()));
        let value = service.clone();
        let observed_writes = writes.clone();
        blob.expect_set_blob_service_properties()
            .returning(move |_, _, params| {
                *value.lock().unwrap() = params.clone();
                let mut count = observed_writes.lock().unwrap();
                *count += 1;
                if *count == 1 {
                    return Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "lost-response".into(),
                            resource_name: "blob-service".into(),
                        },
                    ));
                }
                Ok(params.clone())
            });
        let mut accounts = MockStorageAccountsApi::new();
        accounts
            .expect_get_storage_account_properties()
            .returning(|_, _| Ok(account(false)));
        accounts.expect_update_storage_account().times(0);
        let mut provider = MockPlatformServiceProvider::new();
        let accounts = Arc::new(accounts);
        let blob = Arc::new(blob);
        provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(blob.clone()));
        let provider = Arc::new(provider);
        let resource = AzureStorageAccount::new("default-storage-account".into()).build();
        let seed = SingleControllerExecutor::builder()
            .resource(resource.clone())
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(AzureStorageAccountController::mock_ready("teststorage"))
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(
                Storage::new("data".into())
                    .versioning(true)
                    .cors_allowed_origins(vec!["https://example.test".into()])
                    .build(),
                ResourceLifecycle::Live,
            )
            .service_provider(provider.clone())
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .build()
            .await
            .unwrap();
        let client = seed.with_context(|ctx| ctx.client_config.clone());
        let mut state = seed.stack_state().clone();
        let mut desired = seed.with_context(|ctx| ctx.desired_stack.clone());
        for (id, entry) in &mut desired.resources {
            if id != &resource.id {
                entry.lifecycle = ResourceLifecycle::Live;
            }
        }
        for (id, entry) in &mut state.resources {
            if id != &resource.id {
                entry.lifecycle = Some(ResourceLifecycle::Live);
            }
        }
        let entry = &desired.resources[&resource.id];
        let installed = state.resources.get_mut(&resource.id).unwrap();
        installed.config = entry.config.clone();
        installed.previous_config = Some(entry.config.clone());
        installed.dependencies = entry.combined_dependencies();
        let deployment = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .setup_run(true)
            .build();
        for round in 0..3 {
            let removal = round == 2;
            if round == 1 {
                service
                    .lock()
                    .unwrap()
                    .properties
                    .as_mut()
                    .unwrap()
                    .is_versioning_enabled = Some(false);
            }
            if removal {
                desired.resources.shift_remove("data");
            }
            let executor = StackExecutor::builder(&desired, client.clone())
                .deployment_config(&deployment)
                .lifecycle_filter(vec![ResourceLifecycle::Frozen])
                .step_out_of_scope_resources(false)
                .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                .service_provider(provider.clone())
                .build()
                .unwrap();
            assert_eq!(
                executor
                    .plan(&state)
                    .unwrap()
                    .updates
                    .contains_key(&resource.id),
                round != 1
            );
            if round == 1 {
                // The declaration and persisted receipt are unchanged. Ready must
                // discover cloud drift and persist an update entry before writes.
                state = executor.step(state).await.unwrap().next_state;
                state = serde_json::from_value::<StackState>(serde_json::to_value(&state).unwrap())
                    .unwrap();
                let controller = state.resources[&resource.id]
                    .get_internal_controller_typed::<AzureStorageAccountController>()
                    .unwrap();
                assert_eq!(controller.state, AzureStorageAccountState::UpdateStart);
                assert!(controller.capabilities_revision.is_none());
                assert_eq!(*writes.lock().unwrap(), 1);
            }
            let mut failures = 0;
            for _ in 0..10 {
                // Model a persisted final automatic attempt so the committed-but-lost
                // response exercises UpdateFailed -> explicit retry, not only backoff.
                if round == 0
                    && failures == 0
                    && state.resources[&resource.id].status == ResourceStatus::Updating
                {
                    state.resources.get_mut(&resource.id).unwrap().retry_attempt = 9;
                }
                state = executor.step(state).await.unwrap().next_state;
                state = serde_json::from_value::<StackState>(serde_json::to_value(&state).unwrap())
                    .unwrap();
                if state.resources[&resource.id].status == ResourceStatus::UpdateFailed {
                    failures += 1;
                    assert!(state
                        .resources
                        .get_mut(&resource.id)
                        .unwrap()
                        .retry_failed()
                        .unwrap());
                }
                if state.resources[&resource.id].status == ResourceStatus::Running {
                    break;
                }
            }
            assert_eq!(failures, usize::from(round == 0));
            assert_eq!(
                state.resources[&resource.id].status,
                ResourceStatus::Running
            );
            assert!(executor.plan(&state).unwrap().updates.is_empty());
            let before = *writes.lock().unwrap();
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(
                state.resources[&resource.id].status,
                ResourceStatus::Running
            );
            assert_eq!(
                *writes.lock().unwrap(),
                before,
                "unchanged verified setup must not write"
            );
            let json = serde_json::to_value(service.lock().unwrap().clone()).unwrap();
            assert_eq!(json["properties"]["isVersioningEnabled"], !removal);
            assert_eq!(
                service
                    .lock()
                    .unwrap()
                    .properties
                    .as_ref()
                    .unwrap()
                    .cors
                    .as_ref()
                    .unwrap()
                    .cors_rules
                    .is_empty(),
                removal
            );
            assert_eq!(json["properties"]["deleteRetentionPolicy"]["days"], 17);
        }
        assert_eq!(*writes.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn lifecycle_executor_records_intent_retries_and_disables_removed_expiry() {
        let policy = Arc::new(Mutex::new(
            None::<alien_azure_clients::models::storage::ManagementPolicy>,
        ));
        let writes = Arc::new(Mutex::new(0));
        let mut blob = MockBlobContainerApi::new();
        let value = policy.clone();
        blob.expect_get_management_policy().returning(move |_, _| {
            value.lock().unwrap().clone().ok_or_else(|| {
                AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                })
            })
        });
        let value = policy.clone();
        let count = writes.clone();
        blob.expect_set_management_policy()
            .returning(move |_, _, params| {
                *value.lock().unwrap() = Some(params.clone());
                let mut count = count.lock().unwrap();
                *count += 1;
                if *count == 1 {
                    return Err(AlienError::new(CloudClientErrorData::Timeout {
                        message: "committed lifecycle PUT response lost".into(),
                    }));
                }
                Ok(params.clone())
            });
        blob.expect_get_blob_service_properties().returning(|_, _| {
            Ok(serde_json::from_value(serde_json::json!({"properties":{
                "isVersioningEnabled":false,"cors":{"corsRules":[]}
            }}))
            .unwrap())
        });
        blob.expect_set_blob_service_properties().times(0);
        let mut accounts = MockStorageAccountsApi::new();
        accounts
            .expect_get_storage_account_properties()
            .returning(|_, _| Ok(account(false)));
        accounts.expect_update_storage_account().times(0);
        let mut provider = MockPlatformServiceProvider::new();
        let accounts = Arc::new(accounts);
        let blob = Arc::new(blob);
        provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(blob.clone()));
        let provider = Arc::new(provider);
        let resource = AzureStorageAccount::new("default-storage-account".into()).build();
        let seed = SingleControllerExecutor::builder()
            .resource(resource.clone())
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(AzureStorageAccountController::mock_ready("teststorage"))
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(
                Storage::new("data".into())
                    .lifecycle_rules(vec![alien_core::LifecycleRule {
                        days: 30,
                        prefix: Some("logs/".into()),
                    }])
                    .build(),
                ResourceLifecycle::Live,
            )
            .service_provider(provider.clone())
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .build()
            .await
            .unwrap();
        let client = seed.with_context(|ctx| ctx.client_config.clone());
        let mut state = seed.stack_state().clone();
        let mut desired = seed.with_context(|ctx| ctx.desired_stack.clone());
        for (id, entry) in &mut desired.resources {
            if id != &resource.id {
                entry.lifecycle = ResourceLifecycle::Live;
            }
        }
        for (id, entry) in &mut state.resources {
            if id != &resource.id {
                entry.lifecycle = Some(ResourceLifecycle::Live);
            }
        }
        let entry = &desired.resources[&resource.id];
        let installed = state.resources.get_mut(&resource.id).unwrap();
        installed.config = entry.config.clone();
        installed.previous_config = Some(entry.config.clone());
        installed.dependencies = entry.combined_dependencies();
        let deployment = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .setup_run(true)
            .build();
        for round in 0..3 {
            let removal = round == 2;
            if round == 1 {
                // An external writer removed the singleton, with no declaration change.
                *policy.lock().unwrap() = None;
            }
            if removal {
                // Explicit installer activation after the safe first setup. Removal must disable it.
                policy
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .properties
                    .as_mut()
                    .unwrap()
                    .policy
                    .rules[0]
                    .enabled = Some(true);
                desired.resources.shift_remove("data");
            }
            let executor = StackExecutor::builder(&desired, client.clone())
                .deployment_config(&deployment)
                .lifecycle_filter(vec![ResourceLifecycle::Frozen])
                .step_out_of_scope_resources(false)
                .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                .service_provider(provider.clone())
                .build()
                .unwrap();
            assert_eq!(
                executor
                    .plan(&state)
                    .unwrap()
                    .updates
                    .contains_key(&resource.id),
                round != 1
            );
            if round == 1 {
                state = executor.step(state).await.unwrap().next_state;
                state = serde_json::from_value::<StackState>(serde_json::to_value(&state).unwrap())
                    .unwrap();
                let controller = state.resources[&resource.id]
                    .get_internal_controller_typed::<AzureStorageAccountController>()
                    .unwrap();
                assert_eq!(controller.state, AzureStorageAccountState::UpdateStart);
                assert!(controller.capabilities_revision.is_none());
                assert_eq!(*writes.lock().unwrap(), 1);
            }
            let mut failures = 0;
            for _ in 0..16 {
                // Model a persisted final automatic attempt so the committed-but-lost
                // response exercises UpdateFailed -> explicit retry, not only backoff.
                if round == 0
                    && failures == 0
                    && state.resources[&resource.id].status == ResourceStatus::Updating
                {
                    state.resources.get_mut(&resource.id).unwrap().retry_attempt = 9;
                }
                state = executor.step(state).await.unwrap().next_state;
                state = serde_json::from_value::<StackState>(serde_json::to_value(&state).unwrap())
                    .unwrap();
                if state.resources[&resource.id].status == ResourceStatus::UpdateFailed {
                    failures += 1;
                    let failed = state.resources[&resource.id]
                        .get_internal_controller_typed::<AzureStorageAccountController>()
                        .unwrap();
                    assert_eq!(failed.account_name.as_deref(), Some("teststorage"));
                    assert_eq!(failed.lifecycle_rule_names.len(), 1);
                    assert!(failed.capabilities_revision.is_none());
                    assert!(state
                        .resources
                        .get_mut(&resource.id)
                        .unwrap()
                        .retry_failed()
                        .unwrap());
                }
                if state.resources[&resource.id].status == ResourceStatus::Running {
                    break;
                }
            }
            assert_eq!(failures, usize::from(round == 0));
            assert_eq!(
                state.resources[&resource.id].status,
                ResourceStatus::Running
            );
            assert!(executor.plan(&state).unwrap().updates.is_empty());
            let before = *writes.lock().unwrap();
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(
                state.resources[&resource.id].status,
                ResourceStatus::Running
            );
            assert_eq!(
                *writes.lock().unwrap(),
                before,
                "unchanged verified setup must not write"
            );
            let actual = policy.lock().unwrap();
            let rules = &actual
                .as_ref()
                .unwrap()
                .properties
                .as_ref()
                .unwrap()
                .policy
                .rules;
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].enabled, Some(false));
            assert_eq!(
                rules[0]
                    .definition
                    .filters
                    .as_ref()
                    .unwrap()
                    .prefix_match
                    .len(),
                1
            );
            let controller = state.resources[&resource.id]
                .get_internal_controller_typed::<AzureStorageAccountController>()
                .unwrap();
            assert!(controller.lifecycle_rule_names.contains(&rules[0].name));
        }
        assert_eq!(*writes.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn legacy_permission_checkpoint_must_verify_before_ready() {
        let mut controller = AzureStorageAccountController::mock_ready("teststorage");
        controller.state = AzureStorageAccountState::ApplyingPermissions;
        let mut checkpoint = serde_json::to_value(&controller).unwrap();
        checkpoint
            .as_object_mut()
            .unwrap()
            .remove("capabilitiesRevision");
        checkpoint
            .as_object_mut()
            .unwrap()
            .remove("lifecycleRuleNames");
        let mut accounts = MockStorageAccountsApi::new();
        accounts
            .expect_get_storage_account_properties()
            .returning(|_, _| Ok(account(false)));
        accounts.expect_update_storage_account().times(0);
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_get_blob_service_properties().returning(|_, _| {
            Ok(serde_json::from_value(serde_json::json!({
                "properties":{"isVersioningEnabled":false,"cors":{"corsRules":[]}}
            }))
            .unwrap())
        });
        blob.expect_set_blob_service_properties().times(0);
        let mut provider = MockPlatformServiceProvider::new();
        let accounts = Arc::new(accounts);
        let blob = Arc::new(blob);
        let authorization =
            Arc::new(alien_azure_clients::authorization::MockAuthorizationApi::new());
        provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(blob.clone()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        let mut run = SingleControllerExecutor::builder()
            .resource(AzureStorageAccount::new("default-storage-account".into()).build())
            .controller(
                serde_json::from_value::<AzureStorageAccountController>(checkpoint).unwrap(),
            )
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        run.step().await.unwrap();
        assert_eq!(
            run.internal_state::<AzureStorageAccountController>()
                .unwrap()
                .state,
            AzureStorageAccountState::ConfiguringAccount
        );
        assert!(run
            .internal_state::<AzureStorageAccountController>()
            .unwrap()
            .capabilities_revision
            .is_none());
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(run
            .internal_state::<AzureStorageAccountController>()
            .unwrap()
            .capabilities_revision
            .is_some());
    }

    #[tokio::test]
    async fn failed_blob_configuration_keeps_account_identity_for_explicit_delete() {
        let mut controller = AzureStorageAccountController::mock_ready("teststorage");
        controller.state = AzureStorageAccountState::UpdatingBlobService;
        controller.pending_capabilities_revision = Some(SharedCapabilities::default().revision());
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_get_blob_service_properties().returning(|_, _| {
            Ok(serde_json::from_value(serde_json::json!({
                "properties":{"isVersioningEnabled":true,"cors":{"corsRules":[]}}
            }))
            .unwrap())
        });
        blob.expect_set_blob_service_properties()
            .times(1)
            .returning(|_, _, _| {
                Err(AlienError::new(CloudClientErrorData::Timeout {
                    message: "lost committed response".into(),
                }))
            });
        let mut accounts = MockStorageAccountsApi::new();
        accounts
            .expect_delete_storage_account()
            .times(1)
            .withf(|_, name| name == "teststorage")
            .returning(|_, _| Ok(()));
        accounts
            .expect_get_storage_account_properties()
            .times(1)
            .withf(|_, name| name == "teststorage")
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "account".into(),
                        resource_name: "teststorage".into(),
                    },
                ))
            });
        let mut provider = MockPlatformServiceProvider::new();
        let accounts = Arc::new(accounts);
        let blob = Arc::new(blob);
        provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(blob.clone()));
        let provider = Arc::new(provider);
        let config = AzureStorageAccount::new("default-storage-account".into()).build();
        let mut run = SingleControllerExecutor::builder()
            .resource(config.clone())
            .controller(controller.clone())
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .service_provider(provider.clone())
            .build()
            .await
            .unwrap();
        assert!(run.step().await.is_err());
        controller.transition_to_failure();
        let controller: AzureStorageAccountController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        assert_eq!(controller.account_name.as_deref(), Some("teststorage"));
        let mut run = SingleControllerExecutor::builder()
            .resource(config)
            .controller(controller)
            .platform(Platform::Azure)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .service_provider(provider)
            .build()
            .await
            .unwrap();
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        assert!(run
            .internal_state::<AzureStorageAccountController>()
            .unwrap()
            .account_name
            .is_none());
    }

    #[tokio::test]
    async fn changed_or_missing_pending_aggregate_restarts_before_any_write() {
        for pending in [None, Some(SharedCapabilities::default().revision())] {
            for (state, expected) in [
                (
                    AzureStorageAccountState::ConfiguringBlobService,
                    AzureStorageAccountState::ConfiguringAccount,
                ),
                (
                    AzureStorageAccountState::VerifyingBlobService,
                    AzureStorageAccountState::ConfiguringAccount,
                ),
                (
                    AzureStorageAccountState::UpdatingBlobService,
                    AzureStorageAccountState::UpdateStart,
                ),
                (
                    AzureStorageAccountState::VerifyingUpdatedBlobService,
                    AzureStorageAccountState::UpdateStart,
                ),
                (
                    AzureStorageAccountState::ConfiguringLifecycle,
                    AzureStorageAccountState::ConfiguringAccount,
                ),
                (
                    AzureStorageAccountState::VerifyingLifecycle,
                    AzureStorageAccountState::ConfiguringAccount,
                ),
                (
                    AzureStorageAccountState::UpdatingLifecycle,
                    AzureStorageAccountState::UpdateStart,
                ),
                (
                    AzureStorageAccountState::VerifyingUpdatedLifecycle,
                    AzureStorageAccountState::UpdateStart,
                ),
            ] {
                let mut controller = AzureStorageAccountController::mock_ready("teststorage");
                controller.state = state;
                controller.pending_capabilities_revision = pending.clone();
                let mut run = SingleControllerExecutor::builder()
                    .resource(AzureStorageAccount::new("default-storage-account".into()).build())
                    .controller(
                        serde_json::from_value::<AzureStorageAccountController>(
                            serde_json::to_value(controller).unwrap(),
                        )
                        .unwrap(),
                    )
                    .platform(Platform::Azure)
                    .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                    .with_stack_resource(
                        Storage::new("versioned".into()).versioning(true).build(),
                        ResourceLifecycle::Live,
                    )
                    .with_dependency(
                        alien_core::AzureResourceGroup::new("default-resource-group".into())
                            .build(),
                        crate::infra_requirements::AzureResourceGroupController::mock_ready(
                            "default-resource-group",
                        ),
                    )
                    .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                    .build()
                    .await
                    .unwrap();
                run.step().await.unwrap();
                assert_eq!(
                    run.internal_state::<AzureStorageAccountController>()
                        .unwrap()
                        .state,
                    expected
                );
                assert!(run
                    .internal_state::<AzureStorageAccountController>()
                    .unwrap()
                    .capabilities_revision
                    .is_none());
            }
        }
    }

    #[tokio::test]
    async fn ready_read_denial_preserves_identity_and_imported_runtime_never_reconciles() {
        for authority in [
            InitialSetupAuthority::DirectSetup,
            InitialSetupAuthority::ImportedHandoff,
        ] {
            let direct = authority == InitialSetupAuthority::DirectSetup;
            let mut accounts = MockStorageAccountsApi::new();
            accounts
                .expect_get_storage_account_properties()
                .times(1)
                .returning(move |_, _| {
                    if direct {
                        Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                            resource_type: "account".into(),
                            resource_name: "teststorage".into(),
                        }))
                    } else {
                        Ok(account(false))
                    }
                });
            accounts.expect_update_storage_account().times(0);
            let accounts = Arc::new(accounts);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_azure_storage_accounts_client()
                .returning(move |_| Ok(accounts.clone()));
            provider.expect_get_azure_blob_container_client().times(0);
            let mut controller = AzureStorageAccountController::mock_ready("teststorage");
            controller.capabilities_revision = Some("installed-revision".into());
            controller.lifecycle_rule_names.insert("owned-rule".into());
            let mut run = SingleControllerExecutor::builder()
                .resource(AzureStorageAccount::new("default-storage-account".into()).build())
                .controller(
                    serde_json::from_value::<AzureStorageAccountController>(
                        serde_json::to_value(controller).unwrap(),
                    )
                    .unwrap(),
                )
                .platform(Platform::Azure)
                .initial_setup_authority(authority)
                .service_provider(Arc::new(provider))
                .with_dependency(
                    alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                    crate::infra_requirements::AzureResourceGroupController::mock_ready(
                        "default-resource-group",
                    ),
                )
                .build()
                .await
                .unwrap();
            if direct {
                assert!(run.step().await.is_err());
            } else {
                assert!(!run.needs_update().unwrap());
                run.step().await.unwrap();
            }
            let saved = run
                .internal_state::<AzureStorageAccountController>()
                .unwrap();
            assert_eq!(saved.account_name.as_deref(), Some("teststorage"));
            assert!(saved.lifecycle_rule_names.contains("owned-rule"));
            assert_eq!(
                saved.capabilities_revision.as_deref(),
                if direct {
                    None
                } else {
                    Some("installed-revision")
                }
            );
        }
    }

    #[tokio::test]
    async fn imported_checkpoints_cannot_write_shared_settings() {
        for state in [
            AzureStorageAccountState::CreateStart,
            AzureStorageAccountState::ConfiguringAccount,
            AzureStorageAccountState::ConfiguringBlobService,
            AzureStorageAccountState::VerifyingBlobService,
            AzureStorageAccountState::ApplyingPermissions,
            AzureStorageAccountState::UpdateStart,
            AzureStorageAccountState::UpdatingBlobService,
            AzureStorageAccountState::VerifyingUpdatedBlobService,
            AzureStorageAccountState::ConfiguringLifecycle,
            AzureStorageAccountState::UpdatingLifecycle,
            AzureStorageAccountState::VerifyingLifecycle,
            AzureStorageAccountState::VerifyingUpdatedLifecycle,
        ] {
            let mut controller = AzureStorageAccountController::mock_ready("teststorage");
            controller.state = state;
            let mut run = SingleControllerExecutor::builder()
                .resource(AzureStorageAccount::new("default-storage-account".into()).build())
                .controller(controller)
                .platform(Platform::Azure)
                .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_dependency(
                    alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                    crate::infra_requirements::AzureResourceGroupController::mock_ready(
                        "default-resource-group",
                    ),
                )
                .build()
                .await
                .unwrap();
            assert_eq!(run.step().await.unwrap_err().code, "SETUP_RERUN_REQUIRED");
        }
    }
}
