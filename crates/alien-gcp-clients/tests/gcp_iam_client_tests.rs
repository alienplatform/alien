#![cfg(all(test, feature = "gcp"))]
use alien_client_core::{ErrorData, Result};
use alien_gcp_clients::iam::{
    Binding, CreateRoleRequest, CreateServiceAccountRequest, Expr, IamApi, IamClient, IamPolicy,
    Role, RoleLaunchStage, ServiceAccount,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

struct IamTestContext {
    client: IamClient,
    project_id: String,
    created_service_accounts: Mutex<HashSet<String>>,
    created_roles: Mutex<HashSet<String>>,
}

impl AsyncTestContext for IamTestContext {
    async fn setup() -> IamTestContext {
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
        };

        let client = IamClient::new(Client::new(), config);

        IamTestContext {
            client,
            project_id,
            created_service_accounts: Mutex::new(HashSet::new()),
            created_roles: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting IAM test cleanup...");

        // Cleanup service accounts
        let service_accounts_to_cleanup = {
            let accounts = self.created_service_accounts.lock().unwrap();
            accounts.clone()
        };

        for account_name in service_accounts_to_cleanup {
            self.cleanup_service_account(&account_name).await;
        }

        // Cleanup roles
        let roles_to_cleanup = {
            let roles = self.created_roles.lock().unwrap();
            roles.clone()
        };

        for role_name in roles_to_cleanup {
            self.cleanup_role(&role_name).await;
        }

        info!("✅ IAM test cleanup completed");
    }
}

impl IamTestContext {
    fn track_service_account(&self, account_name: &str) {
        let mut accounts = self.created_service_accounts.lock().unwrap();
        accounts.insert(account_name.to_string());
        info!("📝 Tracking service account for cleanup: {}", account_name);
    }

    fn untrack_service_account(&self, account_name: &str) {
        let mut accounts = self.created_service_accounts.lock().unwrap();
        accounts.remove(account_name);
        info!(
            "✅ Service account {} successfully cleaned up and untracked",
            account_name
        );
    }

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

    async fn cleanup_service_account(&self, account_name: &str) {
        info!("🧹 Cleaning up service account: {}", account_name);

        match self
            .client
            .delete_service_account(account_name.to_string())
            .await
        {
            Ok(_) => {
                info!("✅ Service account {} deleted successfully", account_name);
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Service account {} was already deleted", account_name);
                }
                _ => {
                    warn!(
                        "Failed to delete service account {} during cleanup: {:?}",
                        account_name, infra_err
                    );
                }
            },
        }
    }

    async fn cleanup_role(&self, role_name: &str) {
        info!("🧹 Cleaning up role: {}", role_name);

        match self.client.delete_role(role_name.to_string()).await {
            Ok(_) => {
                info!("✅ Role {} deleted successfully", role_name);
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Role {} was already deleted", role_name);
                }
                _ => {
                    warn!(
                        "Failed to delete role {} during cleanup: {:?}",
                        role_name, infra_err
                    );
                }
            },
        }
    }

    fn generate_unique_service_account_id(&self) -> String {
        format!(
            "alien-test-sa-{}",
            Uuid::new_v4().hyphenated().to_string()[0..8].to_string()
        )
    }

    fn generate_unique_role_id(&self) -> String {
        format!(
            "alien_test_role_{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[0..8].to_string()
        )
    }

    async fn create_test_service_account(
        &self,
        account_id: &str,
        service_account: CreateServiceAccountRequest,
    ) -> Result<ServiceAccount> {
        let result = self
            .client
            .create_service_account(account_id.to_string(), service_account)
            .await;
        if result.is_ok() {
            // For service accounts, the name format is projects/{project}/serviceAccounts/{email}
            // We need to track the full name for deletion
            if let Ok(ref sa) = result {
                if let Some(ref email) = sa.email {
                    self.track_service_account(email);
                }
            }
        }
        result
    }

    async fn create_test_role(&self, role_id: &str, role: CreateRoleRequest) -> Result<Role> {
        let result = self.client.create_role(role_id.to_string(), role).await;
        if result.is_ok() {
            self.track_role(role_id);
        }
        result
    }

    /// Create a client with invalid credentials for error testing
    fn create_invalid_client(&self) -> IamClient {
        let invalid_config = GcpClientConfig {
                project_id: self.project_id.clone(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey {
                    json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
                },
                service_overrides: None,
            };
        IamClient::new(Client::new(), invalid_config)
    }

    async fn wait_for_service_account_availability(&self, sa_email: &str) -> Result<()> {
        let max_attempts = 15; // Increased attempts for better reliability
        let base_delay = Duration::from_millis(500);

        for attempt in 1..=max_attempts {
            match self.client.get_service_account(sa_email.to_string()).await {
                Ok(_) => {
                    info!("✅ Service account {} is now available", sa_email);
                    // Add a small buffer delay to account for eventual consistency
                    // Even after a successful read, GCP IAM might still have consistency issues
                    let buffer_delay = Duration::from_millis(1000);
                    info!(
                        "⏳ Adding {}ms buffer for eventual consistency",
                        buffer_delay.as_millis()
                    );
                    sleep(buffer_delay).await;
                    return Ok(());
                }
                Err(err)
                    if matches!(&err.error, Some(ErrorData::RemoteResourceNotFound { .. })) =>
                {
                    if attempt < max_attempts {
                        let delay = base_delay * (2_u32.pow((attempt - 1).min(6))); // Cap exponential growth
                        info!(
                            "⏳ Service account {} not yet available, waiting {:?} (attempt {}/{})",
                            sa_email, delay, attempt, max_attempts
                        );
                        sleep(delay).await;
                    } else {
                        return Err(alien_error::AlienError::new(ErrorData::GenericError {
                            message: format!(
                                "Service account {} did not become available after {} attempts",
                                sa_email, max_attempts
                            ),
                        }));
                    }
                }
                Err(e) => {
                    // Some other error occurred
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// Retry a GCP IAM operation with exponential backoff to handle eventual consistency
    async fn retry_gcp_operation<T, F, Fut>(&self, operation: F, operation_name: &str) -> Result<T>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T>> + Send,
    {
        let max_attempts = 5;
        let base_delay = Duration::from_millis(200);

        for attempt in 1..=max_attempts {
            match operation().await {
                Ok(result) => {
                    if attempt > 1 {
                        info!("✅ {} succeeded on attempt {}", operation_name, attempt);
                    }
                    return Ok(result);
                }
                Err(err)
                    if matches!(&err.error, Some(ErrorData::RemoteResourceNotFound { .. }))
                        && attempt < max_attempts =>
                {
                    let delay = base_delay * (2_u32.pow(attempt - 1));
                    info!(
                        "⏳ {} failed with 404, retrying in {:?} (attempt {}/{})",
                        operation_name, delay, attempt, max_attempts
                    );
                    sleep(delay).await;
                }
                Err(e) => {
                    // Return the error (either it's not a 404, or we've exhausted attempts)
                    return Err(e);
                }
            }
        }

        unreachable!("Loop should have returned by now")
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_framework_setup_iam(ctx: &mut IamTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    println!(
        "Successfully connected to IAM with project: {}",
        ctx.project_id
    );
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_create_get_delete_service_account(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    println!("Attempting to create test service account: {}", account_id);

    let service_account = ServiceAccount::builder()
        .display_name(format!("Test Service Account {}", account_id))
        .description("Test service account created by Alien Infra tests".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    // Create service account
    let created_sa = ctx
        .create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create service account");

    assert!(created_sa.email.is_some());
    assert!(created_sa.unique_id.is_some());
    assert_eq!(
        created_sa.display_name.as_ref().unwrap(),
        &format!("Test Service Account {}", account_id)
    );
    assert_eq!(
        created_sa.description.as_ref().unwrap(),
        "Test service account created by Alien Infra tests"
    );

    let sa_email = created_sa.email.as_ref().unwrap();
    println!("Successfully created service account: {}", sa_email);

    // Wait for service account to become available before attempting to get it
    ctx.wait_for_service_account_availability(sa_email)
        .await
        .expect("Service account did not become available");

    // Get service account (with retry to handle eventual consistency)
    let fetched_sa = ctx
        .retry_gcp_operation(
            || ctx.client.get_service_account(sa_email.clone()),
            &format!("get service account {}", sa_email),
        )
        .await
        .expect("Failed to get service account");
    assert_eq!(fetched_sa.email.as_ref().unwrap(), sa_email);
    assert_eq!(
        fetched_sa.unique_id.as_ref().unwrap(),
        created_sa.unique_id.as_ref().unwrap()
    );

    println!("Successfully fetched service account: {}", sa_email);

    // Delete service account
    ctx.client
        .delete_service_account(sa_email.clone())
        .await
        .expect("Failed to delete service account");
    println!("Successfully deleted service account: {}", sa_email);
    ctx.untrack_service_account(sa_email);
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_patch_service_account(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    // Create initial service account
    let service_account = ServiceAccount::builder()
        .display_name("Initial Display Name".to_string())
        .description("Initial description".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    let created_sa = ctx
        .create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create service account");

    let sa_email = created_sa.email.as_ref().unwrap();
    println!("Created service account for patch test: {}", sa_email);

    // Wait for service account to become available before patching
    ctx.wait_for_service_account_availability(sa_email)
        .await
        .expect("Service account did not become available");

    // Patch service account
    let updated_sa = ServiceAccount::builder()
        .display_name("Updated Display Name".to_string())
        .description("Updated description".to_string())
        .build();

    let patched_sa = ctx
        .client
        .patch_service_account(
            sa_email.clone(),
            updated_sa,
            Some("displayName,description".to_string()),
        )
        .await
        .expect("Failed to patch service account");

    assert_eq!(
        patched_sa.display_name.as_ref().unwrap(),
        "Updated Display Name"
    );
    assert_eq!(
        patched_sa.description.as_ref().unwrap(),
        "Updated description"
    );

    println!("Successfully patched service account: {}", sa_email);

    // Verify changes persist (with retry to handle eventual consistency)
    let max_attempts = 10;
    let base_delay = Duration::from_millis(500);
    let mut last_sa = None;

    for attempt in 1..=max_attempts {
        match ctx.client.get_service_account(sa_email.clone()).await {
            Ok(fetched_sa) => {
                last_sa = Some(fetched_sa.clone());

                // Check if the values match what we expect
                if fetched_sa.display_name.as_ref() == Some(&"Updated Display Name".to_string())
                    && fetched_sa.description.as_ref() == Some(&"Updated description".to_string())
                {
                    println!(
                        "✅ Service account patch verification succeeded on attempt {}",
                        attempt
                    );
                    break;
                }

                if attempt < max_attempts {
                    let delay = base_delay * (2_u32.pow((attempt - 1).min(4))); // Cap exponential growth
                    info!(
                        "⏳ Service account values not yet updated, waiting {:?} (attempt {}/{})",
                        delay, attempt, max_attempts
                    );
                    sleep(delay).await;
                } else {
                    panic!(
                        "Service account values did not update after {} attempts. Last values: display_name={:?}, description={:?}",
                        max_attempts,
                        fetched_sa.display_name,
                        fetched_sa.description
                    );
                }
            }
            Err(e) => {
                if attempt < max_attempts {
                    let delay = base_delay * (2_u32.pow((attempt - 1).min(4)));
                    info!(
                        "⏳ Failed to get service account, retrying in {:?} (attempt {}/{}): {:?}",
                        delay, attempt, max_attempts, e
                    );
                    sleep(delay).await;
                } else {
                    panic!(
                        "Failed to get service account after {} attempts: {:?}",
                        max_attempts, e
                    );
                }
            }
        }
    }

    println!(
        "Successfully verified patch persistence for service account: {}",
        sa_email
    );
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_service_account_iam_policy(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    // Create service account
    let service_account = ServiceAccount::builder()
        .display_name("IAM Policy Test SA".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    let created_sa = ctx
        .create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create service account");

    let sa_email = created_sa.email.as_ref().unwrap();
    println!("Testing IAM policy for service account: {}", sa_email);

    // Wait for service account to be fully available
    ctx.wait_for_service_account_availability(sa_email)
        .await
        .expect("Service account did not become available");

    // Get original IAM policy (with retry to handle eventual consistency)
    let original_policy = ctx
        .retry_gcp_operation(
            || ctx.client.get_service_account_iam_policy(sa_email.clone()),
            &format!("get original IAM policy for {}", sa_email),
        )
        .await
        .expect("Failed to get original IAM policy");
    println!("Original IAM policy retrieved");

    // Create a test policy with a binding
    let test_binding = Binding::builder()
        .role("roles/iam.serviceAccountTokenCreator".to_string())
        .members(vec![format!("serviceAccount:{}", sa_email)])
        .build();

    let mut new_bindings = original_policy.bindings.clone();
    new_bindings.push(test_binding);

    let policy_to_set = IamPolicy::builder()
        .version(3)
        .bindings(new_bindings.clone())
        .maybe_etag(original_policy.etag.clone())
        .build();

    // Set the modified IAM policy
    let updated_policy = ctx
        .client
        .set_service_account_iam_policy(sa_email.clone(), policy_to_set)
        .await
        .expect("Failed to set IAM policy");

    // Verify the new binding exists
    let binding_exists = updated_policy.bindings.iter().any(|b| {
        b.role == "roles/iam.serviceAccountTokenCreator"
            && b.members.contains(&format!("serviceAccount:{}", sa_email))
    });
    assert!(
        binding_exists,
        "Test IAM binding was not found after setting policy"
    );

    println!(
        "Successfully set and verified IAM policy for service account: {}",
        sa_email
    );

    // Restore original policy
    let restore_policy = IamPolicy::builder()
        .bindings(original_policy.bindings)
        .maybe_etag(updated_policy.etag)
        .build();

    ctx.client
        .set_service_account_iam_policy(sa_email.clone(), restore_policy)
        .await
        .expect("Failed to restore original IAM policy");

    println!(
        "Successfully restored original IAM policy for service account: {}",
        sa_email
    );
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_create_get_delete_role(ctx: &mut IamTestContext) {
    let role_id = ctx.generate_unique_role_id();

    println!("Attempting to create test role: {}", role_id);

    let role = Role::builder()
        .title(format!("Test Role {}", role_id))
        .description("Test role created by Alien Infra tests".to_string())
        .included_permissions(vec![
            "storage.objects.get".to_string(),
            "storage.objects.list".to_string(),
        ])
        .stage(RoleLaunchStage::Ga)
        .build();

    let create_request = CreateRoleRequest::builder().role(role).build();

    // Create role
    let created_role = ctx
        .create_test_role(&role_id, create_request)
        .await
        .expect("Failed to create role");

    assert!(created_role.name.is_some());
    assert_eq!(
        created_role.title.as_ref().unwrap(),
        &format!("Test Role {}", role_id)
    );
    assert_eq!(
        created_role.description.as_ref().unwrap(),
        "Test role created by Alien Infra tests"
    );
    assert_eq!(created_role.included_permissions.len(), 2);
    assert!(created_role
        .included_permissions
        .contains(&"storage.objects.get".to_string()));
    assert!(created_role
        .included_permissions
        .contains(&"storage.objects.list".to_string()));

    println!("Successfully created role: {}", role_id);

    // Get role (with retry to handle eventual consistency)
    let fetched_role = ctx
        .retry_gcp_operation(
            || ctx.client.get_role(role_id.clone()),
            &format!("get role {}", role_id),
        )
        .await
        .expect("Failed to get role");
    assert_eq!(
        fetched_role.title.as_ref().unwrap(),
        &format!("Test Role {}", role_id)
    );
    assert_eq!(fetched_role.included_permissions.len(), 2);

    println!("Successfully fetched role: {}", role_id);

    // Delete role
    let deleted_role = ctx
        .client
        .delete_role(role_id.clone())
        .await
        .expect("Failed to delete role");
    assert!(
        deleted_role.deleted.unwrap_or(false),
        "Role should be marked as deleted"
    );

    println!("Successfully deleted role: {}", role_id);
    ctx.untrack_role(&role_id);
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_patch_role(ctx: &mut IamTestContext) {
    let role_id = ctx.generate_unique_role_id();

    // Create initial role
    let role = Role::builder()
        .title("Initial Role Title".to_string())
        .description("Initial description".to_string())
        .included_permissions(vec!["storage.objects.get".to_string()])
        .stage(RoleLaunchStage::Ga)
        .build();

    let create_request = CreateRoleRequest::builder().role(role).build();

    let created_role = ctx
        .create_test_role(&role_id, create_request)
        .await
        .expect("Failed to create role");

    println!("Created role for patch test: {}", role_id);

    // Patch role
    let updated_role = Role::builder()
        .title("Updated Role Title".to_string())
        .description("Updated description".to_string())
        .included_permissions(vec![
            "storage.objects.get".to_string(),
            "storage.objects.list".to_string(),
            "storage.objects.create".to_string(),
        ])
        .maybe_etag(created_role.etag.clone()) // Include ETag for optimistic concurrency
        .build();

    let patched_role = ctx
        .client
        .patch_role(
            role_id.clone(),
            updated_role,
            Some("title,description,includedPermissions".to_string()),
        )
        .await
        .expect("Failed to patch role");

    assert_eq!(patched_role.title.as_ref().unwrap(), "Updated Role Title");
    assert_eq!(
        patched_role.description.as_ref().unwrap(),
        "Updated description"
    );
    assert_eq!(patched_role.included_permissions.len(), 3);
    assert!(patched_role
        .included_permissions
        .contains(&"storage.objects.create".to_string()));

    println!("Successfully patched role: {}", role_id);

    // Verify changes persist (with retry to handle eventual consistency)
    let fetched_role = ctx
        .retry_gcp_operation(
            || ctx.client.get_role(role_id.clone()),
            &format!("get patched role {}", role_id),
        )
        .await
        .expect("Failed to get patched role");
    assert_eq!(fetched_role.title.as_ref().unwrap(), "Updated Role Title");
    assert_eq!(fetched_role.included_permissions.len(), 3);

    println!(
        "Successfully verified patch persistence for role: {}",
        role_id
    );
}

// === ERROR TESTING ===

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_error_service_account_not_found(ctx: &mut IamTestContext) {
    let non_existent_sa = "non-existent-sa@fake-project.iam.gserviceaccount.com";

    let result = ctx
        .client
        .get_service_account(non_existent_sa.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent service account"
    );

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "IAM");
            assert_eq!(resource_name, non_existent_sa);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound error for service account");
        }
        other => panic!("Expected RemoteResourceNotFound error, got: {:?}", other),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_error_role_not_found(ctx: &mut IamTestContext) {
    let non_existent_role = "non_existent_role_12345";

    let result = ctx.client.get_role(non_existent_role.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent role");

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "IAM");
            assert_eq!(resource_name, non_existent_role);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound error for role");
        }
        other => panic!("Expected RemoteResourceNotFound error, got: {:?}", other),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_error_service_account_already_exists(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    // Create first service account
    let service_account = ServiceAccount::builder()
        .display_name("Test SA".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account.clone())
        .build();

    ctx.create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create first service account");

    // Add a small delay to avoid hitting rate limits when testing duplicate creation
    sleep(Duration::from_millis(1000)).await;

    // Try to create another with the same ID
    let duplicate_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    let result = ctx
        .client
        .create_service_account(account_id.clone(), duplicate_request)
        .await;
    assert!(
        result.is_err(),
        "Expected error when creating duplicate service account"
    );

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceConflict {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "IAM");
            assert_eq!(resource_name.to_string(), account_id.to_string());
            println!("✅ Correctly mapped 409 to RemoteResourceConflict error for service account");
        }
        other => panic!("Expected RemoteResourceConflict error, got: {:?}", other),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_error_access_denied(ctx: &mut IamTestContext) {
    let invalid_client = ctx.create_invalid_client();
    let result = invalid_client
        .get_service_account("any-sa@example.com".to_string())
        .await;

    assert!(result.is_err(), "Expected error with invalid credentials");

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::HttpResponseError { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        other => println!("Got error (acceptable for invalid creds): {:?}", other),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_delete_non_existent_service_account(ctx: &mut IamTestContext) {
    let non_existent_sa = "non-existent-sa@fake-project.iam.gserviceaccount.com";

    let result = ctx
        .client
        .delete_service_account(non_existent_sa.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent service account"
    );

    match &result.unwrap_err().error {
            Some(ErrorData::RemoteResourceNotFound { resource_type, resource_name, .. }) => {
                assert_eq!(resource_type, "IAM");
                assert_eq!(resource_name, non_existent_sa);
                println!("✅ Correctly mapped 404 to RemoteResourceNotFound for service account deletion");
            }
            other => panic!("Expected RemoteResourceNotFound error for non-existent service account deletion, got: {:?}", other),
        }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_delete_non_existent_role(ctx: &mut IamTestContext) {
    let non_existent_role = "non_existent_role_67890";

    let result = ctx.client.delete_role(non_existent_role.to_string()).await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent role"
    );

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "IAM");
            assert_eq!(resource_name, non_existent_role);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for role deletion");
        }
        other => panic!(
            "Expected RemoteResourceNotFound error for non-existent role deletion, got: {:?}",
            other
        ),
    }
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_iam_policy_with_conditions(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    // Create service account
    let service_account = ServiceAccount::builder()
        .display_name("Conditional IAM Test SA".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    let created_sa = ctx
        .create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create service account");

    let sa_email = created_sa.email.as_ref().unwrap();
    println!(
        "Testing conditional IAM policy for service account: {}",
        sa_email
    );

    // Wait for service account to be fully available
    ctx.wait_for_service_account_availability(sa_email)
        .await
        .expect("Service account did not become available");

    // Get original policy (with retry to handle eventual consistency)
    let original_policy = ctx
        .retry_gcp_operation(
            || ctx.client.get_service_account_iam_policy(sa_email.clone()),
            &format!("get original IAM policy for {}", sa_email),
        )
        .await
        .expect("Failed to get original IAM policy");

    // Create a binding with a condition
    let condition = Expr::builder()
        .expression("request.time.getHours() < 12".to_string())
        .title("Morning hours only".to_string())
        .description("Only allow access during morning hours".to_string())
        .build();

    let conditional_binding = Binding::builder()
        .role("roles/iam.serviceAccountUser".to_string())
        .members(vec![format!("serviceAccount:{}", sa_email)])
        .maybe_condition(Some(condition))
        .build();

    let mut new_bindings = original_policy.bindings.clone();
    new_bindings.push(conditional_binding);

    let policy_to_set = IamPolicy::builder()
        .version(3)
        .bindings(new_bindings)
        .maybe_etag(original_policy.etag.clone())
        .build();

    // Set the policy with condition
    let updated_policy = ctx
        .client
        .set_service_account_iam_policy(sa_email.clone(), policy_to_set)
        .await
        .expect("Failed to set conditional IAM policy");

    // Verify the conditional binding exists
    let conditional_binding_exists = updated_policy.bindings.iter().any(|b| {
        b.role == "roles/iam.serviceAccountUser"
            && b.members.contains(&format!("serviceAccount:{}", sa_email))
            && b.condition.is_some()
            && b.condition.as_ref().unwrap().expression == "request.time.getHours() < 12"
    });
    assert!(
        conditional_binding_exists,
        "Conditional IAM binding was not found after setting policy"
    );

    println!(
        "Successfully set and verified conditional IAM policy for service account: {}",
        sa_email
    );
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_patch_service_account_with_explicit_update_mask(ctx: &mut IamTestContext) {
    let account_id = ctx.generate_unique_service_account_id();

    // Create service account
    let service_account = ServiceAccount::builder()
        .display_name("Explicit Mask Test SA".to_string())
        .build();

    let create_request = CreateServiceAccountRequest::builder()
        .service_account(service_account)
        .build();

    let created_sa = ctx
        .create_test_service_account(&account_id, create_request)
        .await
        .expect("Failed to create service account");

    let sa_email = created_sa.email.as_ref().unwrap();

    // Wait for service account to become available before patching
    ctx.wait_for_service_account_availability(sa_email)
        .await
        .expect("Service account did not become available");

    // Patch with explicit update mask (required by Google Cloud IAM)
    let updated_sa = ServiceAccount::builder()
        .display_name("Updated With Explicit Mask".to_string())
        .description("Description added with explicit mask".to_string())
        .build();

    let patched_sa = ctx
        .client
        .patch_service_account(
            sa_email.clone(),
            updated_sa,
            Some("displayName,description".to_string()),
        )
        .await
        .expect("Failed to patch service account with explicit update mask");

    assert_eq!(
        patched_sa.display_name.as_ref().unwrap(),
        "Updated With Explicit Mask"
    );
    assert_eq!(
        patched_sa.description.as_ref().unwrap(),
        "Description added with explicit mask"
    );

    println!(
        "Successfully patched service account with explicit update mask: {}",
        sa_email
    );
}

#[test_context(IamTestContext)]
#[tokio::test]
async fn test_role_with_empty_permissions(ctx: &mut IamTestContext) {
    let role_id = ctx.generate_unique_role_id();

    // Create role with empty permissions list
    let role = Role::builder()
        .title("Empty Permissions Role".to_string())
        .description("Test role with no permissions".to_string())
        .included_permissions(vec![]) // Empty permissions
        .stage(RoleLaunchStage::Beta)
        .build();

    let create_request = CreateRoleRequest::builder().role(role).build();

    let created_role = ctx
        .create_test_role(&role_id, create_request)
        .await
        .expect("Failed to create role with empty permissions");

    assert_eq!(created_role.included_permissions.len(), 0);
    assert_eq!(created_role.stage.as_ref().unwrap(), &RoleLaunchStage::Beta);

    println!(
        "Successfully created role with empty permissions: {}",
        role_id
    );
}
