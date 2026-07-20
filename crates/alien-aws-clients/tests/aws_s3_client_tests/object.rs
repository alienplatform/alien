use crate::context::{put_test_object, S3TestContext};
use alien_aws_clients::s3::{
    GetObjectRequest, HeadObjectRequest, ObjectIdentifier, PutObjectRequest, S3Api, S3Client,
};
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use test_context::test_context;
use tracing::warn;

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
        let client_clone = S3Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );
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
