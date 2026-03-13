/*!
# AWS Systems Manager (SSM) Client Integration Tests

These tests perform real AWS SSM operations including Parameter Store operations
(put, get, delete parameters) and Run Command operations.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=us-east-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `ssm:PutParameter`
- `ssm:GetParameter`
- `ssm:DeleteParameter`
- `ssm:SendCommand`
- `ssm:GetCommandInvocation`

## Running Tests
```bash
# Run all SSM tests
cargo test --package alien-aws-clients --test aws_ssm_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_ssm_client_tests test_parameter_store_e2e -- --nocapture
```
*/

use alien_aws_clients::ssm::*;
use alien_aws_clients::AwsClientConfig;
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct SsmTestContext {
    client: SsmClient,
    created_parameters: Mutex<HashSet<String>>,
}

impl AsyncTestContext for SsmTestContext {
    async fn setup() -> SsmTestContext {
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

        let client = SsmClient::new(Client::new(), aws_config);

        SsmTestContext {
            client,
            created_parameters: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting SSM test cleanup...");

        let parameters_to_cleanup = {
            let parameters = self.created_parameters.lock().unwrap();
            parameters.clone()
        };

        for param_name in parameters_to_cleanup {
            self.cleanup_parameter(&param_name).await;
        }

        info!("✅ SSM test cleanup completed");
    }
}

impl SsmTestContext {
    fn track_parameter(&self, param_name: &str) {
        let mut parameters = self.created_parameters.lock().unwrap();
        parameters.insert(param_name.to_string());
        info!("📝 Tracking parameter for cleanup: {}", param_name);
    }

    fn untrack_parameter(&self, param_name: &str) {
        let mut parameters = self.created_parameters.lock().unwrap();
        parameters.remove(param_name);
        info!("✅ Parameter {} untracked", param_name);
    }

    async fn cleanup_parameter(&self, param_name: &str) {
        info!("🧹 Cleaning up parameter: {}", param_name);

        match self.client.delete_parameter(param_name).await {
            Ok(_) => {
                info!("✅ Parameter {} deleted successfully", param_name);
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
                        "Failed to delete parameter {} during cleanup: {:?}",
                        param_name, e
                    );
                }
            }
        }
    }

    fn get_test_parameter_name(&self) -> String {
        format!("/alien-test/param-{}", Uuid::new_v4().simple())
    }
}

// ============================================================================
// Parameter Store Tests
// ============================================================================

/// Comprehensive test for Parameter Store operations.
/// Creates a String parameter, reads it, updates it, and deletes it.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_parameter_store_e2e(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();
    let initial_value = "initial-test-value";
    let updated_value = "updated-test-value";

    info!("🚀 Starting Parameter Store E2E test: {}", param_name);

    // =========================================================================
    // Step 1: Create a String parameter
    // =========================================================================
    info!("📝 Step 1: Creating String parameter");
    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value(initial_value.to_string())
        .parameter_type("String".to_string())
        .description("Test parameter created by alien-aws-clients tests".to_string())
        .build();

    let create_response = ctx
        .client
        .put_parameter(create_request)
        .await
        .expect("Failed to create parameter");
    ctx.track_parameter(&param_name);

    assert!(
        create_response.version.is_some(),
        "PutParameter should return version"
    );
    assert_eq!(
        create_response.version,
        Some(1),
        "Initial version should be 1"
    );
    info!(
        "✅ Parameter created with version: {:?}",
        create_response.version
    );

    // =========================================================================
    // Step 2: Get the parameter and verify
    // =========================================================================
    info!("🔍 Step 2: Getting parameter value");
    let get_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .with_decryption(false)
        .build();

    let get_response = ctx
        .client
        .get_parameter(get_request)
        .await
        .expect("Failed to get parameter");

    let parameter = get_response.parameter.expect("Parameter should exist");
    assert_eq!(parameter.name.as_deref(), Some(param_name.as_str()));
    assert_eq!(parameter.value.as_deref(), Some(initial_value));
    assert_eq!(parameter.parameter_type.as_deref(), Some("String"));
    assert_eq!(parameter.version, Some(1));
    info!("✅ Parameter value verified: {:?}", parameter.value);

    // =========================================================================
    // Step 3: Update the parameter (overwrite)
    // =========================================================================
    info!("🔄 Step 3: Updating parameter value");
    let update_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value(updated_value.to_string())
        .parameter_type("String".to_string())
        .overwrite(true)
        .build();

    let update_response = ctx
        .client
        .put_parameter(update_request)
        .await
        .expect("Failed to update parameter");

    assert_eq!(
        update_response.version,
        Some(2),
        "Updated version should be 2"
    );
    info!(
        "✅ Parameter updated to version: {:?}",
        update_response.version
    );

    // =========================================================================
    // Step 4: Verify the updated value
    // =========================================================================
    info!("🔍 Step 4: Verifying updated value");
    let verify_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .with_decryption(false)
        .build();

    let verify_response = ctx
        .client
        .get_parameter(verify_request)
        .await
        .expect("Failed to get updated parameter");

    let updated_param = verify_response.parameter.expect("Parameter should exist");
    assert_eq!(updated_param.value.as_deref(), Some(updated_value));
    assert_eq!(updated_param.version, Some(2));
    info!("✅ Updated value verified");

    // =========================================================================
    // Step 5: Delete the parameter
    // =========================================================================
    info!("🗑️ Step 5: Deleting parameter");
    ctx.client
        .delete_parameter(&param_name)
        .await
        .expect("Failed to delete parameter");
    ctx.untrack_parameter(&param_name);
    info!("✅ Parameter deleted");

    // =========================================================================
    // Step 6: Verify deletion (should get NotFound)
    // =========================================================================
    info!("❌ Step 6: Verifying parameter is deleted");
    let verify_deleted_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .build();

    let result = ctx.client.get_parameter(verify_deleted_request).await;
    assert!(result.is_err(), "Should fail to get deleted parameter");

    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Confirmed parameter was deleted");
        }
        other => {
            warn!("Got unexpected error after parameter deletion: {:?}", other);
        }
    }

    info!("🎉 Parameter Store E2E test completed successfully!");
}

/// Test SecureString parameter type with encryption.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_secure_string_parameter(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();
    let secret_value = "my-super-secret-value";

    info!("🔐 Testing SecureString parameter: {}", param_name);

    // Create SecureString parameter
    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value(secret_value.to_string())
        .parameter_type("SecureString".to_string())
        .description("Test secure parameter".to_string())
        .build();

    ctx.client
        .put_parameter(create_request)
        .await
        .expect("Failed to create SecureString parameter");
    ctx.track_parameter(&param_name);
    info!("✅ SecureString parameter created");

    // Get parameter WITHOUT decryption
    let get_encrypted_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .with_decryption(false)
        .build();

    let encrypted_response = ctx
        .client
        .get_parameter(get_encrypted_request)
        .await
        .expect("Failed to get encrypted parameter");

    let encrypted_param = encrypted_response
        .parameter
        .expect("Parameter should exist");
    assert_eq!(
        encrypted_param.parameter_type.as_deref(),
        Some("SecureString")
    );
    // Value should be encrypted (not equal to original)
    assert_ne!(encrypted_param.value.as_deref(), Some(secret_value));
    info!(
        "✅ Retrieved encrypted value (different from plaintext): {:?}",
        encrypted_param.value
    );

    // Get parameter WITH decryption
    let get_decrypted_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .with_decryption(true)
        .build();

    let decrypted_response = ctx
        .client
        .get_parameter(get_decrypted_request)
        .await
        .expect("Failed to get decrypted parameter");

    let decrypted_param = decrypted_response
        .parameter
        .expect("Parameter should exist");
    assert_eq!(decrypted_param.value.as_deref(), Some(secret_value));
    info!("✅ Retrieved decrypted value matches original");
}

/// Test StringList parameter type.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_string_list_parameter(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();
    let list_value = "value1,value2,value3";

    info!("📋 Testing StringList parameter: {}", param_name);

    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value(list_value.to_string())
        .parameter_type("StringList".to_string())
        .build();

    ctx.client
        .put_parameter(create_request)
        .await
        .expect("Failed to create StringList parameter");
    ctx.track_parameter(&param_name);
    info!("✅ StringList parameter created");

    let get_request = GetParameterRequest::builder()
        .name(param_name.clone())
        .build();

    let get_response = ctx
        .client
        .get_parameter(get_request)
        .await
        .expect("Failed to get StringList parameter");

    let parameter = get_response.parameter.expect("Parameter should exist");
    assert_eq!(parameter.parameter_type.as_deref(), Some("StringList"));
    assert_eq!(parameter.value.as_deref(), Some(list_value));
    info!("✅ StringList parameter verified");
}

/// Test error handling for non-existent parameter.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_get_non_existent_parameter(ctx: &mut SsmTestContext) {
    let non_existent_param = "/alien-test/non-existent-parameter-12345";

    info!(
        "❌ Testing get non-existent parameter: {}",
        non_existent_param
    );

    let request = GetParameterRequest::builder()
        .name(non_existent_param.to_string())
        .build();

    let result = ctx.client.get_parameter(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_type, .. }),
            ..
        } => {
            assert_eq!(resource_type, "SSM Parameter");
            info!("✅ Correctly detected non-existent parameter");
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

/// Test trying to create a parameter without overwrite flag when it exists.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_parameter_already_exists_error(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();

    info!("🔄 Testing parameter already exists error: {}", param_name);

    // Create initial parameter
    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value("initial-value".to_string())
        .parameter_type("String".to_string())
        .build();

    ctx.client
        .put_parameter(create_request)
        .await
        .expect("Failed to create parameter");
    ctx.track_parameter(&param_name);

    // Try to create the same parameter without overwrite
    let duplicate_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value("new-value".to_string())
        .parameter_type("String".to_string())
        .overwrite(false)
        .build();

    let result = ctx.client.put_parameter(duplicate_request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceConflict { .. }),
            ..
        } => {
            info!("✅ Correctly detected parameter already exists");
        }
        other => {
            warn!("Got unexpected error for duplicate parameter: {:?}", other);
        }
    }
}

/// Test SSM client with invalid credentials.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_ssm_client_with_invalid_credentials(_ctx: &mut SsmTestContext) {
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
    let invalid_client = SsmClient::new(Client::new(), invalid_config);

    info!("🔐 Testing SSM client with invalid credentials");

    let request = GetParameterRequest::builder()
        .name("/any/parameter".to_string())
        .build();

    let result = invalid_client.get_parameter(request).await;

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

/// Test parameter with tags.
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_parameter_with_tags(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();

    info!("🏷️ Testing parameter with tags: {}", param_name);

    let tags = vec![
        SsmTag::builder()
            .key("Environment".to_string())
            .value("Test".to_string())
            .build(),
        SsmTag::builder()
            .key("Project".to_string())
            .value("alien".to_string())
            .build(),
    ];

    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value("test-value".to_string())
        .parameter_type("String".to_string())
        .tags(tags)
        .build();

    let response = ctx
        .client
        .put_parameter(create_request)
        .await
        .expect("Failed to create parameter with tags");
    ctx.track_parameter(&param_name);

    assert!(response.version.is_some());
    info!("✅ Parameter with tags created successfully");
}

/// Test parameter with tier (Standard vs Advanced).
#[test_context(SsmTestContext)]
#[tokio::test]
async fn test_parameter_with_tier(ctx: &mut SsmTestContext) {
    let param_name = ctx.get_test_parameter_name();

    info!("📊 Testing parameter with explicit tier: {}", param_name);

    let create_request = PutParameterRequest::builder()
        .name(param_name.clone())
        .value("standard-tier-value".to_string())
        .parameter_type("String".to_string())
        .tier("Standard".to_string())
        .build();

    let response = ctx
        .client
        .put_parameter(create_request)
        .await
        .expect("Failed to create parameter with tier");
    ctx.track_parameter(&param_name);

    assert_eq!(response.tier.as_deref(), Some("Standard"));
    info!("✅ Parameter created with Standard tier");
}
