/*!
# STS Client Integration Tests

These tests perform real AWS STS operations including getting caller identity,
assuming roles, and testing various error conditions.

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
- `sts:GetCallerIdentity`
- `sts:AssumeRole` (for testing role assumptions)

## Running Tests
```bash
# Run all STS tests
cargo test --package alien-aws-clients --test aws_sts_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_sts_client_tests test_get_caller_identity -- --nocapture
```
*/

use alien_aws_clients::sts::*;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::path::PathBuf as StdPathBuf;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};

struct StsTestContext {
    client: StsClient,
}

impl AsyncTestContext for StsTestContext {
    async fn setup() -> StsTestContext {
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
        let client = StsClient::new(Client::new(), aws_config);

        StsTestContext { client }
    }

    async fn teardown(self) {
        info!("✅ STS test cleanup completed (no resources to clean up)");
    }
}

impl StsTestContext {
    fn get_test_role_arn(&self, account_id: &str) -> String {
        format!("arn:aws:iam::{}:role/non-existent-test-role", account_id)
    }

    fn get_cross_account_role_arn(&self) -> String {
        "arn:aws:iam::999999999999:role/non-existent-cross-account-role".to_string()
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_get_caller_identity(ctx: &mut StsTestContext) {
    info!("🔍 Testing get caller identity");

    match ctx.client.get_caller_identity().await {
        Ok(response) => {
            info!("✅ Caller identity retrieved successfully");
            let result = &response.get_caller_identity_result;

            // Validate response structure
            if let Some(ref arn) = result.arn {
                info!("👤 Caller ARN: {}", arn);
                assert!(!arn.is_empty());
                assert!(arn.starts_with("arn:aws:"));
            }

            if let Some(ref user_id) = result.user_id {
                info!("🆔 User ID: {}", user_id);
                assert!(!user_id.is_empty());
            }

            if let Some(ref account) = result.account {
                info!("🏢 Account: {}", account);
                assert!(!account.is_empty());
                assert!(account.chars().all(|c| c.is_ascii_digit()));
                assert_eq!(account.len(), 12); // AWS account IDs are always 12 digits
            }
        }
        Err(e) => {
            panic!("Get caller identity failed: {:?}. Please ensure you have proper AWS credentials set up in .env.test", e);
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_get_caller_identity_with_invalid_credentials(ctx: &mut StsTestContext) {
    let region = std::env::var("AWS_MANAGEMENT_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());

    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid_access_key".to_string(),
            secret_access_key: "invalid_secret_key".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let invalid_client = StsClient::new(Client::new(), aws_config);

    info!("🔐 Testing get caller identity with invalid credentials");

    let result = invalid_client.get_caller_identity().await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Correctly detected invalid credentials");
        }
        Error {
            error: Some(ErrorData::AuthenticationError { .. }),
            ..
        } => {
            info!("✅ Got authentication error for invalid credentials (also acceptable)");
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

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_non_existent_role(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!(
        "❌ Testing assume role with non-existent role: {}",
        role_arn
    );

    let request = AssumeRoleRequest::builder()
        .role_arn(role_arn.clone())
        .role_session_name("alien-test-session".to_string())
        .duration_seconds(3600)
        .build();

    let result = ctx.client.assume_role(request).await;

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
            assert_eq!(resource_type, "STS Resource");
            assert_eq!(resource_name, role_arn);
            info!("✅ Correctly detected non-existent role");
        }
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Got access denied for non-existent role (also acceptable)");
        }
        other => {
            warn!("Got unexpected error for non-existent role: {:?}", other);
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_cross_account_role(ctx: &mut StsTestContext) {
    let role_arn = ctx.get_cross_account_role_arn();

    info!(
        "🔒 Testing assume role with cross-account role (should fail): {}",
        role_arn
    );

    let request = AssumeRoleRequest::builder()
        .role_arn(role_arn.clone())
        .role_session_name("alien-test-cross-account-session".to_string())
        .duration_seconds(3600)
        .build();

    let result = ctx.client.assume_role(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Correctly denied cross-account role assumption");
        }
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Cross-account role not found (also acceptable)");
        }
        other => {
            warn!("Got unexpected error for cross-account role: {:?}", other);
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_invalid_role_arn(ctx: &mut StsTestContext) {
    let invalid_role_arns = vec![
        "invalid-arn",
        "arn:aws:iam::invalid:role/test",
        "arn:aws:iam::123456789012:role/",
        "",
        "arn:aws:iam::123456789012:policy/test-policy", // Wrong resource type
    ];

    for invalid_arn in invalid_role_arns {
        info!("❌ Testing assume role with invalid ARN: '{}'", invalid_arn);

        let request = AssumeRoleRequest::builder()
            .role_arn(invalid_arn.to_string())
            .role_session_name("alien-test-invalid-session".to_string())
            .build();

        let result = ctx.client.assume_role(request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            Error {
                error: Some(ErrorData::InvalidInput { .. }),
                ..
            } => {
                info!(
                    "✅ Correctly rejected invalid ARN format: '{}'",
                    invalid_arn
                );
            }
            Error {
                error: Some(ErrorData::RemoteAccessDenied { .. }),
                ..
            } => {
                info!(
                    "✅ Got access denied for invalid ARN: '{}' (also acceptable)",
                    invalid_arn
                );
            }
            other => {
                info!(
                    "Got error for invalid ARN '{}': {:?} (acceptable as long as it fails)",
                    invalid_arn, other
                );
            }
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_invalid_session_name(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!("📝 Testing assume role with invalid session names");

    let long_name = "a".repeat(65);
    let invalid_session_names = vec![
        "",                           // Empty name
        "a",                          // Too short (minimum 2 chars)
        "session with spaces",        // Spaces not allowed
        "session@with#special$chars", // Special chars not allowed
        &long_name,                   // Too long (maximum 64 chars)
    ];

    for session_name in invalid_session_names {
        info!("❌ Testing with invalid session name: '{}'", session_name);

        let request = AssumeRoleRequest::builder()
            .role_arn(role_arn.clone())
            .role_session_name(session_name.to_string())
            .build();

        let result = ctx.client.assume_role(request).await;

        // We expect this to fail, but the specific error might vary
        // Some validation might happen on AWS side, some on client side
        assert!(result.is_err());
        info!(
            "✅ Correctly rejected invalid session name: '{}'",
            session_name
        );
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_invalid_duration(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!("⏱️ Testing assume role with invalid duration values");

    let invalid_durations = vec![
        899,   // Too short (minimum 900 seconds = 15 minutes)
        43201, // Too long (maximum 43200 seconds = 12 hours)
        -1,    // Negative duration
        0,     // Zero duration
    ];

    for duration in invalid_durations {
        info!("❌ Testing with invalid duration: {} seconds", duration);

        let request = AssumeRoleRequest::builder()
            .role_arn(role_arn.clone())
            .role_session_name("alien-test-invalid-duration".to_string())
            .duration_seconds(duration)
            .build();

        let result = ctx.client.assume_role(request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            Error {
                error: Some(ErrorData::InvalidInput { .. }),
                ..
            } => {
                info!("✅ Correctly rejected invalid duration: {}", duration);
            }
            other => {
                info!(
                    "Got error for invalid duration {}: {:?} (acceptable as long as it fails)",
                    duration, other
                );
            }
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_valid_parameters_but_no_permissions(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!("🎯 Testing assume role with valid parameters but non-existent role");

    let request = AssumeRoleRequest::builder()
        .role_arn(role_arn.clone())
        .role_session_name("AlienTestSession2024".to_string()) // Valid session name
        .duration_seconds(3600) // Valid duration (1 hour)
        .external_id("test-external-id-123".to_string()) // Optional but valid
        .build();

    let result = ctx.client.assume_role(request).await;

    assert!(result.is_err());
    info!("✅ Assume role failed as expected for non-existent role with valid parameters");
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_policy_document(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!("📋 Testing assume role with inline policy document");

    let policy_document = r#"{
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": [
                    "s3:GetObject"
                ],
                "Resource": "arn:aws:s3:::test-bucket/*"
            }
        ]
    }"#;

    let request = AssumeRoleRequest::builder()
        .role_arn(role_arn.clone())
        .role_session_name("AlienTestSessionWithPolicy".to_string())
        .duration_seconds(3600)
        .policy(policy_document.to_string())
        .build();

    let result = ctx.client.assume_role(request).await;

    // Should still fail because role doesn't exist, but at least we test the policy parameter
    assert!(result.is_err());
    info!("✅ Assume role with policy failed as expected (role doesn't exist)");
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_assume_role_with_malformed_policy(ctx: &mut StsTestContext) {
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let role_arn = ctx.get_test_role_arn(&account_id);

    info!("❌ Testing assume role with malformed policy document");

    let malformed_policy = r#"{"invalid": "json", "missing": "bracket""#; // Missing closing brace

    let request = AssumeRoleRequest::builder()
        .role_arn(role_arn.clone())
        .role_session_name("AlienTestSessionBadPolicy".to_string())
        .policy(malformed_policy.to_string())
        .build();

    let result = ctx.client.assume_role(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        } => {
            info!("✅ Correctly rejected malformed policy document");
        }
        other => {
            info!(
                "Got error for malformed policy: {:?} (acceptable as long as it fails)",
                other
            );
        }
    }
}

#[test_context(StsTestContext)]
#[tokio::test]
async fn test_sts_client_with_invalid_region(ctx: &mut StsTestContext) {
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
        region: "invalid-region-name".to_string(),
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: access_key,
            secret_access_key: secret_key,
            session_token: None,
        },
        service_overrides: None,
    };
    let invalid_region_client = StsClient::new(Client::new(), aws_config);

    info!("🌍 Testing STS client with invalid region");

    // GetCallerIdentity should still work with invalid region since STS is global
    // but let's see what happens
    let result = invalid_region_client.get_caller_identity().await;

    match result {
        Ok(_) => {
            info!("✅ GetCallerIdentity worked with invalid region (STS is global)");
        }
        Err(e) => {
            info!("ℹ️ GetCallerIdentity failed with invalid region: {:?}", e);
            // This is also acceptable behavior
        }
    }
}
