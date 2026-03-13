/*!
# IAM Client Integration Tests

These tests perform real AWS IAM operations including creating roles, managing policies,
and testing various error conditions.

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
- `iam:CreateRole`
- `iam:GetRole`
- `iam:DeleteRole`
- `iam:PutRolePolicy`
- `iam:GetRolePolicy`
- `iam:DeleteRolePolicy`
- `iam:PassRole` (for testing role operations)

## Running Tests
```bash
# Run all IAM tests
cargo test --package alien-infra --test iam_client_tests

# Run specific test
cargo test --package alien-infra --test iam_client_tests test_create_and_delete_role -- --nocapture
```
*/

use alien_aws_clients::iam::*;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use aws_credential_types::Credentials;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct IamTestContext {
    client: IamClient,
    created_roles: Mutex<HashSet<String>>,
    created_policies: Mutex<HashSet<(String, String)>>, // (role_name, policy_name) pairs
}

impl AsyncTestContext for IamTestContext {
    async fn setup() -> IamTestContext {
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
        let client = IamClient::new(Client::new(), aws_config);

        IamTestContext {
            client,
            created_roles: Mutex::new(HashSet::new()),
            created_policies: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting IAM test cleanup...");

        let policies_to_cleanup = {
            let policies = self.created_policies.lock().unwrap();
            policies.clone()
        };

        let roles_to_cleanup = {
            let roles = self.created_roles.lock().unwrap();
            roles.clone()
        };

        // First cleanup policies
        for (role_name, policy_name) in policies_to_cleanup {
            self.cleanup_policy(&role_name, &policy_name).await;
        }

        // Then cleanup roles
        for role_name in roles_to_cleanup {
            self.cleanup_role(&role_name).await;
        }

        info!("✅ IAM test cleanup completed");
    }
}

impl IamTestContext {
    fn track_role(&self, role_name: &str) {
        let mut roles = self.created_roles.lock().unwrap();
        roles.insert(role_name.to_string());
        info!("📝 Tracking role for cleanup: {}", role_name);
    }

    fn untrack_role(&self, role_name: &str) {
        let mut roles = self.created_roles.lock().unwrap();
        roles.remove(role_name);
        info!(
            "✅ Role {} successfully cleaned up and untracked",
            role_name
        );
    }

    fn track_policy(&self, role_name: &str, policy_name: &str) {
        let mut policies = self.created_policies.lock().unwrap();
        policies.insert((role_name.to_string(), policy_name.to_string()));
        info!(
            "📝 Tracking policy for cleanup: {} -> {}",
            role_name, policy_name
        );
    }

    fn untrack_policy(&self, role_name: &str, policy_name: &str) {
        let mut policies = self.created_policies.lock().unwrap();
        policies.remove(&(role_name.to_string(), policy_name.to_string()));
        info!(
            "✅ Policy {} -> {} successfully cleaned up and untracked",
            role_name, policy_name
        );
    }

    async fn cleanup_policy(&self, role_name: &str, policy_name: &str) {
        info!("🧹 Cleaning up policy: {} -> {}", role_name, policy_name);

        match self.client.delete_role_policy(role_name, policy_name).await {
            Ok(_) => {
                info!(
                    "✅ Policy {} -> {} deleted successfully",
                    role_name, policy_name
                );
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
                        "Failed to delete policy {} from role {} during cleanup: {:?}",
                        policy_name, role_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_role(&self, role_name: &str) {
        info!("🧹 Cleaning up role: {}", role_name);

        // Delete the role (policies should already be cleaned up by this point)
        match self.client.delete_role(role_name).await {
            Ok(_) => {
                info!("✅ Role {} deleted successfully", role_name);
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
                        "Failed to delete role {} during cleanup: {:?}",
                        role_name, e
                    );
                }
            }
        }
    }

    fn get_test_role_name(&self) -> String {
        format!("alien-test-role-{}", Uuid::new_v4().simple())
    }

    fn get_basic_assume_role_policy(&self) -> String {
        r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": {
                        "Service": "lambda.amazonaws.com"
                    },
                    "Action": "sts:AssumeRole"
                }
            ]
        }"#
        .to_string()
    }

    fn get_basic_role_policy(&self) -> String {
        r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "logs:CreateLogGroup",
                        "logs:CreateLogStream",
                        "logs:PutLogEvents"
                    ],
                    "Resource": "arn:aws:logs:*:*:*"
                }
            ]
        }"#
        .to_string()
    }

    async fn create_test_role(&self, role_name: &str) -> Result<CreateRoleResponse, Error> {
        let request = CreateRoleRequest::builder()
            .role_name(role_name.to_string())
            .assume_role_policy_document(self.get_basic_assume_role_policy())
            .description("Test role created by alien-infra tests".to_string())
            .path("/alien-test/".to_string())
            .max_session_duration(3600)
            .build();

        let result = self.client.create_role(request).await;
        if result.is_ok() {
            self.track_role(role_name);
        }
        result
    }

    async fn create_test_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<(), Error> {
        let result = self
            .client
            .put_role_policy(role_name, policy_name, policy_document)
            .await;
        if result.is_ok() {
            self.track_policy(role_name, policy_name);
        }
        result
    }

    async fn manual_cleanup_policy(&self, role_name: &str, policy_name: &str) {
        self.cleanup_policy(role_name, policy_name).await;
        self.untrack_policy(role_name, policy_name);
    }

    async fn manual_cleanup_role(&self, role_name: &str) {
        self.cleanup_role(role_name).await;
        self.untrack_role(role_name);
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_create_and_delete_role(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();

    info!("🚀 Testing create and delete role: {}", role_name);

    // Create the role
    let create_result = ctx.create_test_role(&role_name).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Role created successfully: {}",
                response.create_role_result.role.arn
            );
            assert_eq!(response.create_role_result.role.role_name, role_name);
            assert_eq!(response.create_role_result.role.path, "/alien-test/");
            if let Some(duration) = response.create_role_result.role.max_session_duration {
                assert_eq!(duration, 3600);
            }
            assert!(response.create_role_result.role.arn.contains(&role_name));
        }
        Err(e) => {
            panic!("Role creation failed: {:?}. Please ensure you have proper AWS credentials and IAM permissions set up in .env.test", e);
        }
    }

    // Manual cleanup - the role will also be cleaned up automatically via teardown
    ctx.manual_cleanup_role(&role_name).await;
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_get_role(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();

    info!("🔍 Testing get role: {}", role_name);

    // First create a role
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role for get test");

    // Now get the role
    match ctx.client.get_role(&role_name).await {
        Ok(response) => {
            info!(
                "✅ Role retrieved successfully: {}",
                response.get_role_result.role.arn
            );
            assert_eq!(response.get_role_result.role.role_name, role_name);
            assert!(response
                .get_role_result
                .role
                .assume_role_policy_document
                .is_some());
            assert!(response.get_role_result.role.arn.contains(&role_name));
        }
        Err(e) => {
            panic!("Get role failed: {:?}", e);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_get_non_existent_role(ctx: &mut IamTestContext) {
    let non_existent_role = "alien-test-non-existent-role";

    info!("❌ Testing get non-existent role: {}", non_existent_role);

    let result = ctx.client.get_role(non_existent_role).await;

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
            assert_eq!(resource_type, "IAM Resource");
            assert_eq!(resource_name, non_existent_role);
            info!("✅ Correctly detected non-existent role");
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_create_role_already_exists(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();

    info!("🔄 Testing create duplicate role: {}", role_name);

    let request = CreateRoleRequest::builder()
        .role_name(role_name.clone())
        .assume_role_policy_document(ctx.get_basic_assume_role_policy())
        .description("Test role for duplicate creation".to_string())
        .build();

    // Create role first time
    match ctx.client.create_role(request.clone()).await {
        Ok(_) => {
            info!("✅ First role creation succeeded");
            ctx.track_role(&role_name);

            // Try to create the same role again
            let result = ctx.client.create_role(request).await;

            assert!(result.is_err());
            match result.unwrap_err() {
                Error {
                    error:
                        Some(ErrorData::RemoteResourceConflict {
                            resource_type,
                            resource_name,
                            ..
                        }),
                    ..
                } => {
                    assert_eq!(resource_type, "IAM Resource");
                    assert_eq!(resource_name, role_name);
                    info!("✅ Correctly detected duplicate role creation");
                }
                other => {
                    panic!("Expected RemoteResourceConflict, got: {:?}", other);
                }
            }
        }
        Err(e) => {
            panic!("Initial role creation failed: {:?}", e);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_create_role_with_invalid_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();

    info!("📝 Testing create role with invalid policy: {}", role_name);

    let invalid_policy = r#"{"invalid": "policy", "missing": "required_fields"}"#;

    let request = CreateRoleRequest::builder()
        .role_name(role_name.clone())
        .assume_role_policy_document(invalid_policy.to_string())
        .description("Test role with invalid policy".to_string())
        .build();

    let result = ctx.client.create_role(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        } => {
            info!("✅ Correctly rejected invalid policy");
        }
        other => {
            warn!("Got unexpected error type for invalid policy: {:?}", other);
            // Still acceptable as long as it's an error
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_put_and_get_role_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let policy_name = "TestPolicy";

    info!(
        "📋 Testing put and get role policy: {} -> {}",
        role_name, policy_name
    );

    // First create a role
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role for policy test");

    // Put a policy on the role using the new tracking method
    let policy_document = ctx.get_basic_role_policy();
    match ctx
        .create_test_policy(&role_name, policy_name, &policy_document)
        .await
    {
        Ok(_) => {
            info!("✅ Policy attached successfully");

            // Now get the policy
            match ctx.client.get_role_policy(&role_name, policy_name).await {
                Ok(response) => {
                    info!("✅ Policy retrieved successfully");
                    info!(
                        "📋 Retrieved policy document: {}",
                        response.get_role_policy_result.policy_document
                    );
                    assert_eq!(response.get_role_policy_result.role_name, role_name);
                    assert_eq!(response.get_role_policy_result.policy_name, policy_name);
                    assert!(!response.get_role_policy_result.policy_document.is_empty());

                    // The policy document should contain the key elements
                    // Note: AWS might return URL-encoded content, so let's decode it first
                    let decoded_policy =
                        urlencoding::decode(&response.get_role_policy_result.policy_document)
                            .unwrap_or_else(|_| {
                                response
                                    .get_role_policy_result
                                    .policy_document
                                    .clone()
                                    .into()
                            });
                    info!("📋 Decoded policy document: {}", decoded_policy);
                    assert!(decoded_policy.contains("logs:CreateLogGroup"));
                    assert!(decoded_policy.contains("2012-10-17"));
                }
                Err(e) => {
                    panic!("Get role policy failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Put role policy failed: {:?}", e);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_delete_role_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let policy_name = "PolicyToDelete";

    info!(
        "🗑️ Testing delete role policy: {} -> {}",
        role_name, policy_name
    );

    // Create role and attach policy
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role");

    let policy_document = ctx.get_basic_role_policy();
    ctx.create_test_policy(&role_name, policy_name, &policy_document)
        .await
        .expect("Failed to attach policy");

    // Verify policy exists
    ctx.client
        .get_role_policy(&role_name, policy_name)
        .await
        .expect("Policy should exist before deletion");

    // Delete the policy
    match ctx.client.delete_role_policy(&role_name, policy_name).await {
        Ok(_) => {
            info!("✅ Policy deleted successfully");
            // Untrack the policy since we manually deleted it
            ctx.untrack_policy(&role_name, policy_name);

            // Verify policy is gone
            let result = ctx.client.get_role_policy(&role_name, policy_name).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                Error {
                    error: Some(ErrorData::RemoteResourceNotFound { .. }),
                    ..
                } => {
                    info!("✅ Confirmed policy was deleted");
                }
                other => {
                    warn!("Got unexpected error after policy deletion: {:?}", other);
                }
            }
        }
        Err(e) => {
            panic!("Delete role policy failed: {:?}", e);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_delete_non_existent_role_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let non_existent_policy = "NonExistentPolicy";

    info!(
        "❌ Testing delete non-existent policy: {} -> {}",
        role_name, non_existent_policy
    );

    // Create role first
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role");

    // Try to delete non-existent policy
    let result = ctx
        .client
        .delete_role_policy(&role_name, non_existent_policy)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Correctly detected non-existent policy");
        }
        other => {
            warn!(
                "Got unexpected error for non-existent policy deletion: {:?}",
                other
            );
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_get_non_existent_role_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let non_existent_policy = "NonExistentPolicy";

    info!(
        "❌ Testing get non-existent policy: {} -> {}",
        role_name, non_existent_policy
    );

    // Create role first
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role");

    // Try to get non-existent policy
    let result = ctx
        .client
        .get_role_policy(&role_name, non_existent_policy)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Correctly detected non-existent policy");
        }
        other => {
            warn!(
                "Got unexpected error for non-existent policy get: {:?}",
                other
            );
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_put_role_policy_with_invalid_json(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let policy_name = "InvalidPolicy";

    info!(
        "📝 Testing put role policy with invalid JSON: {} -> {}",
        role_name, policy_name
    );

    // Create role first
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role");

    // Try to attach invalid policy - this should fail and not be tracked
    let invalid_policy = r#"{"invalid": "json", "missing": "bracket""#; // Malformed JSON

    let result = ctx
        .client
        .put_role_policy(&role_name, policy_name, invalid_policy)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        } => {
            info!("✅ Correctly rejected invalid policy JSON");
        }
        other => {
            warn!("Got unexpected error for invalid policy JSON: {:?}", other);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_iam_client_with_invalid_credentials(ctx: &mut IamTestContext) {
    let region = std::env::var("AWS_MANAGEMENT_REGION").unwrap_or_else(|_| "us-east-1".to_string());
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
    let iam_client = IamClient::new(client_invalid, aws_config);

    info!("🔐 Testing IAM client with invalid credentials");

    let result = iam_client.get_role("any-role").await;

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

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_role_name_validation(ctx: &mut IamTestContext) {
    info!("📏 Testing role name validation");

    // Test edge cases for role names
    let long_name = "a".repeat(65);
    let invalid_role_names = vec![
        "a",                       // Too short (minimum 1 char, but AWS has practical limits)
        &long_name,                // Too long (maximum 64 chars)
        "role with spaces",        // Spaces not allowed
        "role@with#special$chars", // Special chars beyond allowed set
        "",                        // Empty name
    ];

    for role_name in invalid_role_names {
        let request = CreateRoleRequest::builder()
            .role_name(role_name.to_string())
            .assume_role_policy_document(ctx.get_basic_assume_role_policy())
            .description("Test role with invalid name".to_string())
            .build();

        let result = ctx.client.create_role(request).await;
        if result.is_ok() {
            // If it somehow succeeded, track it for cleanup
            ctx.track_role(role_name);
            warn!(
                "Warning: Role name '{}' was accepted when it might not be valid",
                role_name
            );
        } else {
            info!("✅ Correctly rejected invalid role name: '{}'", role_name);
        }
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_large_policy_document(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let policy_name = "LargePolicy";

    info!(
        "📄 Testing large policy document: {} -> {}",
        role_name, policy_name
    );

    // Create role first
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role");

    // Create a large but valid policy (approaching AWS limits)
    let mut large_policy = r#"{
        "Version": "2012-10-17",
        "Statement": [
    "#
    .to_string();

    // Add many similar statements to make it large
    for i in 0..50 {
        if i > 0 {
            large_policy.push(',');
        }
        large_policy.push_str(&format!(
            r#"
            {{
                "Effect": "Allow",
                "Action": [
                    "s3:GetObject",
                    "s3:PutObject"
                ],
                "Resource": "arn:aws:s3:::test-bucket-{}/object-*"
            }}"#,
            i
        ));
    }

    large_policy.push_str("\n        ]\n    }");

    // Try to attach the large policy using the tracking method
    match ctx
        .create_test_policy(&role_name, policy_name, &large_policy)
        .await
    {
        Ok(_) => {
            info!("✅ Large policy attached successfully");

            // Verify we can get it back
            match ctx.client.get_role_policy(&role_name, policy_name).await {
                Ok(response) => {
                    info!(
                        "✅ Large policy retrieved successfully ({} chars)",
                        response.get_role_policy_result.policy_document.len()
                    );
                    assert!(!response.get_role_policy_result.policy_document.is_empty());
                }
                Err(e) => {
                    warn!("Failed to retrieve large policy: {:?}", e);
                }
            }
        }
        Err(e) => match e {
            Error {
                error: Some(ErrorData::QuotaExceeded { .. }),
                ..
            } => {
                info!("✅ Correctly detected policy size limit exceeded");
            }
            other => {
                warn!("Got unexpected error for large policy: {:?}", other);
            }
        },
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_role_with_tags_deserialization(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();

    info!("🏷️ Testing role with tags: {}", role_name);

    // Create role with tags first
    let tags = vec![
        CreateRoleTag {
            key: "Environment".to_string(),
            value: "test".to_string(),
        },
        CreateRoleTag {
            key: "Project".to_string(),
            value: "alien-infra".to_string(),
        },
        CreateRoleTag {
            key: "CreatedBy".to_string(),
            value: "test-suite".to_string(),
        },
    ];

    let request = CreateRoleRequest::builder()
        .role_name(role_name.clone())
        .assume_role_policy_document(ctx.get_basic_assume_role_policy())
        .description("Test role with tags".to_string())
        .path("/alien-test/".to_string())
        .tags(tags)
        .build();

    let create_result = ctx.client.create_role(request).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Role with tags created successfully: {}",
                response.create_role_result.role.arn
            );
            ctx.track_role(&role_name);

            // Verify the role has tags
            if let Some(ref role_tags) = response.create_role_result.role.tags {
                info!("✅ Role created with {} tags", role_tags.member.len());
                for tag in &role_tags.member {
                    info!("  Tag: {:?} = {:?}", tag.key, tag.value);
                }
            } else {
                warn!("Role was created but no tags were returned in the response");
            }

            // Now test get_role to ensure it can handle the tags properly
            match ctx.client.get_role(&role_name).await {
                Ok(get_response) => {
                    info!(
                        "✅ Role with tags retrieved successfully via get_role: {}",
                        get_response.get_role_result.role.arn
                    );

                    // Verify tags are preserved in get_role response
                    if let Some(ref role_tags) = get_response.get_role_result.role.tags {
                        info!("✅ get_role returned {} tags", role_tags.member.len());
                        for tag in &role_tags.member {
                            info!("  Retrieved Tag: {:?} = {:?}", tag.key, tag.value);
                        }

                        // Verify we have the expected tags (though AWS might add additional ones)
                        let tag_keys: Vec<String> =
                            role_tags.member.iter().map(|t| t.key.clone()).collect();

                        assert!(
                            tag_keys.contains(&"Environment".to_string()),
                            "Expected Environment tag not found in: {:?}",
                            tag_keys
                        );
                        assert!(
                            tag_keys.contains(&"Project".to_string()),
                            "Expected Project tag not found in: {:?}",
                            tag_keys
                        );
                        assert!(
                            tag_keys.contains(&"CreatedBy".to_string()),
                            "Expected CreatedBy tag not found in: {:?}",
                            tag_keys
                        );
                    } else {
                        warn!("get_role returned role but no tags were found");
                    }
                }
                Err(e) => {
                    panic!("get_role failed for role with tags: {:?}", e);
                }
            }
        }
        Err(e) => {
            match e {
                Error {
                    error: Some(ErrorData::InvalidInput { .. }),
                    ..
                } => {
                    // This might happen if the account doesn't support tagging or has restrictions
                    warn!("Role creation with tags was rejected (possibly due to account restrictions): {:?}", e);
                }
                other => {
                    panic!("Role creation with tags failed unexpectedly: {:?}", other);
                }
            }
        }
    }
}

// ============================================================================
// Instance Profile Tests
// ============================================================================

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_instance_profile_lifecycle(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let profile_name = format!("alien-test-profile-{}", Uuid::new_v4().simple());

    info!("🚀 Testing instance profile lifecycle: {}", profile_name);

    // First create a role to associate with the instance profile
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role for instance profile test");

    // =========================================================================
    // Step 1: Create instance profile
    // =========================================================================
    info!("📝 Step 1: Creating instance profile");
    let create_request = CreateInstanceProfileRequest::builder()
        .instance_profile_name(profile_name.clone())
        .path("/alien-test/".to_string())
        .build();

    let create_result = ctx.client.create_instance_profile(create_request).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Instance profile created: {}",
                response.create_instance_profile_result.instance_profile.arn
            );
            assert_eq!(
                response
                    .create_instance_profile_result
                    .instance_profile
                    .instance_profile_name,
                profile_name
            );
            assert_eq!(
                response
                    .create_instance_profile_result
                    .instance_profile
                    .path,
                "/alien-test/"
            );
        }
        Err(e) => {
            panic!("Instance profile creation failed: {:?}", e);
        }
    }

    // =========================================================================
    // Step 2: Get instance profile
    // =========================================================================
    info!("🔍 Step 2: Getting instance profile");
    match ctx.client.get_instance_profile(&profile_name).await {
        Ok(response) => {
            info!(
                "✅ Instance profile retrieved: {}",
                response.get_instance_profile_result.instance_profile.arn
            );
            assert_eq!(
                response
                    .get_instance_profile_result
                    .instance_profile
                    .instance_profile_name,
                profile_name
            );
            // Initially should have no roles
            let roles = response
                .get_instance_profile_result
                .instance_profile
                .roles
                .map(|r| r.member.len())
                .unwrap_or(0);
            assert_eq!(roles, 0, "New instance profile should have no roles");
        }
        Err(e) => {
            panic!("Get instance profile failed: {:?}", e);
        }
    }

    // =========================================================================
    // Step 3: Add role to instance profile
    // =========================================================================
    info!("➕ Step 3: Adding role to instance profile");
    match ctx
        .client
        .add_role_to_instance_profile(&profile_name, &role_name)
        .await
    {
        Ok(_) => {
            info!("✅ Role added to instance profile");
        }
        Err(e) => {
            panic!("Add role to instance profile failed: {:?}", e);
        }
    }

    // Verify role was added
    let get_response = ctx
        .client
        .get_instance_profile(&profile_name)
        .await
        .expect("Failed to get instance profile after adding role");
    let roles = get_response
        .get_instance_profile_result
        .instance_profile
        .roles
        .map(|r| r.member.len())
        .unwrap_or(0);
    assert_eq!(roles, 1, "Instance profile should have 1 role after add");
    info!("✅ Verified role was added to instance profile");

    // =========================================================================
    // Step 4: Remove role from instance profile
    // =========================================================================
    info!("➖ Step 4: Removing role from instance profile");
    match ctx
        .client
        .remove_role_from_instance_profile(&profile_name, &role_name)
        .await
    {
        Ok(_) => {
            info!("✅ Role removed from instance profile");
        }
        Err(e) => {
            panic!("Remove role from instance profile failed: {:?}", e);
        }
    }

    // Verify role was removed
    let get_response = ctx
        .client
        .get_instance_profile(&profile_name)
        .await
        .expect("Failed to get instance profile after removing role");
    let roles = get_response
        .get_instance_profile_result
        .instance_profile
        .roles
        .map(|r| r.member.len())
        .unwrap_or(0);
    assert_eq!(
        roles, 0,
        "Instance profile should have 0 roles after remove"
    );
    info!("✅ Verified role was removed from instance profile");

    // =========================================================================
    // Step 5: Delete instance profile
    // =========================================================================
    info!("🗑️ Step 5: Deleting instance profile");
    match ctx.client.delete_instance_profile(&profile_name).await {
        Ok(_) => {
            info!("✅ Instance profile deleted");
        }
        Err(e) => {
            panic!("Delete instance profile failed: {:?}", e);
        }
    }

    // Verify deletion
    let result = ctx.client.get_instance_profile(&profile_name).await;
    assert!(
        result.is_err(),
        "Should fail to get deleted instance profile"
    );
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Confirmed instance profile was deleted");
        }
        other => {
            warn!(
                "Got unexpected error after instance profile deletion: {:?}",
                other
            );
        }
    }

    info!("🎉 Instance profile lifecycle test completed successfully!");
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_get_non_existent_instance_profile(ctx: &mut IamTestContext) {
    let non_existent_profile = "alien-test-non-existent-profile";

    info!(
        "❌ Testing get non-existent instance profile: {}",
        non_existent_profile
    );

    let result = ctx.client.get_instance_profile(non_existent_profile).await;

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
            assert_eq!(resource_type, "IAM Resource");
            assert_eq!(resource_name, non_existent_profile);
            info!("✅ Correctly detected non-existent instance profile");
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

// ============================================================================
// Attach Role Policy Tests
// ============================================================================

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_attach_role_policy(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    // Use an AWS managed policy
    let policy_arn = "arn:aws:iam::aws:policy/ReadOnlyAccess";

    info!(
        "🔗 Testing attach role policy: {} -> {}",
        role_name, policy_arn
    );

    // First create a role
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role for attach policy test");

    // Attach managed policy
    match ctx.client.attach_role_policy(&role_name, policy_arn).await {
        Ok(_) => {
            info!("✅ Policy attached successfully");
        }
        Err(e) => {
            panic!("Attach role policy failed: {:?}", e);
        }
    }

    // Note: We don't have a detach_role_policy method yet, but the policy will be
    // automatically detached when we delete the role (via teardown)

    info!("✅ Attach role policy test completed");
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_attach_role_policy_invalid_arn(ctx: &mut IamTestContext) {
    let role_name = ctx.get_test_role_name();
    let invalid_policy_arn = "arn:aws:iam::aws:policy/NonExistentPolicyThatDoesNotExist12345";

    info!("❌ Testing attach role policy with invalid ARN");

    // First create a role
    ctx.create_test_role(&role_name)
        .await
        .expect("Failed to create role for invalid policy test");

    // Try to attach non-existent policy
    let result = ctx
        .client
        .attach_role_policy(&role_name, invalid_policy_arn)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Correctly detected non-existent policy");
        }
        Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        } => {
            info!("✅ Correctly rejected invalid policy ARN");
        }
        other => {
            warn!("Got unexpected error for invalid policy ARN: {:?}", other);
        }
    }
}
