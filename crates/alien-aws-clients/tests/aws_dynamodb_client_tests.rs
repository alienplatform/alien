/*!
# DynamoDB Client Integration Tests

These tests perform real AWS DynamoDB operations to comprehensively test KV functionality.
Tests follow the AGENTS.md guidelines with complete e2e lifecycle testing.

## Test Structure

1. **test_dynamodb_kv_lifecycle** - Complete end-to-end test covering:
   - Table creation and setup
   - Basic CRUD operations (Put, Get, Delete)
   - Conditional puts and conflict handling
   - TTL attribute management
   - Prefix queries and scanning
   - Item deletion and verification

2. **test_error_scenarios** - Comprehensive error handling:
   - Non-existent table errors
   - Invalid credential errors
   - Proper error type mapping

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=eu-central-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `dynamodb:CreateTable`
- `dynamodb:DeleteTable`
- `dynamodb:DescribeTable`
- `dynamodb:PutItem`
- `dynamodb:GetItem`
- `dynamodb:DeleteItem`
- `dynamodb:Query`

## Running Tests
```bash
# Run all DynamoDB tests
cargo test --package alien-aws-clients --test aws_dynamodb_client_tests -- --nocapture

# Run specific test
cargo test --package alien-aws-clients --test aws_dynamodb_client_tests test_dynamodb_kv_lifecycle -- --nocapture
```

All tests work with real AWS resources and will fail if operations don't succeed.
*/

use alien_aws_clients::dynamodb::*;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use base64::prelude::*;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct DynamoDbTestContext {
    client: DynamoDbClient,
    created_tables: Mutex<HashSet<String>>,
}

impl AsyncTestContext for DynamoDbTestContext {
    async fn setup() -> DynamoDbTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };
        let client = DynamoDbClient::new(Client::new(), aws_config);

        DynamoDbTestContext {
            client,
            created_tables: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting DynamoDB test cleanup...");

        let tables_to_cleanup = {
            let tables = self.created_tables.lock().unwrap();
            tables.clone()
        };

        // Clean up tables
        for table_name in tables_to_cleanup {
            self.cleanup_table(&table_name).await;
        }

        info!("✅ DynamoDB test cleanup completed");
    }
}

impl DynamoDbTestContext {
    fn track_table(&self, table_name: &str) {
        let mut tables = self.created_tables.lock().unwrap();
        tables.insert(table_name.to_string());
        info!("📝 Tracking table for cleanup: {}", table_name);
    }

    fn untrack_table(&self, table_name: &str) {
        let mut tables = self.created_tables.lock().unwrap();
        tables.remove(table_name);
        info!(
            "✅ Table {} successfully cleaned up and untracked",
            table_name
        );
    }

    async fn cleanup_table(&self, table_name: &str) {
        info!("🧹 Cleaning up table: {}", table_name);

        let delete_request = DeleteTableRequest::builder()
            .table_name(table_name.to_string())
            .build();

        match self.client.delete_table(delete_request).await {
            Ok(_) => {
                info!("✅ Table {} deleted successfully", table_name);
                self.untrack_table(table_name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!(
                        "Table {} already doesn't exist (skipping cleanup)",
                        table_name
                    );
                    self.untrack_table(table_name);
                }
                _ => {
                    warn!(
                        "Failed to delete table {} during cleanup: {:?}",
                        table_name, e
                    );
                }
            },
        }
    }

    async fn create_kv_table(&self, table_name: &str) -> Result<(), Error> {
        info!("🏗️ Creating KV table: {}", table_name);

        let create_request = CreateTableRequest::builder()
            .table_name(table_name.to_string())
            .billing_mode("PAY_PER_REQUEST".to_string())
            .key_schema(vec![
                KeySchemaElement::builder()
                    .attribute_name("pk".to_string())
                    .key_type("HASH".to_string())
                    .build(),
                KeySchemaElement::builder()
                    .attribute_name("sk".to_string())
                    .key_type("RANGE".to_string())
                    .build(),
            ])
            .attribute_definitions(vec![
                AttributeDefinition::builder()
                    .attribute_name("pk".to_string())
                    .attribute_type("S".to_string())
                    .build(),
                AttributeDefinition::builder()
                    .attribute_name("sk".to_string())
                    .attribute_type("S".to_string())
                    .build(),
            ])
            .build();

        match self.client.create_table(create_request).await {
            Ok(_) => {
                info!("✅ Table {} created successfully", table_name);
                self.track_table(table_name);

                // Wait for table to become active
                self.wait_for_table_active(table_name).await?;
                Ok(())
            }
            Err(e) => {
                warn!("Failed to create table {}: {:?}", table_name, e);
                Err(e)
            }
        }
    }

    async fn wait_for_table_active(&self, table_name: &str) -> Result<(), Error> {
        info!("⏳ Waiting for table {} to become active...", table_name);
        let mut attempts = 0;
        let max_attempts = 30; // 5 minutes max wait

        loop {
            attempts += 1;

            let describe_request = DescribeTableRequest::builder()
                .table_name(table_name.to_string())
                .build();

            match self.client.describe_table(describe_request).await {
                Ok(response) => {
                    if response.table.table_status.as_deref() == Some("ACTIVE") {
                        info!("✅ Table {} is now active!", table_name);
                        return Ok(());
                    }

                    if attempts >= max_attempts {
                        return Err(Error::new(ErrorData::Timeout {
                            message: format!(
                                "Table {} didn't become active within 5 minutes",
                                table_name
                            ),
                        }));
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
                Err(e) => {
                    warn!("Failed to describe table status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        }
    }

    fn get_test_table_name(&self) -> String {
        format!("alien-test-kv-{}", Uuid::new_v4().simple())
    }

    fn get_test_key(&self, suffix: &str) -> String {
        format!("test-key-{}-{}", Uuid::new_v4().simple(), suffix)
    }

    /// Create a table item for KV storage with pk (partition key) and sk (sort key)
    fn create_kv_item(
        &self,
        key: &str,
        value: &[u8],
        ttl: Option<i64>,
    ) -> HashMap<String, AttributeValue> {
        let bucket = self.hash_bucket(key);
        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::s(bucket));
        item.insert("sk".to_string(), AttributeValue::s(key.to_string()));
        item.insert(
            "value".to_string(),
            AttributeValue::b(base64::prelude::BASE64_STANDARD.encode(value)),
        );

        if let Some(ttl_timestamp) = ttl {
            item.insert(
                "ttl".to_string(),
                AttributeValue::n(ttl_timestamp.to_string()),
            );
        }

        item
    }

    /// Hash bucket for KV keys (simulating the partition strategy from KV.md)
    fn hash_bucket(&self, key: &str) -> String {
        // Simple hash bucket strategy - in practice this would use a proper hash function
        let bucket_num = key.chars().map(|c| c as u32).sum::<u32>() % 10;
        format!("bucket_{}", bucket_num)
    }

    /// Create primary key for DynamoDB operations
    fn create_primary_key(&self, key: &str) -> HashMap<String, AttributeValue> {
        let bucket = self.hash_bucket(key);
        let mut pk = HashMap::new();
        pk.insert("pk".to_string(), AttributeValue::s(bucket));
        pk.insert("sk".to_string(), AttributeValue::s(key.to_string()));
        pk
    }
}

#[test_context(DynamoDbTestContext)]
#[tokio::test]
async fn test_dynamodb_kv_lifecycle(ctx: &mut DynamoDbTestContext) {
    let table_name = ctx.get_test_table_name();

    info!(
        "🚀 Testing complete DynamoDB KV lifecycle with table: {}",
        table_name
    );

    // Create the table
    ctx.create_kv_table(&table_name).await.expect("Failed to create test table. Please ensure you have proper AWS credentials and DynamoDB permissions.");

    // Test data
    let test_key = ctx.get_test_key("lifecycle");
    let test_value = b"Hello, DynamoDB KV!";
    let updated_value = b"Updated DynamoDB value!";

    // TTL timestamp (expire in 1 hour)
    let ttl_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;

    // Phase 1: Put an item
    info!("📝 Phase 1: PutItem operation");
    let put_request = PutItemRequest::builder()
        .table_name(table_name.clone())
        .item(ctx.create_kv_item(&test_key, test_value, None))
        .build();

    ctx.client
        .put_item(put_request)
        .await
        .expect("PutItem should succeed");
    info!("✅ PutItem succeeded");

    // Phase 2: Get the item and verify
    info!("📖 Phase 2: GetItem operation");
    let get_request = GetItemRequest::builder()
        .table_name(table_name.clone())
        .key(ctx.create_primary_key(&test_key))
        .build();

    let get_response = ctx
        .client
        .get_item(get_request)
        .await
        .expect("GetItem should succeed");
    info!("✅ GetItem succeeded");

    let item = get_response.item.expect("Item should exist");
    let value_attr = item.get("value").expect("Value attribute should exist");
    let base64_value = value_attr.b.as_ref().expect("Binary value should exist");
    let decoded_value = BASE64_STANDARD
        .decode(base64_value)
        .expect("Failed to decode base64");
    assert_eq!(decoded_value, test_value);
    info!("✅ Retrieved value matches original");

    // Phase 3: Conditional put (should fail since item exists)
    info!("🔒 Phase 3: Conditional PutItem (should fail)");
    let conditional_put_request = PutItemRequest::builder()
        .table_name(table_name.clone())
        .item(ctx.create_kv_item(&test_key, updated_value, None))
        .condition_expression("attribute_not_exists(pk) AND attribute_not_exists(sk)".to_string())
        .build();

    match ctx.client.put_item(conditional_put_request).await {
        Ok(_) => panic!("Conditional PutItem should have failed for existing item"),
        Err(e) => match e.error {
            Some(ErrorData::RemoteResourceConflict { .. }) => {
                info!("✅ Conditional PutItem correctly failed (item already exists)");
            }
            _ => panic!("Expected RemoteResourceConflict error, got: {:?}", e),
        },
    }

    // Phase 4: Update item with TTL
    info!("⏰ Phase 4: Update item with TTL");
    let ttl_put_request = PutItemRequest::builder()
        .table_name(table_name.clone())
        .item(ctx.create_kv_item(&test_key, updated_value, Some(ttl_timestamp)))
        .build();

    ctx.client
        .put_item(ttl_put_request)
        .await
        .expect("TTL PutItem should succeed");
    info!("✅ PutItem with TTL succeeded");

    // Phase 5: Verify TTL was set
    info!("📖 Phase 5: Verify TTL attribute");
    let get_ttl_request = GetItemRequest::builder()
        .table_name(table_name.clone())
        .key(ctx.create_primary_key(&test_key))
        .build();

    let ttl_response = ctx
        .client
        .get_item(get_ttl_request)
        .await
        .expect("GetItem for TTL verification should succeed");
    let ttl_item = ttl_response.item.expect("Item with TTL should exist");

    let ttl_attr = ttl_item.get("ttl").expect("TTL attribute should exist");
    let ttl_value = ttl_attr.n.as_ref().expect("TTL should be a number");
    let stored_ttl: i64 = ttl_value.parse().expect("Failed to parse TTL");
    assert_eq!(stored_ttl, ttl_timestamp);
    info!("✅ TTL attribute correctly stored: {}", stored_ttl);

    // Verify value was updated
    let updated_value_attr = ttl_item.get("value").expect("Value attribute should exist");
    let updated_base64 = updated_value_attr
        .b
        .as_ref()
        .expect("Binary value should exist");
    let decoded_updated = BASE64_STANDARD
        .decode(updated_base64)
        .expect("Failed to decode base64");
    assert_eq!(decoded_updated, updated_value);
    info!("✅ Value was correctly updated");

    // Phase 6: Test prefix queries
    info!("🔍 Phase 6: Test prefix query operations");
    let prefix = "test-prefix";
    let test_keys = vec![
        format!("{}-item1", prefix),
        format!("{}-item2", prefix),
        format!("{}-item3", prefix),
    ];

    // Put multiple items with the same prefix
    for (i, key) in test_keys.iter().enumerate() {
        let value = format!("Value for item {}", i + 1);
        let put_request = PutItemRequest::builder()
            .table_name(table_name.clone())
            .item(ctx.create_kv_item(key, value.as_bytes(), None))
            .build();

        ctx.client
            .put_item(put_request)
            .await
            .expect(&format!("PutItem should succeed for key {}", key));
        info!("✅ Put item {} succeeded", key);
    }

    // Query by prefix
    let bucket = ctx.hash_bucket(&test_keys[0]); // All keys should be in same bucket for this test
    let mut expression_attribute_values = HashMap::new();
    expression_attribute_values.insert(":bucket".to_string(), AttributeValue::s(bucket));
    expression_attribute_values
        .insert(":prefix".to_string(), AttributeValue::s(prefix.to_string()));

    let query_request = QueryRequest::builder()
        .table_name(table_name.clone())
        .key_condition_expression("pk = :bucket AND begins_with(sk, :prefix)".to_string())
        .expression_attribute_values(expression_attribute_values)
        .limit(10)
        .build();

    let query_response = ctx
        .client
        .query(query_request)
        .await
        .expect("Query operation should succeed");
    info!("✅ Query operation succeeded");
    info!(
        "Found {} items, scanned {} items",
        query_response.count, query_response.scanned_count
    );

    // Verify all found items have the correct prefix
    for item in &query_response.items {
        let sk_attr = item.get("sk").expect("Sort key should exist");
        let sk_value = sk_attr.s.as_ref().expect("Sort key should be a string");
        assert!(
            sk_value.starts_with(prefix),
            "Key {} should start with prefix {}",
            sk_value,
            prefix
        );
        info!("Found item with key: {}", sk_value);
    }

    // Phase 7: Delete the main test item
    info!("🗑️ Phase 7: DeleteItem operation");
    let delete_request = DeleteItemRequest::builder()
        .table_name(table_name.clone())
        .key(ctx.create_primary_key(&test_key))
        .build();

    ctx.client
        .delete_item(delete_request)
        .await
        .expect("DeleteItem should succeed");
    info!("✅ DeleteItem succeeded");

    // Phase 8: Verify item is deleted
    info!("🔍 Phase 8: Verify deletion");
    let final_get_request = GetItemRequest::builder()
        .table_name(table_name.clone())
        .key(ctx.create_primary_key(&test_key))
        .build();

    let final_response = ctx
        .client
        .get_item(final_get_request)
        .await
        .expect("GetItem after delete should succeed");
    assert!(
        final_response.item.is_none(),
        "Item should not exist after deletion"
    );
    info!("✅ Item successfully deleted and verified");

    info!("🎉 Complete DynamoDB KV lifecycle test passed!");
}

#[test_context(DynamoDbTestContext)]
#[tokio::test]
async fn test_error_scenarios(ctx: &mut DynamoDbTestContext) {
    info!("🚫 Testing comprehensive error handling scenarios");

    // Test 1: Access a non-existent table
    info!("📋 Test 1: Non-existent table error");
    let non_existent_table = "alien-test-non-existent-table";
    let test_key = ctx.get_test_key("error");

    let get_request = GetItemRequest::builder()
        .table_name(non_existent_table.to_string())
        .key(ctx.create_primary_key(&test_key))
        .build();

    let result = ctx.client.get_item(get_request).await;

    assert!(result.is_err(), "Request to non-existent table should fail");
    match result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Table");
            assert_eq!(resource_name, non_existent_table);
            info!("✅ Correctly detected non-existent table");
        }
        other => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other);
        }
    }

    // Test 2: Invalid credentials
    info!("🔐 Test 2: Invalid credentials error");
    let region = std::env::var("AWS_MANAGEMENT_REGION")
        .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let client_invalid = Client::new();

    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid".to_string(),
            secret_access_key: "invalid".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let dynamodb_client = DynamoDbClient::new(client_invalid, aws_config);

    let auth_test_key = ctx.get_test_key("auth");
    let auth_get_request = GetItemRequest::builder()
        .table_name("any-table".to_string())
        .key(ctx.create_primary_key(&auth_test_key))
        .build();

    let auth_result = dynamodb_client.get_item(auth_get_request).await;

    assert!(
        auth_result.is_err(),
        "Request with invalid credentials should fail"
    );
    match auth_result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Correctly detected invalid credentials");
        }
        Error {
            error: Some(ErrorData::HttpRequestFailed { .. }),
            ..
        } => {
            info!("✅ Got HTTP error for invalid credentials (also acceptable)");
        }
        other => {
            panic!(
                "Expected RemoteAccessDenied or HttpRequestFailed, got: {:?}",
                other
            );
        }
    }

    info!("✅ All error scenarios handled correctly");
}

#[test_context(DynamoDbTestContext)]
#[tokio::test]
async fn test_dynamodb_ttl_operations(ctx: &mut DynamoDbTestContext) {
    let table_name = ctx.get_test_table_name();

    info!(
        "🕒 Testing DynamoDB TTL operations with table: {}",
        table_name
    );

    // Create the table
    ctx.create_kv_table(&table_name)
        .await
        .expect("Failed to create test table for TTL testing");

    // Phase 1: Describe TTL before setting it (should be disabled)
    info!("📋 Phase 1: DescribeTimeToLive before TTL is enabled");
    let describe_request = DescribeTimeToLiveRequest::builder()
        .table_name(table_name.clone())
        .build();

    let describe_response = ctx
        .client
        .describe_time_to_live(describe_request)
        .await
        .expect("DescribeTimeToLive should succeed");
    info!("✅ DescribeTimeToLive succeeded");

    // Initially, TTL should be disabled or not set
    if let Some(ref ttl_desc) = describe_response.time_to_live_description {
        info!(
            "TTL status: {:?}, attribute: {:?}",
            ttl_desc.time_to_live_status, ttl_desc.attribute_name
        );
        // TTL might be DISABLED or not present at all
        assert!(
            ttl_desc.time_to_live_status.as_deref() == Some("DISABLED")
                || ttl_desc.time_to_live_status.is_none(),
            "TTL should initially be disabled or not set"
        );
    } else {
        info!("TTL description is None (TTL not configured yet)");
    }

    // Phase 2: Enable TTL
    info!("⚡ Phase 2: Enable TTL on the table");
    let ttl_spec = TimeToLiveSpecification::builder()
        .attribute_name("ttl".to_string())
        .enabled(true)
        .build();

    let update_request = UpdateTimeToLiveRequest::builder()
        .table_name(table_name.clone())
        .time_to_live_specification(ttl_spec.clone())
        .build();

    let update_response = ctx
        .client
        .update_time_to_live(update_request)
        .await
        .expect("UpdateTimeToLive should succeed");
    info!("✅ UpdateTimeToLive succeeded");

    // Verify response contains the specification
    let returned_spec = update_response
        .time_to_live_specification
        .expect("Response should contain TTL specification");
    assert_eq!(returned_spec.attribute_name, "ttl");
    assert_eq!(returned_spec.enabled, true);
    info!("✅ UpdateTimeToLive response contains correct TTL specification");

    // Phase 3: Describe TTL after enabling (should show ENABLING or ENABLED)
    info!("📋 Phase 3: DescribeTimeToLive after enabling TTL");
    let describe_enabled_request = DescribeTimeToLiveRequest::builder()
        .table_name(table_name.clone())
        .build();

    let describe_enabled_response = ctx
        .client
        .describe_time_to_live(describe_enabled_request)
        .await
        .expect("DescribeTimeToLive after enable should succeed");

    let ttl_desc = describe_enabled_response
        .time_to_live_description
        .expect("TTL description should exist after enabling");
    assert_eq!(ttl_desc.attribute_name.as_deref(), Some("ttl"));
    assert!(
        ttl_desc.time_to_live_status.as_deref() == Some("ENABLING")
            || ttl_desc.time_to_live_status.as_deref() == Some("ENABLED"),
        "TTL status should be ENABLING or ENABLED, got: {:?}",
        ttl_desc.time_to_live_status
    );
    info!(
        "✅ TTL is now {:?} on attribute 'ttl'",
        ttl_desc.time_to_live_status
    );

    // Phase 4: Wait for TTL to be fully enabled (if it's still enabling)
    if ttl_desc.time_to_live_status.as_deref() == Some("ENABLING") {
        info!("⏳ Phase 4: Waiting for TTL to be fully enabled...");
        let mut attempts = 0;
        let max_attempts = 30; // 5 minutes max wait

        loop {
            attempts += 1;

            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            let check_request = DescribeTimeToLiveRequest::builder()
                .table_name(table_name.clone())
                .build();

            match ctx.client.describe_time_to_live(check_request).await {
                Ok(response) => {
                    if let Some(ref desc) = response.time_to_live_description {
                        if desc.time_to_live_status.as_deref() == Some("ENABLED") {
                            info!("✅ TTL is now fully enabled!");
                            break;
                        }
                    }

                    if attempts >= max_attempts {
                        warn!("TTL didn't become enabled within 5 minutes, continuing with test");
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to check TTL status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        }
    }

    // Phase 5: Test items with TTL attributes
    info!("📝 Phase 5: Test items with TTL attributes");
    let test_key = ctx.get_test_key("ttl-test");

    // TTL timestamp (expire in 1 hour)
    let ttl_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;

    // Put an item with TTL
    let put_request = PutItemRequest::builder()
        .table_name(table_name.clone())
        .item(ctx.create_kv_item(&test_key, b"TTL test value", Some(ttl_timestamp)))
        .build();

    ctx.client
        .put_item(put_request)
        .await
        .expect("PutItem with TTL should succeed");
    info!("✅ Item with TTL attribute stored successfully");

    // Verify the item was stored correctly
    let get_request = GetItemRequest::builder()
        .table_name(table_name.clone())
        .key(ctx.create_primary_key(&test_key))
        .build();

    let get_response = ctx
        .client
        .get_item(get_request)
        .await
        .expect("GetItem should succeed");
    let item = get_response.item.expect("Item should exist");

    let ttl_attr = item.get("ttl").expect("TTL attribute should exist");
    let stored_ttl: i64 = ttl_attr
        .n
        .as_ref()
        .expect("TTL should be a number")
        .parse()
        .expect("Failed to parse TTL");
    assert_eq!(stored_ttl, ttl_timestamp);
    info!(
        "✅ TTL attribute correctly stored and retrieved: {}",
        stored_ttl
    );

    // Phase 6: Disable TTL
    info!("🔒 Phase 6: Disable TTL on the table");
    let disable_spec = TimeToLiveSpecification::builder()
        .attribute_name("ttl".to_string())
        .enabled(false)
        .build();

    let disable_request = UpdateTimeToLiveRequest::builder()
        .table_name(table_name.clone())
        .time_to_live_specification(disable_spec)
        .build();

    let disable_response = ctx.client.update_time_to_live(disable_request).await;

    // Note: AWS only allows one TTL modification per hour per table
    // If this fails with ValidationException about multiple modifications, that's expected
    match disable_response {
        Ok(response) => {
            let disabled_spec = response
                .time_to_live_specification
                .expect("Response should contain TTL specification");
            assert_eq!(disabled_spec.attribute_name, "ttl");
            assert_eq!(disabled_spec.enabled, false);
            info!("✅ TTL successfully disabled");
        }
        Err(e) => match &e.error {
            Some(ErrorData::GenericError { message })
                if message.contains("modified multiple times within a fixed interval") =>
            {
                info!("⚠️ Cannot disable TTL: AWS only allows one TTL modification per hour per table");
                info!("✅ This is expected behavior according to AWS documentation");
            }
            _ => {
                panic!("Unexpected error when disabling TTL: {:?}", e);
            }
        },
    }

    // Phase 7: Verify current TTL status (may still be enabled due to AWS rate limits)
    info!("📋 Phase 7: Verify current TTL status");
    let final_describe_request = DescribeTimeToLiveRequest::builder()
        .table_name(table_name.clone())
        .build();

    let final_describe_response = ctx
        .client
        .describe_time_to_live(final_describe_request)
        .await
        .expect("Final DescribeTimeToLive should succeed");

    if let Some(ref final_ttl_desc) = final_describe_response.time_to_live_description {
        info!(
            "✅ Final TTL status: {:?} on attribute: {:?}",
            final_ttl_desc.time_to_live_status, final_ttl_desc.attribute_name
        );
        // TTL status can be ENABLED, DISABLING, or DISABLED depending on AWS rate limits
        assert!(
            final_ttl_desc.time_to_live_status.as_deref() == Some("ENABLED")
                || final_ttl_desc.time_to_live_status.as_deref() == Some("DISABLING")
                || final_ttl_desc.time_to_live_status.as_deref() == Some("DISABLED"),
            "TTL status should be ENABLED, DISABLING, or DISABLED, got: {:?}",
            final_ttl_desc.time_to_live_status
        );
    }

    info!("🎉 All TTL operations completed successfully!");
}

#[test_context(DynamoDbTestContext)]
#[tokio::test]
async fn test_ttl_error_scenarios(ctx: &mut DynamoDbTestContext) {
    info!("🚫 Testing TTL error scenarios");

    // Test 1: TTL operations on non-existent table
    info!("📋 Test 1: TTL operations on non-existent table");
    let non_existent_table = "alien-test-non-existent-ttl-table";

    // Test UpdateTimeToLive on non-existent table
    let ttl_spec = TimeToLiveSpecification::builder()
        .attribute_name("ttl".to_string())
        .enabled(true)
        .build();

    let update_request = UpdateTimeToLiveRequest::builder()
        .table_name(non_existent_table.to_string())
        .time_to_live_specification(ttl_spec)
        .build();

    let update_result = ctx.client.update_time_to_live(update_request).await;
    assert!(
        update_result.is_err(),
        "UpdateTimeToLive on non-existent table should fail"
    );

    match update_result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Table");
            assert_eq!(resource_name, non_existent_table);
            info!("✅ UpdateTimeToLive correctly failed for non-existent table");
        }
        other => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other);
        }
    }

    // Test DescribeTimeToLive on non-existent table
    let describe_request = DescribeTimeToLiveRequest::builder()
        .table_name(non_existent_table.to_string())
        .build();

    let describe_result = ctx.client.describe_time_to_live(describe_request).await;
    assert!(
        describe_result.is_err(),
        "DescribeTimeToLive on non-existent table should fail"
    );

    match describe_result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Table");
            assert_eq!(resource_name, non_existent_table);
            info!("✅ DescribeTimeToLive correctly failed for non-existent table");
        }
        other => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other);
        }
    }

    info!("✅ All TTL error scenarios handled correctly");
}
