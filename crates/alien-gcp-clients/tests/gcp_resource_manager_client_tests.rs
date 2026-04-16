#![cfg(all(test, feature = "gcp"))]
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use alien_gcp_clients::iam::{
    Binding, CreateServiceAccountRequest, IamApi, IamClient, IamPolicy, ServiceAccount,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::resource_manager::{
    GetPolicyOptions, ResourceManagerApi, ResourceManagerClient,
};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use test_context::{test_context, AsyncTestContext};
use tracing::info;

struct ResourceManagerTestContext {
    rm_client: ResourceManagerClient,
    iam_client: IamClient,
    project_id: String,
    original_policy: Option<IamPolicy>,
    test_service_account_email: Option<String>,
}

impl AsyncTestContext for ResourceManagerTestContext {
    async fn setup() -> ResourceManagerTestContext {
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
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let rm_client = ResourceManagerClient::new(Client::new(), config.clone());
        let iam_client = IamClient::new(Client::new(), config);

        // Store original policy for restoration in teardown
        let original_policy = rm_client
            .get_project_iam_policy(project_id.clone(), None)
            .await
            .ok();

        ResourceManagerTestContext {
            rm_client,
            iam_client,
            project_id,
            original_policy,
            test_service_account_email: None,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Resource Manager test cleanup...");

        // Clean up test service account if created
        if let Some(sa_email) = &self.test_service_account_email {
            info!("🗑️ Cleaning up test service account: {}", sa_email);
            match self
                .iam_client
                .delete_service_account(sa_email.clone())
                .await
            {
                Ok(_) => info!("✅ Test service account deleted successfully"),
                Err(e) => info!(
                    "⚠️ Failed to delete test service account (may not exist): {:?}",
                    e
                ),
            }
        }

        // Restore original policy if we have one
        if let Some(original_policy) = self.original_policy {
            info!("🔄 Restoring original project IAM policy");
            match self
                .rm_client
                .set_project_iam_policy(self.project_id.clone(), original_policy, None)
                .await
            {
                Ok(_) => info!("✅ Original project IAM policy restored successfully"),
                Err(e) => info!("⚠️ Failed to restore original policy: {:?}", e),
            }
        }

        info!("✅ Resource Manager test cleanup completed");
    }
}

impl ResourceManagerTestContext {
    /// Create a client with invalid credentials for error testing
    fn create_invalid_rm_client(&self) -> ResourceManagerClient {
        let invalid_config = GcpClientConfig {
                project_id: self.project_id.clone(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey {
                    json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
                },
                service_overrides: None,
            project_number: None,
            };
        ResourceManagerClient::new(Client::new(), invalid_config)
    }

    /// Wait for a service account to be available for use in IAM policies
    async fn wait_for_service_account_availability(&self, sa_email: &str) -> Result<()> {
        const MAX_RETRIES: u32 = 30; // 30 retries
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2); // 2 seconds between retries

        for attempt in 1..=MAX_RETRIES {
            // Try to get the service account - this confirms it exists and is readable
            match self
                .iam_client
                .get_service_account(sa_email.to_string())
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Service account {} is available after {} attempts",
                        sa_email, attempt
                    );
                    return Ok(());
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        return Err(e).context(ErrorData::GenericError {
                            message: format!(
                                "Service account {} still not available after {} attempts",
                                sa_email, MAX_RETRIES
                            ),
                        });
                    }
                    info!(
                        "⏳ Service account {} not yet available (attempt {}), retrying in {:?}...",
                        sa_email, attempt, RETRY_DELAY
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }

        unreachable!()
    }

    /// Create a test service account for testing
    async fn create_test_service_account(&mut self, account_id: &str) -> Result<String> {
        // Add timestamp to avoid conflicts from previous test runs
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let unique_account_id = format!("{}-{}", account_id, timestamp);

        let service_account = ServiceAccount::builder()
            .display_name(format!("Test Service Account {}", unique_account_id))
            .description(
                "Service account created for Resource Manager integration tests".to_string(),
            )
            .build();

        let request = CreateServiceAccountRequest::builder()
            .service_account(service_account)
            .build();

        let created_sa = self
            .iam_client
            .create_service_account(unique_account_id.clone(), request)
            .await
            .context(ErrorData::GenericError {
                message: format!(
                    "Failed to create test service account '{}'",
                    unique_account_id
                ),
            })?;

        let sa_email = created_sa.email.clone().unwrap();
        self.test_service_account_email = Some(sa_email.clone());

        info!("✅ Created test service account: {}", sa_email);

        // Wait for the service account to be available for use
        self.wait_for_service_account_availability(&sa_email)
            .await?;

        Ok(sa_email)
    }
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_framework_setup_resource_manager(ctx: &mut ResourceManagerTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    println!(
        "Successfully connected to Resource Manager and IAM with project: {}",
        ctx.project_id
    );
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_comprehensive_iam_policy_operations(ctx: &mut ResourceManagerTestContext) {
    println!("Testing comprehensive IAM policy operations (Resource Manager + IAM integration)");

    // Create a test service account using the IAM client
    let test_sa_email = ctx
        .create_test_service_account("rm-test-sa")
        .await
        .expect("Failed to create test service account");

    println!("Created test service account: {}", test_sa_email);

    // === Test 1: Project IAM Policy with Service Account ===
    println!("📋 Testing project IAM policy operations...");

    // Get current project policy
    let current_policy = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get current policy");

    println!(
        "Current policy has {} bindings",
        current_policy.bindings.len()
    );

    // Set the updated policy using Resource Manager client with manual retry for ETag conflicts
    let result_policy = {
        let max_attempts = 5;
        let base_delay = std::time::Duration::from_millis(100);

        let mut result_policy = None;
        for attempt in 1..=max_attempts {
            // Re-fetch current policy and apply our changes for each retry attempt
            let fresh_policy = ctx
                .rm_client
                .get_project_iam_policy(ctx.project_id.clone(), None)
                .await
                .expect("Failed to get current policy");

            // Create a binding for the service account with a safe role
            let test_binding = Binding::builder()
                .role("roles/browser".to_string()) // Browser role is read-only and safe
                .members(vec![format!("serviceAccount:{}", test_sa_email)])
                .build();

            // Add the test binding to the fresh policy
            let mut updated_bindings = fresh_policy.bindings.clone();
            updated_bindings.push(test_binding);

            let updated_policy = IamPolicy::builder()
                .bindings(updated_bindings)
                .maybe_version(fresh_policy.version)
                .maybe_etag(fresh_policy.etag.clone())
                .build();

            match ctx
                .rm_client
                .set_project_iam_policy(ctx.project_id.clone(), updated_policy, None)
                .await
            {
                Ok(policy) => {
                    if attempt > 1 {
                        info!(
                            "✅ Set updated project IAM policy succeeded on attempt {}",
                            attempt
                        );
                    }
                    result_policy = Some(policy);
                    break;
                }
                Err(err)
                    if matches!(&err.error, Some(ErrorData::RemoteResourceConflict { .. }))
                        && attempt < max_attempts =>
                {
                    let delay = base_delay * (2_u32.pow(attempt - 1));
                    info!(
                        "⏳ Set updated project IAM policy failed with ETag conflict, retrying in {:?} (attempt {}/{})",
                        delay, attempt, max_attempts
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    // Return the error (either it's not an ETag conflict, or we've exhausted attempts)
                    panic!("Failed to set updated project IAM policy: {:?}", e);
                }
            }
        }

        result_policy.expect("Should have succeeded within retry attempts")
    };

    println!(
        "Updated policy has {} bindings",
        result_policy.bindings.len()
    );

    // Verify the test binding was added
    let test_binding_exists = result_policy.bindings.iter().any(|b| {
        b.role == "roles/browser"
            && b.members
                .contains(&format!("serviceAccount:{}", test_sa_email))
    });

    assert!(
        test_binding_exists,
        "Test binding should exist in the updated policy"
    );

    // Verify we can get the service account's details using IAM client
    let retrieved_sa = ctx
        .iam_client
        .get_service_account(test_sa_email.clone())
        .await
        .expect("Failed to retrieve service account");

    assert_eq!(retrieved_sa.email.unwrap(), test_sa_email);
    println!("✅ Successfully granted project permissions to service account");

    // === Test 2: Update Mask Functionality ===
    println!("📋 Testing update mask functionality...");

    // Test setting policy with specific update mask
    let mask_test_policy = IamPolicy::builder()
        .bindings(result_policy.bindings.clone()) // Keep current bindings
        .maybe_etag(result_policy.etag.clone())
        .build();

    let mask_result = ctx
        .rm_client
        .set_project_iam_policy(
            ctx.project_id.clone(),
            mask_test_policy,
            Some("bindings".to_string()),
        )
        .await
        .expect("Failed to set policy with update mask");

    assert!(
        !mask_result.bindings.is_empty(),
        "Policy should still have bindings"
    );
    println!("✅ Successfully tested update mask functionality");

    // === Test 3: Service Account IAM Policy ===
    println!("📋 Testing service account IAM policy operations...");

    // Get the service account's IAM policy using IAM client
    let sa_policy = ctx
        .iam_client
        .get_service_account_iam_policy(test_sa_email.clone())
        .await
        .expect("Failed to get service account IAM policy");

    println!(
        "Service account IAM policy has {} bindings",
        sa_policy.bindings.len()
    );

    // Create a binding that allows the service account to be used by itself (for testing)
    let admin_binding = Binding::builder()
        .role("roles/iam.serviceAccountUser".to_string())
        .members(vec![format!("serviceAccount:{}", test_sa_email)]) // Self-reference for testing
        .build();

    let updated_sa_policy = IamPolicy::builder()
        .bindings(vec![admin_binding])
        .maybe_etag(sa_policy.etag)
        .build();

    // Set the service account's IAM policy using IAM client
    let result_sa_policy = ctx
        .iam_client
        .set_service_account_iam_policy(test_sa_email.clone(), updated_sa_policy)
        .await
        .expect("Failed to set service account IAM policy");

    // Verify the policy was updated
    assert!(result_sa_policy.bindings.len() > 0);
    assert!(result_sa_policy
        .bindings
        .iter()
        .any(|b| b.role == "roles/iam.serviceAccountUser"));

    println!("✅ Successfully updated service account IAM policy");

    // === Cleanup: Restore Original Project Policy (with ETag handling) ===
    println!("🧹 Cleaning up: restoring original project policy...");

    // Use fresh policy ETag for cleanup to avoid ETag conflicts
    let fresh_policy_for_restore = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get fresh policy for restore");

    let restore_policy = IamPolicy::builder()
        .bindings(current_policy.bindings.clone())
        .maybe_version(current_policy.version)
        .maybe_etag(fresh_policy_for_restore.etag) // Use fresh ETag
        .build();

    match ctx
        .rm_client
        .set_project_iam_policy(ctx.project_id.clone(), restore_policy, None)
        .await
    {
        Ok(_) => {
            println!("✅ Successfully restored original project IAM policy");
        }
        Err(e) => {
            // Log but don't fail the test during cleanup
            println!(
                "⚠️ Failed to restore original policy (this is acceptable): {:?}",
                e
            );
        }
    }

    println!("✅ Comprehensive IAM policy operations test completed successfully");
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_get_project_iam_policy_readonly(ctx: &mut ResourceManagerTestContext) {
    println!("Testing get_project_iam_policy (read-only)");

    let policy = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get project IAM policy");

    // Basic validation of the returned policy
    assert!(
        policy.bindings.len() > 0,
        "Project should have at least one IAM binding"
    );

    // Check that we have some expected default bindings
    let has_owner_binding = policy
        .bindings
        .iter()
        .any(|b| b.role.contains("roles/owner") || b.role.contains("roles/editor"));
    assert!(
        has_owner_binding,
        "Project should have owner or editor bindings"
    );

    println!(
        "✅ Successfully retrieved project IAM policy with {} bindings",
        policy.bindings.len()
    );

    // Test some bindings have members
    let bindings_with_members = policy
        .bindings
        .iter()
        .filter(|b| !b.members.is_empty())
        .count();
    assert!(
        bindings_with_members > 0,
        "At least some bindings should have members"
    );

    println!("✅ Policy validation completed successfully");
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_get_project_iam_policy_with_options_readonly(ctx: &mut ResourceManagerTestContext) {
    println!("Testing get_project_iam_policy with policy options (read-only)");

    let options = GetPolicyOptions::builder()
        .requested_policy_version(3) // Request version 3 to support conditional bindings
        .build();

    let policy = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), Some(options))
        .await
        .expect("Failed to get project IAM policy with options");

    // With version 3, we should get a policy that supports conditions
    assert!(
        policy.bindings.len() > 0,
        "Project should have IAM bindings"
    );

    // If the policy has version set, it should be 3 or compatible
    if let Some(version) = policy.version {
        assert!(version >= 1, "Policy version should be at least 1");
    }

    println!(
        "✅ Successfully retrieved project IAM policy with options, {} bindings",
        policy.bindings.len()
    );
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_policy_etag_handling_readonly(ctx: &mut ResourceManagerTestContext) {
    println!("Testing proper ETag handling in IAM policy operations (read-only)");

    // Get current policy
    let policy1 = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get policy");

    assert!(policy1.etag.is_some(), "Policy should have an ETag");

    // Get policy again - ETag might be the same or different
    let policy2 = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get policy again");

    assert!(
        policy2.etag.is_some(),
        "Second policy should also have an ETag"
    );

    println!("✅ Both policies have ETags");
    println!("Policy 1 ETag: {:?}", policy1.etag);
    println!("Policy 2 ETag: {:?}", policy2.etag);
    println!("✅ ETag handling validation completed");
}

// === ERROR TESTING ===

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_error_invalid_project_id(ctx: &mut ResourceManagerTestContext) {
    let invalid_project_id = "non-existent-project-12345";

    let result = ctx
        .rm_client
        .get_project_iam_policy(invalid_project_id.to_string(), None)
        .await;

    assert!(result.is_err(), "Expected error for invalid project ID");

    match result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "Cloud Resource ManagerResource");
            assert_eq!(resource_name, invalid_project_id);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound error for invalid project");
        }
        Some(ErrorData::RemoteAccessDenied { .. }) => {
            println!("✅ Got access denied error for invalid project (also acceptable)");
        }
        other => println!(
            "Got error for invalid project (may be acceptable): {:?}",
            other
        ),
    }
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_error_access_denied(ctx: &mut ResourceManagerTestContext) {
    let invalid_client = ctx.create_invalid_rm_client();

    let result = invalid_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await;

    assert!(result.is_err(), "Expected error with invalid credentials");

    match result.unwrap_err().error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        other => println!("Got error (acceptable for invalid creds): {:?}", other),
    }
}

#[test_context(ResourceManagerTestContext)]
#[tokio::test]
async fn test_error_set_policy_with_invalid_binding(ctx: &mut ResourceManagerTestContext) {
    println!("Testing set_project_iam_policy with invalid binding");

    // Get current policy (read-only)
    let current_policy = ctx
        .rm_client
        .get_project_iam_policy(ctx.project_id.clone(), None)
        .await
        .expect("Failed to get current policy");

    // Create an invalid binding with a non-existent role
    let invalid_binding = Binding::builder()
        .role("roles/nonexistent.invalidrole".to_string())
        .members(vec!["user:test@example.com".to_string()])
        .build();

    let mut invalid_bindings = current_policy.bindings.clone();
    invalid_bindings.push(invalid_binding);

    let invalid_policy = IamPolicy::builder()
        .bindings(invalid_bindings)
        .maybe_etag(current_policy.etag)
        .build();

    let result = ctx
        .rm_client
        .set_project_iam_policy(ctx.project_id.clone(), invalid_policy, None)
        .await;

    assert!(
        result.is_err(),
        "Expected error for invalid role in binding"
    );

    // The exact error type may vary, but it should be some kind of error
    match result.unwrap_err().error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::InvalidInput { .. })
        | Some(ErrorData::HttpResponseError { .. }) => {
            println!("✅ Got expected error type for invalid binding");
        }
        other => println!(
            "Got error for invalid binding (may be acceptable): {:?}",
            other
        ),
    }
}
