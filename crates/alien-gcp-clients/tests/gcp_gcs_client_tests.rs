#![cfg(all(test, feature = "gcp"))]
use alien_client_core::{ErrorData, Result};
use alien_gcp_clients::gcs::{
    Bucket, GcsApi as _, GcsClient, GcsNotification, IamConfiguration, Object,
    UniformBucketLevelAccess, Versioning,
};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

const TEST_GCS_BUCKET_ENV_VAR: &str = "ALIEN_TEST_GCP_GCS_BUCKET";

struct GcsTestContext {
    client: GcsClient,
    project_id: String,
    main_test_bucket: String,
    created_buckets: Mutex<HashSet<String>>,
}

impl AsyncTestContext for GcsTestContext {
    async fn setup() -> GcsTestContext {
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

        let test_bucket_name = env::var(TEST_GCS_BUCKET_ENV_VAR)
            .expect(&format!("{} must be set", TEST_GCS_BUCKET_ENV_VAR));

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: "us-central1".to_string(), // GCS is global for buckets but some ops might need a default region context
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = GcsClient::new(Client::new(), config);

        GcsTestContext {
            client,
            project_id,
            main_test_bucket: test_bucket_name,
            created_buckets: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting GCS test cleanup...");

        let buckets_to_cleanup = {
            let buckets = self.created_buckets.lock().unwrap();
            buckets.clone()
        };

        for bucket_name in buckets_to_cleanup {
            self.cleanup_bucket(bucket_name.to_string()).await;
        }

        info!("✅ GCS test cleanup completed");
    }
}

impl GcsTestContext {
    fn track_bucket(&self, bucket_name: String) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.insert(bucket_name.to_string());
        info!("📝 Tracking bucket for cleanup: {}", bucket_name);
    }

    fn untrack_bucket(&self, bucket_name: String) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.remove(&bucket_name);
        info!(
            "✅ Bucket {} successfully cleaned up and untracked",
            bucket_name
        );
    }

    async fn cleanup_bucket(&self, bucket_name: String) {
        info!("🧹 Cleaning up bucket: {}", bucket_name);

        match self.client.delete_bucket(bucket_name.clone()).await {
            Ok(_) => {
                info!("✅ Bucket {} deleted successfully", bucket_name);
            }
            Err(alien_err) => {
                if let Some(ErrorData::RemoteResourceNotFound { .. }) = alien_err.error {
                    info!("🔍 Bucket {} was already deleted", bucket_name);
                } else {
                    warn!(
                        "Failed to delete bucket {} during cleanup: {:?}",
                        bucket_name, alien_err
                    );
                }
            }
        }
    }

    fn generate_unique_bucket_name(&self) -> String {
        format!(
            "alien-test-bucket-{}",
            Uuid::new_v4().hyphenated().to_string()
        )
    }

    fn generate_unique_object_name(&self) -> String {
        format!(
            "alien-test-object-{}",
            Uuid::new_v4().hyphenated().to_string()
        )
    }

    async fn create_test_bucket(
        &self,
        bucket_name: String,
        bucket_config: Bucket,
    ) -> Result<Bucket> {
        let result = self
            .client
            .create_bucket(bucket_name.clone(), bucket_config)
            .await;
        if result.is_ok() {
            self.track_bucket(bucket_name);
        }
        result
    }

    /// Create a client with invalid credentials for error testing
    fn create_invalid_client(&self) -> GcsClient {
        let invalid_config = GcpClientConfig {
                project_id: self.project_id.clone(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey {
                    json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
                },
                service_overrides: None,
            project_number: None,
            };
        GcsClient::new(Client::new(), invalid_config)
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_framework_setup_gcs(ctx: &mut GcsTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    assert!(
        !ctx.main_test_bucket.is_empty(),
        "Test bucket name should not be empty"
    );

    // Try a simple operation like getting the main test bucket (which should exist)
    let bucket_result = ctx
        .client
        .get_bucket(ctx.main_test_bucket.to_string())
        .await;
    assert!(
        bucket_result.is_ok(),
        "Failed to get the main test bucket {}: {:?}",
        ctx.main_test_bucket,
        bucket_result.err()
    );
    println!(
        "Successfully connected to GCS and fetched main test bucket: {}",
        ctx.main_test_bucket
    );
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_create_get_delete_bucket(ctx: &mut GcsTestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    println!("Attempting to create test bucket: {}", bucket_name);

    let bucket_to_create = Bucket::builder()
        .location("US-CENTRAL1".to_string()) // Example: Specify a location
        .storage_class("STANDARD".to_string())
        .build();

    let created_bucket = ctx
        .create_test_bucket(bucket_name.clone(), bucket_to_create)
        .await
        .expect("Failed to create bucket");
    assert_eq!(created_bucket.name.unwrap(), bucket_name);
    assert_eq!(
        created_bucket.location.as_ref().unwrap().to_uppercase(),
        "US-CENTRAL1"
    );

    println!("Successfully created test bucket: {}", bucket_name);

    let fetched_bucket = ctx
        .client
        .get_bucket(bucket_name.to_string())
        .await
        .expect("Failed to get bucket");
    assert_eq!(fetched_bucket.name.as_ref().unwrap(), &bucket_name);
    assert_eq!(fetched_bucket.id.as_ref().unwrap(), &bucket_name); // For GCS, bucket id is usually the name

    println!("Successfully fetched test bucket: {}", bucket_name);

    // Delete bucket explicitly
    ctx.client
        .delete_bucket(bucket_name.to_string())
        .await
        .expect("Failed to delete bucket");
    println!(
        "Successfully initiated deletion of test bucket: {}",
        bucket_name
    );
    ctx.untrack_bucket(bucket_name.to_string());
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_update_bucket(ctx: &mut GcsTestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    println!(
        "Attempting to create bucket for update test: {}",
        bucket_name
    );
    let initial_bucket_res = Bucket::builder()
        .location("US".to_string()) // GCS default multi-region
        .build();
    let created_bucket_for_update = ctx
        .create_test_bucket(bucket_name.clone(), initial_bucket_res)
        .await
        .expect("Failed to create bucket for update");
    let _ = created_bucket_for_update; // consume it

    // Update: enable versioning and add a label
    let mut labels_to_set = std::collections::HashMap::new();
    labels_to_set.insert("test_label_key".to_string(), "test_label_value".to_string());

    let bucket_patch = Bucket::builder()
        .versioning(Versioning::builder().enabled(true).build())
        .labels(labels_to_set.clone())
        .build();

    println!("Attempting to update bucket: {}", bucket_name);
    let updated_bucket = ctx
        .client
        .update_bucket(bucket_name.to_string(), bucket_patch)
        .await
        .expect("Failed to update bucket");

    assert!(
        updated_bucket
            .versioning
            .as_ref()
            .map_or(false, |v| v.enabled),
        "Versioning should be enabled after update."
    );
    assert_eq!(
        updated_bucket
            .labels
            .as_ref()
            .unwrap()
            .get("test_label_key")
            .unwrap(),
        "test_label_value",
        "Label was not set correctly."
    );

    println!("Successfully updated bucket: {}", bucket_name);

    // Fetch again to be sure
    let fetched_bucket = ctx
        .client
        .get_bucket(bucket_name.to_string())
        .await
        .expect("Failed to fetch bucket post-update");
    assert!(
        fetched_bucket
            .versioning
            .as_ref()
            .map_or(false, |v| v.enabled),
        "Versioning should persist after fetch."
    );
    assert_eq!(
        fetched_bucket
            .labels
            .as_ref()
            .unwrap()
            .get("test_label_key")
            .unwrap(),
        "test_label_value",
        "Label did not persist after fetch."
    );

    println!(
        "Successfully verified updated bucket persistence: {}",
        bucket_name
    );
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_bucket_iam_policy(ctx: &mut GcsTestContext) {
    println!("Testing IAM policy for bucket: {}", ctx.main_test_bucket);

    // 1. Get original IAM policy to restore it later
    let original_iam_policy = ctx
        .client
        .get_bucket_iam_policy(ctx.main_test_bucket.to_string())
        .await
        .expect("Failed to get original IAM policy");
    println!("Original IAM policy ETag: {:?}", original_iam_policy.etag);

    // 2. Test approach: Instead of adding a fake user, we'll try to add a service account
    // that we can reasonably expect to exist in the same project. If that fails, we'll
    // just test the basic get/set operations with the existing policy.

    // Try to get the current service account email from the environment or use a fallback
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

    // Ensure we have the serviceAccount: prefix
    let test_member = if current_sa_email.starts_with("serviceAccount:") {
        current_sa_email
    } else {
        format!("serviceAccount:{}", current_sa_email)
    };

    let test_role = "roles/storage.objectViewer".to_string();

    let new_binding = Binding::builder()
        .role(test_role.clone())
        .members(vec![test_member.clone()])
        .build();

    let mut modified_policy_bindings = original_iam_policy.bindings.clone();
    modified_policy_bindings.push(new_binding);

    let policy_to_set = IamPolicy::builder()
        .bindings(modified_policy_bindings)
        .maybe_etag(original_iam_policy.etag.clone()) // ETag is required for optimistic concurrency control
        .build();

    // 3. Set the modified IAM policy
    println!(
        "Attempting to set modified IAM policy for bucket: {}",
        ctx.main_test_bucket
    );
    let set_policy_result = ctx
        .client
        .set_bucket_iam_policy(ctx.main_test_bucket.to_string(), policy_to_set)
        .await;

    // Handle potential errors gracefully
    match &set_policy_result {
        Ok(updated_iam_policy) => {
            // 4. Verify the new binding is present
            let binding_exists = updated_iam_policy
                .bindings
                .iter()
                .any(|b| b.role == test_role && b.members.contains(&test_member));
            assert!(
                binding_exists,
                "Test IAM binding was not found after setting policy."
            );
            println!(
                "Successfully set and verified new IAM binding for bucket: {}",
                ctx.main_test_bucket
            );

            // 5. Restore original IAM policy
            // Must use the ETag from the *updated* policy for the restore operation
            let policy_to_restore = IamPolicy::builder()
                .bindings(original_iam_policy.bindings.clone()) // Use the original bindings
                .maybe_etag(updated_iam_policy.etag.clone()) // Use the latest ETag
                .build();

            ctx.client
                .set_bucket_iam_policy(ctx.main_test_bucket.to_string(), policy_to_restore)
                .await
                .expect("Failed to restore original IAM policy");
            println!(
                "Successfully restored original IAM policy for bucket: {}",
                ctx.main_test_bucket
            );

            // Final verification (optional)
            let final_policy = ctx
                .client
                .get_bucket_iam_policy(ctx.main_test_bucket.to_string())
                .await
                .expect("Failed to get IAM policy after restore");
            assert_eq!(
                final_policy.bindings.len(),
                original_iam_policy.bindings.len(),
                "Policy after restore should have original number of bindings"
            );
            // More detailed comparison can be added if necessary, e.g., sorting and comparing all bindings.
        }
        Err(alien_err) => {
            // Handle specific error cases
            match &alien_err.error {
                Some(ErrorData::GenericError { message }) => {
                    if message.contains("does not exist") || message.contains("Invalid member") {
                        println!("Warning: Unable to test IAM policy modification because the service account doesn't exist or is invalid: {}", message);
                        println!("This is acceptable for this test - the basic get/set policy operations work correctly.");

                        // Test basic round-trip: set the same policy back
                        let same_policy_to_set = IamPolicy::builder()
                            .bindings(original_iam_policy.bindings.clone())
                            .maybe_etag(original_iam_policy.etag.clone())
                            .build();

                        let roundtrip_result = ctx
                            .client
                            .set_bucket_iam_policy(
                                ctx.main_test_bucket.to_string(),
                                same_policy_to_set,
                            )
                            .await;
                        assert!(
                            roundtrip_result.is_ok(),
                            "Basic IAM policy round-trip failed: {:?}",
                            roundtrip_result.err()
                        );
                        println!("Successfully performed basic IAM policy round-trip test.");
                        return;
                    } else if message.contains("412 Precondition Failed")
                        || message.contains("ETag mismatch")
                    {
                        eprintln!("Warning: ETag mismatch while setting IAM policy. The policy might have been changed externally. Skipping detailed IAM assertions. Error: {:?}", alien_err);
                        let current_policy_for_restore = ctx
                            .client
                            .get_bucket_iam_policy(ctx.main_test_bucket.to_string())
                            .await
                            .expect("Failed to get current policy for restore after ETag mismatch");
                        let policy_to_restore_with_new_etag = IamPolicy::builder()
                            .bindings(original_iam_policy.bindings.clone())
                            .maybe_etag(current_policy_for_restore.etag)
                            .build();
                        ctx.client
                            .set_bucket_iam_policy(
                                ctx.main_test_bucket.to_string(),
                                policy_to_restore_with_new_etag,
                            )
                            .await
                            .expect("Failed to restore original IAM policy after ETag mismatch");
                        println!("Restored original IAM policy bindings for bucket: {} after ETag mismatch", ctx.main_test_bucket);
                        return;
                    }
                }
                Some(ErrorData::HttpRequestFailed { message }) => {
                    if message.contains("412 Precondition Failed")
                        || message.contains("ETag mismatch")
                    {
                        eprintln!("Warning: ETag mismatch (HttpRequestFailed) while setting IAM policy. Error: {:?}", alien_err);
                        let current_policy_for_restore = ctx
                            .client
                            .get_bucket_iam_policy(ctx.main_test_bucket.to_string())
                            .await
                            .expect("Failed to get current policy for restore after ETag mismatch");
                        let policy_to_restore_with_new_etag = IamPolicy::builder()
                            .bindings(original_iam_policy.bindings.clone())
                            .maybe_etag(current_policy_for_restore.etag)
                            .build();
                        ctx.client
                            .set_bucket_iam_policy(
                                ctx.main_test_bucket.to_string(),
                                policy_to_restore_with_new_etag,
                            )
                            .await
                            .expect("Failed to restore original IAM policy after ETag mismatch");
                        println!("Restored original IAM policy bindings for bucket: {} after ETag mismatch (HttpRequestFailed)", ctx.main_test_bucket);
                        return;
                    }
                }
                _ => {}
            }
            panic!(
                "Failed to set IAM policy: {:?}",
                set_policy_result.unwrap_err()
            );
        }
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_insert_list_delete_object(ctx: &mut GcsTestContext) {
    let object_name_prefix = "alien-test-obj-";
    let object_name = format!(
        "{}-{}",
        object_name_prefix,
        Uuid::new_v4().hyphenated().to_string()
    );
    let object_content = "Hello GCS from Alien Infra test!".as_bytes().to_vec();

    println!(
        "Testing object operations in bucket: {} with object: {}",
        ctx.main_test_bucket, object_name
    );

    // 1. Insert Object
    let object_resource_metadata = Object::builder()
        .name(object_name.clone()) // Name is crucial for insert_object
        .content_type("text/plain".to_string())
        .build();

    println!("Attempting to insert object: {}", object_name);
    let inserted_object = ctx
        .client
        .insert_object(
            ctx.main_test_bucket.to_string(),
            object_resource_metadata,
            object_content.clone(),
        )
        .await
        .expect("Failed to insert object");

    assert_eq!(inserted_object.name.as_ref().unwrap(), &object_name);
    assert_eq!(inserted_object.bucket.unwrap(), ctx.main_test_bucket);
    assert_eq!(
        inserted_object
            .size
            .as_ref()
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        object_content.len()
    );
    assert_eq!(inserted_object.content_type.as_ref().unwrap(), "text/plain");
    println!(
        "Successfully inserted object: {} with size {}",
        object_name,
        inserted_object.size.as_ref().unwrap()
    );

    // 2. List Objects
    println!(
        "Attempting to list objects with prefix: {}",
        object_name_prefix
    );
    // Ensure eventual consistency by retrying list a few times if object not found immediately
    let mut found_in_list = false;
    for i in 0..5 {
        let list_response = ctx
            .client
            .list_objects(
                ctx.main_test_bucket.to_string(),
                Some(object_name.clone()),
                None,
                None,
                Some(10),
                None,
            )
            .await
            .expect("Failed to list objects");
        if list_response
            .items
            .iter()
            .any(|item| item.name.as_ref().unwrap() == &object_name)
        {
            found_in_list = true;
            println!("Object {} found in list on attempt {}", object_name, i + 1);
            break;
        }
        println!(
            "Object {} not found in list on attempt {}, retrying in 1s...",
            object_name,
            i + 1
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    assert!(
        found_in_list,
        "Inserted object not found in list_objects response even after retries."
    );

    // 3. Delete Object
    println!("Attempting to delete object: {}", object_name);
    ctx.client
        .delete_object(ctx.main_test_bucket.to_string(), object_name.clone(), None)
        .await
        .expect("Failed to delete object");
    println!("Successfully deleted object: {}", object_name);

    // 4. Verify Deletion (Optional - by trying to list again)
    // GCS list operations are eventually consistent, so an immediate list might still show the object.
    // A get_object (if implemented and used) would be a more reliable check for non-existence.
    // For now, we assume delete_object call succeeding means it's gone or will be shortly.
    // Let's try listing again and assert it's NOT there, allowing for some delay.
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await; // Wait a bit for consistency
    let list_after_delete = ctx
        .client
        .list_objects(
            ctx.main_test_bucket.to_string(),
            Some(object_name.clone()),
            None,
            None,
            Some(10),
            None,
        )
        .await
        .expect("Failed to list objects after delete");
    assert!(
        !list_after_delete
            .items
            .iter()
            .any(|item| item.name.as_ref().unwrap() == &object_name),
        "Object should not be found in list after deletion, but was present."
    );
    println!("Verified object {} is no longer listed.", object_name);
}

// === NEW COMPREHENSIVE ERROR TESTING ===

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_error_translation_bucket_not_found(ctx: &mut GcsTestContext) {
    let non_existent_bucket = "alien-test-bucket-does-not-exist-12345";

    let result = ctx.client.get_bucket(non_existent_bucket.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent bucket");

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "GCS");
            assert_eq!(resource_name, non_existent_bucket);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound error");
        }
        other => panic!("Expected RemoteResourceNotFound error, got: {:?}", other),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_error_translation_bucket_already_exists(ctx: &mut GcsTestContext) {
    // Try to create a bucket that already exists (using main test bucket)
    let bucket_to_create = Bucket::builder().location("US".to_string()).build();

    let result = ctx
        .client
        .create_bucket(ctx.main_test_bucket.to_string(), bucket_to_create)
        .await;
    assert!(
        result.is_err(),
        "Expected error when creating existing bucket"
    );

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceConflict {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "GCS");
            assert_eq!(resource_name.to_string(), ctx.main_test_bucket.to_string());
            println!("✅ Correctly mapped 409 to RemoteResourceConflict error");
        }
        other => panic!("Expected RemoteResourceConflict error, got: {:?}", other),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_error_translation_access_denied(ctx: &mut GcsTestContext) {
    // Test with invalid credentials
    let invalid_config = GcpClientConfig {
            project_id: "fake-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7VJTUt9Us8cKB\nfake/invalid/key\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
            },
            service_overrides: None,
            project_number: None,
        };

    let invalid_client = GcsClient::new(Client::new(), invalid_config);
    let result = invalid_client.get_bucket("any-bucket".to_string()).await;

    assert!(result.is_err(), "Expected error with invalid credentials");

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::GenericError { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        other => println!("Got error (acceptable for invalid creds): {:?}", other),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_object_operations_with_special_characters(ctx: &mut GcsTestContext) {
    // Test object names with special characters that need URL encoding
    let special_object_names = vec![
        "test object with spaces.txt",
        "test/object/with/slashes.txt",
        "test%20object%20with%20encoding.txt",
        "test+object+with+plus.txt",
        "test&object&with&ampersand.txt",
        "test?object?with?question.txt",
        "test#object#with#hash.txt",
    ];

    for object_name in special_object_names {
        let object_content = format!("Content for {}", object_name).into_bytes();

        println!("Testing object with special characters: {}", object_name);

        // 1. Insert object
        let object_resource = Object::builder()
            .name(object_name.to_string())
            .content_type("text/plain".to_string())
            .build();

        let insert_result = ctx
            .client
            .insert_object(
                ctx.main_test_bucket.to_string(),
                object_resource,
                object_content,
            )
            .await;
        assert!(
            insert_result.is_ok(),
            "Failed to insert object with special characters: {}",
            object_name
        );

        // 2. List objects to verify it exists
        let list_result = ctx
            .client
            .list_objects(
                ctx.main_test_bucket.to_string(),
                Some(object_name.to_string()),
                None,
                None,
                Some(10),
                None,
            )
            .await;
        assert!(list_result.is_ok(), "Failed to list objects");
        let found = list_result
            .unwrap()
            .items
            .iter()
            .any(|item| item.name.as_ref().unwrap() == object_name);
        assert!(
            found,
            "Object with special characters not found in list: {}",
            object_name
        );

        // 3. Delete object
        let delete_result = ctx
            .client
            .delete_object(
                ctx.main_test_bucket.to_string(),
                object_name.to_string(),
                None,
            )
            .await;
        assert!(
            delete_result.is_ok(),
            "Failed to delete object with special characters: {}",
            object_name
        );

        println!("✅ Successfully tested object: {}", object_name);
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_insert_object_without_name_error(ctx: &mut GcsTestContext) {
    // Test error when object resource doesn't have a name
    let object_resource = Object::builder()
        .content_type("text/plain".to_string())
        .build(); // No name set

    let object_content = "Test content".as_bytes().to_vec();

    let result = ctx
        .client
        .insert_object(
            ctx.main_test_bucket.to_string(),
            object_resource,
            object_content,
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when inserting object without name"
    );

    match result.unwrap_err().error {
        Some(ErrorData::InvalidInput { field_name, .. }) => {
            assert_eq!(field_name, Some("name".to_string()));
            println!("✅ Correctly caught missing object name error");
        }
        other => panic!(
            "Expected Generic error for missing object name, got: {:?}",
            other
        ),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_list_objects_with_pagination(ctx: &mut GcsTestContext) {
    // Create multiple objects to test pagination
    let base_name = ctx.generate_unique_object_name();
    let mut created_objects = Vec::new();

    // Create 5 test objects
    for i in 0..5 {
        let object_name = format!("{}-{:02}", base_name, i);
        let object_content = format!("Content for object {}", i).into_bytes();

        let object_resource = Object::builder()
            .name(object_name.clone())
            .content_type("text/plain".to_string())
            .build();

        ctx.client
            .insert_object(
                ctx.main_test_bucket.to_string(),
                object_resource,
                object_content,
            )
            .await
            .expect("Failed to create test object");
        created_objects.push(object_name);
    }

    // Wait for eventual consistency
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Test listing with small page size
    let list_result = ctx
        .client
        .list_objects(
            ctx.main_test_bucket.to_string(),
            Some(base_name.clone()),
            None,
            None,
            Some(2),
            None,
        )
        .await;
    assert!(
        list_result.is_ok(),
        "Failed to list objects with pagination"
    );

    let list_response = list_result.unwrap();
    assert!(
        !list_response.items.is_empty(),
        "Should have found some objects"
    );
    assert!(
        list_response.items.len() <= 2,
        "Should respect max_results parameter"
    );

    println!(
        "✅ Pagination test: found {} objects (max 2 requested)",
        list_response.items.len()
    );

    // Test with delimiter
    let list_with_delimiter = ctx
        .client
        .list_objects(
            ctx.main_test_bucket.to_string(),
            Some(base_name.clone()),
            Some("/".to_string()),
            None,
            Some(10),
            None,
        )
        .await;
    assert!(
        list_with_delimiter.is_ok(),
        "Failed to list objects with delimiter"
    );

    // Clean up created objects
    for object_name in created_objects {
        ctx.client
            .delete_object(ctx.main_test_bucket.to_string(), object_name, None)
            .await
            .expect("Failed to clean up test object");
    }

    println!("✅ Successfully tested list_objects with pagination and delimiters");
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_delete_non_existent_object_error(ctx: &mut GcsTestContext) {
    let non_existent_object = "alien-test-object-does-not-exist-12345.txt";

    let result = ctx
        .client
        .delete_object(
            ctx.main_test_bucket.to_string(),
            non_existent_object.to_string(),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent object"
    );

    match result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "GCS");
            assert_eq!(resource_name, non_existent_object);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for object deletion");
        }
        other => panic!(
            "Expected RemoteResourceNotFound error for non-existent object, got: {:?}",
            other
        ),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_update_non_existent_bucket_error(ctx: &mut GcsTestContext) {
    let non_existent_bucket = "alien-test-bucket-does-not-exist-54321";

    let bucket_patch = Bucket::builder()
        .versioning(Versioning::builder().enabled(true).build())
        .build();

    let result = ctx
        .client
        .update_bucket(non_existent_bucket.to_string(), bucket_patch)
        .await;
    assert!(
        result.is_err(),
        "Expected error when updating non-existent bucket"
    );

    match result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
            ..
        }) => {
            assert_eq!(resource_type, "GCS");
            assert_eq!(resource_name, non_existent_bucket);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for bucket update");
        }
        other => panic!(
            "Expected RemoteResourceNotFound error for non-existent bucket update, got: {:?}",
            other
        ),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_iam_policy_operations_on_non_existent_bucket(ctx: &mut GcsTestContext) {
    let non_existent_bucket = "alien-test-bucket-does-not-exist-67890";

    // Test get_bucket_iam_policy
    let get_result = ctx
        .client
        .get_bucket_iam_policy(non_existent_bucket.to_string())
        .await;
    assert!(
        get_result.is_err(),
        "Expected error when getting IAM policy for non-existent bucket"
    );

    match get_result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) => {
            assert_eq!(resource_name, non_existent_bucket);
            println!("✅ get_bucket_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        other => panic!(
            "Expected RemoteResourceNotFound error for get IAM policy, got: {:?}",
            other
        ),
    }

    // Test set_bucket_iam_policy with a valid policy structure but no ETag
    // Note: GCS validates request format before checking if bucket exists,
    // so we might get validation errors instead of 404 for missing bucket
    let test_binding = Binding::builder()
        .role("roles/storage.objectViewer".to_string())
        .members(vec!["serviceAccount:test@example.com".to_string()])
        .build();

    let test_policy = IamPolicy::builder().bindings(vec![test_binding]).build(); // No ETag provided

    let set_result = ctx
        .client
        .set_bucket_iam_policy(non_existent_bucket.to_string(), test_policy)
        .await;
    assert!(
        set_result.is_err(),
        "Expected error when setting IAM policy for non-existent bucket"
    );

    match set_result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) => {
            assert_eq!(resource_name, non_existent_bucket);
            println!("✅ set_bucket_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        other => panic!(
            "Expected RemoteResourceNotFound error for set IAM policy, got: {:?}",
            other
        ),
    }
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_large_object_operations(ctx: &mut GcsTestContext) {
    let object_name = ctx.generate_unique_object_name();

    // Create a larger object (1MB)
    let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

    println!(
        "Testing large object operations with {} bytes",
        large_content.len()
    );

    let object_resource = Object::builder()
        .name(object_name.clone())
        .content_type("application/octet-stream".to_string())
        .build();

    // Insert large object
    let insert_result = ctx
        .client
        .insert_object(
            ctx.main_test_bucket.to_string(),
            object_resource,
            large_content.clone(),
        )
        .await;
    assert!(insert_result.is_ok(), "Failed to insert large object");

    let inserted_object = insert_result.unwrap();
    assert_eq!(
        inserted_object
            .size
            .as_ref()
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        large_content.len()
    );

    println!(
        "✅ Successfully inserted large object of {} bytes",
        large_content.len()
    );

    // List to verify
    let list_result = ctx
        .client
        .list_objects(
            ctx.main_test_bucket.to_string(),
            Some(object_name.clone()),
            None,
            None,
            Some(10),
            None,
        )
        .await;
    assert!(list_result.is_ok(), "Failed to list large object");

    let found = list_result
        .unwrap()
        .items
        .iter()
        .any(|item| item.name.as_ref().unwrap() == &object_name);
    assert!(found, "Large object not found in list");

    // Delete large object
    let delete_result = ctx
        .client
        .delete_object(ctx.main_test_bucket.to_string(), object_name.clone(), None)
        .await;
    assert!(delete_result.is_ok(), "Failed to delete large object");

    println!("✅ Successfully tested large object operations");
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_bucket_with_complex_configuration(ctx: &mut GcsTestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    // Create bucket with complex configuration
    let mut labels = std::collections::HashMap::new();
    labels.insert("environment".to_string(), "test".to_string());
    labels.insert("team".to_string(), "alien".to_string());

    let bucket_config = Bucket::builder()
        .location("US-CENTRAL1".to_string())
        .storage_class("STANDARD".to_string())
        .versioning(Versioning::builder().enabled(true).build())
        .iam_configuration(
            IamConfiguration::builder()
                .uniform_bucket_level_access(
                    // Org policy (constraints/storage.uniformBucketLevelAccess) enforces
                    // uniform bucket-level access on all buckets in this project.
                    UniformBucketLevelAccess::builder().enabled(true).build(),
                )
                .public_access_prevention("enforced".to_string())
                .build(),
        )
        .labels(labels.clone())
        .build();

    println!(
        "Creating bucket with complex configuration: {}",
        bucket_name
    );

    let created_bucket = ctx
        .create_test_bucket(bucket_name.clone(), bucket_config)
        .await
        .expect("Failed to create bucket with complex configuration");

    // Verify configuration
    assert_eq!(created_bucket.name.as_ref().unwrap(), &bucket_name);
    assert_eq!(
        created_bucket.location.as_ref().unwrap().to_uppercase(),
        "US-CENTRAL1"
    );
    assert_eq!(created_bucket.storage_class.as_ref().unwrap(), "STANDARD");
    assert!(created_bucket.versioning.as_ref().unwrap().enabled);
    assert!(
        created_bucket
            .iam_configuration
            .as_ref()
            .unwrap()
            .uniform_bucket_level_access
            .as_ref()
            .unwrap()
            .enabled
    );
    assert_eq!(
        created_bucket
            .labels
            .as_ref()
            .unwrap()
            .get("environment")
            .unwrap(),
        "test"
    );
    assert_eq!(
        created_bucket.labels.as_ref().unwrap().get("team").unwrap(),
        "alien"
    );

    println!("✅ Successfully created and verified bucket with complex configuration");

    // Test updating the configuration
    let mut new_labels = labels.clone();
    new_labels.insert("updated".to_string(), "true".to_string());

    let update_config = Bucket::builder()
        .versioning(Versioning::builder().enabled(false).build())
        .labels(new_labels)
        .build();

    let updated_bucket = ctx
        .client
        .update_bucket(bucket_name.clone(), update_config)
        .await
        .expect("Failed to update bucket configuration");

    // Verify updates
    assert!(!updated_bucket.versioning.as_ref().unwrap().enabled);
    assert_eq!(
        updated_bucket
            .labels
            .as_ref()
            .unwrap()
            .get("updated")
            .unwrap(),
        "true"
    );

    println!("✅ Successfully updated bucket configuration");
}

// -------------------------------------------------------------------------
// Notification tests
// -------------------------------------------------------------------------

/// Helper to create a PubSub client from the same credentials used by the GCS test context.
fn create_pubsub_client() -> (alien_gcp_clients::pubsub::PubSubClient, String) {
    let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
        .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set");
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

    (
        alien_gcp_clients::pubsub::PubSubClient::new(Client::new(), config),
        project_id,
    )
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_insert_and_delete_notification(ctx: &mut GcsTestContext) {
    use alien_gcp_clients::pubsub::{PubSubApi, Topic};

    let bucket_name = ctx.generate_unique_bucket_name();

    // Create a test bucket
    let bucket_config = Bucket::builder()
        .location("US-CENTRAL1".to_string())
        .build();
    ctx.create_test_bucket(bucket_name.clone(), bucket_config)
        .await
        .expect("Failed to create bucket for notification test");

    // Create a Pub/Sub topic for the notification
    let (pubsub_client, project_id) = create_pubsub_client();
    let topic_id = format!(
        "alien-test-notif-topic-{}",
        Uuid::new_v4().simple()
    );

    let topic = Topic::builder().build();
    pubsub_client
        .create_topic(topic_id.clone(), topic)
        .await
        .expect("Failed to create Pub/Sub topic for notification test");

    let topic_name = format!("projects/{}/topics/{}", project_id, topic_id);

    // Grant the GCS service agent permission to publish to the topic.
    // GCS notifications require the service agent to have roles/pubsub.publisher.
    {
        use alien_gcp_clients::iam::{Binding, IamPolicy};
        // Look up the project number from the Resource Manager API.
        // The GCS service agent is service-{PROJECT_NUMBER}@gs-project-accounts.iam.gserviceaccount.com
        let project_number = {
            use alien_gcp_clients::resource_manager::{ResourceManagerApi, ResourceManagerClient};
            let gcp_creds_json = std::env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").unwrap();
            let rm_cfg = GcpClientConfig {
                project_id: project_id.clone(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey { json: gcp_creds_json },
                service_overrides: None,
                project_number: None,
            };
            let rm_client = ResourceManagerClient::new(reqwest::Client::new(), rm_cfg);
            let project = rm_client.get_project_metadata(project_id.clone()).await
                .expect("Failed to get project metadata for project number");
            project.project_number.expect("Project metadata missing project_number")
        };
        let gcs_service_agent = format!(
            "serviceAccount:service-{}@gs-project-accounts.iam.gserviceaccount.com",
            project_number
        );
        let policy = IamPolicy {
            bindings: vec![Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec![gcs_service_agent],
                condition: None,
            }],
            etag: None,
            version: Some(1),
            kind: None,
            resource_id: None,
        };
        pubsub_client
            .set_topic_iam_policy(topic_id.clone(), policy)
            .await
            .expect("Failed to grant GCS service agent pubsub.publisher on topic");
    }

    // Insert a notification configuration
    let notification = GcsNotification {
        topic: Some(topic_name.clone()),
        payload_format: Some("JSON_API_V1".to_string()),
        ..Default::default()
    };

    let created_notification = ctx
        .client
        .insert_notification(bucket_name.clone(), notification)
        .await
        .expect("Failed to insert notification");

    assert!(
        created_notification.id.is_some(),
        "Created notification should have a server-assigned ID"
    );
    assert_eq!(
        created_notification.topic.as_deref(),
        Some(format!("//pubsub.googleapis.com/{}", topic_name).as_str()),
        "Topic should match the one we specified"
    );
    // GCS may return payload_format as "JSON_API_V1" or omit it (defaults to JSON_API_V1)
    if let Some(fmt) = &created_notification.payload_format {
        assert_eq!(fmt, "JSON_API_V1", "Payload format should be JSON_API_V1 if present");
    }

    // Delete the notification
    let notification_id = created_notification.id.unwrap();
    ctx.client
        .delete_notification(bucket_name.clone(), notification_id.clone())
        .await
        .expect("Failed to delete notification");

    // Clean up the Pub/Sub topic
    pubsub_client
        .delete_topic(topic_id)
        .await
        .expect("Failed to delete Pub/Sub topic");
}

#[test_context(GcsTestContext)]
#[tokio::test]
async fn test_insert_notification_with_event_types(ctx: &mut GcsTestContext) {
    use alien_gcp_clients::pubsub::{PubSubApi, Topic};

    let bucket_name = ctx.generate_unique_bucket_name();

    // Create a test bucket
    let bucket_config = Bucket::builder()
        .location("US-CENTRAL1".to_string())
        .build();
    ctx.create_test_bucket(bucket_name.clone(), bucket_config)
        .await
        .expect("Failed to create bucket for notification event types test");

    // Create a Pub/Sub topic
    let (pubsub_client, project_id) = create_pubsub_client();
    let topic_id = format!(
        "alien-test-notif-evt-topic-{}",
        Uuid::new_v4().simple()
    );

    let topic = Topic::builder().build();
    pubsub_client
        .create_topic(topic_id.clone(), topic)
        .await
        .expect("Failed to create Pub/Sub topic for event types test");

    let topic_name = format!("projects/{}/topics/{}", project_id, topic_id);

    // Grant the GCS service agent permission to publish to the topic
    {
        use alien_gcp_clients::iam::{Binding, IamPolicy};
        // Look up the project number from the Resource Manager API.
        // The GCS service agent is service-{PROJECT_NUMBER}@gs-project-accounts.iam.gserviceaccount.com
        let project_number = {
            use alien_gcp_clients::resource_manager::{ResourceManagerApi, ResourceManagerClient};
            let gcp_creds_json = std::env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").unwrap();
            let rm_cfg = GcpClientConfig {
                project_id: project_id.clone(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey { json: gcp_creds_json },
                service_overrides: None,
                project_number: None,
            };
            let rm_client = ResourceManagerClient::new(reqwest::Client::new(), rm_cfg);
            let project = rm_client.get_project_metadata(project_id.clone()).await
                .expect("Failed to get project metadata for project number");
            project.project_number.expect("Project metadata missing project_number")
        };
        let gcs_service_agent = format!(
            "serviceAccount:service-{}@gs-project-accounts.iam.gserviceaccount.com",
            project_number
        );
        let policy = IamPolicy {
            bindings: vec![Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec![gcs_service_agent],
                condition: None,
            }],
            etag: None,
            version: Some(1),
            kind: None,
            resource_id: None,
        };
        pubsub_client
            .set_topic_iam_policy(topic_id.clone(), policy)
            .await
            .expect("Failed to grant GCS service agent pubsub.publisher on topic");
    }

    // Insert a notification with specific event types and a prefix filter
    let mut custom_attrs = std::collections::HashMap::new();
    custom_attrs.insert("env".to_string(), "test".to_string());

    let notification = GcsNotification {
        topic: Some(topic_name.clone()),
        event_types: vec![
            "OBJECT_FINALIZE".to_string(),
            "OBJECT_DELETE".to_string(),
        ],
        payload_format: Some("JSON_API_V1".to_string()),
        object_name_prefix: Some("uploads/".to_string()),
        custom_attributes: custom_attrs.clone(),
        ..Default::default()
    };

    let created_notification = ctx
        .client
        .insert_notification(bucket_name.clone(), notification)
        .await
        .expect("Failed to insert notification with event types");

    assert!(
        created_notification.id.is_some(),
        "Created notification should have a server-assigned ID"
    );
    // GCS may return event_types in the response or omit them.
    // If returned, verify they match what we set.
    if !created_notification.event_types.is_empty() {
        assert_eq!(created_notification.event_types.len(), 2, "Should have two event types");
        assert!(created_notification.event_types.contains(&"OBJECT_FINALIZE".to_string()));
        assert!(created_notification.event_types.contains(&"OBJECT_DELETE".to_string()));
    }
    if let Some(prefix) = &created_notification.object_name_prefix {
        assert_eq!(prefix, "uploads/", "Object name prefix should match if returned");
    }
    if let Some(env_val) = created_notification.custom_attributes.get("env") {
        assert_eq!(env_val, "test", "Custom attribute 'env' should match if returned");
    }

    // Clean up: delete the notification, then the topic
    let notification_id = created_notification.id.unwrap();
    ctx.client
        .delete_notification(bucket_name.clone(), notification_id)
        .await
        .expect("Failed to delete notification");

    pubsub_client
        .delete_topic(topic_id)
        .await
        .expect("Failed to delete Pub/Sub topic");
}
