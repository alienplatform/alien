//! Controller for managing Azure Storage Containers.

use crate::{
    core::{AzurePermissionsHelper, ResourceControllerContext, ResourcePermissionsHelper},
    error::{ErrorData, Result},
    infra_requirements::azure_utils,
};
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::azure::models::blob::{
    BlobContainer, BlobServiceProperties, ContainerProperties, ContainerPropertiesPublicAccess,
};
use alien_azure_clients::azure::models::storage::StorageAccount;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureBlobStorageHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use std::{fmt::Debug, time::Duration};
use tracing::{debug, info, warn};

fn get_azure_container_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
        .to_lowercase()
        .replace('_', "-")
}

fn azure_permission_scope_and_context(
    ctx: &ResourceControllerContext<'_>,
    container_name: &str,
) -> Result<(Scope, alien_permissions::PermissionContext)> {
    let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;
    let resource_scope = Scope::Resource {
        resource_group_name: azure_utils::get_resource_group_name(ctx.state)?,
        resource_provider: "Microsoft.Storage".to_string(),
        parent_resource_path: Some(format!(
            "storageAccounts/{storage_account_name}/blobServices/default"
        )),
        resource_type: "containers".to_string(),
        resource_name: container_name.to_string(),
    };
    let permission_context =
        ResourcePermissionsHelper::build_azure_permission_context(ctx, container_name)?;
    Ok((resource_scope, permission_context))
}

#[controller]
pub struct AzureStorageController {
    /// The name of the created Azure Storage Container
    pub(crate) container_name: Option<String>,
    /// The name of the Azure Storage Account (from infrastructure dependencies)
    pub(crate) storage_account_name: Option<String>,
    /// Full deterministic Azure resource IDs of runtime-owned role assignments.
    /// IDs are recorded before creation so partial failures remain cleanable.
    #[serde(default)]
    pub(crate) role_assignment_ids: Vec<String>,
    /// Whether the role assignment IDs were durably planned in an earlier controller step.
    #[serde(default)]
    pub(crate) role_assignments_planned: bool,
}

#[controller]
impl AzureStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        // Determine container name (idempotent)
        let container_name = if let Some(name) = &self.container_name {
            name.clone()
        } else {
            get_azure_container_name(ctx.resource_prefix, config.id())
        };

        info!(
            resource_id = %config.id(),
            container_name = %container_name,
            "Creating Azure Storage Container"
        );

        // Log warnings for unsupported features
        // TODO: Validate this earlier...
        if config.versioning {
            warn!(resource_id = %config.id(), "Azure Storage does not support per-container versioning. The 'versioning' flag will be ignored.");
        }
        if !config.lifecycle_rules.is_empty() {
            warn!(resource_id = %config.id(), "Azure Storage does not support per-container lifecycle rules via this controller. 'lifecycle_rules' will be ignored.");
        }

        // Get infrastructure dependencies
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

        // Store the storage account name in controller state for environment variables
        self.storage_account_name = Some(storage_account_name.clone());

        // Build container configuration
        let mut properties = ContainerProperties::default();
        properties.public_access = if config.public_read {
            Some(ContainerPropertiesPublicAccess::Blob)
        } else {
            Some(ContainerPropertiesPublicAccess::None)
        };

        let container_body = BlobContainer {
            properties: Some(properties),
            ..Default::default()
        };

        // Execute container creation
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_blob_container_client(azure_config)?;

        // Fail fast on any error - executor handles retries
        client
            .create_blob_container(
                &resource_group_name,
                &storage_account_name,
                &container_name,
                &container_body,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Storage Container '{}'",
                    container_name
                ),
                resource_id: Some(config.id().to_string()),
            })?;

        info!(
            "Successfully created Azure Storage Container '{}'",
            container_name
        );

        self.container_name = Some(container_name.clone());

        Ok(HandlerAction::Continue {
            state: PlanningPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = PlanningPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn planning_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        self.role_assignment_ids = if let Some(container_name) = &self.container_name {
            let (resource_scope, permission_context) =
                azure_permission_scope_and_context(ctx, container_name)?;
            AzurePermissionsHelper::plan_resource_scoped_role_assignment_ids(
                ctx,
                &config.id,
                "storage",
                resource_scope,
                &permission_context,
            )
            .await?
        } else {
            Vec::new()
        };
        self.role_assignments_planned = true;

        info!(
            resource_id = %config.id(),
            role_assignments = self.role_assignment_ids.len(),
            "Planned Azure Storage role assignments"
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
        let config = ctx.desired_resource_config::<Storage>()?;

        if !self.role_assignments_planned {
            return Ok(HandlerAction::Continue {
                state: PlanningPermissions,
                suggested_delay: None,
            });
        }

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_name) = &self.container_name {
            let (resource_scope, permission_context) =
                azure_permission_scope_and_context(ctx, container_name)?;
            AzurePermissionsHelper::apply_resource_scoped_permissions_from_checkpoint(
                ctx,
                &config.id,
                "storage",
                resource_scope,
                &permission_context,
                &self.role_assignment_ids,
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
        let config = ctx.desired_resource_config::<Storage>()?;

        if let Some(container_name) = &self.container_name {
            let azure_config = ctx.get_azure_config()?;
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;
            let storage_accounts_client = ctx
                .service_provider
                .get_azure_storage_accounts_client(azure_config)?;

            // Check if container still exists
            let container = client
                .get_blob_container(&resource_group_name, &storage_account_name, container_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check Azure Storage Container during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let storage_account = storage_accounts_client
                .get_storage_account_properties(&resource_group_name, &storage_account_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get Azure Storage account during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let blob_service = client
                .get_blob_service_properties(&resource_group_name, &storage_account_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get Azure Blob service during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_azure_storage_heartbeat(
                ctx,
                &config.id,
                &resource_group_name,
                &storage_account_name,
                container,
                storage_account,
                blob_service,
            );

            debug!(name = %config.id, container = %container_name, "Azure Storage Container exists and is accessible");
        }

        debug!(name = %config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
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
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev = ctx.previous_resource_config::<Storage>()?;

        info!(resource_id = %config.id(), "Starting update of Azure Storage Container");

        let container_name = self.container_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container name not set during update".to_string(),
                resource_id: Some(config.id().to_string()),
            })
        })?;

        info!(
            resource_id = %config.id(),
            container_name,
            "Updating Azure Storage Container"
        );

        // Only perform update if public_read setting changed
        if config.public_read != prev.public_read {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

            let mut properties = ContainerProperties::default();
            properties.public_access = if config.public_read {
                Some(ContainerPropertiesPublicAccess::Blob)
            } else {
                Some(ContainerPropertiesPublicAccess::None)
            };
            let container_body = BlobContainer {
                properties: Some(properties),
                ..Default::default()
            };

            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;

            client
                .update_blob_container(
                    &resource_group_name,
                    &storage_account_name,
                    container_name,
                    &container_body,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update Azure Storage Container '{}': public_read change",
                        container_name
                    ),
                    resource_id: Some(config.id().to_string()),
                })?;
            info!(
                resource_id = %config.id(),
                container_name,
                "Updated public access to {}",
                config.public_read
            );
        }

        if config.versioning != prev.versioning {
            warn!(resource_id = %config.id(), "Azure Storage does not support per-container versioning. The 'versioning' flag change will be ignored.");
        }
        if config.lifecycle_rules != prev.lifecycle_rules {
            warn!(resource_id = %config.id(), "Azure Storage does not support per-container lifecycle rules via this controller. 'lifecycle_rules' changes will be ignored.");
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
        let config = ctx.desired_resource_config::<Storage>()?;

        if !self.role_assignment_ids.is_empty() {
            let azure_config = ctx.get_azure_config()?;
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;

            for assignment_id in &self.role_assignment_ids {
                match authorization_client
                    .delete_role_assignment_by_id(assignment_id.clone())
                    .await
                {
                    Ok(_) => {
                        info!(
                            resource_id = %config.id(),
                            assignment_id,
                            "Deleted Azure Storage role assignment"
                        );
                    }
                    Err(error)
                        if matches!(
                            &error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(
                            resource_id = %config.id(),
                            assignment_id,
                            "Azure Storage role assignment was already deleted"
                        );
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete Azure role assignment '{}' for Storage",
                                assignment_id
                            ),
                            resource_id: Some(config.id().to_string()),
                        }));
                    }
                }
            }
            self.role_assignment_ids.clear();
        }

        Ok(HandlerAction::Continue {
            state: DeletingContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingContainer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        if let Some(container_name) = &self.container_name {
            info!(
                resource_id = %config.id(),
                container_name,
                "Deleting Azure Storage Container"
            );
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;

            match client
                .delete_blob_container(&resource_group_name, &storage_account_name, container_name)
                .await
            {
                Ok(_) => {
                    info!(
                        "Successfully deleted Azure Storage Container '{}'",
                        container_name
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        "Azure Storage Container '{}' was already deleted.",
                        container_name
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete Azure Storage Container '{}'",
                            container_name
                        ),
                        resource_id: Some(config.id().to_string()),
                    }));
                }
            }
        } else {
            warn!(resource_id = %config.id(), "Delete called on storage resource with no container name, assuming it was never created.");
        }

        self.container_name = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
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
        self.container_name.as_ref().map(|container_name| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: container_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::StorageBinding;

        if let (Some(storage_account_name), Some(container_name)) =
            (&self.storage_account_name, &self.container_name)
        {
            let binding =
                StorageBinding::blob(storage_account_name.clone(), container_name.clone());
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

fn emit_azure_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    storage_account_name: &str,
    container: BlobContainer,
    storage_account: StorageAccount,
    blob_service: BlobServiceProperties,
) {
    let container_name = container
        .name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());
    let container_public_access = container
        .properties
        .as_ref()
        .and_then(|properties| properties.public_access.as_ref())
        .map(ToString::to_string);
    let account_properties = storage_account.properties.as_ref();
    let blob_service_properties = blob_service.properties.as_ref();
    let location = storage_account.location.clone();
    let sku_name = storage_account.sku.as_ref().map(|sku| sku.name.to_string());
    let sku_tier = storage_account
        .sku
        .as_ref()
        .and_then(|sku| sku.tier.as_ref())
        .map(ToString::to_string);
    let access_tier = account_properties
        .and_then(|properties| properties.access_tier.as_ref())
        .map(ToString::to_string);
    let encryption = account_properties.and_then(|properties| properties.encryption.as_ref());
    let encryption_services = encryption.and_then(|encryption| encryption.services.as_ref());
    let blob_delete_retention =
        blob_service_properties.and_then(|properties| properties.delete_retention_policy.as_ref());
    let container_delete_retention = blob_service_properties
        .and_then(|properties| properties.container_delete_retention_policy.as_ref());
    let change_feed =
        blob_service_properties.and_then(|properties| properties.change_feed.as_ref());
    let public_network_access = account_properties
        .and_then(|properties| properties.public_network_access.as_ref())
        .map(ToString::to_string);
    let allow_blob_public_access =
        account_properties.and_then(|properties| properties.allow_blob_public_access);
    let provisioning_state = account_properties
        .and_then(|properties| properties.provisioning_state.as_ref())
        .map(ToString::to_string);
    let health = match provisioning_state.as_deref() {
        Some("Succeeded") => ObservedHealth::Healthy,
        Some("Failed") => ObservedHealth::Unhealthy,
        _ => ObservedHealth::Degraded,
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Storage::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Storage(StorageHeartbeatData::AzureBlob(
            AzureBlobStorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "Azure Storage container '{}' metadata is reachable",
                        container_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: container_name,
                storage_account_name: Some(storage_account_name.to_string()),
                resource_group: Some(resource_group_name.to_string()),
                location: Some(location),
                account_kind: storage_account.kind.as_ref().map(ToString::to_string),
                sku_name,
                sku_tier,
                access_tier,
                provisioning_state,
                primary_location: account_properties
                    .and_then(|properties| properties.primary_location.clone()),
                secondary_location: account_properties
                    .and_then(|properties| properties.secondary_location.clone()),
                status_of_primary: account_properties
                    .and_then(|properties| properties.status_of_primary.as_ref())
                    .map(ToString::to_string),
                status_of_secondary: account_properties
                    .and_then(|properties| properties.status_of_secondary.as_ref())
                    .map(ToString::to_string),
                public_network_access,
                allow_blob_public_access,
                encryption_key_source: encryption
                    .map(|encryption| encryption.key_source.to_string()),
                blob_encryption_enabled: encryption_services
                    .and_then(|services| services.blob.as_ref())
                    .and_then(|service| service.enabled),
                file_encryption_enabled: encryption_services
                    .and_then(|services| services.file.as_ref())
                    .and_then(|service| service.enabled),
                queue_encryption_enabled: encryption_services
                    .and_then(|services| services.queue.as_ref())
                    .and_then(|service| service.enabled),
                table_encryption_enabled: encryption_services
                    .and_then(|services| services.table.as_ref())
                    .and_then(|service| service.enabled),
                blob_versioning_enabled: blob_service_properties
                    .and_then(|properties| properties.is_versioning_enabled),
                blob_delete_retention_enabled: blob_delete_retention
                    .and_then(|policy| policy.enabled),
                blob_delete_retention_days: blob_delete_retention
                    .and_then(|policy| policy.days)
                    .map(|days| days.get()),
                container_delete_retention_enabled: container_delete_retention
                    .and_then(|policy| policy.enabled),
                container_delete_retention_days: container_delete_retention
                    .and_then(|policy| policy.days)
                    .map(|days| days.get()),
                change_feed_enabled: change_feed.and_then(|feed| feed.enabled),
                change_feed_retention_days: change_feed
                    .and_then(|feed| feed.retention_in_days)
                    .map(|days| u64::from(days.get())),
                container_public_access,
            },
        )),
        raw: vec![],
    });
}

impl AzureStorageController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AzureStorageState::Ready,
            container_name: Some(get_azure_container_name("test-stack", storage_name)),
            storage_account_name: Some("test-storage-account".to_string()),
            role_assignment_ids: Vec::new(),
            role_assignments_planned: true,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Storage Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_azure_clients::models::blob::{
        BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
    };
    use alien_azure_clients::{
        authorization::{AuthorizationApi, MockAuthorizationApi},
        blob_containers::{BlobContainerApi, MockBlobContainerApi},
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{LifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs};
    use alien_error::AlienError;
    use mockall::{predicate::eq, Sequence};
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };
    use crate::storage::{fixtures::*, AzureStorageController};

    // ─────────────── MOCK SETUP HELPERS ────────────────────────

    fn create_successful_container_response(container_name: &str) -> BlobContainer {
        BlobContainer {
            name: Some(container_name.to_string()),
            properties: Some(ContainerProperties {
                public_access: Some(ContainerPropertiesPublicAccess::None),
                last_modified_time: Some("2023-01-01T00:00:00Z".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn setup_mock_client_for_creation_and_deletion(
        container_name: &str,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        // Mock successful container creation
        let container_name = container_name.to_string();
        let container_name_clone1 = container_name.clone();

        mock_blob
            .expect_create_blob_container()
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));

        // Mock successful container deletion
        mock_blob
            .expect_delete_blob_container()
            .returning(|_, _, _| Ok(()));

        Arc::new(mock_blob)
    }

    fn setup_mock_client_for_creation_and_update(
        container_name: &str,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        // Mock container updates for public read changes
        let container_name = container_name.to_string();

        mock_blob
            .expect_update_blob_container()
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));

        Arc::new(mock_blob)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _container_name: &str,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        // Mock container deletion failure (container doesn't exist)
        mock_blob
            .expect_delete_blob_container()
            .returning(|_, _, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Azure Blob Container".to_string(),
                        resource_name: "test-container".to_string(),
                    },
                ))
            });

        Arc::new(mock_blob)
    }

    fn setup_mock_service_provider(
        mock_blob: Arc<MockBlobContainerApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        setup_mock_service_provider_with_authorization(
            mock_blob,
            Arc::new(MockAuthorizationApi::new()),
        )
    }

    fn setup_mock_service_provider_with_authorization(
        mock_blob: Arc<MockBlobContainerApi>,
        authorization: Arc<MockAuthorizationApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(mock_blob.clone()));

        let authorization: Arc<dyn AuthorizationApi> = authorization;
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));

        Arc::new(mock_provider)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())] // Versioning will be ignored with warning
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())] // Lifecycle rules will be ignored with warning
    #[case::complete_config(storage_complete_config())] // Some features will be ignored with warnings
    #[case::custom_lifecycle(storage_custom_lifecycle())] // Lifecycle rules will be ignored with warning
    #[case::versioning_only(storage_versioning_only())] // Versioning will be ignored with warning
    #[case::public_only(storage_public_only())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] storage: Storage) {
        let container_name = format!("test-{}", storage.id);
        let mock_blob = setup_mock_client_for_creation_and_deletion(&container_name);
        let mock_provider = setup_mock_service_provider(mock_blob);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let storage_outputs = outputs.downcast_ref::<StorageOutputs>().unwrap();
        assert!(storage_outputs.bucket_name.starts_with("test-"));

        // Delete the storage
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn test_role_assignment_plan_is_serialized_before_apply_step() {
        let storage = basic_storage();
        let container_name = format!("test-{}", storage.id);
        let mut blob = MockBlobContainerApi::new();
        blob.expect_create_blob_container()
            .times(1)
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(0);
        authorization
            .expect_create_or_update_role_definition()
            .times(0);
        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let resource_id = storage.id.clone();
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("Azure Storage executor should build");

        executor
            .step()
            .await
            .expect("container creation should reach permission planning");
        executor
            .step()
            .await
            .expect("permission planning should complete without Azure mutations");

        let controller = executor
            .internal_state::<AzureStorageController>()
            .expect("Azure Storage controller state");
        assert!(matches!(
            controller.state,
            crate::storage::azure::AzureStorageState::ApplyingPermissions
        ));
        assert!(controller.role_assignments_planned);

        let persisted = executor
            .stack_state()
            .resources
            .get(&resource_id)
            .and_then(|resource| resource.internal_state.as_ref())
            .expect("planned controller state should be serialized");
        assert_eq!(
            persisted
                .get("roleAssignmentsPlanned")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(persisted.get("roleAssignmentIds").is_some());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_public_read(basic_storage(), storage_with_public_read())]
    #[case::public_read_to_basic(storage_with_public_read(), basic_storage())]
    #[case::versioning_to_public_read(storage_with_versioning(), storage_with_public_read())] // Versioning changes will be ignored
    #[case::lifecycle_to_public_read(storage_with_lifecycle_rules(), storage_with_public_read())] // Lifecycle changes will be ignored
    #[case::for_update_test_to_complete(storage_for_update_test(), storage_complete_config())] // Mixed feature changes
    #[case::public_only_to_versioning_only(storage_public_only(), storage_versioning_only())] // Public read change + versioning ignored
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_storage: Storage, #[case] to_storage: Storage) {
        // Ensure both storages have the same ID for valid updates
        let storage_id = "test-update-storage".to_string();
        let mut from_storage = from_storage;
        from_storage.id = storage_id.clone();

        let mut to_storage = to_storage;
        to_storage.id = storage_id.clone();

        let container_name = format!("test-{}", storage_id);
        let mock_blob = setup_mock_client_for_creation_and_update(&container_name);
        let mock_provider = setup_mock_service_provider(mock_blob);

        // Start with the "from" storage in Ready state
        let ready_controller = AzureStorageController::mock_ready(&storage_id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new storage
        executor.update(to_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())]
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())]
    #[case::complete_config(storage_complete_config())]
    #[case::versioning_only(storage_versioning_only())]
    #[case::public_only(storage_public_only())]
    #[tokio::test]
    async fn test_best_effort_deletion_when_container_missing(#[case] storage: Storage) {
        let container_name = format!("test-{}", storage.id);
        let mock_blob = setup_mock_client_for_best_effort_deletion(&container_name);
        let mock_provider = setup_mock_service_provider(mock_blob);

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though container deletion fails
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn test_role_assignments_are_deleted_before_container() {
        let storage = basic_storage();
        let assignment_id = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/assignment-123";
        let mut sequence = Sequence::new();

        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_delete_role_assignment_by_id()
            .with(eq(assignment_id.to_string()))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(None));

        let mut blob = MockBlobContainerApi::new();
        blob.expect_delete_blob_container()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));

        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let mut controller = AzureStorageController::mock_ready(&storage.id);
        controller.role_assignment_ids = vec![assignment_id.to_string()];
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_missing_role_assignment_does_not_block_container_deletion() {
        let storage = basic_storage();
        let assignment_id = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/missing-assignment";
        let mut sequence = Sequence::new();

        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_delete_role_assignment_by_id()
            .with(eq(assignment_id.to_string()))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Azure role assignment".to_string(),
                        resource_name: "missing-assignment".to_string(),
                    },
                ))
            });

        let mut blob = MockBlobContainerApi::new();
        blob.expect_delete_blob_container()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));

        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let mut controller = AzureStorageController::mock_ready(&storage.id);
        controller.role_assignment_ids = vec![assignment_id.to_string()];
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_no_container_name() {
        let storage = basic_storage();

        // Create a controller with no container name set (simulates a failed creation)
        let controller = AzureStorageController {
            state: crate::storage::azure::AzureStorageState::Ready,
            container_name: None, // No container name set
            storage_account_name: None,
            role_assignment_ids: Vec::new(),
            role_assignments_planned: true,
            _internal_stay_count: None,
        };

        let mock_blob = MockBlobContainerApi::new();
        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though no container was created
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies correct container naming convention
    #[tokio::test]
    async fn test_container_naming_validation() {
        let mut storage = basic_storage();
        storage.id = "My_Awesome_Storage".to_string(); // Use special naming case for this test

        let mut mock_blob = MockBlobContainerApi::new();

        // Validate that container names are converted to lowercase and underscores replaced with dashes
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, container_name, _| container_name == "test-my-awesome-storage")
            .returning(|_, _, container_name, _| {
                Ok(create_successful_container_response(container_name))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public read configuration is applied correctly
    #[tokio::test]
    async fn test_public_read_configuration() {
        let storage = storage_with_public_read();

        let mut mock_blob = MockBlobContainerApi::new();

        // Validate that public read configuration is set correctly during creation
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::Blob
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                let mut response = create_successful_container_response(container_name);
                if let Some(properties) = &mut response.properties {
                    properties.public_access = Some(ContainerPropertiesPublicAccess::Blob);
                }
                Ok(response)
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies private access configuration is applied correctly
    #[tokio::test]
    async fn test_private_access_configuration() {
        let storage = basic_storage(); // By default, public_read is false

        let mut mock_blob = MockBlobContainerApi::new();

        // Validate that private access configuration is set correctly during creation
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::None
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                Ok(create_successful_container_response(container_name))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public access update configuration is applied correctly
    #[tokio::test]
    async fn test_public_access_update_configuration() {
        let initial_storage = storage_for_update_test(); // Has public_read = false
        let mut updated_storage = storage_with_public_read(); // Has public_read = true
        updated_storage.id = initial_storage.id.clone(); // Ensure same ID for valid update

        let mut mock_blob = MockBlobContainerApi::new();

        // Validate that the update call has the correct public access configuration
        mock_blob
            .expect_update_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::Blob
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                let mut response = create_successful_container_response(container_name);
                if let Some(properties) = &mut response.properties {
                    properties.public_access = Some(ContainerPropertiesPublicAccess::Blob);
                }
                Ok(response)
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready(&initial_storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(initial_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to enable public read
        executor.update(updated_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies unsupported features (versioning, lifecycle rules) are properly ignored
    #[tokio::test]
    async fn test_unsupported_features_are_ignored() {
        let storage = storage_complete_config(); // Has versioning + lifecycle rules that should be ignored

        let mut mock_blob = MockBlobContainerApi::new();

        // The container should be created normally, ignoring unsupported features
        mock_blob
            .expect_create_blob_container()
            .returning(|_, _, container_name, _| {
                Ok(create_successful_container_response(container_name))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify that outputs are still correctly generated
        let outputs = executor.outputs().unwrap();
        let storage_outputs = outputs.downcast_ref::<StorageOutputs>().unwrap();
        assert!(storage_outputs.bucket_name.starts_with("test-"));
    }

    /// Test that verifies update operation correctly ignores unsupported feature changes
    #[tokio::test]
    async fn test_unsupported_features_update_are_ignored() {
        let mut initial_storage = storage_for_update_test(); // Basic config: no versioning, no public read
        let mut updated_storage = storage_complete_config(); // Has versioning + public read + lifecycle rules

        // Ensure both storages have the same ID for valid updates
        let storage_id = "unsupported-update-test".to_string();
        initial_storage.id = storage_id.clone();
        updated_storage.id = storage_id.clone();

        let mut mock_blob = MockBlobContainerApi::new();

        // Only the public_read change should trigger an actual API call
        mock_blob
            .expect_update_blob_container()
            .times(1) // Should be called exactly once for public_read change
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::Blob
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                let mut response = create_successful_container_response(container_name);
                if let Some(properties) = &mut response.properties {
                    properties.public_access = Some(ContainerPropertiesPublicAccess::Blob);
                }
                Ok(response)
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready("unsupported-update-test");

        let mut executor = SingleControllerExecutor::builder()
            .resource(initial_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update with mixed supported and unsupported features
        executor.update(updated_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies update operation is skipped when only unsupported features change
    #[tokio::test]
    async fn test_update_skip_when_only_unsupported_features_change() {
        let initial_storage = storage_for_update_test(); // Has versioning=false, public_read=false
        let mut updated_storage = storage_with_versioning(); // Has versioning=true, but versioning will be ignored

        // Ensure same ID and public_read setting (no change in supported features)
        updated_storage.id = initial_storage.id.clone();
        updated_storage.public_read = false; // Keep same as initial to ensure no supported feature changes

        let mut mock_blob = MockBlobContainerApi::new();

        // No API calls should be made since only unsupported features changed
        mock_blob.expect_update_blob_container().times(0); // Should not be called

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready(&initial_storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(initial_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update with only unsupported feature changes
        executor.update(updated_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
