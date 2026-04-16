/*!
# ECR Client Integration Tests

These tests perform real AWS ECR operations including creating repositories, managing policies,
and testing various error conditions.

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
- `ecr:CreateRepository`
- `ecr:DeleteRepository`
- `ecr:DescribeRepositories`
- `ecr:SetRepositoryPolicy`
- `ecr:GetRepositoryPolicy`
- `ecr:GetAuthorizationToken`

## Running Tests
```bash
# Run all ECR tests
cargo test --package alien-aws-clients --test aws_ecr_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_ecr_client_tests test_create_and_delete_repository -- --nocapture
```
*/

use alien_aws_clients::ecr::*;
use alien_aws_clients::AwsClientConfig;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct EcrTestContext {
    client: EcrClient,
    created_repositories: Mutex<HashSet<String>>,
    created_policies: Mutex<HashSet<String>>, // repository names with policies
}

impl AsyncTestContext for EcrTestContext {
    async fn setup() -> EcrTestContext {
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
        let client = EcrClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        EcrTestContext {
            client,
            created_repositories: Mutex::new(HashSet::new()),
            created_policies: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting ECR test cleanup...");

        let policies_to_cleanup = {
            let policies = self.created_policies.lock().unwrap();
            policies.clone()
        };

        let repositories_to_cleanup = {
            let repositories = self.created_repositories.lock().unwrap();
            repositories.clone()
        };

        // First cleanup policies
        for repository_name in policies_to_cleanup {
            self.cleanup_repository_policy(&repository_name).await;
        }

        // Then cleanup repositories with force deletion
        for repository_name in repositories_to_cleanup {
            self.cleanup_repository(&repository_name).await;
        }

        info!("✅ ECR test cleanup completed");
    }
}

impl EcrTestContext {
    fn track_repository(&self, repository_name: &str) {
        let mut repositories = self.created_repositories.lock().unwrap();
        repositories.insert(repository_name.to_string());
        info!("📝 Tracking repository for cleanup: {}", repository_name);
    }

    fn untrack_repository(&self, repository_name: &str) {
        let mut repositories = self.created_repositories.lock().unwrap();
        repositories.remove(repository_name);
        info!(
            "✅ Repository {} successfully cleaned up and untracked",
            repository_name
        );
    }

    fn track_policy(&self, repository_name: &str) {
        let mut policies = self.created_policies.lock().unwrap();
        policies.insert(repository_name.to_string());
        info!("📝 Tracking policy for cleanup: {}", repository_name);
    }

    fn untrack_policy(&self, repository_name: &str) {
        let mut policies = self.created_policies.lock().unwrap();
        policies.remove(repository_name);
        info!(
            "✅ Policy {} successfully cleaned up and untracked",
            repository_name
        );
    }

    async fn cleanup_repository_policy(&self, repository_name: &str) {
        info!("🧹 Cleaning up repository policy: {}", repository_name);

        // Try to delete the policy first (ECR doesn't have a separate delete policy API, but we track this for consistency)
        // Repository policies are deleted when the repository is deleted
        // For now, we just mark it as cleaned up
    }

    async fn cleanup_repository(&self, repository_name: &str) {
        info!("🧹 Cleaning up repository: {}", repository_name);

        let request = DeleteRepositoryRequest::builder()
            .repository_name(repository_name.to_string())
            .force(true) // Force delete even if it contains images
            .build();

        match self.client.delete_repository(request).await {
            Ok(_) => {
                info!("✅ Repository {} deleted successfully", repository_name);
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
                        "Failed to delete repository {} during cleanup: {:?}",
                        repository_name, e
                    );
                }
            }
        }
    }

    fn get_test_repository_name(&self) -> String {
        // ECR repository names must be lowercase and can contain letters, numbers, hyphens, underscores, and forward slashes
        format!(
            "alien-test-repo-{}",
            Uuid::new_v4().simple().to_string().to_lowercase()
        )
    }

    fn get_basic_repository_policy(&self) -> String {
        r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": {
                        "AWS": "*"
                    },
                    "Action": [
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage"
                    ]
                }
            ]
        }"#
        .to_string()
    }

    async fn create_test_repository(
        &self,
        repository_name: &str,
    ) -> Result<CreateRepositoryResponse, Error> {
        let request = CreateRepositoryRequest::builder()
            .repository_name(repository_name.to_string())
            .image_tag_mutability("MUTABLE".to_string())
            .image_scanning_configuration(ImageScanningConfiguration {
                scan_on_push: Some(false),
            })
            .tags(vec![
                Tag {
                    key: "Environment".to_string(),
                    value: "Test".to_string(),
                },
                Tag {
                    key: "Project".to_string(),
                    value: "alien-test".to_string(),
                },
            ])
            .build();

        let result = self.client.create_repository(request).await;
        if result.is_ok() {
            self.track_repository(repository_name);
        }
        result
    }

    async fn create_test_repository_policy(
        &self,
        repository_name: &str,
        policy_text: &str,
    ) -> Result<SetRepositoryPolicyResponse, Error> {
        let request = SetRepositoryPolicyRequest::builder()
            .repository_name(repository_name.to_string())
            .policy_text(policy_text.to_string())
            .force(false)
            .build();

        let result = self.client.set_repository_policy(request).await;
        if result.is_ok() {
            self.track_policy(repository_name);
        }
        result
    }

    async fn manual_cleanup_repository(&self, repository_name: &str) {
        self.cleanup_repository(repository_name).await;
        self.untrack_repository(repository_name);
    }

    async fn manual_cleanup_policy(&self, repository_name: &str) {
        self.cleanup_repository_policy(repository_name).await;
        self.untrack_policy(repository_name);
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_create_and_delete_repository(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "🚀 Testing create and delete repository: {}",
        repository_name
    );

    // Create the repository
    let create_result = ctx.create_test_repository(&repository_name).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Repository created successfully: {}",
                response.repository.repository_arn
            );
            assert_eq!(response.repository.repository_name, repository_name);
            assert!(response
                .repository
                .repository_uri
                .contains(&repository_name));
            assert!(response
                .repository
                .repository_arn
                .contains(&repository_name));
            assert_eq!(
                response.repository.image_tag_mutability,
                Some("MUTABLE".to_string())
            );
        }
        Err(e) => {
            panic!("Repository creation failed: {:?}. Please ensure you have proper AWS credentials and ECR permissions set up in .env.test", e);
        }
    }

    // Manual cleanup - the repository will also be cleaned up automatically via teardown
    ctx.manual_cleanup_repository(&repository_name).await;
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_describe_repositories(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!("🔍 Testing describe repositories: {}", repository_name);

    // First create a repository
    ctx.create_test_repository(&repository_name)
        .await
        .expect("Failed to create repository for describe test");

    // Now describe the specific repository
    let request = DescribeRepositoriesRequest::builder()
        .repository_names(vec![repository_name.clone()])
        .build();

    match ctx.client.describe_repositories(request).await {
        Ok(response) => {
            info!(
                "✅ Repositories described successfully, found {} repositories",
                response.repositories.len()
            );
            assert!(!response.repositories.is_empty());

            // Find our repository
            let our_repo = response
                .repositories
                .iter()
                .find(|repo| repo.repository_name == repository_name)
                .expect("Our repository should be in the response");

            assert_eq!(our_repo.repository_name, repository_name);
            assert!(our_repo.repository_uri.contains(&repository_name));
            assert!(our_repo.repository_arn.contains(&repository_name));
            assert!(our_repo.created_at > 0.0);
        }
        Err(e) => {
            panic!("Describe repositories failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_describe_all_repositories(ctx: &mut EcrTestContext) {
    info!("📋 Testing describe all repositories");

    // Describe all repositories (no filter)
    let request = DescribeRepositoriesRequest::builder().build();

    match ctx.client.describe_repositories(request).await {
        Ok(response) => {
            info!(
                "✅ All repositories described successfully, found {} repositories",
                response.repositories.len()
            );
            // We can't assert much about the count since there might be existing repositories
            // Just verify the structure is correct
            for repo in &response.repositories {
                assert!(!repo.repository_name.is_empty());
                assert!(!repo.repository_uri.is_empty());
                assert!(!repo.repository_arn.is_empty());
                assert!(repo.created_at > 0.0);
            }
        }
        Err(e) => {
            panic!("Describe all repositories failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_describe_non_existent_repository(ctx: &mut EcrTestContext) {
    let non_existent_repo = "alien-test-non-existent-repo-12345";

    info!(
        "❌ Testing describe non-existent repository: {}",
        non_existent_repo
    );

    let request = DescribeRepositoriesRequest::builder()
        .repository_names(vec![non_existent_repo.to_string()])
        .build();

    let result = ctx.client.describe_repositories(request).await;

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
            assert_eq!(resource_type, "ECR Repository");
            assert_eq!(resource_name, non_existent_repo);
            info!("✅ Correctly detected non-existent repository");
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_create_repository_already_exists(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "🔄 Testing create duplicate repository: {}",
        repository_name
    );

    let request = CreateRepositoryRequest::builder()
        .repository_name(repository_name.clone())
        .image_tag_mutability("IMMUTABLE".to_string())
        .build();

    // Create repository first time
    match ctx.client.create_repository(request.clone()).await {
        Ok(_) => {
            info!("✅ First repository creation succeeded");
            ctx.track_repository(&repository_name);

            // Try to create the same repository again
            let result = ctx.client.create_repository(request).await;

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
                    assert_eq!(resource_type, "ECR Repository");
                    assert_eq!(resource_name, repository_name);
                    info!("✅ Correctly detected duplicate repository creation");
                }
                other => {
                    panic!("Expected RemoteResourceConflict, got: {:?}", other);
                }
            }
        }
        Err(e) => {
            panic!("Initial repository creation failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_create_repository_with_encryption(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "🔒 Testing create repository with encryption: {}",
        repository_name
    );

    let request = CreateRepositoryRequest::builder()
        .repository_name(repository_name.clone())
        .image_tag_mutability("IMMUTABLE".to_string())
        .encryption_configuration(EncryptionConfiguration {
            encryption_type: "AES256".to_string(),
            kms_key: None, // Use default AWS managed key
        })
        .build();

    match ctx.client.create_repository(request).await {
        Ok(response) => {
            info!("✅ Repository with encryption created successfully");
            ctx.track_repository(&repository_name);
            assert_eq!(response.repository.repository_name, repository_name);
            assert!(response.repository.encryption_configuration.is_some());
            if let Some(encryption) = response.repository.encryption_configuration {
                assert_eq!(encryption.encryption_type, "AES256");
            }
        }
        Err(e) => {
            panic!("Repository creation with encryption failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_set_and_get_repository_policy(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "📋 Testing set and get repository policy: {}",
        repository_name
    );

    // First create a repository
    ctx.create_test_repository(&repository_name)
        .await
        .expect("Failed to create repository for policy test");

    // Set a policy on the repository
    let policy_document = ctx.get_basic_repository_policy();
    match ctx
        .create_test_repository_policy(&repository_name, &policy_document)
        .await
    {
        Ok(response) => {
            info!("✅ Repository policy set successfully");
            assert_eq!(response.repository_name, repository_name);
            assert!(!response.policy_text.is_empty());

            // Now get the policy
            let get_request = GetRepositoryPolicyRequest::builder()
                .repository_name(repository_name.clone())
                .build();

            match ctx.client.get_repository_policy(get_request).await {
                Ok(get_response) => {
                    info!("✅ Repository policy retrieved successfully");
                    assert_eq!(get_response.repository_name, repository_name);
                    assert!(!get_response.policy_text.is_empty());

                    // The policy should contain the key elements
                    let decoded_policy = urlencoding::decode(&get_response.policy_text)
                        .unwrap_or_else(|_| get_response.policy_text.clone().into());
                    info!("📋 Decoded policy document: {}", decoded_policy);
                    assert!(decoded_policy.contains("ecr:GetDownloadUrlForLayer"));
                    assert!(decoded_policy.contains("2012-10-17"));
                }
                Err(e) => {
                    panic!("Get repository policy failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Set repository policy failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_get_non_existent_repository_policy(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "❌ Testing get policy from repository without policy: {}",
        repository_name
    );

    // Create repository without setting a policy
    ctx.create_test_repository(&repository_name)
        .await
        .expect("Failed to create repository");

    // Try to get the policy (should fail since none is set)
    let request = GetRepositoryPolicyRequest::builder()
        .repository_name(repository_name.clone())
        .build();

    let result = ctx.client.get_repository_policy(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Correctly detected missing repository policy");
        }
        other => {
            warn!("Got unexpected error for missing policy get: {:?}", other);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_set_repository_policy_with_invalid_json(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "📝 Testing set repository policy with invalid JSON: {}",
        repository_name
    );

    // Create repository first
    ctx.create_test_repository(&repository_name)
        .await
        .expect("Failed to create repository");

    // Try to set invalid policy
    let invalid_policy = r#"{"invalid": "json", "missing": "bracket""#; // Malformed JSON

    let request = SetRepositoryPolicyRequest::builder()
        .repository_name(repository_name.clone())
        .policy_text(invalid_policy.to_string())
        .build();

    let result = ctx.client.set_repository_policy(request).await;

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

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_get_authorization_token(ctx: &mut EcrTestContext) {
    info!("🔑 Testing get authorization token");

    let request = GetAuthorizationTokenRequest::builder().build();

    match ctx.client.get_authorization_token(request).await {
        Ok(response) => {
            info!("✅ Authorization token retrieved successfully");
            assert!(!response.authorization_data.is_empty());

            let auth_data = &response.authorization_data[0];
            assert!(!auth_data.authorization_token.is_empty());
            assert!(!auth_data.proxy_endpoint.is_empty());
            assert!(auth_data.expires_at > 0.0);

            // Authorization token should be base64 encoded
            use base64::prelude::*;
            assert!(BASE64_STANDARD
                .decode(&auth_data.authorization_token)
                .is_ok());

            info!("🔑 Token expires at: {}", auth_data.expires_at);
            info!("🔗 Proxy endpoint: {}", auth_data.proxy_endpoint);
        }
        Err(e) => {
            panic!("Get authorization token failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_get_authorization_token_with_registry_ids(ctx: &mut EcrTestContext) {
    info!("🔑 Testing get authorization token with specific registry IDs");

    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());

    let request = GetAuthorizationTokenRequest::builder()
        .registry_ids(vec![account_id.clone()])
        .build();

    match ctx.client.get_authorization_token(request).await {
        Ok(response) => {
            info!("✅ Authorization token with registry ID retrieved successfully");
            assert!(!response.authorization_data.is_empty());

            let auth_data = &response.authorization_data[0];
            assert!(!auth_data.authorization_token.is_empty());
            assert!(!auth_data.proxy_endpoint.is_empty());
            assert!(auth_data.expires_at > 0.0);

            // Proxy endpoint should contain our account ID
            assert!(auth_data.proxy_endpoint.contains(&account_id));
        }
        Err(e) => {
            panic!("Get authorization token with registry IDs failed: {:?}", e);
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_repository_name_validation(ctx: &mut EcrTestContext) {
    info!("📏 Testing repository name validation");

    // Test edge cases for repository names
    let long_name = "a".repeat(257);
    let invalid_repository_names = vec![
        "A",                       // Uppercase not allowed
        "repo with spaces",        // Spaces not allowed
        "repo@with#special$chars", // Special chars beyond allowed set
        "",                        // Empty name
        &long_name,                // Too long (maximum 256 chars)
        "UPPERCASE-REPO",          // Uppercase not allowed
    ];

    for repo_name in invalid_repository_names {
        let request = CreateRepositoryRequest::builder()
            .repository_name(repo_name.to_string())
            .build();

        let result = ctx.client.create_repository(request).await;
        if result.is_ok() {
            // If it somehow succeeded, track it for cleanup
            ctx.track_repository(repo_name);
            warn!(
                "Warning: Repository name '{}' was accepted when it might not be valid",
                repo_name
            );
        } else {
            info!(
                "✅ Correctly rejected invalid repository name: '{}'",
                repo_name
            );
        }
    }
}

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_ecr_client_with_invalid_credentials(ctx: &mut EcrTestContext) {
    let region =
        std::env::var("AWS_MANAGEMENT_REGION").unwrap_or_else(|_| "eu-central-1".to_string());
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let client_invalid = Client::new();

    let aws_config = AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid".to_string(),
            secret_access_key: "invalid".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let ecr_client = EcrClient::new(
        client_invalid,
        AwsCredentialProvider::from_config_sync(aws_config),
    );

    info!("🔐 Testing ECR client with invalid credentials");

    let request = DescribeRepositoriesRequest::builder().build();
    let result = ecr_client.describe_repositories(request).await;

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

#[test_context(EcrTestContext)]
#[tokio::test]
async fn test_delete_repository_with_force(ctx: &mut EcrTestContext) {
    let repository_name = ctx.get_test_repository_name();

    info!(
        "🗑️ Testing delete repository with force flag: {}",
        repository_name
    );

    // Create the repository first
    ctx.create_test_repository(&repository_name)
        .await
        .expect("Failed to create repository");

    // Delete with force (to simulate deleting a repository that might have images)
    let delete_request = DeleteRepositoryRequest::builder()
        .repository_name(repository_name.clone())
        .force(true)
        .build();

    match ctx.client.delete_repository(delete_request).await {
        Ok(response) => {
            info!("✅ Repository deleted with force successfully");
            ctx.untrack_repository(&repository_name); // Untrack since we manually deleted it
            assert_eq!(response.repository.repository_name, repository_name);

            // Verify repository is gone by trying to describe it
            let describe_request = DescribeRepositoriesRequest::builder()
                .repository_names(vec![repository_name.clone()])
                .build();

            let result = ctx.client.describe_repositories(describe_request).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                Error {
                    error: Some(ErrorData::RemoteResourceNotFound { .. }),
                    ..
                } => {
                    info!("✅ Confirmed repository was deleted");
                }
                other => {
                    warn!(
                        "Got unexpected error after repository deletion: {:?}",
                        other
                    );
                }
            }
        }
        Err(e) => {
            panic!("Delete repository with force failed: {:?}", e);
        }
    }
}
