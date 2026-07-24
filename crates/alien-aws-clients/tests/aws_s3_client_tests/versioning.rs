use crate::context::{put_test_object, S3TestContext};
use alien_aws_clients::s3::{
    GetObjectRequest, HeadObjectRequest, ObjectIdentifier, PutObjectRequest, S3Api,
    VersioningStatus,
};
use test_context::test_context;

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
    assert!(
        delete_bucket_result.is_ok(),
        "Failed to delete bucket after versioned empty_bucket, likely not empty: {:?}. List output: {:?}",
        delete_bucket_result.err(),
        list_after_empty_result
    );
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
