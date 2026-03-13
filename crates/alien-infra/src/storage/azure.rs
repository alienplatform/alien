//! Controller for managing Azure Storage Containers.

use crate::{
    core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult},
    error::{ErrorData, Result},
    infra_requirements::azure_utils,
};
use alien_azure_clients::azure::models::blob::{
    BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    Resource, ResourceDefinition, ResourceOutputs, ResourceStatus, Storage, StorageOutputs,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{any::Any, fmt::Debug, time::Duration};
use tracing::{debug, info, warn};

fn get_azure_container_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
        .to_lowercase()
        .replace('_', "-")
}

#[controller]
pub struct AzureStorageController {
    /// The name of the created Azure Storage Container
    pub(crate) container_name: Option<String>,
    /// The name of the Azure Storage Account (from infrastructure dependencies)
    pub(crate) storage_account_name: Option<String>,
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

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_name) = &self.container_name {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            let config = ctx.desired_resource_config::<Storage>()?;

            // Build Azure resource scope for the storage container
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;
            let resource_scope = Scope::Resource {
                resource_group_name: azure_utils::get_resource_group_name(ctx.state)?,
                resource_provider: "Microsoft.Storage".to_string(),
                parent_resource_path: Some(format!(
                    "storageAccounts/{}/blobServices/default",
                    storage_account_name
                )),
                resource_type: "containers".to_string(),
                resource_name: container_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                container_name,
                resource_scope,
                "Storage",
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

            // Check if container still exists
            client
                .get_blob_container(&resource_group_name, &storage_account_name, container_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check Azure Storage Container during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

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

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, StorageBinding};

        if let (Some(storage_account_name), Some(container_name)) =
            (&self.storage_account_name, &self.container_name)
        {
            let binding =
                StorageBinding::blob(storage_account_name.clone(), container_name.clone());
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

impl AzureStorageController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AzureStorageState::Ready,
            container_name: Some(get_azure_container_name("test-stack", storage_name)),
            storage_account_name: Some("test-storage-account".to_string()),
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

    use alien_azure_clients::blob_containers::{BlobContainerApi, MockBlobContainerApi};
    use alien_azure_clients::models::blob::{
        BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{LifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs};
    use alien_error::AlienError;
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
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(mock_blob.clone()));

        // Mock Azure authorization client for resource-scoped permissions
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(|_| {
                use alien_azure_clients::authorization::MockAuthorizationApi;
                Ok(Arc::new(MockAuthorizationApi::new()))
            });

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
    async fn test_best_effort_deletion_when_no_container_name() {
        let storage = basic_storage();

        // Create a controller with no container name set (simulates a failed creation)
        let controller = AzureStorageController {
            state: crate::storage::azure::AzureStorageState::Ready,
            container_name: None, // No container name set
            storage_account_name: None,
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
