/*!
# AWS Secrets Manager Client Integration Tests

These tests perform real AWS Secrets Manager operations including creating, updating,
retrieving, and deleting secrets.

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
- `secretsmanager:CreateSecret`
- `secretsmanager:UpdateSecret`
- `secretsmanager:DeleteSecret`
- `secretsmanager:DescribeSecret`
- `secretsmanager:GetSecretValue`
- `secretsmanager:PutSecretValue`

## Running Tests
```bash
# Run all Secrets Manager tests
cargo test --package alien-aws-clients --test aws_secrets_manager_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_secrets_manager_client_tests test_create_and_delete_secret -- --nocapture
```
*/

use alien_aws_clients::secrets_manager::{
    CreateSecretRequest, DeleteSecretRequest, DescribeSecretRequest, GetSecretValueRequest,
    PutSecretValueRequest, SecretsManagerApi, SecretsManagerClient, Tag, UpdateSecretRequest,
};
use alien_aws_clients::AwsClientConfig;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::{Error, ErrorData};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct SecretsManagerTestContext {
    client: SecretsManagerClient,
    created_secrets: Mutex<HashSet<String>>,
}

impl AsyncTestContext for SecretsManagerTestContext {
    async fn setup() -> SecretsManagerTestContext {
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

        let aws_config = AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };

        let client = SecretsManagerClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        SecretsManagerTestContext {
            client,
            created_secrets: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Secrets Manager test cleanup...");

        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for secret_name in secrets_to_cleanup {
            self.cleanup_secret(&secret_name).await;
        }

        info!("✅ Secrets Manager test cleanup completed");
    }
}

impl SecretsManagerTestContext {
    fn track_secret(&self, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.insert(secret_name.to_string());
        info!("📝 Tracking secret for cleanup: {}", secret_name);
    }

    async fn cleanup_secret(&self, secret_name: &str) {
        info!("🧹 Cleaning up secret: {}", secret_name);

        let request = DeleteSecretRequest::builder()
            .secret_id(secret_name.to_string())
            .force_delete_without_recovery(true)
            .build();

        match self.client.delete_secret(request).await {
            Ok(_) => {
                info!("✅ Secret {} deleted successfully", secret_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete secret {} during cleanup: {:?}",
                        secret_name, e
                    );
                }
            }
        }
    }

    fn get_test_secret_name(&self) -> String {
        format!("alien-test-secret-{}", Uuid::new_v4().simple().to_string())
    }
}

#[test_context(SecretsManagerTestContext)]
#[tokio::test]
async fn test_update_secret_value(ctx: &mut SecretsManagerTestContext) {
    let secret_name = ctx.get_test_secret_name();
    let initial_value = "initial-secret-value";
    let updated_value = "updated-secret-value";

    info!("🔄 Testing update secret value: {}", secret_name);

    // Create a secret first
    let create_request = CreateSecretRequest::builder()
        .name(secret_name.clone())
        .description("Test secret for value update".to_string())
        .secret_string(initial_value.to_string())
        .build();

    ctx.client
        .create_secret(create_request)
        .await
        .expect("Failed to create secret for update test");
    ctx.track_secret(&secret_name);

    // Update the secret value using update_secret
    let update_request = UpdateSecretRequest::builder()
        .secret_id(secret_name.clone())
        .secret_string(updated_value.to_string())
        .build();

    match ctx.client.update_secret(update_request).await {
        Ok(response) => {
            info!("✅ Secret value updated successfully");
            assert_eq!(response.name.as_ref().unwrap(), &secret_name);
            assert!(
                response.version_id.is_some(),
                "UpdateSecret should return version_id when updating value"
            );

            // Verify the value was updated
            let get_request = GetSecretValueRequest::builder()
                .secret_id(secret_name.clone())
                .build();

            let get_response = ctx
                .client
                .get_secret_value(get_request)
                .await
                .expect("Failed to get updated secret value");
            assert_eq!(get_response.secret_string.as_ref().unwrap(), updated_value);
        }
        Err(e) => {
            panic!("Update secret value failed: {:?}", e);
        }
    }
}

#[test_context(SecretsManagerTestContext)]
#[tokio::test]
async fn test_binary_secrets_workflow(ctx: &mut SecretsManagerTestContext) {
    let secret_name = ctx.get_test_secret_name();
    let initial_binary_data = b"Hello, Binary World!";
    let updated_binary_data = b"Updated Binary Data!";

    info!("🔐 Testing binary secrets workflow: {}", secret_name);

    // Convert binary data to base64 for AWS Secrets Manager
    let initial_binary_b64 = base64_standard.encode(initial_binary_data);
    let updated_binary_b64 = base64_standard.encode(updated_binary_data);

    // 1. Create secret with binary data
    let create_request = CreateSecretRequest::builder()
        .name(secret_name.clone())
        .description("Test binary secret".to_string())
        .secret_binary(initial_binary_b64.clone())
        .tags(vec![Tag::builder()
            .key("Type".to_string())
            .value("Binary".to_string())
            .build()])
        .build();

    ctx.client
        .create_secret(create_request)
        .await
        .expect("Failed to create binary secret");
    ctx.track_secret(&secret_name);
    info!("✅ Binary secret created successfully");

    // 2. Retrieve and verify binary data
    let get_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let get_response = ctx
        .client
        .get_secret_value(get_request)
        .await
        .expect("Failed to get binary secret value");

    assert!(
        get_response.secret_binary.is_some(),
        "Binary secret should have secret_binary field"
    );
    assert!(
        get_response.secret_string.is_none(),
        "Binary secret should not have secret_string field"
    );
    assert_eq!(
        get_response.secret_binary.as_ref().unwrap(),
        &initial_binary_b64
    );
    info!("✅ Binary secret value retrieved and verified");

    // 3. Update binary data using put_secret_value
    let put_request = PutSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .secret_binary(updated_binary_b64.clone())
        .build();

    ctx.client
        .put_secret_value(put_request)
        .await
        .expect("Failed to update binary secret value");
    info!("✅ Binary secret value updated via put_secret_value");

    // 4. Verify updated binary data
    let get_updated_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let get_updated_response = ctx
        .client
        .get_secret_value(get_updated_request)
        .await
        .expect("Failed to get updated binary secret value");

    assert_eq!(
        get_updated_response.secret_binary.as_ref().unwrap(),
        &updated_binary_b64
    );
    info!("✅ Updated binary secret value verified");

    // 5. Update binary data using update_secret
    let final_binary_data = b"Final Binary Update!";
    let final_binary_b64 = base64_standard.encode(final_binary_data);

    let update_request = UpdateSecretRequest::builder()
        .secret_id(secret_name.clone())
        .secret_binary(final_binary_b64.clone())
        .build();

    ctx.client
        .update_secret(update_request)
        .await
        .expect("Failed to update binary secret via update_secret");
    info!("✅ Binary secret value updated via update_secret");

    // 6. Verify final binary data
    let get_final_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let get_final_response = ctx
        .client
        .get_secret_value(get_final_request)
        .await
        .expect("Failed to get final binary secret value");

    assert_eq!(
        get_final_response.secret_binary.as_ref().unwrap(),
        &final_binary_b64
    );
    info!("✅ Final binary secret value verified");
}

#[test_context(SecretsManagerTestContext)]
#[tokio::test]
async fn test_comprehensive_secret_workflow(ctx: &mut SecretsManagerTestContext) {
    let secret_name = ctx.get_test_secret_name();
    let initial_value = "initial-secret-value";
    let updated_value_via_put = "updated-via-put";
    let updated_value_via_update = "updated-via-update";
    let final_description = "Final comprehensive test description";

    info!("🔄 Testing comprehensive secret workflow: {}", secret_name);

    // STEP 1: Create secret with initial value and metadata
    info!("📝 Step 1: Creating secret with initial value and metadata");
    let create_request = CreateSecretRequest::builder()
        .name(secret_name.clone())
        .description("Initial description for comprehensive test".to_string())
        .secret_string(initial_value.to_string())
        .tags(vec![
            Tag::builder()
                .key("Project".to_string())
                .value("alien".to_string())
                .build(),
            Tag::builder()
                .key("TestType".to_string())
                .value("comprehensive".to_string())
                .build(),
        ])
        .build();

    let create_response = ctx
        .client
        .create_secret(create_request)
        .await
        .expect("Failed to create secret for comprehensive test");
    ctx.track_secret(&secret_name);

    assert!(
        create_response.name.is_some(),
        "CreateSecret should return secret name"
    );
    assert_eq!(create_response.name.as_ref().unwrap(), &secret_name);
    assert!(
        create_response.version_id.is_some(),
        "CreateSecret should return version_id"
    );
    info!(
        "✅ Step 1: Secret created successfully with version_id: {:?}",
        create_response.version_id
    );

    // STEP 2: Describe secret to verify metadata
    info!("📋 Step 2: Describing secret to verify metadata");
    let describe_request = DescribeSecretRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let describe_response = ctx
        .client
        .describe_secret(describe_request)
        .await
        .expect("Failed to describe secret in comprehensive test");

    assert_eq!(describe_response.name.as_ref().unwrap(), &secret_name);
    assert_eq!(
        describe_response.description.as_ref().unwrap(),
        "Initial description for comprehensive test"
    );
    assert!(describe_response.tags.is_some(), "Secret should have tags");
    assert_eq!(
        describe_response.tags.as_ref().unwrap().len(),
        2,
        "Secret should have 2 tags"
    );
    assert!(
        describe_response.created_date.is_some(),
        "Secret should have created_date"
    );
    info!("✅ Step 2: Secret metadata verified successfully");

    // STEP 3: Get initial secret value
    info!("🔍 Step 3: Getting initial secret value");
    let get_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let get_response = ctx
        .client
        .get_secret_value(get_request)
        .await
        .expect("Failed to get initial secret value");

    assert_eq!(get_response.secret_string.as_ref().unwrap(), initial_value);
    assert_eq!(get_response.name.as_ref().unwrap(), &secret_name);
    assert!(
        get_response.version_id.is_some(),
        "GetSecretValue should return version_id"
    );
    assert!(
        get_response.created_date.is_some(),
        "GetSecretValue should return created_date"
    );
    info!("✅ Step 3: Initial secret value retrieved and verified");

    // STEP 4: Update secret value using put_secret_value
    info!("🔄 Step 4: Updating secret value using put_secret_value");
    let put_request = PutSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .secret_string(updated_value_via_put.to_string())
        .build();

    let put_response = ctx
        .client
        .put_secret_value(put_request)
        .await
        .expect("Failed to put new secret value");

    assert_eq!(put_response.name.as_ref().unwrap(), &secret_name);
    assert!(
        put_response.version_id.is_some(),
        "PutSecretValue should return version_id"
    );
    assert_ne!(
        put_response.version_id, create_response.version_id,
        "New version should have different version_id"
    );
    info!(
        "✅ Step 4: Secret value updated via put_secret_value with new version_id: {:?}",
        put_response.version_id
    );

    // STEP 5: Verify value was updated via put_secret_value
    info!("🔍 Step 5: Verifying value was updated via put_secret_value");
    let get_put_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let get_put_response = ctx
        .client
        .get_secret_value(get_put_request)
        .await
        .expect("Failed to get secret value after put");

    assert_eq!(
        get_put_response.secret_string.as_ref().unwrap(),
        updated_value_via_put
    );
    assert_eq!(
        get_put_response.version_id, put_response.version_id,
        "Retrieved version should match put version"
    );
    info!("✅ Step 5: Value update via put_secret_value verified");

    // STEP 6: Update secret value using update_secret
    info!("🔄 Step 6: Updating secret value using update_secret");
    let update_request = UpdateSecretRequest::builder()
        .secret_id(secret_name.clone())
        .secret_string(updated_value_via_update.to_string())
        .build();

    let update_response = ctx
        .client
        .update_secret(update_request)
        .await
        .expect("Failed to update secret value via update_secret");

    assert_eq!(update_response.name.as_ref().unwrap(), &secret_name);
    assert!(
        update_response.version_id.is_some(),
        "UpdateSecret should return version_id"
    );
    assert_ne!(
        update_response.version_id, put_response.version_id,
        "Update version should be different from put version"
    );
    info!(
        "✅ Step 6: Secret value updated via update_secret with new version_id: {:?}",
        update_response.version_id
    );

    // STEP 7: Update secret metadata (description)
    info!("📝 Step 7: Updating secret metadata (description)");
    let update_metadata_request = UpdateSecretRequest::builder()
        .secret_id(secret_name.clone())
        .description(final_description.to_string())
        .build();

    let update_metadata_response = ctx
        .client
        .update_secret(update_metadata_request)
        .await
        .expect("Failed to update secret metadata");

    assert_eq!(
        update_metadata_response.name.as_ref().unwrap(),
        &secret_name
    );
    // Note: AWS may or may not return a version_id when only updating metadata
    info!("✅ Step 7: Secret metadata updated successfully");

    // STEP 8: Verify final state - both value and metadata
    info!("🔍 Step 8: Verifying final state - both value and metadata");
    let final_get_request = GetSecretValueRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let final_get_response = ctx
        .client
        .get_secret_value(final_get_request)
        .await
        .expect("Failed to get final secret value");

    assert_eq!(
        final_get_response.secret_string.as_ref().unwrap(),
        updated_value_via_update
    );
    assert_eq!(final_get_response.name.as_ref().unwrap(), &secret_name);
    info!("✅ Step 8: Final secret value verified");

    let final_describe_request = DescribeSecretRequest::builder()
        .secret_id(secret_name.clone())
        .build();

    let final_describe_response = ctx
        .client
        .describe_secret(final_describe_request)
        .await
        .expect("Failed to describe final secret state");

    assert_eq!(
        final_describe_response.description.as_ref().unwrap(),
        final_description
    );
    assert_eq!(final_describe_response.name.as_ref().unwrap(), &secret_name);
    assert!(
        final_describe_response.tags.is_some(),
        "Final secret should still have tags"
    );
    assert_eq!(
        final_describe_response.tags.as_ref().unwrap().len(),
        2,
        "Final secret should still have 2 tags"
    );
    info!("✅ Step 8: Final secret metadata verified");

    info!("🎉 Comprehensive secret workflow test completed successfully - all 8 steps validated!");
}

#[test_context(SecretsManagerTestContext)]
#[tokio::test]
async fn test_get_non_existent_secret(ctx: &mut SecretsManagerTestContext) {
    let non_existent_secret = "alien-test-non-existent-secret-12345";

    info!(
        "❌ Testing get non-existent secret: {}",
        non_existent_secret
    );

    let request = GetSecretValueRequest::builder()
        .secret_id(non_existent_secret.to_string())
        .build();

    let result = ctx.client.get_secret_value(request).await;

    assert!(result.is_err());
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
            assert_eq!(resource_type, "AWS Secrets Manager Secret");
            assert_eq!(resource_name, non_existent_secret);
            info!("✅ Correctly detected non-existent secret");
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(SecretsManagerTestContext)]
#[tokio::test]
async fn test_secrets_manager_with_invalid_credentials(_ctx: &mut SecretsManagerTestContext) {
    let region = std::env::var("AWS_MANAGEMENT_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());

    let invalid_config = AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid".to_string(),
            secret_access_key: "invalid".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let invalid_client = SecretsManagerClient::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(invalid_config),
    );

    info!("🔐 Testing Secrets Manager client with invalid credentials");

    let request = DescribeSecretRequest::builder()
        .secret_id("any-secret".to_string())
        .build();
    let result = invalid_client.describe_secret(request).await;

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
