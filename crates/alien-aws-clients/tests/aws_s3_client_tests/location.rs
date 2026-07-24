use crate::context::S3TestContext;
use alien_aws_clients::s3::{S3Api, S3Client};
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use test_context::test_context;
use tracing::info;

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
    // Rebuild client with us-east-1 region
    let root: StdPathBuf = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
    let access_key_id =
        env::var("AWS_MANAGEMENT_ACCESS_KEY_ID").expect("AWS_MANAGEMENT_ACCESS_KEY_ID not set");
    let secret_access_key = env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
        .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY not set");
    let session_token = env::var("AWS_SESSION_TOKEN").ok();
    let account_id =
        env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id,
        region: "us-east-1".to_string(),
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        },
        service_overrides: None,
    };
    ctx.client = S3Client::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(aws_config),
    );

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
            region,
            ctx.client.region(),
            "Bucket {} region '{}' doesn't match client region '{}'",
            bucket_name,
            region,
            ctx.client.region()
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
        let client_clone = S3Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );
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
            ctx.client.region(),
            "Request {} returned wrong region",
            request_id
        );
    }
}
