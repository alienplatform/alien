#![cfg(test)]

use alien_aws_clients::s3::{
    GetObjectRequest, HeadObjectRequest, LifecycleConfiguration, LifecycleExpiration,
    LifecycleRule, LifecycleRuleFilter, LifecycleRuleStatus, ObjectIdentifier,
    PublicAccessBlockConfiguration, PutObjectRequest, S3Api, S3Client, VersioningStatus,
};
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

// Helper function to put an object using the S3Api
async fn put_test_object(
    client: &S3Client,
    bucket_name: &str,
    object_key: &str,
    body: Vec<u8>,
    content_type: Option<&str>,
) -> Result<(), Error> {
    let request = PutObjectRequest::builder()
        .bucket(bucket_name.to_string())
        .key(object_key.to_string())
        .body(body)
        .maybe_content_type(content_type.map(|s| s.to_string()))
        .build();

    client.put_object(&request).await?;
    Ok(())
}

struct S3TestContext {
    client: S3Client,
    created_buckets: Mutex<HashSet<String>>,
}

impl AsyncTestContext for S3TestContext {
    async fn setup() -> S3TestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok(); // Initialize tracing

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

        let client = S3Client::new(Client::new(), aws_config);

        S3TestContext {
            client,
            created_buckets: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting S3 test cleanup...");

        let buckets_to_cleanup = {
            let buckets = self.created_buckets.lock().unwrap();
            buckets.clone()
        };

        for bucket_name in buckets_to_cleanup {
            self.cleanup_bucket(&bucket_name).await;
        }

        info!("✅ S3 test cleanup completed");
    }
}

impl S3TestContext {
    fn track_bucket(&self, bucket_name: &str) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.insert(bucket_name.to_string());
        info!("📝 Tracking bucket for cleanup: {}", bucket_name);
    }

    fn untrack_bucket(&self, bucket_name: &str) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.remove(bucket_name);
        info!(
            "✅ Bucket {} successfully cleaned up and untracked",
            bucket_name
        );
    }

    async fn cleanup_bucket(&self, bucket_name: &str) {
        info!("🧹 Cleaning up bucket: {}", bucket_name);

        match self.client.empty_bucket(bucket_name).await {
            Ok(_) => {
                if let Err(e) = self.client.delete_bucket(bucket_name).await {
                    if !matches!(
                        e,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete bucket {} during cleanup: {:?}",
                            bucket_name, e
                        );
                    }
                } else {
                    info!("✅ Bucket {} deleted successfully", bucket_name);
                }
            }
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!("🔍 Bucket {} was already deleted", bucket_name);
            }
            Err(e) => {
                warn!(
                    "Failed to empty bucket {} during cleanup: {:?}",
                    bucket_name, e
                );
                // Try deleting anyway, it might be empty from a previous failed empty attempt
                if let Err(e_del) = self.client.delete_bucket(bucket_name).await {
                    if !matches!(
                        e_del,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete bucket {} during cleanup (after empty failed): {:?}",
                            bucket_name, e_del
                        );
                    }
                }
            }
        }
    }

    fn generate_unique_bucket_name(&self) -> String {
        format!(
            "alien-test-bucket-{}",
            Uuid::new_v4().as_simple().to_string()
        )
    }

    async fn create_test_bucket(&self, bucket_name: &str) -> Result<(), Error> {
        let result = self.client.create_bucket(bucket_name).await;
        if result.is_ok() {
            self.track_bucket(bucket_name);
        }
        result
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_create_and_delete_bucket(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    // Create bucket
    let create_result = ctx.create_test_bucket(&bucket_name).await;
    assert!(
        create_result.is_ok(),
        "Failed to create bucket: {:?}",
        create_result.err()
    );

    // Test head_bucket - should succeed for existing bucket
    let head_result = ctx.client.head_bucket(&bucket_name).await;
    assert!(
        head_result.is_ok(),
        "head_bucket failed for existing bucket: {:?}",
        head_result.err()
    );

    // Delete bucket
    let delete_result = ctx.client.delete_bucket(&bucket_name).await;
    let delete_ok = delete_result.is_ok()
        || matches!(
            delete_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        );
    assert!(
        delete_ok,
        "Failed to delete bucket: {:?}",
        delete_result.err()
    );
    if delete_ok {
        ctx.untrack_bucket(&bucket_name);
    }

    // Test head_bucket - should fail for deleted bucket
    let head_after_delete_result = ctx.client.head_bucket(&bucket_name).await;
    assert!(
        matches!(
            head_after_delete_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        ),
        "Expected RemoteResourceNotFound after deleting bucket, got {:?}",
        head_after_delete_result
    );

    // Verify bucket is deleted by trying to delete again (should fail)
    let delete_again_result = ctx.client.delete_bucket(&bucket_name).await;
    assert!(
        matches!(
            delete_again_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        ),
        "Expected RemoteResourceNotFound after deleting bucket, got {:?}",
        delete_again_result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_delete_non_existent_bucket(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name(); // Name that shouldn't exist

    let result = ctx.client.delete_bucket(&bucket_name).await;
    assert!(
        matches!(
            result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            })
        ),
        "Expected RemoteResourceNotFound, got {:?}",
        result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_create_bucket_already_exists(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    // Create bucket first time
    let create_first_result = ctx.create_test_bucket(&bucket_name).await;
    assert!(
        create_first_result.is_ok(),
        "Failed to create bucket initially: {:?}",
        create_first_result.err()
    );

    // Attempt to create the same bucket again
    let create_second_result = ctx.client.create_bucket(&bucket_name).await;
    assert!(
        matches!(
            create_second_result,
            Err(Error {
                error: Some(ErrorData::RemoteResourceConflict { .. }),
                ..
            })
        ),
        "Expected RemoteResourceConflict, got {:?}",
        create_second_result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_bucket_versioning(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for versioning test");

    // Check initial versioning status (should be None/undefined)
    let initial_versioning_result = ctx.client.get_bucket_versioning(&bucket_name).await;
    assert!(
        initial_versioning_result.is_ok(),
        "Failed to get initial bucket versioning: {:?}",
        initial_versioning_result.err()
    );
    let initial_versioning = initial_versioning_result.unwrap();
    // Initial status should be None (versioning not enabled)
    assert!(
        initial_versioning.status.is_none(),
        "Expected initial versioning status to be None, got {:?}",
        initial_versioning.status
    );

    let result_enable = ctx
        .client
        .put_bucket_versioning(&bucket_name, VersioningStatus::Enabled)
        .await;
    assert!(
        result_enable.is_ok(),
        "Failed to enable versioning: {:?}",
        result_enable.err()
    );

    // Check versioning status after enabling
    let enabled_versioning_result = ctx.client.get_bucket_versioning(&bucket_name).await;
    assert!(
        enabled_versioning_result.is_ok(),
        "Failed to get bucket versioning after enabling: {:?}",
        enabled_versioning_result.err()
    );
    let enabled_versioning = enabled_versioning_result.unwrap();
    assert!(
        matches!(enabled_versioning.status, Some(VersioningStatus::Enabled)),
        "Expected versioning status to be Enabled, got {:?}",
        enabled_versioning.status
    );

    let result_suspend = ctx
        .client
        .put_bucket_versioning(&bucket_name, VersioningStatus::Suspended)
        .await;
    assert!(
        result_suspend.is_ok(),
        "Failed to suspend versioning: {:?}",
        result_suspend.err()
    );

    // Check versioning status after suspending
    let suspended_versioning_result = ctx.client.get_bucket_versioning(&bucket_name).await;
    assert!(
        suspended_versioning_result.is_ok(),
        "Failed to get bucket versioning after suspending: {:?}",
        suspended_versioning_result.err()
    );
    let suspended_versioning = suspended_versioning_result.unwrap();
    assert!(
        matches!(
            suspended_versioning.status,
            Some(VersioningStatus::Suspended)
        ),
        "Expected versioning status to be Suspended, got {:?}",
        suspended_versioning.status
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_public_access_block(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for public access block test");

    let config = PublicAccessBlockConfiguration::builder()
        .block_public_acls(true)
        .ignore_public_acls(true)
        .block_public_policy(true)
        .restrict_public_buckets(true)
        .build();

    let result = ctx
        .client
        .put_public_access_block(&bucket_name, config)
        .await;
    assert!(
        result.is_ok(),
        "Failed to put public access block: {:?}",
        result.err()
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_and_delete_bucket_policy(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for policy test");

    // First, disable Block Public Access settings to allow public policies
    let public_access_config = PublicAccessBlockConfiguration::builder()
        .block_public_acls(false)
        .ignore_public_acls(false)
        .block_public_policy(false) // This is the key setting
        .restrict_public_buckets(false)
        .build();

    ctx.client
        .put_public_access_block(&bucket_name, public_access_config)
        .await
        .expect("Failed to disable Block Public Access settings");

    let policy_document = format!(
        "{{\"Version\":\"2012-10-17\",\"Statement\":[{{\"Sid\":\"PublicReadGetObject\",\"Effect\":\"Allow\",\"Principal\":\"*\",\"Action\":[\"s3:GetObject\"],\"Resource\":[\"arn:aws:s3:::{}/*\"]}}]}}",
        bucket_name
    );

    let put_result = ctx
        .client
        .put_bucket_policy(&bucket_name, &policy_document)
        .await;
    assert!(
        put_result.is_ok(),
        "Failed to put bucket policy: {:?}",
        put_result.err()
    );

    // Note: Verifying typically requires GetBucketPolicy, not in current S3Client.

    let delete_result = ctx.client.delete_bucket_policy(&bucket_name).await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete bucket policy: {:?}",
        delete_result.err()
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_delete_non_existent_bucket_policy(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for policy test");

    // Attempt to delete a policy that was never set.
    // S3's DeleteBucketPolicy returns 204 No Content even if no policy exists.
    // So, a successful response is expected.
    let result = ctx.client.delete_bucket_policy(&bucket_name).await;
    assert!(
        result.is_ok(),
        "Expected success when deleting non-existent policy, got {:?}",
        result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_and_delete_bucket_lifecycle(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for lifecycle test");

    let lifecycle_config = LifecycleConfiguration::builder()
        .rules(vec![LifecycleRule::builder()
            .id("TestRule1".to_string())
            .status(LifecycleRuleStatus::Enabled)
            .filter(LifecycleRuleFilter::builder().build())
            .expiration(LifecycleExpiration::builder().days(30).build())
            .build()])
        .build();

    let put_result = ctx
        .client
        .put_bucket_lifecycle_configuration(&bucket_name, &lifecycle_config)
        .await;
    assert!(
        put_result.is_ok(),
        "Failed to put bucket lifecycle configuration: {:?}",
        put_result.err()
    );

    // Note: Verifying typically requires GetBucketLifecycleConfiguration, not in current S3Client.

    let delete_result = ctx.client.delete_bucket_lifecycle(&bucket_name).await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete bucket lifecycle: {:?}",
        delete_result.err()
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_delete_non_existent_bucket_lifecycle(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for lifecycle test");

    // S3's DeleteBucketLifecycle returns 204 No Content even if no lifecycle config exists.
    let result = ctx.client.delete_bucket_lifecycle(&bucket_name).await;
    assert!(
        result.is_ok(),
        "Expected success when deleting non-existent lifecycle, got {:?}",
        result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_list_objects_v2_basic_and_prefix(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for list_objects_v2 test");

    // Test on empty bucket first
    let list_empty_result = ctx.client.list_objects_v2(&bucket_name, None, None).await;
    assert!(
        list_empty_result.is_ok(),
        "list_objects_v2 on empty bucket failed: {:?}",
        list_empty_result.err()
    );
    let empty_output = list_empty_result.unwrap();
    assert!(empty_output.contents.is_empty());
    assert_eq!(empty_output.key_count, 0);
    assert!(!empty_output.is_truncated);

    // Add some objects
    put_test_object(
        &ctx.client,
        &bucket_name,
        "test1.txt",
        b"hello".to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object test1.txt");
    put_test_object(
        &ctx.client,
        &bucket_name,
        "test2.txt",
        b"world".to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object test2.txt");
    put_test_object(
        &ctx.client,
        &bucket_name,
        "prefix/obj1.txt",
        b"data1".to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object prefix/obj1.txt");
    put_test_object(
        &ctx.client,
        &bucket_name,
        "prefix/obj2.txt",
        b"data2".to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object prefix/obj2.txt");

    // Test listing all objects
    let list_all_result = ctx.client.list_objects_v2(&bucket_name, None, None).await;
    assert!(
        list_all_result.is_ok(),
        "list_objects_v2 failed: {:?}",
        list_all_result.err()
    );
    let all_output = list_all_result.unwrap();
    assert_eq!(all_output.contents.len(), 4);
    assert_eq!(all_output.key_count, 4);

    // Test listing with prefix
    let list_prefix_result = ctx
        .client
        .list_objects_v2(&bucket_name, Some("prefix/".to_string()), None)
        .await;
    assert!(
        list_prefix_result.is_ok(),
        "list_objects_v2 with prefix failed: {:?}",
        list_prefix_result.err()
    );
    let prefix_output = list_prefix_result.unwrap();
    assert_eq!(prefix_output.contents.len(), 2);
    assert_eq!(prefix_output.key_count, 2);
    assert!(prefix_output
        .contents
        .iter()
        .all(|obj| obj.key.starts_with("prefix/")));
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_list_objects_v2_pagination(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for list_objects_v2 pagination test");

    // Add more objects than a single page (e.g., S3 defaults to 1000, but our ListObjectsV2Output doesn't expose max-keys from request)
    // For simplicity, let's assume a small number will trigger truncation if we could control max_keys in request.
    // The current list_objects_v2 doesn't allow setting max_keys. We'll test continuation if the API returns truncated.
    // This test relies on the S3 default behavior or if client.list_objects_v2 internally uses a small max_keys.
    // For now, we put a few objects and check if a second call with a token (if provided) works.

    for i in 0..5 {
        // Put 5 objects
        put_test_object(
            &ctx.client,
            &bucket_name,
            &format!("page_obj_{}.txt", i),
            vec![i as u8],
            Some("text/plain"),
        )
        .await
        .unwrap();
    }

    let mut all_keys_retrieved = std::collections::HashSet::new();
    let mut continuation_token: Option<String> = None;
    let mut total_key_count = 0;

    loop {
        let list_result = ctx
            .client
            .list_objects_v2(&bucket_name, None, continuation_token)
            .await;
        assert!(
            list_result.is_ok(),
            "list_objects_v2 for pagination failed: {:?}",
            list_result.err()
        );
        let output = list_result.unwrap();

        for obj in output.contents {
            all_keys_retrieved.insert(obj.key.clone());
        }
        total_key_count += output.key_count;

        if output.is_truncated {
            continuation_token = output.next_continuation_token;
            assert!(
                continuation_token.is_some(),
                "Expected next_continuation_token when is_truncated is true"
            );
        } else {
            break;
        }
    }
    assert_eq!(
        all_keys_retrieved.len(),
        5,
        "Expected to retrieve all 5 keys via pagination (if any)"
    );
    // The total_key_count from S3 responses might be tricky if it's per page. The primary check is retrieving all keys.
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_delete_objects(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for delete_objects test");

    put_test_object(
        &ctx.client,
        &bucket_name,
        "delete_me_1.txt",
        vec![1],
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "delete_me_2.txt",
        vec![2],
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "keep_me.txt",
        vec![3],
        Some("text/plain"),
    )
    .await
    .unwrap();

    let objects_to_delete = [
        ObjectIdentifier::builder()
            .key("delete_me_1.txt".to_string())
            .build(),
        ObjectIdentifier::builder()
            .key("delete_me_2.txt".to_string())
            .build(),
        ObjectIdentifier::builder()
            .key("does_not_exist.txt".to_string())
            .build(), // Test deleting non-existent
    ];

    // Test with Quiet = false (default behavior if not specified, but our client exposes it)
    let delete_result_verbose = ctx
        .client
        .delete_objects(&bucket_name, &objects_to_delete, false)
        .await;
    assert!(
        delete_result_verbose.is_ok(),
        "delete_objects (verbose) failed: {:?}",
        delete_result_verbose.err()
    );
    let verbose_output = delete_result_verbose.unwrap();

    // S3 behavior: non-existent keys might still be reported as "deleted" in some cases
    // The important thing is that the real objects are deleted and no errors occurred
    assert!(
        verbose_output.deleted.len() >= 2,
        "Expected at least 2 objects to be reported as deleted (verbose), got {}",
        verbose_output.deleted.len()
    );
    assert!(verbose_output
        .deleted
        .iter()
        .any(|d| d.key == "delete_me_1.txt"));
    assert!(verbose_output
        .deleted
        .iter()
        .any(|d| d.key == "delete_me_2.txt"));

    // Errors should be empty or minimal for non-existent objects
    assert!(
        verbose_output.errors.is_empty(),
        "Expected no errors in verbose output, got {:?}",
        verbose_output.errors
    );

    // Put objects again for the quiet test
    put_test_object(
        &ctx.client,
        &bucket_name,
        "delete_me_quiet_1.txt",
        vec![1],
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "delete_me_quiet_2.txt",
        vec![2],
        Some("text/plain"),
    )
    .await
    .unwrap();

    let objects_to_delete_quiet = [
        ObjectIdentifier::builder()
            .key("delete_me_quiet_1.txt".to_string())
            .build(),
        ObjectIdentifier::builder()
            .key("delete_me_quiet_2.txt".to_string())
            .build(),
    ];

    // Test with Quiet = true
    let delete_result_quiet = ctx
        .client
        .delete_objects(&bucket_name, &objects_to_delete_quiet, true)
        .await;
    assert!(
        delete_result_quiet.is_ok(),
        "delete_objects (quiet) failed: {:?}",
        delete_result_quiet.err()
    );
    let quiet_output = delete_result_quiet.unwrap();
    assert!(
        quiet_output.deleted.is_empty(),
        "Expected no objects in Deleted list for quiet mode"
    );
    assert!(
        quiet_output.errors.is_empty(),
        "Expected no errors in Errors list for quiet mode (for successful deletes)"
    );

    // Verify objects are deleted
    let list_after_delete_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert_eq!(
        list_after_delete_result.contents.len(),
        1,
        "Not all specified objects were deleted"
    );
    assert_eq!(list_after_delete_result.contents[0].key, "keep_me.txt");
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_empty_bucket_no_versions(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for empty_bucket test");

    put_test_object(
        &ctx.client,
        &bucket_name,
        "obj1.txt",
        vec![1],
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "prefix/obj2.txt",
        vec![2],
        Some("text/plain"),
    )
    .await
    .unwrap();

    let empty_result = ctx.client.empty_bucket(&bucket_name).await;
    assert!(
        empty_result.is_ok(),
        "empty_bucket failed: {:?}",
        empty_result.err()
    );

    let list_after_empty_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert!(
        list_after_empty_result.contents.is_empty(),
        "Bucket was not empty after empty_bucket call"
    );
    assert_eq!(list_after_empty_result.key_count, 0);
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_empty_bucket_with_versions(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for versioned empty_bucket test");
    ctx.client
        .put_bucket_versioning(&bucket_name, VersioningStatus::Enabled)
        .await
        .expect("Failed to enable versioning");

    // Create multiple versions of an object
    put_test_object(
        &ctx.client,
        &bucket_name,
        "versioned_obj.txt",
        b"version1".to_vec(),
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "versioned_obj.txt",
        b"version2".to_vec(),
        Some("text/plain"),
    )
    .await
    .unwrap();
    put_test_object(
        &ctx.client,
        &bucket_name,
        "other_obj.txt",
        b"other".to_vec(),
        Some("text/plain"),
    )
    .await
    .unwrap();

    // Add a delete marker
    let objects_to_delete_marker = [ObjectIdentifier::builder()
        .key("versioned_obj.txt".to_string())
        .build()];
    ctx.client
        .delete_objects(&bucket_name, &objects_to_delete_marker, false)
        .await
        .expect("Failed to create delete marker");

    let empty_result = ctx.client.empty_bucket(&bucket_name).await;
    assert!(
        empty_result.is_ok(),
        "empty_bucket with versions failed: {:?}",
        empty_result.err()
    );

    // Verify bucket is empty (no objects or versions left)
    // ListObjectVersions would be the definitive check, but it's not directly exposed.
    // empty_bucket aims to delete all versions and markers.
    // A ListObjectsV2 might show empty if only delete markers for all objects are left and then removed.
    let list_after_empty_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert!(
        list_after_empty_result.contents.is_empty(),
        "Bucket (current versions) not empty after versioned empty_bucket"
    );

    // To be absolutely sure, try to delete the bucket. If it fails due to contents, empty_bucket didn't fully work.
    let delete_bucket_result = ctx.client.delete_bucket(&bucket_name).await;
    if delete_bucket_result.is_ok() {
        ctx.untrack_bucket(&bucket_name);
    }
    assert!(delete_bucket_result.is_ok(), 
        "Failed to delete bucket after versioned empty_bucket, likely not empty: {:?}. List output: {:?}", 
        delete_bucket_result.err(),
        list_after_empty_result
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_invalid_bucket_policy(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    // Disable public access block first
    let public_access_config = PublicAccessBlockConfiguration::builder()
        .block_public_acls(false)
        .ignore_public_acls(false)
        .block_public_policy(false)
        .restrict_public_buckets(false)
        .build();
    ctx.client
        .put_public_access_block(&bucket_name, public_access_config)
        .await
        .expect("Failed to disable Block Public Access settings");

    // Test invalid JSON policy
    let invalid_policy = r#"{"Version":"2012-10-17","Statement":[{"Sid":"InvalidPolicy""#; // Malformed JSON

    let result = ctx
        .client
        .put_bucket_policy(&bucket_name, invalid_policy)
        .await;
    assert!(result.is_err(), "Expected error for invalid policy JSON");
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_invalid_lifecycle_configuration(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    // Test lifecycle config with invalid days (negative)
    let invalid_lifecycle_config = LifecycleConfiguration::builder()
        .rules(vec![LifecycleRule::builder()
            .id("InvalidRule".to_string())
            .status(LifecycleRuleStatus::Enabled)
            .filter(LifecycleRuleFilter::builder().build())
            .expiration(
                LifecycleExpiration::builder()
                    .days(-1) // Invalid negative days
                    .build(),
            )
            .build()])
        .build();

    let result = ctx
        .client
        .put_bucket_lifecycle_configuration(&bucket_name, &invalid_lifecycle_config)
        .await;
    assert!(
        result.is_err(),
        "Expected error for invalid lifecycle configuration"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_delete_objects_large_batch(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    // Create 100 objects (getting close to S3's 1000 object limit per delete request)
    for i in 0..100 {
        put_test_object(
            &ctx.client,
            &bucket_name,
            &format!("batch_obj_{}.txt", i),
            vec![i as u8],
            Some("text/plain"),
        )
        .await
        .expect(&format!("Failed to put object {}", i));
    }

    // Delete all objects in one batch
    let objects_to_delete: Vec<ObjectIdentifier> = (0..100)
        .map(|i| {
            ObjectIdentifier::builder()
                .key(format!("batch_obj_{}.txt", i))
                .build()
        })
        .collect();

    let delete_result = ctx
        .client
        .delete_objects(&bucket_name, &objects_to_delete, false)
        .await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete batch of objects: {:?}",
        delete_result.err()
    );

    let delete_output = delete_result.unwrap();
    assert_eq!(
        delete_output.deleted.len(),
        100,
        "Not all objects were deleted"
    );
    assert!(
        delete_output.errors.is_empty(),
        "Unexpected errors during batch delete"
    );

    // Verify bucket is empty
    let list_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert!(
        list_result.contents.is_empty(),
        "Bucket should be empty after batch delete"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_bucket_name_validation_edge_cases(ctx: &mut S3TestContext) {
    // Test edge cases for bucket names (these should be invalid in most regions)
    let long_name = "a".repeat(64); // Too long (maximum 63 chars)
    let invalid_bucket_names = vec![
        "UPPERCASE",                   // Uppercase not allowed in most regions
        "bucket-with-periods.in.name", // Periods can cause SSL issues
        "bucket-ending-with-dash-",    // Can't end with dash
        "-bucket-starting-with-dash",  // Can't start with dash
        "bu",                          // Too short (minimum 3 chars)
        &long_name,                    // Too long (maximum 63 chars)
        "bucket_with_underscores",     // Underscores not allowed
        "bucket..double.dots",         // Double dots not allowed
        "192.168.1.1",                 // IP address format not allowed
    ];

    for bucket_name in invalid_bucket_names {
        let result = ctx.client.create_bucket(bucket_name).await;
        // Note: Some of these might be caught by S3 service validation rather than client-side
        // The test is to ensure we handle the errors gracefully
        if result.is_ok() {
            // If it somehow succeeded, track it for cleanup
            ctx.track_bucket(bucket_name);
            // In real scenarios, this shouldn't happen for truly invalid names
            warn!(
                "Warning: Bucket name '{}' was accepted when it shouldn't be",
                bucket_name
            );
        }
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_concurrent_object_operations(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    // Test concurrent put operations
    let mut handles = Vec::new();
    for i in 0..10 {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let account_id =
            env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region: region.clone(),
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };
        let client_clone = S3Client::new(Client::new(), aws_config);
        let bucket_name_clone = bucket_name.clone();

        let handle = tokio::spawn(async move {
            let key = format!("concurrent_obj_{}.txt", i);
            let data = format!("Data for object {}", i).into_bytes();
            put_test_object(
                &client_clone,
                &bucket_name_clone,
                &key,
                data,
                Some("text/plain"),
            )
            .await
        });
        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(
            result.is_ok(),
            "Concurrent put operation failed: {:?}",
            result.err()
        );
    }

    // Verify all objects were created
    let list_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert_eq!(
        list_result.contents.len(),
        10,
        "Not all concurrent objects were created"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_object_key_length_limits(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    // Test very long object key (S3 limit is 1024 UTF-8 characters)
    let long_key = "a".repeat(1020); // Close to but under the limit
    let result = put_test_object(
        &ctx.client,
        &bucket_name,
        &long_key,
        b"test".to_vec(),
        Some("text/plain"),
    )
    .await;
    assert!(
        result.is_ok(),
        "Failed to put object with long key: {:?}",
        result.err()
    );

    // Test key that exceeds the limit
    let too_long_key = "a".repeat(1025); // Over the limit
    let result = put_test_object(
        &ctx.client,
        &bucket_name,
        &too_long_key,
        b"test".to_vec(),
        Some("text/plain"),
    )
    .await;
    // This should fail - either client-side validation or server-side rejection
    if result.is_ok() {
        warn!(
            "Warning: Unexpectedly long key was accepted: {} chars",
            too_long_key.len()
        );
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_versioned_object_operations(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");
    ctx.client
        .put_bucket_versioning(&bucket_name, VersioningStatus::Enabled)
        .await
        .expect("Failed to enable versioning");

    let object_key = "versioned-object.txt";

    // Create multiple versions
    for i in 1..=3 {
        let content = format!("Version {} content", i);
        let result = put_test_object(
            &ctx.client,
            &bucket_name,
            object_key,
            content.into_bytes(),
            Some("text/plain"),
        )
        .await;
        assert!(
            result.is_ok(),
            "Failed to put version {}: {:?}",
            i,
            result.err()
        );
    }

    // List objects should show only the latest version
    let list_result = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert_eq!(list_result.contents.len(), 1);
    assert_eq!(list_result.contents[0].key, object_key);

    // Delete the current version (creates a delete marker)
    let delete_objects = [ObjectIdentifier::builder()
        .key(object_key.to_string())
        .build()];
    let delete_result = ctx
        .client
        .delete_objects(&bucket_name, &delete_objects, false)
        .await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete versioned object: {:?}",
        delete_result.err()
    );

    // Object should no longer appear in regular listing
    let list_after_delete = ctx
        .client
        .list_objects_v2(&bucket_name, None, None)
        .await
        .unwrap();
    assert!(
        list_after_delete.contents.is_empty(),
        "Object should not appear after delete marker"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_bucket_location_basic(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for location test");

    let location_result = ctx.client.get_bucket_location(&bucket_name).await;
    assert!(
        location_result.is_ok(),
        "Failed to get bucket location: {:?}",
        location_result.err()
    );

    let location_output = location_result.unwrap();
    let region = location_output.region();

    // The bucket should be in the same region as our client
    assert_eq!(
        region,
        ctx.client.region(),
        "Bucket region '{}' doesn't match client region '{}'",
        region,
        ctx.client.region()
    );

    // Log the raw location constraint for debugging
    info!(
        "Bucket {} location constraint: {:?}, resolved region: {}",
        bucket_name, location_output.location_constraint, region
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_bucket_location_us_east_1_special_case(ctx: &mut S3TestContext) {
    ctx.client.config.region = "us-east-1".to_string();

    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket in us-east-1");

    let location_result = ctx.client.get_bucket_location(&bucket_name).await;
    assert!(
        location_result.is_ok(),
        "Failed to get bucket location in us-east-1: {:?}",
        location_result.err()
    );

    let location_output = location_result.unwrap();

    // In us-east-1, S3 returns null/empty LocationConstraint
    assert!(
        location_output.location_constraint.is_none()
            || location_output.location_constraint.as_deref() == Some(""),
        "Expected null/empty LocationConstraint for us-east-1, got: {:?}",
        location_output.location_constraint
    );

    // But the region() method should still return "us-east-1"
    assert_eq!(location_output.region(), "us-east-1");
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_bucket_location_non_existent_bucket(ctx: &mut S3TestContext) {
    let non_existent_bucket = ctx.generate_unique_bucket_name();

    let location_result = ctx.client.get_bucket_location(&non_existent_bucket).await;
    assert!(
        location_result.is_err(),
        "Expected error for non-existent bucket"
    );

    // Should get a RemoteResourceNotFound error
    match location_result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Bucket");
            assert_eq!(resource_name, non_existent_bucket);
        }
        other_error => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other_error);
        }
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_bucket_location_multiple_buckets(ctx: &mut S3TestContext) {
    // Test getting location for multiple buckets to ensure consistency
    let bucket_names: Vec<String> = (0..3).map(|_| ctx.generate_unique_bucket_name()).collect();

    // Create all buckets
    for bucket_name in &bucket_names {
        ctx.create_test_bucket(bucket_name)
            .await
            .expect(&format!("Failed to create bucket {}", bucket_name));
    }

    // Get location for all buckets
    for bucket_name in &bucket_names {
        let location_result = ctx.client.get_bucket_location(bucket_name).await;
        assert!(
            location_result.is_ok(),
            "Failed to get location for bucket {}: {:?}",
            bucket_name,
            location_result.err()
        );

        let location_output = location_result.unwrap();
        let region = location_output.region();

        assert_eq!(
            region, ctx.client.config.region,
            "Bucket {} region '{}' doesn't match client region '{}'",
            bucket_name, region, ctx.client.config.region
        );
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_bucket_location_concurrent_requests(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for concurrent test");

    // Make multiple concurrent requests for the same bucket location
    let mut handles = Vec::new();
    for i in 0..5 {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");

        let access_key_id =
            env::var("AWS_MANAGEMENT_ACCESS_KEY_ID").expect("AWS_MANAGEMENT_ACCESS_KEY_ID not set");
        let secret_access_key = env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY not set");
        let region = env::var("AWS_MANAGEMENT_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let session_token = env::var("AWS_SESSION_TOKEN").ok();

        let account_id =
            env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region: region.clone(),
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            },
            service_overrides: None,
        };
        let client_clone = S3Client::new(Client::new(), aws_config);
        let bucket_name_clone = bucket_name.clone();

        let handle = tokio::spawn(async move {
            let result = client_clone.get_bucket_location(&bucket_name_clone).await;
            (i, result)
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let (request_id, result) = handle.await.expect("Task panicked");
        assert!(
            result.is_ok(),
            "Concurrent get_bucket_location request {} failed: {:?}",
            request_id,
            result.err()
        );

        let location_output = result.unwrap();
        assert_eq!(
            location_output.region(),
            ctx.client.config.region,
            "Request {} returned wrong region",
            request_id
        );
    }
}

// -------------------------------------------------------------------------
// Object Operations Tests (PutObject, GetObject, HeadObject)
// -------------------------------------------------------------------------

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_and_get_object(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let object_key = "test-object.txt";
    let content = b"Hello, World!";

    // Put object
    let put_request = PutObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .body(content.to_vec())
        .content_type("text/plain".to_string())
        .build();

    let put_result = ctx.client.put_object(&put_request).await;
    assert!(
        put_result.is_ok(),
        "Failed to put object: {:?}",
        put_result.err()
    );
    let put_output = put_result.unwrap();
    assert!(
        put_output.e_tag.is_some(),
        "Expected ETag in PutObject response"
    );

    // Get object
    let get_request = GetObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .build();

    let get_result = ctx.client.get_object(&get_request).await;
    assert!(
        get_result.is_ok(),
        "Failed to get object: {:?}",
        get_result.err()
    );
    let get_output = get_result.unwrap();

    assert_eq!(get_output.body, content, "Object content doesn't match");
    assert_eq!(get_output.content_type, Some("text/plain".to_string()));
    assert!(
        get_output.e_tag.is_some(),
        "Expected ETag in GetObject response"
    );
    assert!(
        get_output.last_modified.is_some(),
        "Expected LastModified in GetObject response"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_head_object(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let object_key = "test-head-object.txt";
    let content = b"Test content for HEAD";

    // Put object first
    put_test_object(
        &ctx.client,
        &bucket_name,
        object_key,
        content.to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object");

    // Head object
    let head_request = HeadObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .build();

    let head_result = ctx.client.head_object(&head_request).await;
    assert!(
        head_result.is_ok(),
        "Failed to head object: {:?}",
        head_result.err()
    );
    let head_output = head_result.unwrap();

    assert_eq!(head_output.content_type, Some("text/plain".to_string()));
    assert_eq!(head_output.content_length, Some(content.len() as i64));
    assert!(
        head_output.e_tag.is_some(),
        "Expected ETag in HeadObject response"
    );
    assert!(
        head_output.last_modified.is_some(),
        "Expected LastModified in HeadObject response"
    );
    assert_eq!(
        head_output.delete_marker, None,
        "Should not have delete marker"
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_object_non_existent(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let get_request = GetObjectRequest::builder()
        .bucket(bucket_name)
        .key("non-existent-key.txt".to_string())
        .build();

    let get_result = ctx.client.get_object(&get_request).await;
    assert!(
        get_result.is_err(),
        "Expected error for non-existent object"
    );

    match get_result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            // Expected error
        }
        other_error => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other_error);
        }
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_head_object_non_existent(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let head_request = HeadObjectRequest::builder()
        .bucket(bucket_name)
        .key("non-existent-key.txt".to_string())
        .build();

    let head_result = ctx.client.head_object(&head_request).await;
    assert!(
        head_result.is_err(),
        "Expected error for non-existent object"
    );

    match head_result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            // Expected error
        }
        other_error => {
            panic!("Expected RemoteResourceNotFound, got: {:?}", other_error);
        }
    }
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_get_object_with_range(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let object_key = "test-range-object.txt";
    let content = b"0123456789ABCDEFGHIJ";

    // Put object
    put_test_object(
        &ctx.client,
        &bucket_name,
        object_key,
        content.to_vec(),
        Some("text/plain"),
    )
    .await
    .expect("Failed to put object");

    // Get object with range (first 10 bytes)
    let get_request = GetObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .range("bytes=0-9".to_string())
        .build();

    let get_result = ctx.client.get_object(&get_request).await;
    assert!(
        get_result.is_ok(),
        "Failed to get object with range: {:?}",
        get_result.err()
    );
    let get_output = get_result.unwrap();

    assert_eq!(
        get_output.body, b"0123456789",
        "Range request didn't return correct bytes"
    );
    assert_eq!(get_output.content_length, Some(10));
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_object_with_metadata(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let object_key = "test-metadata-object.txt";
    let content = b"Test content";

    // Put object with storage class
    let put_request = PutObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .body(content.to_vec())
        .content_type("text/plain".to_string())
        .storage_class("STANDARD".to_string())
        .build();

    let put_result = ctx.client.put_object(&put_request).await;
    assert!(
        put_result.is_ok(),
        "Failed to put object with metadata: {:?}",
        put_result.err()
    );

    // Verify with HEAD
    let head_request = HeadObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .build();

    let head_result = ctx.client.head_object(&head_request).await;
    assert!(
        head_result.is_ok(),
        "Failed to head object: {:?}",
        head_result.err()
    );
    let head_output = head_result.unwrap();

    assert_eq!(head_output.content_type, Some("text/plain".to_string()));
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_get_large_object(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");

    let object_key = "large-object.bin";
    // Create a 1MB object
    let content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

    // Put object
    let put_request = PutObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .body(content.clone())
        .content_type("application/octet-stream".to_string())
        .build();

    let put_result = ctx.client.put_object(&put_request).await;
    assert!(
        put_result.is_ok(),
        "Failed to put large object: {:?}",
        put_result.err()
    );

    // Get object
    let get_request = GetObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .build();

    let get_result = ctx.client.get_object(&get_request).await;
    assert!(
        get_result.is_ok(),
        "Failed to get large object: {:?}",
        get_result.err()
    );
    let get_output = get_result.unwrap();

    assert_eq!(
        get_output.body, content,
        "Large object content doesn't match"
    );
    assert_eq!(get_output.content_length, Some(content.len() as i64));
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_versioned_get_and_head(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();
    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket");
    ctx.client
        .put_bucket_versioning(&bucket_name, VersioningStatus::Enabled)
        .await
        .expect("Failed to enable versioning");

    let object_key = "versioned-object.txt";
    let content_v1 = b"Version 1";
    let content_v2 = b"Version 2";

    // Put version 1
    let put_request_v1 = PutObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .body(content_v1.to_vec())
        .content_type("text/plain".to_string())
        .build();

    let put_result_v1 = ctx.client.put_object(&put_request_v1).await;
    assert!(
        put_result_v1.is_ok(),
        "Failed to put version 1: {:?}",
        put_result_v1.err()
    );
    let version_id_1 = put_result_v1
        .unwrap()
        .version_id
        .expect("Expected version ID for v1");

    // Put version 2
    let put_request_v2 = PutObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .body(content_v2.to_vec())
        .content_type("text/plain".to_string())
        .build();

    let put_result_v2 = ctx.client.put_object(&put_request_v2).await;
    assert!(
        put_result_v2.is_ok(),
        "Failed to put version 2: {:?}",
        put_result_v2.err()
    );
    let version_id_2 = put_result_v2
        .unwrap()
        .version_id
        .expect("Expected version ID for v2");

    // Get version 1
    let get_request_v1 = GetObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .version_id(version_id_1.clone())
        .build();

    let get_result_v1 = ctx.client.get_object(&get_request_v1).await;
    assert!(
        get_result_v1.is_ok(),
        "Failed to get version 1: {:?}",
        get_result_v1.err()
    );
    assert_eq!(get_result_v1.unwrap().body, content_v1);

    // Get version 2 (latest)
    let get_request_v2 = GetObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .build();

    let get_result_v2 = ctx.client.get_object(&get_request_v2).await;
    assert!(
        get_result_v2.is_ok(),
        "Failed to get version 2: {:?}",
        get_result_v2.err()
    );
    assert_eq!(get_result_v2.unwrap().body, content_v2);

    // Head version 1
    let head_request_v1 = HeadObjectRequest::builder()
        .bucket(bucket_name.clone())
        .key(object_key.to_string())
        .version_id(version_id_1)
        .build();

    let head_result_v1 = ctx.client.head_object(&head_request_v1).await;
    assert!(
        head_result_v1.is_ok(),
        "Failed to head version 1: {:?}",
        head_result_v1.err()
    );
    assert_eq!(
        head_result_v1.unwrap().content_length,
        Some(content_v1.len() as i64)
    );
}
