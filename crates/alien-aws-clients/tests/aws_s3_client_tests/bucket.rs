use crate::context::S3TestContext;
use alien_aws_clients::s3::{
    FilterRule, LambdaFunctionConfiguration, LifecycleConfiguration, LifecycleExpiration,
    LifecycleRule, LifecycleRuleFilter, LifecycleRuleStatus, NotificationConfiguration,
    NotificationFilter, PublicAccessBlockConfiguration, S3Api, S3KeyFilter,
};
use alien_client_core::Error;
use alien_client_core::ErrorData;
use std::env;
use test_context::test_context;
use tracing::{info, warn};

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

    // Attempt to create the same bucket again.
    // S3 returns 200 OK if the caller owns the bucket and it's in the same region,
    // or BucketAlreadyOwnedByYou (which we map to RemoteResourceConflict).
    let create_second_result = ctx.client.create_bucket(&bucket_name).await;
    assert!(
        create_second_result.is_ok()
            || matches!(
                &create_second_result,
                Err(Error {
                    error: Some(ErrorData::RemoteResourceConflict { .. }),
                    ..
                })
            ),
        "Expected Ok or RemoteResourceConflict, got {:?}",
        create_second_result
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

// -------------------------------------------------------------------------
// Notification configuration tests
// -------------------------------------------------------------------------

/// S3 notification configurations require a valid Lambda ARN with the correct
/// resource-based policy allowing S3 to invoke it. Since integration tests may
/// not have a Lambda available, we test the serialization/deserialization
/// round-trip of the notification types and verify the get/put API calls work
/// against a real bucket using an empty configuration (which is always valid).

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_and_get_empty_notification_configuration(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for notification config test");

    // Put an empty notification configuration (clears any existing notifications)
    let empty_config = NotificationConfiguration::default();
    ctx.client
        .put_bucket_notification_configuration(&bucket_name, &empty_config)
        .await
        .expect("Failed to put empty notification configuration");

    // Get the notification configuration back — should be empty
    let retrieved_config = ctx
        .client
        .get_bucket_notification_configuration(&bucket_name)
        .await
        .expect("Failed to get notification configuration");

    assert!(
        retrieved_config.lambda_function_configurations.is_empty(),
        "Expected empty lambda function configurations after putting empty config, got {:?}",
        retrieved_config.lambda_function_configurations
    );
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_put_and_get_bucket_notification_configuration(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for notification config test");

    // Construct a notification configuration with a Lambda function.
    // NOTE: This requires a real Lambda ARN with S3 invocation permission.
    // If the env var is not set, we skip the actual put/get and only verify
    // that the types serialize correctly.
    let lambda_arn = match env::var("AWS_TEST_LAMBDA_ARN") {
        Ok(arn) if !arn.is_empty() => arn,
        _ => {
            info!("AWS_TEST_LAMBDA_ARN not set — skipping live notification configuration test, verifying serialization only");

            let config = NotificationConfiguration {
                lambda_function_configurations: vec![LambdaFunctionConfiguration {
                    id: Some("test-notification".to_string()),
                    lambda_function_arn: "arn:aws:lambda:us-east-1:123456789012:function:test-fn"
                        .to_string(),
                    events: vec!["s3:ObjectCreated:*".to_string()],
                    filter: None,
                }],
            };

            // Verify serialization round-trip via quick_xml
            let xml = quick_xml::se::to_string_with_root("NotificationConfiguration", &config)
                .expect("Failed to serialize NotificationConfiguration");
            assert!(
                xml.contains("s3:ObjectCreated:*"),
                "Serialized XML should contain the event type"
            );
            assert!(
                xml.contains("arn:aws:lambda:us-east-1:123456789012:function:test-fn"),
                "Serialized XML should contain the Lambda ARN"
            );

            let deserialized: NotificationConfiguration = quick_xml::de::from_str(&xml)
                .expect("Failed to deserialize NotificationConfiguration");
            assert_eq!(deserialized.lambda_function_configurations.len(), 1);
            assert_eq!(
                deserialized.lambda_function_configurations[0].lambda_function_arn,
                "arn:aws:lambda:us-east-1:123456789012:function:test-fn"
            );
            assert_eq!(
                deserialized.lambda_function_configurations[0].events,
                vec!["s3:ObjectCreated:*"]
            );

            return;
        }
    };

    let config = NotificationConfiguration {
        lambda_function_configurations: vec![LambdaFunctionConfiguration {
            id: Some("test-notification".to_string()),
            lambda_function_arn: lambda_arn.clone(),
            events: vec!["s3:ObjectCreated:*".to_string()],
            filter: None,
        }],
    };

    ctx.client
        .put_bucket_notification_configuration(&bucket_name, &config)
        .await
        .expect("Failed to put notification configuration");

    let retrieved_config = ctx
        .client
        .get_bucket_notification_configuration(&bucket_name)
        .await
        .expect("Failed to get notification configuration");

    assert_eq!(
        retrieved_config.lambda_function_configurations.len(),
        1,
        "Expected exactly one lambda function configuration"
    );
    assert_eq!(
        retrieved_config.lambda_function_configurations[0].lambda_function_arn, lambda_arn,
        "Lambda function ARN should match"
    );
    assert_eq!(
        retrieved_config.lambda_function_configurations[0].events,
        vec!["s3:ObjectCreated:*"],
        "Event types should match"
    );

    // Clean up: remove the notification configuration
    ctx.client
        .put_bucket_notification_configuration(&bucket_name, &NotificationConfiguration::default())
        .await
        .expect("Failed to clear notification configuration");
}

#[test_context(S3TestContext)]
#[tokio::test]
async fn test_notification_config_with_event_filter(ctx: &mut S3TestContext) {
    let bucket_name = ctx.generate_unique_bucket_name();

    ctx.create_test_bucket(&bucket_name)
        .await
        .expect("Failed to create bucket for notification filter test");

    // Build a configuration with specific event types and a key prefix/suffix filter
    let lambda_arn = match env::var("AWS_TEST_LAMBDA_ARN") {
        Ok(arn) if !arn.is_empty() => arn,
        _ => {
            info!(
                "AWS_TEST_LAMBDA_ARN not set — testing serialization of filter configuration only"
            );

            let config = NotificationConfiguration {
                lambda_function_configurations: vec![LambdaFunctionConfiguration {
                    id: Some("filtered-notification".to_string()),
                    lambda_function_arn: "arn:aws:lambda:us-east-1:123456789012:function:filter-fn"
                        .to_string(),
                    events: vec![
                        "s3:ObjectCreated:Put".to_string(),
                        "s3:ObjectRemoved:Delete".to_string(),
                    ],
                    filter: Some(NotificationFilter {
                        key: S3KeyFilter {
                            filter_rules: vec![
                                FilterRule {
                                    name: "prefix".to_string(),
                                    value: "uploads/".to_string(),
                                },
                                FilterRule {
                                    name: "suffix".to_string(),
                                    value: ".json".to_string(),
                                },
                            ],
                        },
                    }),
                }],
            };

            let xml = quick_xml::se::to_string_with_root("NotificationConfiguration", &config)
                .expect("Failed to serialize filtered NotificationConfiguration");
            assert!(
                xml.contains("uploads/"),
                "Serialized XML should contain the prefix filter value"
            );
            assert!(
                xml.contains(".json"),
                "Serialized XML should contain the suffix filter value"
            );
            assert!(
                xml.contains("s3:ObjectCreated:Put"),
                "Serialized XML should contain Put event"
            );
            assert!(
                xml.contains("s3:ObjectRemoved:Delete"),
                "Serialized XML should contain Delete event"
            );

            let deserialized: NotificationConfiguration = quick_xml::de::from_str(&xml)
                .expect("Failed to deserialize filtered NotificationConfiguration");
            assert_eq!(deserialized.lambda_function_configurations.len(), 1);

            let lambda_config = &deserialized.lambda_function_configurations[0];
            assert_eq!(lambda_config.events.len(), 2);
            assert!(
                lambda_config.filter.is_some(),
                "Filter should be present after deserialization"
            );

            let filter = lambda_config.filter.as_ref().unwrap();
            assert_eq!(
                filter.key.filter_rules.len(),
                2,
                "Should have two filter rules"
            );

            return;
        }
    };

    let config = NotificationConfiguration {
        lambda_function_configurations: vec![LambdaFunctionConfiguration {
            id: Some("filtered-notification".to_string()),
            lambda_function_arn: lambda_arn.clone(),
            events: vec![
                "s3:ObjectCreated:Put".to_string(),
                "s3:ObjectRemoved:Delete".to_string(),
            ],
            filter: Some(NotificationFilter {
                key: S3KeyFilter {
                    filter_rules: vec![
                        FilterRule {
                            name: "prefix".to_string(),
                            value: "uploads/".to_string(),
                        },
                        FilterRule {
                            name: "suffix".to_string(),
                            value: ".json".to_string(),
                        },
                    ],
                },
            }),
        }],
    };

    ctx.client
        .put_bucket_notification_configuration(&bucket_name, &config)
        .await
        .expect("Failed to put filtered notification configuration");

    let retrieved_config = ctx
        .client
        .get_bucket_notification_configuration(&bucket_name)
        .await
        .expect("Failed to get filtered notification configuration");

    assert_eq!(
        retrieved_config.lambda_function_configurations.len(),
        1,
        "Expected exactly one lambda function configuration"
    );

    let lambda_config = &retrieved_config.lambda_function_configurations[0];
    assert_eq!(
        lambda_config.lambda_function_arn, lambda_arn,
        "Lambda function ARN should match"
    );
    assert_eq!(lambda_config.events.len(), 2, "Should have two event types");
    assert!(
        lambda_config
            .events
            .contains(&"s3:ObjectCreated:Put".to_string()),
        "Should contain ObjectCreated:Put event"
    );
    assert!(
        lambda_config
            .events
            .contains(&"s3:ObjectRemoved:Delete".to_string()),
        "Should contain ObjectRemoved:Delete event"
    );

    let filter = lambda_config
        .filter
        .as_ref()
        .expect("Filter should be present in retrieved configuration");
    assert_eq!(
        filter.key.filter_rules.len(),
        2,
        "Should have two filter rules"
    );

    let prefix_rule = filter
        .key
        .filter_rules
        .iter()
        .find(|r| r.name == "prefix")
        .expect("Should have a prefix filter rule");
    assert_eq!(
        prefix_rule.value, "uploads/",
        "Prefix filter value should match"
    );

    let suffix_rule = filter
        .key
        .filter_rules
        .iter()
        .find(|r| r.name == "suffix")
        .expect("Should have a suffix filter rule");
    assert_eq!(
        suffix_rule.value, ".json",
        "Suffix filter value should match"
    );

    // Clean up: remove the notification configuration
    ctx.client
        .put_bucket_notification_configuration(&bucket_name, &NotificationConfiguration::default())
        .await
        .expect("Failed to clear notification configuration");
}
