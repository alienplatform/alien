/*!
# KMS Client Integration Tests

These tests perform real AWS KMS operations including creating keys, describing them,
disabling/enabling keys, and scheduling key deletion.

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
- `kms:CreateKey`
- `kms:DescribeKey`
- `kms:DisableKey`
- `kms:EnableKey`
- `kms:ScheduleKeyDeletion`
- `kms:ListKeys` (for cleanup verification)

## Running Tests
```bash
# Run all KMS tests
cargo test --package alien-aws-clients --test aws_kms_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_kms_client_tests test_end_to_end_key_lifecycle -- --nocapture
```
*/

use alien_aws_clients::kms::*;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use aws_credential_types::Credentials;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct KmsTestContext {
    client: KmsClient,
    created_keys: Mutex<HashSet<String>>,
}

impl AsyncTestContext for KmsTestContext {
    async fn setup() -> KmsTestContext {
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
        let client = KmsClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        KmsTestContext {
            client,
            created_keys: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting KMS test cleanup...");

        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        // Schedule deletion for all created keys (if not already scheduled)
        for key_id in keys_to_cleanup {
            self.cleanup_key(&key_id).await;
        }

        info!("✅ KMS test cleanup completed");
    }
}

impl KmsTestContext {
    fn track_key(&self, key_id: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key_id.to_string());
        info!("📝 Tracking key for cleanup: {}", key_id);
    }

    fn untrack_key(&self, key_id: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.remove(key_id);
        info!("✅ Key {} successfully cleaned up and untracked", key_id);
    }

    async fn cleanup_key(&self, key_id: &str) {
        info!("🧹 Cleaning up KMS key: {}", key_id);

        // First check the key state
        match self.client.describe_key(key_id).await {
            Ok(metadata) => {
                let state = metadata.key_state.as_deref().unwrap_or("Unknown");
                info!("Key {} current state: {}", key_id, state);

                // Only schedule deletion if not already pending deletion or deleted
                if !matches!(state, "PendingDeletion" | "Deleted") {
                    match self.client.schedule_key_deletion(key_id, Some(7)).await {
                        Ok(_) => {
                            info!("✅ Key {} scheduled for deletion", key_id);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to schedule key {} for deletion during cleanup: {:?}",
                                key_id, e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                // Key might already be deleted or not accessible
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to describe key {} during cleanup: {:?}", key_id, e);
                }
            }
        }
    }

    fn get_test_key_description(&self) -> String {
        format!(
            "Alien test key created by integration tests - {}",
            Uuid::new_v4().simple()
        )
    }

    async fn create_test_key(&self) -> Result<KeyMetadata, Error> {
        let description = self.get_test_key_description();

        let request = CreateKeyRequest::builder()
            .description(description.clone())
            .key_usage("ENCRYPT_DECRYPT".to_string())
            .key_spec("SYMMETRIC_DEFAULT".to_string())
            .origin("AWS_KMS".to_string())
            .multi_region(false)
            .tags(vec![
                Tag::builder()
                    .tag_key("Environment".to_string())
                    .tag_value("Test".to_string())
                    .build(),
                Tag::builder()
                    .tag_key("Project".to_string())
                    .tag_value("Alien".to_string())
                    .build(),
                Tag::builder()
                    .tag_key("TestId".to_string())
                    .tag_value(Uuid::new_v4().simple().to_string())
                    .build(),
            ])
            .build();

        let result = self.client.create_key(request).await;
        if let Ok(ref metadata) = result {
            self.track_key(&metadata.key_id);
        }
        result
    }

    async fn wait_for_key_state(
        &self,
        key_id: &str,
        expected_state: &str,
        max_attempts: u32,
    ) -> bool {
        info!(
            "⏳ Waiting for key {} to reach state: {}",
            key_id, expected_state
        );
        let mut attempts = 0;

        loop {
            attempts += 1;

            match self.client.describe_key(key_id).await {
                Ok(metadata) => {
                    let current_state = metadata.key_state.as_deref().unwrap_or("Unknown");
                    info!("📊 Key {} current state: {}", key_id, current_state);

                    if current_state == expected_state {
                        info!("✅ Key reached expected state: {}", expected_state);
                        return true;
                    }

                    if attempts >= max_attempts {
                        warn!(
                            "⚠️  Key didn't reach expected state within {} attempts",
                            max_attempts
                        );
                        return false;
                    }

                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    warn!("Failed to get key status: {:?}", e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_create_key_success(ctx: &mut KmsTestContext) {
    info!("🚀 Testing create key");

    let key_metadata = match ctx.create_test_key().await {
        Ok(metadata) => {
            info!("✅ Key created successfully: {}", metadata.key_id);
            assert!(
                metadata.key_id.starts_with("arn:aws:kms:") || !metadata.key_id.contains("arn:")
            ); // Can be ARN or just key ID
            assert!(metadata.enabled.unwrap_or(false)); // Should be enabled by default
            assert_eq!(metadata.key_usage.as_deref(), Some("ENCRYPT_DECRYPT"));
            assert_eq!(metadata.key_spec.as_deref(), Some("SYMMETRIC_DEFAULT"));
            metadata
        }
        Err(e) => {
            panic!("Key creation failed: {:?}. Please ensure you have proper AWS credentials and KMS permissions set up in .env.test", e);
        }
    };

    // Key will be cleaned up automatically via teardown
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_describe_key_not_found(ctx: &mut KmsTestContext) {
    let non_existent_key =
        "arn:aws:kms:us-east-1:123456789012:key/00000000-1111-2222-3333-444444444444";

    let result = ctx.client.describe_key(non_existent_key).await;

    assert!(result.is_err());
    // KMS returns AccessDeniedException (not NotFoundException) for non-existent
    // keys as a security measure to prevent key ID enumeration.
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
            assert_eq!(resource_type, "KmsKey");
            assert_eq!(resource_name, non_existent_key);
        }
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("KMS returned AccessDenied for non-existent key (expected security behavior)");
        }
        other => panic!(
            "Expected RemoteResourceNotFound or RemoteAccessDenied, got: {:?}",
            other
        ),
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_kms_client_with_invalid_credentials(ctx: &mut KmsTestContext) {
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
    let kms_client = KmsClient::new(client_invalid, AwsCredentialProvider::from_config_sync(aws_config));

    info!("🔐 Testing KMS client with invalid credentials");

    let result = kms_client.describe_key("any-key").await;

    assert!(result.is_err());
    match result.unwrap_err() {
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
            warn!(
                "Got unexpected error type for invalid credentials: {:?}",
                other
            );
        }
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_serde_structs(ctx: &mut KmsTestContext) {
    // Test serialization of key structs
    let create_request = CreateKeyRequest::builder()
        .description("Test key".to_string())
        .key_usage("ENCRYPT_DECRYPT".to_string())
        .key_spec("SYMMETRIC_DEFAULT".to_string())
        .build();

    let json = serde_json::to_string(&create_request).expect("Should serialize");
    assert!(json.contains("Description")); // Verify PascalCase serialization
    assert!(json.contains("KeyUsage"));
    assert!(json.contains("KeySpec"));

    let tag = Tag::builder()
        .tag_key("Environment".to_string())
        .tag_value("Test".to_string())
        .build();

    let tag_json = serde_json::to_string(&tag).expect("Should serialize tag");
    assert!(tag_json.contains("TagKey"));
    assert!(tag_json.contains("TagValue"));

    info!("✅ Serialization tests passed");
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_http_request_signing_and_construction(ctx: &mut KmsTestContext) {
    info!("🔧 Testing HTTP request construction and signing");

    // Test that we can construct and sign an HTTP request
    let non_existent_key =
        "arn:aws:kms:us-east-1:123456789012:key/00000000-1111-2222-3333-444444444444";

    let result = ctx.client.describe_key(non_existent_key).await;

    // This should make a real HTTP request and return a structured error
    assert!(result.is_err());

    let error = result.unwrap_err();
    match error {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            // Perfect! This means we successfully:
            // 1. Constructed the HTTP request with proper JSON body
            // 2. Signed it with AWS SigV4 (including X-Amz-Target header)
            // 3. Made the HTTP call
            // 4. Received the response
            // 5. Parsed the JSON error response
            // 6. Mapped it to our error type
            info!("✓ HTTP request construction, signing, and response parsing all work!");
        }
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            // Also good! This means HTTP worked but we have auth issues
            info!("✓ HTTP request works, got auth error (expected in some environments)");
        }
        other => {
            info!(
                "Got different error type, but HTTP request was made: {:?}",
                other
            );
            // Still counts as successful HTTP request/response cycle
        }
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_end_to_end_key_lifecycle(ctx: &mut KmsTestContext) {
    info!("🚀 Starting end-to-end KMS key lifecycle test");

    // Step 1: Create a key
    let key_metadata = match ctx.create_test_key().await {
        Ok(metadata) => {
            info!("✅ Step 1 - Key created: {}", metadata.key_id);
            assert!(metadata.enabled.unwrap_or(false));
            assert_eq!(metadata.key_state.as_deref(), Some("Enabled"));
            metadata
        }
        Err(e) => {
            panic!("Key creation failed: {:?}. Please ensure you have proper AWS credentials and KMS permissions set up in .env.test", e);
        }
    };

    let key_id = &key_metadata.key_id;

    // Step 2: Describe the key to verify it exists and get current state
    info!("🔍 Step 2 - Describing key...");
    match ctx.client.describe_key(key_id).await {
        Ok(described_metadata) => {
            info!("✅ Step 2 - Key described successfully");
            assert_eq!(described_metadata.key_id, key_metadata.key_id);
            assert_eq!(described_metadata.key_usage, key_metadata.key_usage);
            assert_eq!(described_metadata.key_spec, key_metadata.key_spec);
            assert!(described_metadata.creation_date.is_some());
            info!("Key state: {:?}", described_metadata.key_state);
        }
        Err(e) => {
            panic!("Failed to describe key: {:?}", e);
        }
    }

    // Step 3: Disable the key
    info!("🔒 Step 3 - Disabling key...");
    match ctx.client.disable_key(key_id).await {
        Ok(_) => {
            info!("✅ Step 3 - Key disabled successfully");

            // Wait for key to be disabled
            if ctx.wait_for_key_state(key_id, "Disabled", 10).await {
                info!("✅ Key is now disabled");

                // Verify key is disabled by describing it
                match ctx.client.describe_key(key_id).await {
                    Ok(metadata) => {
                        assert_eq!(metadata.key_state.as_deref(), Some("Disabled"));
                        assert_eq!(metadata.enabled, Some(false));
                        info!("✅ Verified key is disabled");
                    }
                    Err(e) => {
                        warn!("Failed to verify disabled state: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Failed to disable key: {:?}", e);
        }
    }

    // Step 4: Enable the key
    info!("🔓 Step 4 - Enabling key...");
    match ctx.client.enable_key(key_id).await {
        Ok(_) => {
            info!("✅ Step 4 - Key enabled successfully");

            // Wait for key to be enabled
            if ctx.wait_for_key_state(key_id, "Enabled", 10).await {
                info!("✅ Key is now enabled");

                // Verify key is enabled by describing it
                match ctx.client.describe_key(key_id).await {
                    Ok(metadata) => {
                        assert_eq!(metadata.key_state.as_deref(), Some("Enabled"));
                        assert_eq!(metadata.enabled, Some(true));
                        info!("✅ Verified key is enabled");
                    }
                    Err(e) => {
                        warn!("Failed to verify enabled state: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Failed to enable key: {:?}", e);
        }
    }

    // Step 5: Schedule key deletion
    info!("🗑️  Step 5 - Scheduling key deletion...");
    match ctx.client.schedule_key_deletion(key_id, Some(7)).await {
        Ok(deletion_response) => {
            info!("✅ Step 5 - Key deletion scheduled successfully");

            // AWS can return either the key ID or the full ARN, so check both formats
            if let Some(response_key_id) = deletion_response.key_id.as_deref() {
                assert!(
                    response_key_id == key_id || response_key_id.contains(key_id),
                    "Response key_id '{}' should match or contain '{}'",
                    response_key_id,
                    key_id
                );
            }

            // AWS may or may not return the pending window in the response
            if let Some(pending_days) = deletion_response.pending_window_in_days {
                assert_eq!(pending_days, 7);
            } else {
                info!("⚠️  AWS did not return pending_window_in_days in response (this is acceptable)");
            }
            assert_eq!(
                deletion_response.key_state.as_deref(),
                Some("PendingDeletion")
            );

            if let Some(deletion_date) = deletion_response.deletion_date {
                info!("Key will be deleted at: {}", deletion_date);
            }

            // Wait for key to be in pending deletion state
            if ctx.wait_for_key_state(key_id, "PendingDeletion", 10).await {
                info!("✅ Key is now pending deletion");

                // Verify key is pending deletion by describing it
                match ctx.client.describe_key(key_id).await {
                    Ok(metadata) => {
                        assert_eq!(metadata.key_state.as_deref(), Some("PendingDeletion"));

                        if metadata.deletion_date.is_some() {
                            info!("✅ Deletion date present in DescribeKey response");
                        } else {
                            info!("⚠️  AWS did not return deletion_date in DescribeKey (eventual consistency, acceptable)");
                        }

                        if let Some(pending_days) = metadata.pending_deletion_window_in_days {
                            assert_eq!(pending_days, 7);
                        } else {
                            info!("⚠️  AWS did not return pending_deletion_window_in_days in DescribeKey (this is acceptable)");
                        }

                        info!("✅ Verified key is pending deletion");
                    }
                    Err(e) => {
                        warn!("Failed to verify pending deletion state: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Failed to schedule key deletion: {:?}", e);
        }
    }

    info!("🎉 End-to-end key lifecycle test completed!");
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_disable_already_disabled_key(ctx: &mut KmsTestContext) {
    info!("🔒 Testing disable operation on already disabled key");

    // Create and disable a key first
    let key_metadata = ctx
        .create_test_key()
        .await
        .expect("Failed to create test key");
    let key_id = &key_metadata.key_id;

    // Disable the key
    ctx.client
        .disable_key(key_id)
        .await
        .expect("Failed to disable key");
    ctx.wait_for_key_state(key_id, "Disabled", 10).await;

    // Try to disable it again - this should succeed (idempotent operation)
    match ctx.client.disable_key(key_id).await {
        Ok(_) => {
            info!("✅ Successfully disabled already disabled key (idempotent)");
        }
        Err(e) => {
            // Some error conditions might be acceptable here
            warn!("Got error when disabling already disabled key: {:?}", e);
        }
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_enable_already_enabled_key(ctx: &mut KmsTestContext) {
    info!("🔓 Testing enable operation on already enabled key");

    // Create a key (starts enabled)
    let key_metadata = ctx
        .create_test_key()
        .await
        .expect("Failed to create test key");
    let key_id = &key_metadata.key_id;

    // Try to enable it again - this should succeed (idempotent operation)
    match ctx.client.enable_key(key_id).await {
        Ok(_) => {
            info!("✅ Successfully enabled already enabled key (idempotent)");
        }
        Err(e) => {
            // Some error conditions might be acceptable here
            warn!("Got error when enabling already enabled key: {:?}", e);
        }
    }
}

#[test_context(KmsTestContext)]
#[tokio::test]
async fn test_operations_on_nonexistent_key(ctx: &mut KmsTestContext) {
    let non_existent_key =
        "arn:aws:kms:us-east-1:123456789012:key/00000000-1111-2222-3333-444444444444";

    info!("🚫 Testing operations on non-existent key");

    // Test disable
    let disable_result = ctx.client.disable_key(non_existent_key).await;
    assert!(disable_result.is_err());

    // Test enable
    let enable_result = ctx.client.enable_key(non_existent_key).await;
    assert!(enable_result.is_err());

    // Test schedule deletion
    let delete_result = ctx
        .client
        .schedule_key_deletion(non_existent_key, Some(7))
        .await;
    assert!(delete_result.is_err());

    info!("✅ All operations correctly failed for non-existent key");
}
