#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::long_running_operation::{
    LongRunningOperationApi, LongRunningOperationClient,
};
use alien_azure_clients::models::resources::{ResourceGroup, ResourceGroupPatchable};
use alien_azure_clients::resources::{AzureResourcesClient, ResourcesApi};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct ResourceGroupTestContext {
    resources_client: AzureResourcesClient,
    long_running_operation_client: LongRunningOperationClient,
    subscription_id: String,
    created_resource_groups: Mutex<HashSet<String>>,
}

impl AsyncTestContext for ResourceGroupTestContext {
    async fn setup() -> ResourceGroupTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");

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

        let client = Client::new();
        ResourceGroupTestContext {
            resources_client: AzureResourcesClient::new(client.clone(), client_config.clone()),
            long_running_operation_client: LongRunningOperationClient::new(client, client_config),
            subscription_id,
            created_resource_groups: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Resource Group test cleanup...");
        let groups_to_cleanup = {
            let groups = self.created_resource_groups.lock().unwrap();
            groups.clone()
        };
        for rg_name in groups_to_cleanup {
            match self.resources_client.delete_resource_group(&rg_name).await {
                Ok(op_result) => {
                    let _ = op_result
                        .wait_for_operation_completion(
                            &self.long_running_operation_client,
                            "DeleteResourceGroup",
                            &rg_name,
                        )
                        .await;
                    info!("✅ Resource group '{}' deleted during cleanup", rg_name);
                }
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!("🔍 Resource group '{}' was already deleted", rg_name);
                }
                Err(e) => {
                    warn!(
                        "Failed to delete resource group '{}' during cleanup: {:?}",
                        rg_name, e
                    );
                }
            }
        }
        info!("✅ Resource Group test cleanup completed");
    }
}

impl ResourceGroupTestContext {
    fn track_resource_group(&self, name: &str) {
        let mut groups = self.created_resource_groups.lock().unwrap();
        groups.insert(name.to_string());
        info!("📝 Tracking resource group for cleanup: {}", name);
    }
    fn untrack_resource_group(&self, name: &str) {
        let mut groups = self.created_resource_groups.lock().unwrap();
        groups.remove(name);
        info!(
            "✅ Resource group '{}' successfully cleaned up and untracked",
            name
        );
    }
    fn generate_unique_resource_group_name(&self) -> String {
        format!(
            "alien-test-rg-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }
}

#[test_context(ResourceGroupTestContext)]
#[tokio::test]
async fn test_resource_group_e2e(ctx: &mut ResourceGroupTestContext) {
    let rg_name = ctx.generate_unique_resource_group_name();
    let location = "eastus".to_string();
    let mut tags = std::collections::HashMap::new();
    tags.insert("env".to_string(), "test".to_string());

    // Create resource group
    let rg = ResourceGroup {
        id: None,
        location: location.clone(),
        managed_by: None,
        name: Some(rg_name.clone()),
        properties: None,
        tags: tags.clone(),
        type_: None,
    };
    let created = ctx
        .resources_client
        .create_or_update_resource_group(&rg_name, &rg)
        .await;
    assert!(
        created.is_ok(),
        "Failed to create resource group: {:?}",
        created.err()
    );
    ctx.track_resource_group(&rg_name);
    let created_rg = created.unwrap();
    assert_eq!(created_rg.location, location);
    assert_eq!(created_rg.name.as_deref(), Some(rg_name.as_str()));
    assert_eq!(created_rg.tags.get("env"), Some(&"test".to_string()));

    // Update resource group tags
    let mut new_tags = tags.clone();
    new_tags.insert("updated".to_string(), "true".to_string());
    let patch = ResourceGroupPatchable {
        managed_by: None,
        name: Some(rg_name.clone()),
        properties: None,
        tags: new_tags.clone(),
    };
    let updated = ctx
        .resources_client
        .update_resource_group(&rg_name, &patch)
        .await;
    assert!(
        updated.is_ok(),
        "Failed to update resource group: {:?}",
        updated.err()
    );
    let updated_rg = updated.unwrap();
    assert_eq!(updated_rg.tags.get("updated"), Some(&"true".to_string()));

    // Delete resource group (long running)
    let delete_result = ctx.resources_client.delete_resource_group(&rg_name).await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete resource group: {:?}",
        delete_result.err()
    );
    let op_result = delete_result.unwrap();
    let wait_result = op_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteResourceGroup",
            &rg_name,
        )
        .await;
    assert!(
        wait_result.is_ok(),
        "Failed to wait for resource group deletion: {:?}",
        wait_result.err()
    );
    ctx.untrack_resource_group(&rg_name);

    // Check custom error: resource not found
    let get_result = ctx.resources_client.get_resource_group(&rg_name).await;
    match get_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}
