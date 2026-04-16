#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::containerregistry::{AzureContainerRegistryClient, ContainerRegistryApi};
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::models::containerregistry::{
    Registry, RegistryPropertiesUpdateParameters,
    RegistryPropertiesUpdateParametersNetworkRuleBypassOptions, RegistryUpdateParameters,
    ScopeMapProperties, ScopeMapPropertiesUpdateParameters, ScopeMapUpdateParameters, Sku, SkuName,
    TokenProperties, TokenPropertiesStatus, TokenUpdateParameters, TokenUpdateProperties,
    TokenUpdatePropertiesStatus,
};
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct TrackedRegistry {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedScopeMap {
    registry_name: String,
    scope_map_name: String,
}

#[derive(Debug, Clone)]
struct TrackedToken {
    registry_name: String,
    token_name: String,
}

struct ContainerRegistryTestContext {
    container_registry_client: AzureContainerRegistryClient,
    long_running_operation_client: LongRunningOperationClient,
    subscription_id: String,
    resource_group_name: String,
    created_registries: Mutex<Vec<TrackedRegistry>>,
    created_scope_maps: Mutex<Vec<TrackedScopeMap>>,
    created_tokens: Mutex<Vec<TrackedToken>>,
}

impl AsyncTestContext for ContainerRegistryTestContext {
    async fn setup() -> ContainerRegistryTestContext {
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
            "🔧 Using subscription: {} and resource group: {} for container registry testing",
            subscription_id, resource_group_name
        );

        let client = Client::new();
        ContainerRegistryTestContext {
            container_registry_client: AzureContainerRegistryClient::new(
                client.clone(),
                AzureTokenCache::new(client_config.clone()),
            ),
            long_running_operation_client: LongRunningOperationClient::new(
                client,
                AzureTokenCache::new(client_config),
            ),
            subscription_id,
            resource_group_name,
            created_registries: Mutex::new(Vec::new()),
            created_scope_maps: Mutex::new(Vec::new()),
            created_tokens: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Container Registry test cleanup...");

        // Cleanup tokens first (they depend on scope maps)
        let tokens_to_cleanup = {
            let tokens = self.created_tokens.lock().unwrap();
            tokens.clone()
        };

        for tracked_token in tokens_to_cleanup {
            self.cleanup_token(&tracked_token).await;
        }

        // Cleanup scope maps (they depend on registries)
        let scope_maps_to_cleanup = {
            let scope_maps = self.created_scope_maps.lock().unwrap();
            scope_maps.clone()
        };

        for tracked_scope_map in scope_maps_to_cleanup {
            self.cleanup_scope_map(&tracked_scope_map).await;
        }

        // Finally cleanup registries
        let registries_to_cleanup = {
            let registries = self.created_registries.lock().unwrap();
            registries.clone()
        };

        for tracked_registry in registries_to_cleanup {
            self.cleanup_registry(&tracked_registry).await;
        }

        info!("✅ Container Registry test cleanup completed");
    }
}

impl ContainerRegistryTestContext {
    fn track_registry(&self, registry_name: &str) {
        let tracked = TrackedRegistry {
            name: registry_name.to_string(),
        };
        let mut registries = self.created_registries.lock().unwrap();
        registries.push(tracked);
        info!("📝 Tracking registry for cleanup: {}", registry_name);
    }

    fn untrack_registry(&self, registry_name: &str) {
        let mut registries = self.created_registries.lock().unwrap();
        registries.retain(|tracked| tracked.name != registry_name);
        info!(
            "✅ Registry {} successfully cleaned up and untracked",
            registry_name
        );
    }

    fn track_scope_map(&self, registry_name: &str, scope_map_name: &str) {
        let tracked = TrackedScopeMap {
            registry_name: registry_name.to_string(),
            scope_map_name: scope_map_name.to_string(),
        };
        let mut scope_maps = self.created_scope_maps.lock().unwrap();
        scope_maps.push(tracked);
        info!(
            "📝 Tracking scope map for cleanup: {}/{}",
            registry_name, scope_map_name
        );
    }

    fn untrack_scope_map(&self, registry_name: &str, scope_map_name: &str) {
        let mut scope_maps = self.created_scope_maps.lock().unwrap();
        scope_maps.retain(|tracked| {
            !(tracked.registry_name == registry_name && tracked.scope_map_name == scope_map_name)
        });
        info!(
            "✅ Scope map {}/{} successfully cleaned up and untracked",
            registry_name, scope_map_name
        );
    }

    fn track_token(&self, registry_name: &str, token_name: &str) {
        let tracked = TrackedToken {
            registry_name: registry_name.to_string(),
            token_name: token_name.to_string(),
        };
        let mut tokens = self.created_tokens.lock().unwrap();
        tokens.push(tracked);
        info!(
            "📝 Tracking token for cleanup: {}/{}",
            registry_name, token_name
        );
    }

    fn untrack_token(&self, registry_name: &str, token_name: &str) {
        let mut tokens = self.created_tokens.lock().unwrap();
        tokens.retain(|tracked| {
            !(tracked.registry_name == registry_name && tracked.token_name == token_name)
        });
        info!(
            "✅ Token {}/{} successfully cleaned up and untracked",
            registry_name, token_name
        );
    }

    async fn cleanup_registry(&self, tracked: &TrackedRegistry) {
        info!("🧹 Cleaning up registry: {}", tracked.name);

        match self
            .container_registry_client
            .delete_registry(&self.resource_group_name, &tracked.name)
            .await
        {
            Ok(_) => {
                info!("✅ Registry {} deleted successfully", tracked.name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Registry {} was already deleted", tracked.name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete registry {} during cleanup: {:?}",
                    tracked.name, e
                );
            }
        }
    }

    async fn cleanup_scope_map(&self, tracked: &TrackedScopeMap) {
        info!(
            "🧹 Cleaning up scope map: {}/{}",
            tracked.registry_name, tracked.scope_map_name
        );

        match self
            .container_registry_client
            .delete_scope_map(
                &self.resource_group_name,
                &tracked.registry_name,
                &tracked.scope_map_name,
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Scope map {}/{} deleted successfully",
                    tracked.registry_name, tracked.scope_map_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Scope map {}/{} was already deleted",
                    tracked.registry_name, tracked.scope_map_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete scope map {}/{} during cleanup: {:?}",
                    tracked.registry_name, tracked.scope_map_name, e
                );
            }
        }
    }

    async fn cleanup_token(&self, tracked: &TrackedToken) {
        info!(
            "🧹 Cleaning up token: {}/{}",
            tracked.registry_name, tracked.token_name
        );

        match self
            .container_registry_client
            .delete_token(
                &self.resource_group_name,
                &tracked.registry_name,
                &tracked.token_name,
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Token {}/{} deleted successfully",
                    tracked.registry_name, tracked.token_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Token {}/{} was already deleted",
                    tracked.registry_name, tracked.token_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete token {}/{} during cleanup: {:?}",
                    tracked.registry_name, tracked.token_name, e
                );
            }
        }
    }

    fn generate_unique_registry_name(&self) -> String {
        format!(
            "alientest{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_scope_map_name(&self) -> String {
        format!(
            "alien-test-scope-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_token_name(&self) -> String {
        format!(
            "alien-test-token-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    async fn create_test_registry(&self, registry_name: &str) -> Result<Registry, Error> {
        let registry = Registry {
            id: None,
            name: Some(registry_name.to_string()),
            type_: None,
            location: "eastus".to_string(),
            tags: std::collections::HashMap::new(),
            system_data: None,
            sku: Sku {
                name: SkuName::Basic,
                tier: None,
            },
            identity: None,
            properties: None,
        };

        let result = self
            .container_registry_client
            .create_registry(&self.resource_group_name, registry_name, &registry)
            .await;

        if let Ok(operation_result) = result {
            // Wait for the operation to complete and get the final registry
            operation_result
                .wait_for_operation_completion(
                    &self.long_running_operation_client,
                    "CreateRegistry",
                    registry_name,
                )
                .await?;

            self.track_registry(registry_name);

            // Get the final registry state
            return self
                .container_registry_client
                .get_registry(&self.resource_group_name, registry_name)
                .await;
        }

        Err(Error::from(result.unwrap_err()))
    }

    async fn create_test_scope_map(
        &self,
        registry_name: &str,
        scope_map_name: &str,
        actions: Vec<String>,
    ) -> Result<String, Error> {
        let scope_map_properties = ScopeMapProperties {
            description: Some("Test scope map created by alien-infra tests".to_string()),
            type_: None,
            creation_date: None,
            provisioning_state: None,
            actions,
        };

        let result = self
            .container_registry_client
            .create_scope_map(
                &self.resource_group_name,
                registry_name,
                scope_map_name,
                &scope_map_properties,
            )
            .await;

        if let Ok(operation_result) = result {
            // Wait for the operation to complete
            operation_result
                .wait_for_operation_completion(
                    &self.long_running_operation_client,
                    "CreateScopeMap",
                    scope_map_name,
                )
                .await?;

            self.track_scope_map(registry_name, scope_map_name);

            // Get the final scope map state to get its ID
            let scope_map = self
                .container_registry_client
                .get_scope_map(&self.resource_group_name, registry_name, scope_map_name)
                .await?;
            return Ok(scope_map.id.unwrap_or_else(|| format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps/{}", 
                self.subscription_id, self.resource_group_name, registry_name, scope_map_name)));
        }

        Err(Error::from(result.unwrap_err()))
    }

    async fn create_test_token(
        &self,
        registry_name: &str,
        token_name: &str,
        scope_map_id: &str,
    ) -> Result<(), Error> {
        let token_properties = TokenProperties {
            creation_date: None,
            provisioning_state: None,
            scope_map_id: Some(scope_map_id.to_string()),
            credentials: None,
            status: Some(TokenPropertiesStatus::Enabled),
        };

        let result = self
            .container_registry_client
            .create_token(
                &self.resource_group_name,
                registry_name,
                token_name,
                &token_properties,
            )
            .await;

        if let Ok(operation_result) = result {
            // Wait for the operation to complete
            operation_result
                .wait_for_operation_completion(
                    &self.long_running_operation_client,
                    "CreateToken",
                    token_name,
                )
                .await?;

            self.track_token(registry_name, token_name);
            return Ok(());
        }

        Err(Error::from(result.unwrap_err()))
    }
}

// -------------------------------------------------------------------------
// Basic tests
// -------------------------------------------------------------------------

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_get_non_existent_registry(ctx: &mut ContainerRegistryTestContext) {
    let registry_name = ctx.generate_unique_registry_name();

    let result = ctx
        .container_registry_client
        .get_registry(&ctx.resource_group_name, &registry_name)
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Get non-existent registry correctly returned RemoteResourceNotFound");
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_delete_non_existent_registry(ctx: &mut ContainerRegistryTestContext) {
    let registry_name = ctx.generate_unique_registry_name();

    let result = ctx
        .container_registry_client
        .delete_registry(&ctx.resource_group_name, &registry_name)
        .await;
    // Azure may return either Ok (idempotent delete) or RemoteResourceNotFound
    match result {
        Ok(_) => {
            info!("✅ Delete non-existent registry returned OK (idempotent delete behavior)");
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Delete non-existent registry returned RemoteResourceNotFound as expected");
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound after deleting non-existent registry, got {:?}", other);
        }
    }
}

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_list_registries(ctx: &mut ContainerRegistryTestContext) {
    // Test listing registries by resource group - should not fail even if no registries exist
    let list_result = ctx
        .container_registry_client
        .list_registries(Some(ctx.resource_group_name.clone()))
        .await;
    assert!(
        list_result.is_ok(),
        "Failed to list registries: {:?}",
        list_result.err()
    );

    let registries = list_result.unwrap();
    info!(
        "Found {} registries in resource group {}",
        registries.len(),
        ctx.resource_group_name
    );
}

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_get_non_existent_scope_map(ctx: &mut ContainerRegistryTestContext) {
    let registry_name = "nonexistent-registry";
    let scope_map_name = "nonexistent-scope-map";

    let result = ctx
        .container_registry_client
        .get_scope_map(&ctx.resource_group_name, registry_name, scope_map_name)
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Get non-existent scope map correctly returned RemoteResourceNotFound");
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_get_non_existent_token(ctx: &mut ContainerRegistryTestContext) {
    let registry_name = "nonexistent-registry";
    let token_name = "nonexistent-token";

    let result = ctx
        .container_registry_client
        .get_token(&ctx.resource_group_name, registry_name, token_name)
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Get non-existent token correctly returned RemoteResourceNotFound");
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

// -------------------------------------------------------------------------
// End-to-End CRUD Tests
// -------------------------------------------------------------------------

#[test_context(ContainerRegistryTestContext)]
#[tokio::test]
async fn test_full_container_registry_crud_workflow(ctx: &mut ContainerRegistryTestContext) {
    let registry_name = ctx.generate_unique_registry_name();
    let scope_map_name = ctx.generate_unique_scope_map_name();
    let token_name = ctx.generate_unique_token_name();

    info!("🚀 Starting full CRUD test for registry: {}", registry_name);

    // Step 1: Create Registry
    info!("📝 Step 1: Creating registry...");
    let created_registry = ctx
        .create_test_registry(&registry_name)
        .await
        .expect("Failed to create registry");

    assert_eq!(created_registry.name.as_ref(), Some(&registry_name));
    assert_eq!(created_registry.location, "eastus");
    assert_eq!(created_registry.sku.name, SkuName::Basic);
    info!("✅ Registry created successfully");

    // Step 2: Get Registry
    info!("📝 Step 2: Getting registry...");
    let retrieved_registry = ctx
        .container_registry_client
        .get_registry(&ctx.resource_group_name, &registry_name)
        .await
        .expect("Failed to get registry");

    assert_eq!(retrieved_registry.name.as_ref(), Some(&registry_name));
    info!("✅ Registry retrieved successfully");

    // Step 3: List Registries (should include our registry)
    info!("📝 Step 3: Listing registries...");
    let registries = ctx
        .container_registry_client
        .list_registries(Some(ctx.resource_group_name.clone()))
        .await
        .expect("Failed to list registries");

    let found_registry = registries
        .iter()
        .find(|r| r.name.as_ref() == Some(&registry_name));
    assert!(
        found_registry.is_some(),
        "Our registry should appear in the list"
    );
    info!("✅ Registry found in list");

    // Step 4: Update Registry
    info!("📝 Step 4: Updating registry...");
    let update_params = RegistryUpdateParameters {
        identity: None,
        properties: Some(RegistryPropertiesUpdateParameters {
            admin_user_enabled: Some(true),
            network_rule_set: None,
            policies: None,
            encryption: None,
            data_endpoint_enabled: None,
            public_network_access: None,
            network_rule_bypass_options:
                RegistryPropertiesUpdateParametersNetworkRuleBypassOptions::AzureServices,
            anonymous_pull_enabled: None,
        }),
        sku: None,
        tags: std::collections::HashMap::new(),
    };

    let update_result = ctx
        .container_registry_client
        .update_registry(&ctx.resource_group_name, &registry_name, &update_params)
        .await
        .expect("Failed to update registry");

    // Wait for update to complete
    update_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "UpdateRegistry",
            &registry_name,
        )
        .await
        .expect("Failed to wait for registry update");
    info!("✅ Registry updated successfully");

    // Step 5: Create Scope Map
    info!("📝 Step 5: Creating scope map...");
    let scope_map_actions = vec![
        "repositories/test-repo/content/read".to_string(),
        "repositories/test-repo/content/write".to_string(),
    ];

    let scope_map_id = ctx
        .create_test_scope_map(&registry_name, &scope_map_name, scope_map_actions.clone())
        .await
        .expect("Failed to create scope map");
    info!("✅ Scope map created with ID: {}", scope_map_id);

    // Step 6: Get Scope Map
    info!("📝 Step 6: Getting scope map...");
    let retrieved_scope_map = ctx
        .container_registry_client
        .get_scope_map(&ctx.resource_group_name, &registry_name, &scope_map_name)
        .await
        .expect("Failed to get scope map");

    assert_eq!(retrieved_scope_map.name.as_ref(), Some(&scope_map_name));
    if let Some(properties) = &retrieved_scope_map.properties {
        assert_eq!(properties.actions, scope_map_actions);
    }
    info!("✅ Scope map retrieved successfully");

    // Step 7: List Scope Maps
    info!("📝 Step 7: Listing scope maps...");
    let scope_maps = ctx
        .container_registry_client
        .list_scope_maps(&ctx.resource_group_name, &registry_name)
        .await
        .expect("Failed to list scope maps");

    let found_scope_map = scope_maps
        .iter()
        .find(|sm| sm.name.as_ref() == Some(&scope_map_name));
    assert!(
        found_scope_map.is_some(),
        "Our scope map should appear in the list"
    );
    info!("✅ Scope map found in list");

    // Step 8: Update Scope Map
    info!("📝 Step 8: Updating scope map...");
    let updated_actions = vec![
        "repositories/test-repo/content/read".to_string(),
        "repositories/test-repo/content/write".to_string(),
        "repositories/test-repo/content/delete".to_string(),
    ];

    let scope_map_update_params = ScopeMapUpdateParameters {
        properties: Some(ScopeMapPropertiesUpdateParameters {
            description: Some("Updated test scope map".to_string()),
            actions: updated_actions,
        }),
    };

    let scope_map_update_result = ctx
        .container_registry_client
        .update_scope_map(
            &ctx.resource_group_name,
            &registry_name,
            &scope_map_name,
            &scope_map_update_params,
        )
        .await
        .expect("Failed to update scope map");

    // Wait for update to complete
    scope_map_update_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "UpdateScopeMap",
            &scope_map_name,
        )
        .await
        .expect("Failed to wait for scope map update");
    info!("✅ Scope map updated successfully");

    // Step 9: Create Token
    info!("📝 Step 9: Creating token...");
    ctx.create_test_token(&registry_name, &token_name, &scope_map_id)
        .await
        .expect("Failed to create token");
    info!("✅ Token created successfully");

    // Step 10: Get Token
    info!("📝 Step 10: Getting token...");
    let retrieved_token = ctx
        .container_registry_client
        .get_token(&ctx.resource_group_name, &registry_name, &token_name)
        .await
        .expect("Failed to get token");

    assert_eq!(retrieved_token.name.as_ref(), Some(&token_name));
    if let Some(properties) = &retrieved_token.properties {
        assert_eq!(properties.scope_map_id.as_ref(), Some(&scope_map_id));
        assert_eq!(properties.status, Some(TokenPropertiesStatus::Enabled));
    }
    info!("✅ Token retrieved successfully");

    // Step 11: List Tokens
    info!("📝 Step 11: Listing tokens...");
    let tokens = ctx
        .container_registry_client
        .list_tokens(&ctx.resource_group_name, &registry_name)
        .await
        .expect("Failed to list tokens");

    let found_token = tokens.iter().find(|t| t.name.as_ref() == Some(&token_name));
    assert!(found_token.is_some(), "Our token should appear in the list");
    info!("✅ Token found in list");

    // Step 12: Update Token
    info!("📝 Step 12: Updating token...");
    let token_update_params = TokenUpdateParameters {
        properties: Some(TokenUpdateProperties {
            credentials: None,
            scope_map_id: Some(scope_map_id.clone()),
            status: Some(TokenUpdatePropertiesStatus::Disabled),
        }),
    };

    let token_update_result = ctx
        .container_registry_client
        .update_token(
            &ctx.resource_group_name,
            &registry_name,
            &token_name,
            &token_update_params,
        )
        .await
        .expect("Failed to update token");

    // Wait for update to complete
    token_update_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "UpdateToken",
            &token_name,
        )
        .await
        .expect("Failed to wait for token update");
    info!("✅ Token updated successfully");

    // Step 13: Verify Token Update
    info!("📝 Step 13: Verifying token update...");
    let updated_token = ctx
        .container_registry_client
        .get_token(&ctx.resource_group_name, &registry_name, &token_name)
        .await
        .expect("Failed to get updated token");

    if let Some(properties) = &updated_token.properties {
        assert_eq!(properties.status, Some(TokenPropertiesStatus::Disabled));
    }
    info!("✅ Token update verified successfully");

    // Step 14: Delete Token
    info!("📝 Step 14: Deleting token...");
    ctx.container_registry_client
        .delete_token(&ctx.resource_group_name, &registry_name, &token_name)
        .await
        .expect("Failed to delete token");
    ctx.untrack_token(&registry_name, &token_name);
    info!("✅ Token deleted successfully");

    // Step 15: Delete Scope Map
    info!("📝 Step 15: Deleting scope map...");
    ctx.container_registry_client
        .delete_scope_map(&ctx.resource_group_name, &registry_name, &scope_map_name)
        .await
        .expect("Failed to delete scope map");
    ctx.untrack_scope_map(&registry_name, &scope_map_name);
    info!("✅ Scope map deleted successfully");

    // Step 16: Delete Registry
    info!("📝 Step 16: Deleting registry...");
    ctx.container_registry_client
        .delete_registry(&ctx.resource_group_name, &registry_name)
        .await
        .expect("Failed to delete registry");
    ctx.untrack_registry(&registry_name);
    info!("✅ Registry deleted successfully");

    // Step 17: Verify Cleanup
    info!("📝 Step 17: Verifying cleanup...");
    let registry_get_result = ctx
        .container_registry_client
        .get_registry(&ctx.resource_group_name, &registry_name)
        .await;
    match registry_get_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Registry correctly not found after deletion");
        }
        Ok(_) => {
            info!("⚠️ Registry still found due to Azure eventual consistency - this is acceptable");
        }
        Err(other_error) => {
            panic!(
                "Expected Ok or RemoteResourceNotFound after deleting registry, got {:?}",
                other_error
            );
        }
    }

    info!("🎉 Full CRUD workflow completed successfully!");
}
