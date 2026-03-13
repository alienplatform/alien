#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::models::table::{Table, TableAccessPolicy, TableSignedIdentifier};
use alien_azure_clients::storage_accounts::{AzureStorageAccountsClient, StorageAccountsApi};
use alien_azure_clients::tables::{
    AzureTableManagementClient, AzureTableStorageClient, EntityQueryOptions, TableEntity,
    TableManagementApi, TableStorageApi,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::ErrorData;
use anyhow::{bail, Result};
use reqwest::Client;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct TrackedTable {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedEntity {
    partition_key: String,
    row_key: String,
    table_name: String,
}

struct TableStorageTestContext {
    management_client: AzureTableManagementClient,
    storage_client: AzureTableStorageClient,
    resource_group_name: String,
    storage_account_name: String,
    created_tables: Mutex<Vec<TrackedTable>>,
    created_entities: Mutex<Vec<TrackedEntity>>,
}

impl AsyncTestContext for TableStorageTestContext {
    async fn setup() -> TableStorageTestContext {
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
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");
        let storage_account_name = env::var("ALIEN_TEST_AZURE_STORAGE_ACCOUNT")
            .expect("ALIEN_TEST_AZURE_STORAGE_ACCOUNT not set");

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

        let management_client =
            AzureTableManagementClient::new(Client::new(), client_config.clone());

        // Fetch storage account key for the data plane client
        let storage_accounts_client =
            AzureStorageAccountsClient::new(Client::new(), client_config.clone());
        let keys_result = storage_accounts_client
            .list_storage_account_keys(&resource_group_name, &storage_account_name)
            .await
            .expect("Failed to fetch storage account keys for test");

        let storage_account_key = keys_result
            .keys
            .into_iter()
            .find(|key| key.key_name.as_deref() == Some("key1"))
            .and_then(|key| key.value)
            .expect("No access key found for storage account in test");

        let storage_client =
            AzureTableStorageClient::new(Client::new(), client_config, storage_account_key);

        TableStorageTestContext {
            management_client,
            storage_client,
            resource_group_name,
            storage_account_name,
            created_tables: Mutex::new(Vec::new()),
            created_entities: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("Cleaning up table storage test resources...");

        // Clean up entities first
        let entities = self.created_entities.into_inner().unwrap_or_default();
        for entity in entities {
            match self
                .storage_client
                .delete_entity(
                    &self.resource_group_name,
                    &self.storage_account_name,
                    &entity.table_name,
                    &entity.partition_key,
                    &entity.row_key,
                    None,
                )
                .await
            {
                Ok(()) => info!(
                    "Deleted entity {}:{} from table {}",
                    entity.partition_key, entity.row_key, entity.table_name
                ),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!(
                        "Entity {}:{} from table {} already deleted",
                        entity.partition_key, entity.row_key, entity.table_name
                    );
                }
                Err(e) => warn!(
                    "Failed to delete entity {}:{} from table {}: {}",
                    entity.partition_key, entity.row_key, entity.table_name, e
                ),
            }
        }

        // Clean up tables
        let tables = self.created_tables.into_inner().unwrap_or_default();
        for table in tables {
            match self
                .management_client
                .delete_table(
                    &self.resource_group_name,
                    &self.storage_account_name,
                    &table.name,
                )
                .await
            {
                Ok(()) => info!("Deleted table {}", table.name),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!("Table {} already deleted", table.name);
                }
                Err(e) => warn!("Failed to delete table {}: {}", table.name, e),
            }
        }

        info!("Table storage test cleanup completed");
    }
}

impl TableStorageTestContext {
    async fn create_test_table(&self, table_name: &str) -> Result<Table> {
        let table = self
            .management_client
            .create_table(
                &self.resource_group_name,
                &self.storage_account_name,
                table_name,
            )
            .await?;

        self.created_tables.lock().unwrap().push(TrackedTable {
            name: table_name.to_string(),
        });

        Ok(table)
    }

    async fn create_test_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
    ) -> Result<TableEntity> {
        let mut properties = HashMap::new();
        properties.insert("Name".to_string(), json!("Test Entity"));
        properties.insert("Value".to_string(), json!(42));
        properties.insert("Active".to_string(), json!(true));

        let entity = TableEntity {
            partition_key: partition_key.to_string(),
            row_key: row_key.to_string(),
            timestamp: None,
            properties,
        };

        let created_entity = self
            .storage_client
            .insert_entity(
                &self.resource_group_name,
                &self.storage_account_name,
                table_name,
                &entity,
            )
            .await?;

        self.created_entities.lock().unwrap().push(TrackedEntity {
            partition_key: partition_key.to_string(),
            row_key: row_key.to_string(),
            table_name: table_name.to_string(),
        });

        Ok(created_entity)
    }

    fn generate_unique_name(&self, prefix: &str) -> String {
        format!(
            "{}{}",
            prefix,
            Uuid::new_v4().simple().to_string()[..8].to_lowercase()
        )
    }
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_table_lifecycle(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");

    info!("Creating table: {}", table_name);
    let table = ctx.create_test_table(&table_name).await?;

    assert!(table.name.is_some());
    assert_eq!(table.name.as_ref().unwrap(), &table_name);

    info!("Deleting table: {}", table_name);
    ctx.management_client
        .delete_table(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
        )
        .await?;

    // Remove from tracked tables since we manually deleted it
    ctx.created_tables
        .lock()
        .unwrap()
        .retain(|t| t.name != table_name);

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_insert_and_get(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "TestPartition";
    let row_key = ctx.generate_unique_name("testrow");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    info!("Creating entity: {}:{}", partition_key, row_key);
    let entity = ctx
        .create_test_entity(&table_name, partition_key, &row_key)
        .await?;

    assert_eq!(entity.partition_key, partition_key);
    assert_eq!(entity.row_key, row_key);
    assert!(entity.properties.contains_key("Name"));

    info!("Getting entity: {}:{}", partition_key, row_key);
    let retrieved_entity = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;

    assert_eq!(retrieved_entity.partition_key, partition_key);
    assert_eq!(retrieved_entity.row_key, row_key);
    assert_eq!(
        retrieved_entity.properties.get("Name"),
        entity.properties.get("Name")
    );
    assert_eq!(
        retrieved_entity.properties.get("Value"),
        entity.properties.get("Value")
    );

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_update(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "TestPartition";
    let row_key = ctx.generate_unique_name("testrow");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    info!("Creating entity: {}:{}", partition_key, row_key);
    ctx.create_test_entity(&table_name, partition_key, &row_key)
        .await?;

    // Update the entity
    let mut updated_properties = HashMap::new();
    updated_properties.insert("Name".to_string(), json!("Updated Entity"));
    updated_properties.insert("Value".to_string(), json!(100));
    updated_properties.insert("NewProperty".to_string(), json!("New Value"));

    let updated_entity = TableEntity {
        partition_key: partition_key.to_string(),
        row_key: row_key.to_string(),
        timestamp: None,
        properties: updated_properties,
    };

    info!("Updating entity: {}:{}", partition_key, row_key);
    let result = ctx
        .storage_client
        .update_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            &updated_entity,
            None, // Use wildcard ETag
        )
        .await?;

    assert_eq!(result.partition_key, partition_key);
    assert_eq!(result.row_key, row_key);

    // Verify the update by retrieving the entity
    let retrieved_entity = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;

    assert_eq!(
        retrieved_entity.properties.get("Name"),
        Some(&json!("Updated Entity"))
    );
    assert_eq!(retrieved_entity.properties.get("Value"), Some(&json!(100)));
    assert_eq!(
        retrieved_entity.properties.get("NewProperty"),
        Some(&json!("New Value"))
    );

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_merge(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "TestPartition";
    let row_key = ctx.generate_unique_name("testrow");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    info!("Creating entity: {}:{}", partition_key, row_key);
    ctx.create_test_entity(&table_name, partition_key, &row_key)
        .await?;

    // Merge partial updates
    let mut merge_properties = HashMap::new();
    merge_properties.insert("Value".to_string(), json!(200));
    merge_properties.insert("MergedProperty".to_string(), json!("Merged Value"));

    let merge_entity = TableEntity {
        partition_key: partition_key.to_string(),
        row_key: row_key.to_string(),
        timestamp: None,
        properties: merge_properties,
    };

    info!("Merging entity: {}:{}", partition_key, row_key);
    ctx.storage_client
        .merge_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            &merge_entity,
            None,
        )
        .await?;

    // Verify the merge by retrieving the entity
    let retrieved_entity = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;

    // Original properties should still exist
    assert_eq!(
        retrieved_entity.properties.get("Name"),
        Some(&json!("Test Entity"))
    );
    assert_eq!(
        retrieved_entity.properties.get("Active"),
        Some(&json!(true))
    );

    // Merged properties should be updated/added
    assert_eq!(retrieved_entity.properties.get("Value"), Some(&json!(200)));
    assert_eq!(
        retrieved_entity.properties.get("MergedProperty"),
        Some(&json!("Merged Value"))
    );

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_insert_or_replace(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "TestPartition";
    let row_key = ctx.generate_unique_name("testrow");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    // First, insert an entity using insert_or_replace (should work like insert)
    let mut properties = HashMap::new();
    properties.insert("Name".to_string(), json!("First Entity"));
    properties.insert("Value".to_string(), json!(1));

    let entity = TableEntity {
        partition_key: partition_key.to_string(),
        row_key: row_key.to_string(),
        timestamp: None,
        properties,
    };

    info!(
        "Insert or replace entity (insert): {}:{}",
        partition_key, row_key
    );
    ctx.storage_client
        .insert_or_replace_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            &entity,
        )
        .await?;

    ctx.created_entities.lock().unwrap().push(TrackedEntity {
        partition_key: partition_key.to_string(),
        row_key: row_key.to_string(),
        table_name: table_name.to_string(),
    });

    // Now replace the entity completely
    let mut new_properties = HashMap::new();
    new_properties.insert("Name".to_string(), json!("Replaced Entity"));
    new_properties.insert("NewValue".to_string(), json!(999));

    let replace_entity = TableEntity {
        partition_key: partition_key.to_string(),
        row_key: row_key.to_string(),
        timestamp: None,
        properties: new_properties,
    };

    info!(
        "Insert or replace entity (replace): {}:{}",
        partition_key, row_key
    );
    ctx.storage_client
        .insert_or_replace_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            &replace_entity,
        )
        .await?;

    // Verify the replacement
    let retrieved_entity = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;

    assert_eq!(
        retrieved_entity.properties.get("Name"),
        Some(&json!("Replaced Entity"))
    );
    assert_eq!(
        retrieved_entity.properties.get("NewValue"),
        Some(&json!(999))
    );
    // Old Value property should be gone
    assert!(!retrieved_entity.properties.contains_key("Value"));

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_query(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "QueryPartition";

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    // Create multiple entities for querying
    for i in 1..=5 {
        let row_key = format!("row{}", i);
        let mut properties = HashMap::new();
        properties.insert("Name".to_string(), json!(format!("Entity {}", i)));
        properties.insert("Value".to_string(), json!(i * 10));
        properties.insert("Active".to_string(), json!(i % 2 == 0));

        let entity = TableEntity {
            partition_key: partition_key.to_string(),
            row_key: row_key.clone(),
            timestamp: None,
            properties,
        };

        ctx.storage_client
            .insert_entity(
                &ctx.resource_group_name,
                &ctx.storage_account_name,
                &table_name,
                &entity,
            )
            .await?;

        ctx.created_entities.lock().unwrap().push(TrackedEntity {
            partition_key: partition_key.to_string(),
            row_key,
            table_name: table_name.to_string(),
        });
    }

    // Query all entities in the partition
    info!("Querying all entities in partition: {}", partition_key);
    let query_options = EntityQueryOptions {
        filter: Some(format!("PartitionKey eq '{}'", partition_key)),
        select: None,
        top: None,
    };

    let query_result = ctx
        .storage_client
        .query_entities(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            Some(query_options),
        )
        .await?;

    assert_eq!(query_result.entities.len(), 5);

    // Query with filter
    info!("Querying entities with Value > 20");
    let filter_options = EntityQueryOptions {
        filter: Some(format!(
            "PartitionKey eq '{}' and Value gt 20",
            partition_key
        )),
        select: None,
        top: None,
    };

    let filtered_result = ctx
        .storage_client
        .query_entities(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            Some(filter_options),
        )
        .await?;

    assert_eq!(filtered_result.entities.len(), 3); // rows 3, 4, 5

    // Query with select
    info!("Querying entities with select");
    let select_options = EntityQueryOptions {
        filter: Some(format!("PartitionKey eq '{}'", partition_key)),
        select: Some("PartitionKey,RowKey,Name".to_string()),
        top: Some(2),
    };

    let select_result = ctx
        .storage_client
        .query_entities(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            Some(select_options),
        )
        .await?;

    assert_eq!(select_result.entities.len(), 2);
    for entity in &select_result.entities {
        assert!(entity.properties.contains_key("Name"));
        assert!(!entity.properties.contains_key("Value")); // Should not be included due to $select
    }

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_entity_delete(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");
    let partition_key = "DeletePartition";
    let row_key = ctx.generate_unique_name("deleterow");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    info!("Creating entity: {}:{}", partition_key, row_key);
    ctx.create_test_entity(&table_name, partition_key, &row_key)
        .await?;

    // Verify entity exists
    let entity = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;
    assert_eq!(entity.partition_key, partition_key);
    assert_eq!(entity.row_key, row_key);

    info!("Deleting entity: {}:{}", partition_key, row_key);
    ctx.storage_client
        .delete_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await?;

    // Remove from tracked entities since we manually deleted it
    ctx.created_entities
        .lock()
        .unwrap()
        .retain(|e| !(e.partition_key == partition_key && e.row_key == row_key));

    // Verify entity is deleted
    let result = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            partition_key,
            &row_key,
            None,
        )
        .await;

    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("Entity successfully deleted");
        }
        _ => bail!("Expected entity to be deleted"),
    }

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_table_acl_operations(ctx: &TableStorageTestContext) -> Result<()> {
    let table_name = ctx.generate_unique_name("alientest");

    info!("Creating table: {}", table_name);
    ctx.create_test_table(&table_name).await?;

    // Get initial ACL (should be empty)
    info!("Getting initial table ACL");
    let initial_acl = ctx
        .management_client
        .get_table_acl(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
        )
        .await?;

    assert!(initial_acl.is_empty());

    // Set ACL with signed identifiers
    let access_policy = TableAccessPolicy {
        permission: "raud".to_string(), // read, add, update, delete
        start_time: Some("2024-01-01T00:00:00Z".to_string()),
        expiry_time: Some("2024-12-31T23:59:59Z".to_string()),
    };

    let signed_identifier = TableSignedIdentifier {
        id: "test-policy-1".to_string(),
        access_policy: Some(access_policy),
    };

    info!("Setting table ACL");
    ctx.management_client
        .set_table_acl(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
            &[signed_identifier.clone()],
        )
        .await?;

    // Get ACL and verify
    info!("Getting updated table ACL");
    let updated_acl = ctx
        .management_client
        .get_table_acl(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            &table_name,
        )
        .await?;

    assert_eq!(updated_acl.len(), 1);
    assert_eq!(updated_acl[0].id, "test-policy-1");
    assert!(updated_acl[0].access_policy.is_some());

    let policy = updated_acl[0].access_policy.as_ref().unwrap();
    assert_eq!(policy.permission, "raud");

    Ok(())
}

#[test_context(TableStorageTestContext)]
#[tokio::test]
async fn test_error_handling(ctx: &TableStorageTestContext) -> Result<()> {
    let nonexistent_table = "nonexistenttable";
    let nonexistent_partition = "NonexistentPartition";
    let nonexistent_row = "nonexistentrow";

    // Test getting non-existent entity
    info!("Testing get non-existent entity");
    let result = ctx
        .storage_client
        .get_entity(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            nonexistent_table,
            nonexistent_partition,
            nonexistent_row,
            None,
        )
        .await;

    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("Correctly received NotFound error for non-existent entity");
        }
        _ => bail!("Expected RemoteResourceNotFound error for non-existent entity"),
    }

    // Test deleting non-existent table
    info!("Testing delete non-existent table");
    let result = ctx
        .management_client
        .delete_table(
            &ctx.resource_group_name,
            &ctx.storage_account_name,
            nonexistent_table,
        )
        .await;

    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("Correctly received NotFound error for non-existent table");
        }
        _ => bail!("Expected RemoteResourceNotFound error for non-existent table"),
    }

    Ok(())
}
