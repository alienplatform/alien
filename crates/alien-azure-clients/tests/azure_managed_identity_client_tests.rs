#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi};
use alien_azure_clients::models::managed_identity::{
    Identity, IdentityUpdate, UserAssignedIdentityProperties,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_azure_clients::AzureTokenCache;
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct ManagedIdentityTestContext {
    identity_client: AzureManagedIdentityClient,
    subscription_id: String,
    resource_group_name: String,
    created_identities: Mutex<HashSet<String>>,
}

impl AsyncTestContext for ManagedIdentityTestContext {
    async fn setup() -> ManagedIdentityTestContext {
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

        info!(
            "🔧 Using subscription: {} and resource group: {} for managed identity testing",
            subscription_id, resource_group_name
        );

        let client = Client::new();
        ManagedIdentityTestContext {
            identity_client: AzureManagedIdentityClient::new(client, AzureTokenCache::new(client_config)),
            subscription_id,
            resource_group_name,
            created_identities: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Managed Identity test cleanup...");

        let identities_to_cleanup = {
            let identities = self.created_identities.lock().unwrap();
            identities.clone()
        };

        for identity_name in identities_to_cleanup {
            self.cleanup_managed_identity(&identity_name).await;
        }

        info!("✅ Managed Identity test cleanup completed");
    }
}

impl ManagedIdentityTestContext {
    fn track_managed_identity(&self, identity_name: &str) {
        let mut identities = self.created_identities.lock().unwrap();
        identities.insert(identity_name.to_string());
        info!(
            "📝 Tracking managed identity for cleanup: {}",
            identity_name
        );
    }

    fn untrack_managed_identity(&self, identity_name: &str) {
        let mut identities = self.created_identities.lock().unwrap();
        identities.remove(identity_name);
        info!(
            "✅ Managed identity {} successfully cleaned up and untracked",
            identity_name
        );
    }

    async fn cleanup_managed_identity(&self, identity_name: &str) {
        info!("🧹 Cleaning up managed identity: {}", identity_name);

        match self
            .identity_client
            .delete_user_assigned_identity(&self.resource_group_name, identity_name)
            .await
        {
            Ok(_) => {
                info!("✅ Managed identity {} deleted successfully", identity_name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Managed identity {} was already deleted", identity_name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete managed identity {} during cleanup: {:?}",
                    identity_name, e
                );
            }
        }
    }

    fn generate_unique_identity_name(&self) -> String {
        format!(
            "alien-test-identity-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    async fn create_test_identity(
        &self,
        identity_name: &str,
        location: &str,
    ) -> Result<Identity, Error> {
        let mut tags = HashMap::new();
        tags.insert("CreatedBy".to_string(), "alien-infra-tests".to_string());
        tags.insert("Purpose".to_string(), "integration-testing".to_string());

        let identity = Identity {
            id: None,
            name: None,
            type_: None,
            location: location.to_string(),
            tags,
            properties: Some(UserAssignedIdentityProperties {
                tenant_id: None,
                principal_id: None,
                client_id: None,
                isolation_scope: None,
            }),
            system_data: None,
        };

        let result = self
            .identity_client
            .create_or_update_user_assigned_identity(
                &self.resource_group_name,
                identity_name,
                &identity,
            )
            .await;

        if result.is_ok() {
            self.track_managed_identity(identity_name);
        }

        result
    }
}

// -------------------------------------------------------------------------
// Basic CRUD tests
// -------------------------------------------------------------------------

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_create_and_delete_managed_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // Create managed identity
    let create_result = ctx.create_test_identity(&identity_name, location).await;
    assert!(
        create_result.is_ok(),
        "Failed to create managed identity: {:?}",
        create_result.err()
    );

    let created_identity = create_result.unwrap();
    assert_eq!(created_identity.location, location);
    assert!(created_identity.properties.is_some());

    let properties = created_identity.properties.as_ref().unwrap();
    assert!(properties.tenant_id.is_some());
    assert!(properties.principal_id.is_some());
    assert!(properties.client_id.is_some());

    // Verify the identity has expected tags
    assert!(!created_identity.tags.is_empty());
    assert_eq!(
        created_identity.tags.get("CreatedBy"),
        Some(&"alien-infra-tests".to_string())
    );

    // Delete managed identity
    let delete_result = ctx
        .identity_client
        .delete_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete managed identity: {:?}",
        delete_result.err()
    );
    ctx.untrack_managed_identity(&identity_name);

    // Verify identity is deleted by trying to get it
    let get_after_delete_result = ctx
        .identity_client
        .get_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;
    match get_after_delete_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!(
            "Expected RemoteResourceNotFound after deleting identity, got {:?}",
            other
        ),
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_get_managed_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // Create identity first
    let created_identity = ctx
        .create_test_identity(&identity_name, location)
        .await
        .expect("Failed to create identity for get test");

    // Get managed identity
    let get_result = ctx
        .identity_client
        .get_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;
    assert!(
        get_result.is_ok(),
        "Failed to get managed identity: {:?}",
        get_result.err()
    );

    let retrieved_identity = get_result.unwrap();

    // Verify basic properties
    assert_eq!(retrieved_identity.location, location);
    assert!(retrieved_identity.properties.is_some());

    // Verify properties match what was created
    let retrieved_props = retrieved_identity.properties.as_ref().unwrap();
    let created_props = created_identity.properties.as_ref().unwrap();

    assert_eq!(
        retrieved_props.tenant_id.as_ref().map(|id| id.to_string()),
        created_props.tenant_id.as_ref().map(|id| id.to_string())
    );
    assert_eq!(
        retrieved_props
            .principal_id
            .as_ref()
            .map(|id| id.to_string()),
        created_props.principal_id.as_ref().map(|id| id.to_string())
    );
    assert_eq!(
        retrieved_props.client_id.as_ref().map(|id| id.to_string()),
        created_props.client_id.as_ref().map(|id| id.to_string())
    );

    // Verify tags are preserved
    assert!(!retrieved_identity.tags.is_empty());
    assert_eq!(
        retrieved_identity.tags.get("CreatedBy"),
        Some(&"alien-infra-tests".to_string())
    );
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_update_managed_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // Create identity first
    let _created_identity = ctx
        .create_test_identity(&identity_name, location)
        .await
        .expect("Failed to create identity for update test");

    // Update managed identity with new tags
    let mut updated_tags = HashMap::new();
    updated_tags.insert("CreatedBy".to_string(), "alien-infra-tests".to_string());
    updated_tags.insert(
        "Purpose".to_string(),
        "integration-testing-updated".to_string(),
    );
    updated_tags.insert("UpdatedAt".to_string(), chrono::Utc::now().to_rfc3339());

    let identity_update = IdentityUpdate {
        id: None,
        name: None,
        type_: None,
        location: Some(location.to_string()),
        tags: updated_tags.clone(),
        properties: None,
        system_data: None,
    };

    let update_result = ctx
        .identity_client
        .update_user_assigned_identity(&ctx.resource_group_name, &identity_name, &identity_update)
        .await;

    assert!(
        update_result.is_ok(),
        "Failed to update managed identity: {:?}",
        update_result.err()
    );

    let updated_identity = update_result.unwrap();

    // Verify tags were updated
    assert!(!updated_identity.tags.is_empty());
    assert_eq!(
        updated_identity.tags.get("Purpose"),
        Some(&"integration-testing-updated".to_string())
    );
    assert!(updated_identity.tags.contains_key("UpdatedAt"));

    // Verify properties are still intact
    assert!(updated_identity.properties.is_some());
    let properties = updated_identity.properties.as_ref().unwrap();
    assert!(properties.tenant_id.is_some());
    assert!(properties.principal_id.is_some());
    assert!(properties.client_id.is_some());
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_create_or_update_idempotent(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // Create identity first time
    let create_first_result = ctx.create_test_identity(&identity_name, location).await;
    assert!(
        create_first_result.is_ok(),
        "Failed to create identity initially: {:?}",
        create_first_result.err()
    );

    let first_identity = create_first_result.unwrap();

    // Create the same identity again (should be idempotent update)
    let mut tags = HashMap::new();
    tags.insert("CreatedBy".to_string(), "alien-infra-tests".to_string());
    tags.insert(
        "Purpose".to_string(),
        "integration-testing-updated".to_string(),
    );

    let identity_update = Identity {
        id: None,
        name: None,
        type_: None,
        location: location.to_string(),
        tags,
        properties: Some(UserAssignedIdentityProperties {
            tenant_id: None,
            principal_id: None,
            client_id: None,
            isolation_scope: None,
        }),
        system_data: None,
    };

    let create_second_result = ctx
        .identity_client
        .create_or_update_user_assigned_identity(
            &ctx.resource_group_name,
            &identity_name,
            &identity_update,
        )
        .await;

    // This should succeed as an update operation
    assert!(
        create_second_result.is_ok(),
        "Identity update should succeed: {:?}",
        create_second_result.err()
    );

    let second_identity = create_second_result.unwrap();

    // Verify that the principal_id and client_id remain the same (identity properties are immutable)
    let first_props = first_identity.properties.as_ref().unwrap();
    let second_props = second_identity.properties.as_ref().unwrap();

    assert_eq!(
        first_props.principal_id.as_ref().map(|id| id.to_string()),
        second_props.principal_id.as_ref().map(|id| id.to_string())
    );
    assert_eq!(
        first_props.client_id.as_ref().map(|id| id.to_string()),
        second_props.client_id.as_ref().map(|id| id.to_string())
    );

    // Verify tags were updated
    assert_eq!(
        second_identity.tags.get("Purpose"),
        Some(&"integration-testing-updated".to_string())
    );
}

// -------------------------------------------------------------------------
// Error scenario tests
// -------------------------------------------------------------------------

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_get_non_existent_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();

    let result = ctx
        .identity_client
        .get_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_delete_non_existent_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();

    let result = ctx
        .identity_client
        .delete_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;

    // Azure may return either Ok (idempotent delete) or RemoteResourceNotFound
    match result {
        Ok(_) => {
            info!("✅ Delete non-existent identity returned OK (idempotent delete behavior)");
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Delete non-existent identity returned RemoteResourceNotFound as expected");
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound after deleting non-existent identity, got {:?}", other);
        }
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_update_non_existent_identity(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();

    let mut tags = HashMap::new();
    tags.insert("Test".to_string(), "Value".to_string());

    let identity_update = IdentityUpdate {
        id: None,
        name: None,
        type_: None,
        location: Some("eastus".to_string()),
        tags,
        properties: None,
        system_data: None,
    };

    let result = ctx
        .identity_client
        .update_user_assigned_identity(&ctx.resource_group_name, &identity_name, &identity_update)
        .await;

    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!(
            "Expected RemoteResourceNotFound when updating non-existent identity, got {:?}",
            other
        ),
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_create_identity_invalid_resource_group(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let invalid_resource_group = "non-existent-resource-group-12345";
    let location = "eastus";

    let mut tags = HashMap::new();
    tags.insert("Test".to_string(), "Invalid".to_string());

    let identity = Identity {
        id: None,
        name: None,
        type_: None,
        location: location.to_string(),
        tags,
        properties: Some(UserAssignedIdentityProperties {
            tenant_id: None,
            principal_id: None,
            client_id: None,
            isolation_scope: None,
        }),
        system_data: None,
    };

    let result = ctx
        .identity_client
        .create_or_update_user_assigned_identity(invalid_resource_group, &identity_name, &identity)
        .await;

    // This should fail with some kind of error (likely ResourceNotFound for the resource group)
    assert!(
        result.is_err(),
        "Expected error for invalid resource group, got success"
    );

    match result.err().unwrap() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Got expected RemoteResourceNotFound for invalid resource group");
        }
        err if matches!(err.error, Some(ErrorData::InvalidInput { .. })) => {
            info!("✅ Got expected Generic error for invalid resource group");
        }
        other => {
            info!("Got error for invalid resource group: {:?}", other);
        }
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_create_identity_invalid_location(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let invalid_location = "invalid-location";

    let mut tags = HashMap::new();
    tags.insert("Test".to_string(), "Invalid".to_string());

    let identity = Identity {
        id: None,
        name: None,
        type_: None,
        location: invalid_location.to_string(),
        tags,
        properties: Some(UserAssignedIdentityProperties {
            tenant_id: None,
            principal_id: None,
            client_id: None,
            isolation_scope: None,
        }),
        system_data: None,
    };

    let result = ctx
        .identity_client
        .create_or_update_user_assigned_identity(
            &ctx.resource_group_name,
            &identity_name,
            &identity,
        )
        .await;

    // This should fail with some kind of error (likely BadRequest for invalid location)
    assert!(
        result.is_err(),
        "Expected error for invalid location, got success"
    );

    match result.err().unwrap() {
        err if matches!(err.error, Some(ErrorData::InvalidInput { .. })) => {
            info!("✅ Got expected error for invalid location");
        }
        other => {
            info!("Got error for invalid location: {:?}", other);
        }
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_create_identity_invalid_name(ctx: &mut ManagedIdentityTestContext) {
    let invalid_identity_name = "invalid-name-with-@#$%^&*()";
    let location = "eastus";

    let mut tags = HashMap::new();
    tags.insert("Test".to_string(), "Invalid".to_string());

    let identity = Identity {
        id: None,
        name: None,
        type_: None,
        location: location.to_string(),
        tags,
        properties: Some(UserAssignedIdentityProperties {
            tenant_id: None,
            principal_id: None,
            client_id: None,
            isolation_scope: None,
        }),
        system_data: None,
    };

    let result = ctx
        .identity_client
        .create_or_update_user_assigned_identity(
            &ctx.resource_group_name,
            invalid_identity_name,
            &identity,
        )
        .await;

    // This should fail with some kind of error (likely BadRequest for invalid name)
    assert!(
        result.is_err(),
        "Expected error for invalid identity name, got success"
    );

    match result.err().unwrap() {
        err if matches!(err.error, Some(ErrorData::InvalidInput { .. })) => {
            info!("✅ Got expected error for invalid identity name");
        }
        other => {
            info!("Got error for invalid identity name: {:?}", other);
        }
    }
}

// -------------------------------------------------------------------------
// Helper method tests
// -------------------------------------------------------------------------

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_build_user_assigned_identity_id(ctx: &mut ManagedIdentityTestContext) {
    let resource_group = "my-resource-group";
    let identity_name = "my-identity";

    let id = ctx
        .identity_client
        .build_user_assigned_identity_id(resource_group, identity_name);
    let expected = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
        ctx.subscription_id, resource_group, identity_name
    );

    assert_eq!(id, expected);
}

// -------------------------------------------------------------------------
// Advanced scenario tests
// -------------------------------------------------------------------------

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_identity_with_complex_tags(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // Create identity with complex tags
    let mut complex_tags = HashMap::new();
    complex_tags.insert("Environment".to_string(), "Test".to_string());
    complex_tags.insert("Owner".to_string(), "alien-infra-team".to_string());
    complex_tags.insert("CostCenter".to_string(), "12345".to_string());
    complex_tags.insert("Project".to_string(), "Integration Testing".to_string());
    complex_tags.insert("Unicode".to_string(), "测试🚀".to_string());

    let identity = Identity {
        id: None,
        name: None,
        type_: None,
        location: location.to_string(),
        tags: complex_tags.clone(),
        properties: Some(UserAssignedIdentityProperties {
            tenant_id: None,
            principal_id: None,
            client_id: None,
            isolation_scope: None,
        }),
        system_data: None,
    };

    let create_result = ctx
        .identity_client
        .create_or_update_user_assigned_identity(
            &ctx.resource_group_name,
            &identity_name,
            &identity,
        )
        .await;

    if create_result.is_ok() {
        ctx.track_managed_identity(&identity_name);
    }

    assert!(
        create_result.is_ok(),
        "Failed to create identity with complex tags: {:?}",
        create_result.err()
    );

    let created_identity = create_result.unwrap();

    // Verify complex tags were preserved
    assert!(!created_identity.tags.is_empty());

    for (key, expected_value) in complex_tags.iter() {
        assert_eq!(
            created_identity.tags.get(key),
            Some(expected_value),
            "Tag '{}' value mismatch",
            key
        );
    }
}

#[test_context(ManagedIdentityTestContext)]
#[tokio::test]
async fn test_identity_lifecycle_complete(ctx: &mut ManagedIdentityTestContext) {
    let identity_name = ctx.generate_unique_identity_name();
    let location = "eastus";

    // 1. Create identity
    let created_identity = ctx
        .create_test_identity(&identity_name, location)
        .await
        .expect("Failed to create identity for lifecycle test");

    // Store original properties for comparison
    let original_props = created_identity.properties.as_ref().unwrap();
    let original_principal_id = original_props.principal_id.clone();
    let original_client_id = original_props.client_id.clone();

    // 2. Get identity and verify it exists
    let retrieved_identity = ctx
        .identity_client
        .get_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await
        .expect("Failed to get created identity");

    assert_eq!(retrieved_identity.location, location);

    // 3. Update identity with new tags
    let mut updated_tags = HashMap::new();
    updated_tags.insert("Lifecycle".to_string(), "Updated".to_string());
    updated_tags.insert("Step".to_string(), "3".to_string());

    let identity_update = IdentityUpdate {
        id: None,
        name: None,
        type_: None,
        location: Some(location.to_string()),
        tags: updated_tags.clone(),
        properties: None,
        system_data: None,
    };

    let updated_identity = ctx
        .identity_client
        .update_user_assigned_identity(&ctx.resource_group_name, &identity_name, &identity_update)
        .await
        .expect("Failed to update identity");

    // Verify the update preserved identity properties but updated tags
    let updated_props = updated_identity.properties.as_ref().unwrap();
    assert_eq!(
        updated_props.principal_id.as_ref().map(|id| id.to_string()),
        original_principal_id.as_ref().map(|id| id.to_string())
    );
    assert_eq!(
        updated_props.client_id.as_ref().map(|id| id.to_string()),
        original_client_id.as_ref().map(|id| id.to_string())
    );

    assert_eq!(
        updated_identity.tags.get("Lifecycle"),
        Some(&"Updated".to_string())
    );

    // 4. Delete identity
    ctx.identity_client
        .delete_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await
        .expect("Failed to delete identity");
    ctx.untrack_managed_identity(&identity_name);

    // 5. Verify deletion
    let get_after_delete = ctx
        .identity_client
        .get_user_assigned_identity(&ctx.resource_group_name, &identity_name)
        .await;
    assert!(
        matches!(get_after_delete, Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. }))),
        "Identity should not exist after deletion"
    );

    info!("✅ Complete identity lifecycle test passed");
}
