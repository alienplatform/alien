#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::blob_containers::{AzureBlobContainerClient, BlobContainerApi};
use alien_azure_clients::long_running_operation::{
    LongRunningOperationApi, LongRunningOperationClient,
};
use alien_azure_clients::models::blob::{
    BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
};
use alien_azure_clients::models::storage::{
    Sku, SkuName, StorageAccountCreateParameters, StorageAccountCreateParametersKind,
    StorageAccountPropertiesUpdateParameters, StorageAccountUpdateParameters, Tier,
};
use alien_azure_clients::storage_accounts::{AzureStorageAccountsClient, StorageAccountsApi};
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use chrono;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct TrackedBlobContainer {
    resource_group_name: String,
    storage_account_name: String,
    container_name: String,
}

#[derive(Debug, Clone)]
struct TrackedStorageAccount {
    resource_group_name: String,
    storage_account_name: String,
}

struct BlobContainerTestContext {
    blob_container_client: AzureBlobContainerClient,
    storage_accounts_client: AzureStorageAccountsClient,
    long_running_operation_client: LongRunningOperationClient,
    subscription_id: String,
    resource_group_name: String,
    storage_account_name: String,
    created_containers: Mutex<HashSet<TrackedBlobContainer>>,
    created_storage_accounts: Mutex<HashSet<TrackedStorageAccount>>,
}

impl AsyncTestContext for BlobContainerTestContext {
    async fn setup() -> BlobContainerTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok(); // Initialize tracing

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");
        let storage_account_name = env::var("ALIEN_TEST_AZURE_STORAGE_ACCOUNT")
            .expect("ALIEN_TEST_AZURE_STORAGE_ACCOUNT not set");

        // Create platform config with service principal credentials
        let client_config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        info!("🔧 Using subscription: {}, resource group: {}, and storage account: {} for blob container testing", 
              subscription_id, resource_group_name, storage_account_name);

        let client = Client::new();
        BlobContainerTestContext {
            blob_container_client: AzureBlobContainerClient::new(
                client.clone(),
                AzureTokenCache::new(client_config.clone()),
            ),
            storage_accounts_client: AzureStorageAccountsClient::new(
                client.clone(),
                AzureTokenCache::new(client_config.clone()),
            ),
            long_running_operation_client: LongRunningOperationClient::new(
                client,
                AzureTokenCache::new(client_config),
            ),
            subscription_id,
            resource_group_name,
            storage_account_name,
            created_containers: Mutex::new(HashSet::new()),
            created_storage_accounts: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting blob container test cleanup...");

        // Cleanup all created containers first
        let containers_to_cleanup = {
            let containers = self.created_containers.lock().unwrap();
            containers.clone()
        };

        for tracked_container in containers_to_cleanup {
            self.cleanup_blob_container(&tracked_container).await;
        }

        // Cleanup all created storage accounts
        let storage_accounts_to_cleanup = {
            let storage_accounts = self.created_storage_accounts.lock().unwrap();
            storage_accounts.clone()
        };

        for tracked_storage_account in storage_accounts_to_cleanup {
            self.cleanup_storage_account(&tracked_storage_account).await;
        }

        info!("✅ Blob container test cleanup completed");
    }
}

impl BlobContainerTestContext {
    fn track_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) {
        let tracked = TrackedBlobContainer {
            resource_group_name: resource_group_name.to_string(),
            storage_account_name: storage_account_name.to_string(),
            container_name: container_name.to_string(),
        };
        let mut containers = self.created_containers.lock().unwrap();
        containers.insert(tracked.clone());
        info!(
            "📝 Tracking blob container for cleanup: {}/{}/{}",
            resource_group_name, storage_account_name, container_name
        );
    }

    fn untrack_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) {
        let tracked = TrackedBlobContainer {
            resource_group_name: resource_group_name.to_string(),
            storage_account_name: storage_account_name.to_string(),
            container_name: container_name.to_string(),
        };
        let mut containers = self.created_containers.lock().unwrap();
        containers.remove(&tracked);
        info!(
            "✅ Blob container {}/{}/{} successfully cleaned up and untracked",
            resource_group_name, storage_account_name, container_name
        );
    }

    async fn cleanup_blob_container(&self, tracked: &TrackedBlobContainer) {
        info!(
            "🧹 Cleaning up blob container: {}/{}/{}",
            tracked.resource_group_name, tracked.storage_account_name, tracked.container_name
        );

        match self
            .blob_container_client
            .delete_blob_container(
                &tracked.resource_group_name,
                &tracked.storage_account_name,
                &tracked.container_name,
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Blob container {}/{}/{} deleted successfully",
                    tracked.resource_group_name,
                    tracked.storage_account_name,
                    tracked.container_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Blob container {}/{}/{} was already deleted",
                    tracked.resource_group_name,
                    tracked.storage_account_name,
                    tracked.container_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete blob container {}/{}/{} during cleanup: {:?}",
                    tracked.resource_group_name,
                    tracked.storage_account_name,
                    tracked.container_name,
                    e
                );
            }
        }
    }

    fn generate_unique_container_name(&self) -> String {
        format!(
            "alien-test-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    async fn create_test_blob_container(
        &self,
        container_name: &str,
        public_access: Option<ContainerPropertiesPublicAccess>,
    ) -> Result<BlobContainer, Error> {
        let mut properties = ContainerProperties::default();
        if let Some(access) = public_access {
            properties.public_access = Some(access);
        }

        let blob_container = BlobContainer {
            properties: Some(properties),
            ..Default::default()
        };

        let result = self
            .blob_container_client
            .create_blob_container(
                &self.resource_group_name,
                &self.storage_account_name,
                container_name,
                &blob_container,
            )
            .await;

        if result.is_ok() {
            self.track_container(
                &self.resource_group_name,
                &self.storage_account_name,
                container_name,
            );
        }

        result
    }

    fn track_storage_account(&self, resource_group_name: &str, storage_account_name: &str) {
        let tracked = TrackedStorageAccount {
            resource_group_name: resource_group_name.to_string(),
            storage_account_name: storage_account_name.to_string(),
        };
        let mut storage_accounts = self.created_storage_accounts.lock().unwrap();
        storage_accounts.insert(tracked.clone());
        info!(
            "📝 Tracking storage account for cleanup: {}/{}",
            resource_group_name, storage_account_name
        );
    }

    fn untrack_storage_account(&self, resource_group_name: &str, storage_account_name: &str) {
        let tracked = TrackedStorageAccount {
            resource_group_name: resource_group_name.to_string(),
            storage_account_name: storage_account_name.to_string(),
        };
        let mut storage_accounts = self.created_storage_accounts.lock().unwrap();
        storage_accounts.remove(&tracked);
        info!(
            "✅ Storage account {}/{} successfully cleaned up and untracked",
            resource_group_name, storage_account_name
        );
    }

    async fn cleanup_storage_account(&self, tracked: &TrackedStorageAccount) {
        info!(
            "🧹 Cleaning up storage account: {}/{}",
            tracked.resource_group_name, tracked.storage_account_name
        );

        match self
            .storage_accounts_client
            .delete_storage_account(&tracked.resource_group_name, &tracked.storage_account_name)
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Storage account {}/{} deleted successfully",
                    tracked.resource_group_name, tracked.storage_account_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Storage account {}/{} was already deleted",
                    tracked.resource_group_name, tracked.storage_account_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete storage account {}/{} during cleanup: {:?}",
                    tracked.resource_group_name, tracked.storage_account_name, e
                );
            }
        }
    }

    fn generate_unique_storage_account_name(&self) -> String {
        // Storage account names must be 3-24 characters, lowercase alphanumeric only
        let uuid = Uuid::new_v4().as_simple().to_string();
        format!("alientest{}", &uuid[0..8])
    }

    async fn create_test_storage_account(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
    ) -> Result<(), Error> {
        let create_params = StorageAccountCreateParameters {
            sku: Sku {
                name: SkuName::StandardLrs,
                tier: Some(Tier::Standard),
            },
            kind: StorageAccountCreateParametersKind::StorageV2,
            location: "eastus".to_string(),
            tags: std::collections::HashMap::new(),
            properties: None,
            identity: None,
            extended_location: None,
        };

        let operation_result = self
            .storage_accounts_client
            .create_storage_account(resource_group_name, storage_account_name, &create_params)
            .await?;

        // Wait for the operation to complete
        operation_result
            .wait_for_operation_completion(
                &self.long_running_operation_client,
                "CreateStorageAccount",
                storage_account_name,
            )
            .await?;

        self.track_storage_account(resource_group_name, storage_account_name);
        info!(
            "✅ Storage account {}/{} created successfully",
            resource_group_name, storage_account_name
        );

        Ok(())
    }
}

// Implement Hash and PartialEq for TrackedBlobContainer to use in HashSet
impl std::hash::Hash for TrackedBlobContainer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.resource_group_name.hash(state);
        self.storage_account_name.hash(state);
        self.container_name.hash(state);
    }
}

impl PartialEq for TrackedBlobContainer {
    fn eq(&self, other: &Self) -> bool {
        self.resource_group_name == other.resource_group_name
            && self.storage_account_name == other.storage_account_name
            && self.container_name == other.container_name
    }
}

impl Eq for TrackedBlobContainer {}

// Implement Hash, PartialEq, and Eq for TrackedStorageAccount to use in HashSet
impl std::hash::Hash for TrackedStorageAccount {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.resource_group_name.hash(state);
        self.storage_account_name.hash(state);
    }
}

impl PartialEq for TrackedStorageAccount {
    fn eq(&self, other: &Self) -> bool {
        self.resource_group_name == other.resource_group_name
            && self.storage_account_name == other.storage_account_name
    }
}

impl Eq for TrackedStorageAccount {}

// -------------------------------------------------------------------------
// Blob Container CRUD tests
// -------------------------------------------------------------------------

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_create_and_delete_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    // Create blob container
    let create_result = ctx.create_test_blob_container(&container_name, None).await;
    assert!(
        create_result.is_ok(),
        "Failed to create blob container: {:?}",
        create_result.err()
    );

    let created_container = create_result.unwrap();
    assert_eq!(created_container.name.as_ref(), Some(&container_name));
    assert!(created_container.properties.is_some());

    // Delete blob container
    let delete_result = ctx
        .blob_container_client
        .delete_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        )
        .await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete blob container: {:?}",
        delete_result.err()
    );
    ctx.untrack_container(
        &ctx.resource_group_name,
        &ctx.storage_account_name,
        &container_name,
    );

    // Verify container is deleted by trying to get it
    let get_after_delete_result = ctx
        .blob_container_client
        .get_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        )
        .await;
    match get_after_delete_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!(
            "Expected RemoteResourceNotFound after deleting blob container, got {:?}",
            other
        ),
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_get_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    // Create blob container first
    let _created_container = ctx
        .create_test_blob_container(&container_name, Some(ContainerPropertiesPublicAccess::None))
        .await
        .expect("Failed to create blob container for get test");

    // Get blob container
    let get_result = ctx
        .blob_container_client
        .get_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        )
        .await;
    assert!(
        get_result.is_ok(),
        "Failed to get blob container: {:?}",
        get_result.err()
    );

    let retrieved_container = get_result.unwrap();
    assert_eq!(retrieved_container.name.as_ref(), Some(&container_name));

    let properties = retrieved_container
        .properties
        .as_ref()
        .expect("Container should have properties");
    assert_eq!(
        properties.public_access.as_ref(),
        Some(&ContainerPropertiesPublicAccess::None)
    );
    assert!(properties.last_modified_time.is_some());
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_update_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    // Create blob container first
    let _created_container = ctx
        .create_test_blob_container(&container_name, Some(ContainerPropertiesPublicAccess::None))
        .await
        .expect("Failed to create blob container for update test");

    // Update blob container with different public access (skip if not supported)
    let mut updated_properties = ContainerProperties::default();
    // Don't set public access as the test storage account might not support it
    // Add some metadata (Azure metadata keys must be valid C# identifiers)
    updated_properties
        .metadata
        .insert("testkey".to_string(), "test-value".to_string());
    updated_properties
        .metadata
        .insert("environment".to_string(), "test".to_string());

    let updated_container = BlobContainer {
        properties: Some(updated_properties),
        ..Default::default()
    };

    let update_result = ctx
        .blob_container_client
        .update_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
            &updated_container,
        )
        .await;
    assert!(
        update_result.is_ok(),
        "Failed to update blob container: {:?}",
        update_result.err()
    );

    let updated_result = update_result.unwrap();
    let properties = updated_result
        .properties
        .as_ref()
        .expect("Updated container should have properties");
    // Don't check public access since we're not setting it
    assert_eq!(
        properties.metadata.get("testkey"),
        Some(&"test-value".to_string())
    );
    assert_eq!(
        properties.metadata.get("environment"),
        Some(&"test".to_string())
    );
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_create_blob_container_with_different_public_access_levels(
    ctx: &mut BlobContainerTestContext,
) {
    // Only test private access since public access may not be enabled on the test storage account
    let container_name = format!("{}-private", ctx.generate_unique_container_name());

    let create_result = ctx
        .create_test_blob_container(&container_name, Some(ContainerPropertiesPublicAccess::None))
        .await;
    assert!(
        create_result.is_ok(),
        "Failed to create blob container with private access: {:?}",
        create_result.err()
    );

    let created_container = create_result.unwrap();
    let properties = created_container
        .properties
        .as_ref()
        .expect("Container should have properties");
    assert_eq!(
        properties.public_access.as_ref(),
        Some(&ContainerPropertiesPublicAccess::None)
    );

    // Test that public access fails gracefully if not supported
    let container_name_blob = format!("{}-blob", ctx.generate_unique_container_name());
    let create_result_blob = ctx
        .create_test_blob_container(
            &container_name_blob,
            Some(ContainerPropertiesPublicAccess::Blob),
        )
        .await;

    match create_result_blob {
        Ok(_) => {
            info!("✅ Public blob access is supported on this storage account");
        }
        Err(err) => {
            info!(
                "ℹ️ Public access not supported on this storage account: {:?}",
                err
            );
            // This is expected and acceptable for test storage accounts
        }
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_create_blob_container_with_metadata(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    let mut properties = ContainerProperties::default();
    // Azure metadata keys must be valid C# identifiers (alphanumeric + underscore, start with letter/underscore)
    properties
        .metadata
        .insert("project".to_string(), "alien-test".to_string());
    properties
        .metadata
        .insert("team".to_string(), "infrastructure".to_string());
    properties
        .metadata
        .insert("environment".to_string(), "test".to_string());

    let blob_container = BlobContainer {
        properties: Some(properties),
        ..Default::default()
    };

    let create_result = ctx
        .blob_container_client
        .create_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;

    if create_result.is_ok() {
        ctx.track_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        );
    }

    assert!(
        create_result.is_ok(),
        "Failed to create blob container with metadata: {:?}",
        create_result.err()
    );

    let created_container = create_result.unwrap();
    let properties = created_container
        .properties
        .as_ref()
        .expect("Container should have properties");
    assert_eq!(
        properties.metadata.get("project"),
        Some(&"alien-test".to_string())
    );
    assert_eq!(
        properties.metadata.get("team"),
        Some(&"infrastructure".to_string())
    );
    assert_eq!(
        properties.metadata.get("environment"),
        Some(&"test".to_string())
    );
}

// -------------------------------------------------------------------------
// Error scenario tests
// -------------------------------------------------------------------------

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_get_non_existent_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    let result = ctx
        .blob_container_client
        .get_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        )
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_delete_non_existent_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    let result = ctx
        .blob_container_client
        .delete_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        )
        .await;

    // Azure Storage typically returns success for idempotent deletes, but some scenarios might return NotFound
    match result {
        Ok(_) => {
            info!("✅ Delete non-existent blob container returned OK (idempotent delete behavior)");
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!(
                "✅ Delete non-existent blob container returned RemoteResourceNotFound as expected"
            );
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound after deleting non-existent blob container, got {:?}", other);
        }
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_create_blob_container_already_exists(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    // Create blob container first time
    let create_first_result = ctx.create_test_blob_container(&container_name, None).await;
    assert!(
        create_first_result.is_ok(),
        "Failed to create blob container initially: {:?}",
        create_first_result.err()
    );

    // Attempt to create the same container again
    let blob_container = BlobContainer {
        properties: Some(ContainerProperties::default()),
        ..Default::default()
    };

    let create_second_result = ctx
        .blob_container_client
        .create_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;

    // Azure's create operation has upsert semantics - it succeeds if the container already exists
    match create_second_result {
        Ok(_) => {
            info!(
                "✅ Azure blob container create has upsert semantics (succeeds if already exists)"
            );
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceConflict { .. })) => {
            info!("✅ Azure returned RemoteResourceConflict as expected");
        }
        Err(other) => {
            panic!(
                "Expected Ok or RemoteResourceConflict when creating duplicate container, got {:?}",
                other
            );
        }
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_invalid_storage_account_name(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();
    let invalid_storage_account = "invalid-storage-account-name-that-does-not-exist";

    let blob_container = BlobContainer {
        properties: Some(ContainerProperties::default()),
        ..Default::default()
    };

    let result = ctx
        .blob_container_client
        .create_blob_container(
            &ctx.resource_group_name,
            invalid_storage_account,
            &container_name,
            &blob_container,
        )
        .await;

    // This should fail with ResourceNotFound because the storage account doesn't exist
    assert!(
        result.is_err(),
        "Expected error for invalid storage account, got success"
    );
    match result.err().unwrap() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Invalid storage account correctly returned RemoteResourceNotFound");
        }
        other => {
            // Could also be other errors depending on Azure's response
            info!("Invalid storage account returned error: {:?}", other);
        }
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_invalid_resource_group_name(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();
    let invalid_resource_group = "invalid-resource-group-name-that-does-not-exist";

    let blob_container = BlobContainer {
        properties: Some(ContainerProperties::default()),
        ..Default::default()
    };

    let result = ctx
        .blob_container_client
        .create_blob_container(
            invalid_resource_group,
            &ctx.storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;

    // This should fail with ResourceNotFound because the resource group doesn't exist
    assert!(
        result.is_err(),
        "Expected error for invalid resource group, got success"
    );
    match result.err().unwrap() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Invalid resource group correctly returned RemoteResourceNotFound");
        }
        other => {
            // Could also be other errors depending on Azure's response
            info!("Invalid resource group returned error: {:?}", other);
        }
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_container_name_validation(ctx: &mut BlobContainerTestContext) {
    let too_long_name = "a".repeat(64);
    let invalid_container_names = vec![
        "UPPERCASE",                  // Container names must be lowercase
        "container-with-CAPS",        // Mixed case not allowed
        "a",                          // Too short (minimum 3 chars)
        "ab",                         // Too short
        "container_with_underscores", // Underscores not allowed
        "container..double.dots",     // Double dots not allowed
        "-container",                 // Cannot start with dash
        "container-",                 // Cannot end with dash
        &too_long_name,               // Too long (maximum 63 chars)
    ];

    for invalid_name in invalid_container_names {
        let blob_container = BlobContainer {
            properties: Some(ContainerProperties::default()),
            ..Default::default()
        };

        let result = ctx
            .blob_container_client
            .create_blob_container(
                &ctx.resource_group_name,
                &ctx.storage_account_name,
                invalid_name,
                &blob_container,
            )
            .await;

        assert!(
            result.is_err(),
            "Expected error for invalid container name '{}', got success",
            invalid_name
        );
        info!(
            "✅ Invalid container name '{}' correctly returned error: {:?}",
            invalid_name,
            result.err().unwrap()
        );
    }
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_update_non_existent_blob_container(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    let blob_container = BlobContainer {
        properties: Some(ContainerProperties::default()),
        ..Default::default()
    };

    let result = ctx
        .blob_container_client
        .update_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;

    // Azure's update operation has upsert semantics - it creates the container if it doesn't exist
    match result {
        Ok(_) => {
            info!("✅ Azure blob container update has upsert semantics (creates if not exists)");
            // Track for cleanup since it was created
            ctx.track_container(
                &ctx.resource_group_name,
                &ctx.storage_account_name,
                &container_name,
            );
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Azure returned RemoteResourceNotFound as expected");
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound when updating non-existent container, got {:?}", other);
        }
    }
}

// -------------------------------------------------------------------------
// Performance and edge case tests
// -------------------------------------------------------------------------

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_container_with_maximum_metadata(ctx: &mut BlobContainerTestContext) {
    let container_name = ctx.generate_unique_container_name();

    let mut properties = ContainerProperties::default();

    // Add the maximum allowed metadata (Azure allows up to 8KB of metadata)
    // Each metadata key-value pair has some overhead, so we'll add a reasonable number
    // Azure metadata keys must be valid C# identifiers
    for i in 0..50 {
        let key = format!("metadatakey{:02}", i); // Valid C# identifier
        let value = format!(
            "metadata-value-for-key-{:02}-with-some-additional-content-to-make-it-longer",
            i
        );
        properties.metadata.insert(key, value);
    }

    let blob_container = BlobContainer {
        properties: Some(properties),
        ..Default::default()
    };

    let create_result = ctx
        .blob_container_client
        .create_blob_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;

    if create_result.is_ok() {
        ctx.track_container(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &container_name,
        );
    }

    assert!(
        create_result.is_ok(),
        "Failed to create blob container with maximum metadata: {:?}",
        create_result.err()
    );

    let created_container = create_result.unwrap();
    let properties = created_container
        .properties
        .as_ref()
        .expect("Container should have properties");
    assert_eq!(properties.metadata.len(), 50);

    // Verify a few sample metadata entries
    assert!(properties.metadata.contains_key("metadatakey00"));
    assert!(properties.metadata.contains_key("metadatakey25"));
    assert!(properties.metadata.contains_key("metadatakey49"));
}

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_container_name_edge_cases(ctx: &mut BlobContainerTestContext) {
    let max_length_name = "a".repeat(63);
    let valid_edge_case_names = vec![
        "abc",                   // Minimum length (3 chars)
        &max_length_name,        // Maximum length (63 chars)
        "container-with-dashes", // Dashes allowed in middle
        "123numeric456",         // Numbers allowed
        "test123abc",            // Mixed alphanumeric
    ];

    for name in valid_edge_case_names {
        let create_result = ctx.create_test_blob_container(name, None).await;
        assert!(
            create_result.is_ok(),
            "Failed to create container with valid edge case name '{}': {:?}",
            name,
            create_result.err()
        );

        let created_container = create_result.unwrap();
        assert_eq!(created_container.name.as_ref(), Some(&name.to_string()));

        info!(
            "✅ Successfully created container with edge case name: '{}'",
            name
        );
    }
}

// -------------------------------------------------------------------------
// Comprehensive test that creates storage account, blob container, updates, and cleans up
// -------------------------------------------------------------------------

#[test_context(BlobContainerTestContext)]
#[tokio::test]
async fn test_comprehensive_storage_account_and_blob_container_lifecycle(
    ctx: &mut BlobContainerTestContext,
) {
    // Generate unique names
    let storage_account_name = ctx.generate_unique_storage_account_name();
    let container_name = ctx.generate_unique_container_name();

    info!(
        "🚀 Starting comprehensive test with storage account: {} and container: {}",
        storage_account_name, container_name
    );

    // Step 1: Create a storage account
    info!("📦 Step 1: Creating storage account...");
    let create_result = ctx
        .create_test_storage_account(&ctx.resource_group_name, &storage_account_name)
        .await;
    assert!(
        create_result.is_ok(),
        "Failed to create storage account: {:?}",
        create_result.err()
    );
    info!("✅ Storage account created successfully");

    // Step 2: Create a blob container inside the storage account
    info!("📦 Step 2: Creating blob container...");
    let mut properties = ContainerProperties::default();
    properties
        .metadata
        .insert("test".to_string(), "comprehensive-lifecycle".to_string());
    properties
        .metadata
        .insert("created_by".to_string(), "alien-infra-test".to_string());

    let blob_container = BlobContainer {
        properties: Some(properties),
        ..Default::default()
    };

    let create_container_result = ctx
        .blob_container_client
        .create_blob_container(
            &ctx.resource_group_name,
            &storage_account_name,
            &container_name,
            &blob_container,
        )
        .await;
    assert!(
        create_container_result.is_ok(),
        "Failed to create blob container: {:?}",
        create_container_result.err()
    );

    // Track the container for cleanup (note: we use the test storage account, not the default one)
    ctx.track_container(
        &ctx.resource_group_name,
        &storage_account_name,
        &container_name,
    );
    info!("✅ Blob container created successfully");

    // Step 3: Update the storage account (add some tags)
    info!("📦 Step 3: Updating storage account...");
    let mut update_tags = std::collections::HashMap::new();
    update_tags.insert("environment".to_string(), "test".to_string());
    update_tags.insert("team".to_string(), "alien-infra".to_string());
    update_tags.insert("lifecycle_test".to_string(), "comprehensive".to_string());

    let update_params = StorageAccountUpdateParameters {
        tags: update_tags,
        properties: Some(StorageAccountPropertiesUpdateParameters {
            supports_https_traffic_only: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };

    let update_result = ctx
        .storage_accounts_client
        .update_storage_account(
            &ctx.resource_group_name,
            &storage_account_name,
            &update_params,
        )
        .await;
    assert!(
        update_result.is_ok(),
        "Failed to update storage account: {:?}",
        update_result.err()
    );

    // Wait for the update operation to complete
    let update_operation_result = update_result.unwrap();
    let wait_result = update_operation_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "UpdateStorageAccount",
            &storage_account_name,
        )
        .await;
    assert!(
        wait_result.is_ok(),
        "Failed to wait for storage account update: {:?}",
        wait_result.err()
    );
    info!("✅ Storage account updated successfully");

    // Step 4: Verify the storage account properties
    info!("📦 Step 4: Verifying storage account properties...");
    let get_result = ctx
        .storage_accounts_client
        .get_storage_account_properties(&ctx.resource_group_name, &storage_account_name)
        .await;
    assert!(
        get_result.is_ok(),
        "Failed to get storage account properties: {:?}",
        get_result.err()
    );

    let storage_account = get_result.unwrap();
    assert_eq!(
        storage_account.tags.get("environment"),
        Some(&"test".to_string())
    );
    assert_eq!(
        storage_account.tags.get("team"),
        Some(&"alien-infra".to_string())
    );
    assert_eq!(
        storage_account.tags.get("lifecycle_test"),
        Some(&"comprehensive".to_string())
    );

    if let Some(properties) = &storage_account.properties {
        assert_eq!(properties.supports_https_traffic_only, Some(true));
    }
    info!("✅ Storage account properties verified");

    // Step 5: Update the blob container metadata
    info!("📦 Step 5: Updating blob container...");
    let mut updated_properties = ContainerProperties::default();
    updated_properties.metadata.insert(
        "test".to_string(),
        "comprehensive-lifecycle-updated".to_string(),
    );
    updated_properties
        .metadata
        .insert("created_by".to_string(), "alien-infra-test".to_string());
    updated_properties
        .metadata
        .insert("updated_at".to_string(), chrono::Utc::now().to_rfc3339());

    let updated_blob_container = BlobContainer {
        properties: Some(updated_properties),
        ..Default::default()
    };

    let update_container_result = ctx
        .blob_container_client
        .update_blob_container(
            &ctx.resource_group_name,
            &storage_account_name,
            &container_name,
            &updated_blob_container,
        )
        .await;
    assert!(
        update_container_result.is_ok(),
        "Failed to update blob container: {:?}",
        update_container_result.err()
    );
    info!("✅ Blob container updated successfully");

    // Step 6: Verify the blob container properties
    info!("📦 Step 6: Verifying blob container properties...");
    let get_container_result = ctx
        .blob_container_client
        .get_blob_container(
            &ctx.resource_group_name,
            &storage_account_name,
            &container_name,
        )
        .await;
    assert!(
        get_container_result.is_ok(),
        "Failed to get blob container: {:?}",
        get_container_result.err()
    );

    let retrieved_container = get_container_result.unwrap();
    let properties = retrieved_container
        .properties
        .as_ref()
        .expect("Container should have properties");
    assert_eq!(
        properties.metadata.get("test"),
        Some(&"comprehensive-lifecycle-updated".to_string())
    );
    assert_eq!(
        properties.metadata.get("created_by"),
        Some(&"alien-infra-test".to_string())
    );
    assert!(properties.metadata.contains_key("updated_at"));
    info!("✅ Blob container properties verified");

    // Step 7: Delete the blob container
    info!("📦 Step 7: Deleting blob container...");
    let delete_container_result = ctx
        .blob_container_client
        .delete_blob_container(
            &ctx.resource_group_name,
            &storage_account_name,
            &container_name,
        )
        .await;
    assert!(
        delete_container_result.is_ok(),
        "Failed to delete blob container: {:?}",
        delete_container_result.err()
    );
    ctx.untrack_container(
        &ctx.resource_group_name,
        &storage_account_name,
        &container_name,
    );
    info!("✅ Blob container deleted successfully");

    // Step 8: Delete the storage account
    info!("📦 Step 8: Deleting storage account...");
    let delete_storage_result = ctx
        .storage_accounts_client
        .delete_storage_account(&ctx.resource_group_name, &storage_account_name)
        .await;
    assert!(
        delete_storage_result.is_ok(),
        "Failed to delete storage account: {:?}",
        delete_storage_result.err()
    );
    ctx.untrack_storage_account(&ctx.resource_group_name, &storage_account_name);
    info!("✅ Storage account deleted successfully");

    // Step 9: Verify resources are deleted
    info!("📦 Step 9: Verifying resources are deleted...");

    // Verify storage account is deleted
    let get_after_delete_result = ctx
        .storage_accounts_client
        .get_storage_account_properties(&ctx.resource_group_name, &storage_account_name)
        .await;
    match get_after_delete_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!(
            "Expected RemoteResourceNotFound after deleting storage account, got {:?}",
            other
        ),
    }

    info!("🎉 Comprehensive test completed successfully! All resources created, updated, and cleaned up properly.");
}
