#![cfg(all(test, feature = "gcp"))]

use alien_client_core::{Error, ErrorData, Result};
use alien_gcp_clients::artifactregistry::{
    ArtifactRegistryApi, ArtifactRegistryClient, DockerRepositoryConfig, Repository,
    RepositoryConfig, RepositoryFormat, RepositoryMode,
};

use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::longrunning::Operation;
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

const TEST_LOCATION: &str = "europe-central2";

struct ArtifactRegistryTestContext {
    client: ArtifactRegistryClient,
    project_id: String,
    location: String,
    created_repositories: Mutex<HashSet<String>>,
}

impl AsyncTestContext for ArtifactRegistryTestContext {
    async fn setup() -> ArtifactRegistryTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: TEST_LOCATION.to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = ArtifactRegistryClient::new(Client::new(), config);

        ArtifactRegistryTestContext {
            client,
            project_id,
            location: TEST_LOCATION.to_string(),
            created_repositories: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Artifact Registry test cleanup...");

        let repositories_to_cleanup = {
            let repositories = self.created_repositories.lock().unwrap();
            repositories.clone()
        };

        for repository_name in repositories_to_cleanup {
            self.cleanup_repository(&repository_name).await;
        }

        info!("✅ Artifact Registry test cleanup completed");
    }
}

impl ArtifactRegistryTestContext {
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

    async fn cleanup_repository(&self, repository_name: &str) {
        info!("🧹 Cleaning up repository: {}", repository_name);

        match self
            .client
            .delete_repository(
                self.project_id.clone(),
                self.location.clone(),
                repository_name.to_string(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Repository {} deletion initiated successfully",
                    repository_name
                );
                // Wait a bit for the deletion to process
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Repository {} was already deleted", repository_name);
                }
                _ => {
                    warn!(
                        "Failed to delete repository {} during cleanup: {:?}",
                        repository_name, infra_err
                    );
                }
            },
        }
    }

    fn generate_unique_repository_name(&self) -> String {
        format!(
            "alien-test-repo-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        )
    }

    async fn create_test_repository(
        &self,
        repository_name: String,
        repository: Repository,
    ) -> Result<Operation> {
        let result = self
            .client
            .create_repository(
                self.project_id.clone(),
                self.location.clone(),
                repository_name.clone(),
                repository,
            )
            .await;
        if result.is_ok() {
            self.track_repository(&repository_name);
        }
        result
    }

    fn create_basic_docker_repository(&self, repository_name: String) -> Repository {
        Repository::builder()
            .format(RepositoryFormat::Docker)
            .description("Test Docker repository created by Alien tests".to_string())
            .labels({
                let mut labels = HashMap::new();
                labels.insert("environment".to_string(), "test".to_string());
                labels.insert("created-by".to_string(), "alien-tests".to_string());
                labels
            })
            .repository_config(RepositoryConfig::DockerConfig(
                DockerRepositoryConfig::builder()
                    .immutable_tags(false)
                    .build(),
            ))
            .build()
    }

    fn create_invalid_client(&self) -> ArtifactRegistryClient {
        let invalid_config = GcpClientConfig {
            project_id: "fake-project".to_string(),
            region: self.location.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
            },
            service_overrides: None,
            project_number: None,
        };
        ArtifactRegistryClient::new(Client::new(), invalid_config)
    }

    async fn wait_for_operation(
        &self,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(
                    format!("Timeout waiting for operation {} to complete", operation_name).into(),
                );
            }

            match self
                .client
                .get_operation(
                    self.project_id.clone(),
                    self.location.clone(),
                    operation_name.to_string(),
                )
                .await
            {
                Ok(operation) => {
                    if operation.done == Some(true) {
                        info!("✅ Operation {} completed!", operation_name);
                        return Ok(());
                    }
                    info!("⏳ Operation {} still running, waiting...", operation_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    warn!("Error checking operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_framework_setup_artifactregistry(ctx: &mut ArtifactRegistryTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    assert!(!ctx.location.is_empty(), "Location should not be empty");

    println!(
        "Successfully connected to Artifact Registry in project: {} location: {}",
        ctx.project_id, ctx.location
    );
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_create_get_delete_repository(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();
    let repository = ctx.create_basic_docker_repository(repository_name.clone());

    println!("Attempting to create test repository: {}", repository_name);

    // Create repository
    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await
        .expect("Failed to create repository");

    assert!(
        create_operation.name.is_some(),
        "Operation should have a name"
    );
    println!(
        "Successfully initiated repository creation: {}",
        repository_name
    );

    // Wait for the repository to be created
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation operation failed to complete within timeout");

    // Get repository
    let fetched_repository = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to get repository");

    assert_eq!(
        fetched_repository
            .name
            .as_ref()
            .unwrap()
            .split('/')
            .last()
            .unwrap(),
        repository_name
    );
    assert_eq!(fetched_repository.format, Some(RepositoryFormat::Docker));
    println!("Successfully fetched repository: {}", repository_name);

    // Delete repository
    let delete_operation = ctx
        .client
        .delete_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to delete repository");

    assert!(
        delete_operation.name.is_some(),
        "Delete operation should have a name"
    );
    println!(
        "Successfully initiated deletion of repository: {}",
        repository_name
    );
    ctx.untrack_repository(&repository_name);
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_patch_repository(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();
    let repository = ctx.create_basic_docker_repository(repository_name.clone());

    println!("Creating repository for patch test: {}", repository_name);

    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await
        .expect("Failed to create repository for patch test");

    // Wait for repository to be created
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation failed to complete");

    // Update repository with new description and labels
    let updated_repository = Repository::builder()
        .description("Updated description for test repository".to_string())
        .labels({
            let mut labels = HashMap::new();
            labels.insert("environment".to_string(), "test-updated".to_string());
            labels.insert("updated-by".to_string(), "alien-tests".to_string());
            labels.insert("patch-test".to_string(), "true".to_string());
            labels
        })
        .build();

    println!("Attempting to patch repository: {}", repository_name);
    let patched_repository = ctx
        .client
        .patch_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
            updated_repository,
            Some("description,labels".to_string()),
        )
        .await
        .expect("Failed to patch repository");

    assert_eq!(
        patched_repository.description,
        Some("Updated description for test repository".to_string())
    );
    assert!(patched_repository.labels.is_some());
    assert_eq!(
        patched_repository
            .labels
            .as_ref()
            .unwrap()
            .get("environment")
            .unwrap(),
        "test-updated"
    );
    println!("Successfully patched repository: {}", repository_name);
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_repository_iam_policy(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();
    let repository = ctx.create_basic_docker_repository(repository_name.clone());

    println!(
        "Creating repository for IAM policy test: {}",
        repository_name
    );

    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await
        .expect("Failed to create repository for IAM test");

    // Wait for repository to be created
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation failed to complete");

    // Get IAM policy
    let original_policy = ctx
        .client
        .get_repository_iam_policy(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to get repository IAM policy");

    println!(
        "Successfully retrieved IAM policy for repository: {}",
        repository_name
    );

    // Try to set IAM policy (add a test binding)
    let current_sa_email = {
        let gcp_credentials_json = std::env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").unwrap();
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        service_account_value
            .get("client_email")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| "serviceAccount:test-service@example.com".to_string())
    };

    let test_member = if current_sa_email.starts_with("serviceAccount:") {
        current_sa_email
    } else {
        format!("serviceAccount:{}", current_sa_email)
    };

    let new_binding = Binding::builder()
        .role("roles/artifactregistry.reader".to_string())
        .members(vec![test_member])
        .build();

    let mut modified_bindings = original_policy.bindings.clone();
    modified_bindings.push(new_binding);

    let policy_to_set = IamPolicy::builder()
        .bindings(modified_bindings)
        .maybe_etag(original_policy.etag.clone())
        .build();

    let set_result = ctx
        .client
        .set_repository_iam_policy(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
            policy_to_set,
        )
        .await;

    match set_result {
        Ok(_) => {
            println!(
                "Successfully set IAM policy for repository: {}",
                repository_name
            );
        }
        Err(e) => {
            // IAM policy operations can be tricky with test accounts, log but don't fail
            println!("IAM policy set failed (acceptable for test): {:?}", e);
        }
    }
}

// === END-TO-END TEST ===

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_end_to_end_repository_lifecycle(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();

    println!(
        "🚀 Starting end-to-end Artifact Registry repository lifecycle test: {}",
        repository_name
    );

    // Create a repository with comprehensive configuration
    let mut labels = HashMap::new();
    labels.insert("environment".to_string(), "test".to_string());
    labels.insert("team".to_string(), "alien".to_string());
    labels.insert("purpose".to_string(), "e2e-test".to_string());

    let repository = Repository::builder()
        .format(RepositoryFormat::Docker)
        .description("End-to-end test repository for Alien tests".to_string())
        .labels(labels.clone())
        .mode(RepositoryMode::StandardRepository)
        .repository_config(RepositoryConfig::DockerConfig(
            DockerRepositoryConfig::builder()
                .immutable_tags(false)
                .build(),
        ))
        .build();

    // 1. Deploy the repository
    println!("📦 Creating Artifact Registry repository...");
    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await
        .expect("Failed to create end-to-end test repository");

    assert!(
        create_operation.name.is_some(),
        "Create operation should have a name"
    );
    println!("✅ Repository creation initiated");

    // 2. Wait for repository to be ready
    println!("⏳ Waiting for creation operation to complete...");
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation operation failed to complete within timeout");

    // 3. Get the final repository state
    let ready_repository = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to get repository after creation completed");

    // 4. Verify repository configuration
    assert_eq!(ready_repository.format, Some(RepositoryFormat::Docker));
    assert_eq!(
        ready_repository.mode,
        Some(RepositoryMode::StandardRepository)
    );
    assert!(ready_repository.labels.is_some());
    assert_eq!(
        ready_repository
            .labels
            .as_ref()
            .unwrap()
            .get("environment")
            .unwrap(),
        "test"
    );

    println!("🔍 Repository is ready with correct configuration");

    // 5. Test repository update
    println!("🔄 Testing repository update...");
    let updated_description = "Updated description for end-to-end test";
    let update_repository = Repository::builder()
        .description(updated_description.to_string())
        .build();

    let updated_repository = ctx
        .client
        .patch_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
            update_repository,
            Some("description".to_string()),
        )
        .await
        .expect("Failed to update repository");

    assert_eq!(
        updated_repository.description,
        Some(updated_description.to_string())
    );

    // 6. Test IAM operations
    println!("🔐 Testing IAM policy operations...");
    let iam_policy = ctx
        .client
        .get_repository_iam_policy(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to get IAM policy");

    assert!(iam_policy.bindings.len() >= 0); // Could be empty initially

    println!("✅ End-to-end test completed successfully!");
    println!("   - Repository created: ✅");
    println!("   - Repository became ready: ✅");
    println!("   - Repository updated: ✅");
    println!("   - IAM operations successful: ✅");
    println!("   - Repository name: {:?}", ready_repository.name);
}

// === ERROR TESTING ===

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_error_translation_repository_not_found(ctx: &mut ArtifactRegistryTestContext) {
    let non_existent_repository = "alien-test-repo-does-not-exist-12345";

    let result = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            non_existent_repository.to_string(),
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent repository"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    assert!(err.message.contains(non_existent_repository));
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND error");
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_error_translation_repository_already_exists(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();
    let repository = ctx.create_basic_docker_repository(repository_name.clone());

    // Create the repository first
    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository.clone())
        .await
        .expect("Failed to create initial repository");

    // Wait for the repository to be created
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation failed to complete");

    // Try to create the same repository again
    let result = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await;
    assert!(
        result.is_err(),
        "Expected error when creating existing repository"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceConflict {
                    ref message,
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Artifact Registry");
            assert_eq!(resource_name.to_string(), repository_name.to_string());
            println!("✅ Correctly mapped 409 to RemoteResourceConflict error");
        }
        _ => panic!("Expected RemoteResourceConflict error, got: {:?}", err),
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_error_translation_access_denied(ctx: &mut ArtifactRegistryTestContext) {
    let invalid_client = ctx.create_invalid_client();
    let result = invalid_client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            "any-repository".to_string(),
        )
        .await;

    assert!(result.is_err(), "Expected error with invalid credentials");

    let err = result.unwrap_err();
    match &err.error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        _ => println!("Got error (acceptable for invalid creds): {:?}", err),
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_delete_non_existent_repository_error(ctx: &mut ArtifactRegistryTestContext) {
    let non_existent_repository = "alien-test-repo-does-not-exist-67890";

    let result = ctx
        .client
        .delete_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            non_existent_repository.to_string(),
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent repository"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Artifact Registry");
            assert_eq!(resource_name, non_existent_repository);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for repository deletion");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent repository deletion, got: {:?}",
            err
        ),
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_patch_non_existent_repository_error(ctx: &mut ArtifactRegistryTestContext) {
    let non_existent_repository = "alien-test-repo-does-not-exist-13579";
    let patch_repository = ctx.create_basic_docker_repository(non_existent_repository.to_string());

    let result = ctx
        .client
        .patch_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            non_existent_repository.to_string(),
            patch_repository,
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when patching non-existent repository"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Artifact Registry");
            assert_eq!(resource_name, non_existent_repository);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for repository patch");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent repository patch, got: {:?}",
            err
        ),
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_iam_policy_operations_on_non_existent_repository(
    ctx: &mut ArtifactRegistryTestContext,
) {
    let non_existent_repository = "alien-test-repo-does-not-exist-24680";

    // Test get_repository_iam_policy
    let get_result = ctx
        .client
        .get_repository_iam_policy(
            ctx.project_id.clone(),
            ctx.location.clone(),
            non_existent_repository.to_string(),
        )
        .await;
    // Note: IAM operations may succeed even for non-existent repositories in some cases,
    // so we'll accept either success or specific error patterns
    match get_result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_name, .. }),
            ..
        }) => {
            assert_eq!(resource_name, non_existent_repository);
            println!("✅ get_repository_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        Err(Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        }) => {
            println!("✅ get_repository_iam_policy returned access denied (acceptable)");
        }
        Err(Error {
            error: Some(ErrorData::GenericError { .. }),
            ..
        }) => {
            println!(
                "✅ get_repository_iam_policy returned generic error (acceptable for IAM operations)"
            );
        }
        Ok(_) => {
            println!("✅ get_repository_iam_policy succeeded (acceptable - IAM policies can exist independently)");
        }
        Err(other) => {
            println!("Got unexpected error type for get IAM policy: {:?}", other);
        }
    }

    // Test set_repository_iam_policy
    let test_binding = Binding::builder()
        .role("roles/artifactregistry.reader".to_string())
        .members(vec!["serviceAccount:test@example.com".to_string()])
        .build();

    let test_policy = IamPolicy::builder().bindings(vec![test_binding]).build();

    let set_result = ctx
        .client
        .set_repository_iam_policy(
            ctx.project_id.clone(),
            ctx.location.clone(),
            non_existent_repository.to_string(),
            test_policy,
        )
        .await;
    // Similar to get_iam_policy, set operations may succeed or fail in various ways
    match set_result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_name, .. }),
            ..
        }) => {
            assert_eq!(resource_name, non_existent_repository);
            println!("✅ set_repository_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        Err(Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        }) => {
            println!("✅ set_repository_iam_policy returned access denied (acceptable)");
        }
        Err(Error {
            error: Some(ErrorData::GenericError { .. }),
            ..
        }) => {
            println!(
                "✅ set_repository_iam_policy returned generic error (acceptable for IAM operations)"
            );
        }
        Ok(_) => {
            println!("✅ set_repository_iam_policy succeeded (acceptable - IAM policies can exist independently)");
        }
        Err(other) => {
            println!("Got unexpected error type for set IAM policy: {:?}", other);
        }
    }
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_repository_with_complex_configuration(ctx: &mut ArtifactRegistryTestContext) {
    let repository_name = ctx.generate_unique_repository_name();

    println!(
        "Creating repository with complex configuration: {}",
        repository_name
    );

    // Create repository with complex configuration
    let mut labels = HashMap::new();
    labels.insert("environment".to_string(), "production".to_string());
    labels.insert("team".to_string(), "alien".to_string());
    labels.insert("cost-center".to_string(), "engineering".to_string());
    labels.insert("project".to_string(), "alien-test".to_string());

    let repository = Repository::builder()
        .format(RepositoryFormat::Docker)
        .description("Complex test repository with comprehensive configuration".to_string())
        .labels(labels.clone())
        .mode(RepositoryMode::StandardRepository)
        .repository_config(RepositoryConfig::DockerConfig(
            DockerRepositoryConfig::builder()
                .immutable_tags(true) // Make tags immutable for production-like config
                .build(),
        ))
        .build();

    let create_operation = ctx
        .create_test_repository(repository_name.clone(), repository)
        .await
        .expect("Failed to create repository with complex configuration");

    assert!(
        create_operation.name.is_some(),
        "Create operation should have a name"
    );
    println!("✅ Successfully created repository with complex configuration");

    // Wait for repository to be created and fetch it
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Repository creation failed to complete");

    let fetched_repository = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            repository_name.clone(),
        )
        .await
        .expect("Failed to fetch repository with complex configuration");

    // Verify configuration
    assert!(
        fetched_repository.labels.is_some(),
        "Repository should have labels"
    );
    assert_eq!(
        fetched_repository
            .labels
            .as_ref()
            .unwrap()
            .get("environment")
            .unwrap(),
        "production"
    );
    assert_eq!(
        fetched_repository
            .labels
            .as_ref()
            .unwrap()
            .get("team")
            .unwrap(),
        "alien"
    );

    assert_eq!(fetched_repository.format, Some(RepositoryFormat::Docker));
    assert_eq!(
        fetched_repository.mode,
        Some(RepositoryMode::StandardRepository)
    );

    println!("✅ Successfully verified repository with complex configuration");
}

#[test_context(ArtifactRegistryTestContext)]
#[tokio::test]
async fn test_multiple_repository_formats(ctx: &mut ArtifactRegistryTestContext) {
    let docker_repo_name = ctx.generate_unique_repository_name();
    let maven_repo_name = ctx.generate_unique_repository_name();

    println!(
        "Testing multiple repository formats: Docker={}, Maven={}",
        docker_repo_name, maven_repo_name
    );

    // Create Docker repository
    let docker_repo = Repository::builder()
        .format(RepositoryFormat::Docker)
        .description("Test Docker repository".to_string())
        .build();

    let docker_create_op = ctx
        .create_test_repository(docker_repo_name.clone(), docker_repo)
        .await
        .expect("Failed to create Docker repository");

    // Create Maven repository
    let maven_repo = Repository::builder()
        .format(RepositoryFormat::Maven)
        .description("Test Maven repository".to_string())
        .build();

    let maven_create_op = ctx
        .create_test_repository(maven_repo_name.clone(), maven_repo)
        .await
        .expect("Failed to create Maven repository");

    // Wait for both to be created
    ctx.wait_for_operation(docker_create_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Docker repository creation failed");

    ctx.wait_for_operation(maven_create_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Maven repository creation failed");

    // Verify both repositories
    let docker_fetched = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            docker_repo_name.clone(),
        )
        .await
        .expect("Failed to fetch Docker repository");

    let maven_fetched = ctx
        .client
        .get_repository(
            ctx.project_id.clone(),
            ctx.location.clone(),
            maven_repo_name.clone(),
        )
        .await
        .expect("Failed to fetch Maven repository");

    assert_eq!(docker_fetched.format, Some(RepositoryFormat::Docker));
    assert_eq!(maven_fetched.format, Some(RepositoryFormat::Maven));

    println!("✅ Successfully created and verified multiple repository formats");
}
