use std::sync::{Arc, Mutex};

use alien_aws_clients::{
    lambda::{AddPermissionResponse, MockLambdaApi},
    s3::{MockS3Api, NotificationConfiguration},
};
use alien_client_core::ErrorData as CloudError;
use alien_core::{
    AwsClientConfig, ClientConfig, Platform, ResourceDefinition, ResourceStatus, Storage, Worker,
    WorkerTrigger,
};
use alien_error::AlienError;

use super::{AwsWorkerController, AwsWorkerState};
use crate::{
    core::{
        controller_test::SingleControllerExecutor, DefaultPlatformServiceProvider,
        MockPlatformServiceProvider, PlatformServiceProvider,
    },
    storage::AwsStorageController,
    worker::fixtures::basic_function,
};

fn worker(storage: &Storage, event: &str) -> Worker {
    let mut worker = basic_function();
    worker.triggers = vec![WorkerTrigger::Storage {
        storage: alien_core::ResourceRef::new(Storage::RESOURCE_TYPE, storage.id.clone()),
        events: vec![event.into()],
    }];
    worker
}

fn notifications() -> NotificationConfiguration {
    serde_json::from_value(serde_json::json!({
        "CloudFunctionConfiguration": [
            {"Id":"physical-worker-s3-objects", "CloudFunction":"arn:aws:lambda:us-east-1:123456789012:function:physical-worker", "Event":["s3:ObjectCreated:*"]},
            {"Id":"physical-worker-s3-objects-foreign", "CloudFunction":"arn:aws:lambda:us-east-1:123456789012:function:foreign", "Event":["s3:ObjectCreated:*"], "Filter":{"S3Key":{"FilterRule":[{"Name":"prefix","Value":"foreign/"}]}}}
        ],
        "QueueConfiguration": [{"Id":"manual-queue","Queue":"arn:aws:sqs:us-east-1:123456789012:manual","Event":["s3:ObjectCreated:*"],"Filter":{"S3Key":{"FilterRule":[{"Name":"suffix","Value":".json"}]}}}],
        "TopicConfiguration": [{"Id":"manual-topic","Topic":"arn:aws:sns:us-east-1:123456789012:manual","Event":["s3:ObjectRemoved:*"]}],
        "EventBridgeConfiguration": {}
    })).unwrap()
}

fn provider(s3: MockS3Api, lambda: MockLambdaApi) -> Arc<MockPlatformServiceProvider> {
    let s3 = Arc::new(s3);
    let lambda = Arc::new(lambda);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_s3_client()
        .returning(move |_| Ok(s3.clone()));
    provider
        .expect_get_aws_lambda_client()
        .returning(move |_| Ok(lambda.clone()));
    Arc::new(provider)
}

#[tokio::test]
async fn storage_trigger_update_replaces_only_owned_entry_without_emptying_bucket() {
    let storage = Storage::new("objects".into()).build();
    let mut expected = notifications();
    let mut owned = expected.lambda_function_configurations.remove(0);
    owned.events = vec!["s3:ObjectRemoved:*".into()];
    expected.lambda_function_configurations.push(owned);
    let observed = Arc::new(Mutex::new(notifications()));
    let mut s3 = MockS3Api::new();
    let read = observed.clone();
    s3.expect_get_bucket_notification_configuration()
        .times(1)
        .returning(move |bucket| {
            assert_eq!(bucket, "physical-bucket");
            Ok(read.lock().unwrap().clone())
        });
    let write = observed.clone();
    s3.expect_put_bucket_notification_configuration()
        .times(1)
        .returning(move |bucket, config| {
            assert_eq!(bucket, "physical-bucket");
            assert_eq!(config, &expected);
            *write.lock().unwrap() = config.clone();
            Ok(())
        });
    let mut lambda = MockLambdaApi::new();
    lambda
        .expect_add_permission()
        .times(1)
        .returning(|name, request| {
            assert_eq!(name, "physical-worker");
            assert_eq!(request.statement_id, "physical-worker-s3-objects");
            Ok(AddPermissionResponse { statement: None })
        });
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker(&storage, "deleted"))
        .previous_resource(worker(&storage, "created"))
        .controller(AwsWorkerController {
            state: AwsWorkerState::UpdateEventSourceMappings,
            s3_permission_statement_ids: vec!["physical-worker-s3-objects".into()],
            ..AwsWorkerController::mock_ready("physical-worker")
        })
        .platform(Platform::Aws)
        .service_provider(provider(s3, lambda))
        .with_test_dependencies()
        .with_dependency(storage, storage_controller("physical-bucket"))
        .build()
        .await
        .unwrap();
    executor.step().await.unwrap();
    let state = executor.internal_state::<AwsWorkerController>().unwrap();
    assert_eq!(
        state.s3_permission_statement_ids,
        ["physical-worker-s3-objects"]
    );
    assert_eq!(state.state, AwsWorkerState::UpdatingConcurrency);
    assert_eq!(
        observed
            .lock()
            .unwrap()
            .lambda_function_configurations
            .len(),
        2
    );
}

#[tokio::test]
async fn storage_trigger_removal_and_interrupted_delete_preserve_foreign_entries() {
    for deleting in [false, true] {
        let storage = Storage::new("objects".into()).build();
        let mut expected = notifications();
        expected.lambda_function_configurations.remove(0);
        let mut s3 = MockS3Api::new();
        s3.expect_get_bucket_notification_configuration()
            .times(1)
            .returning(|_| Ok(notifications()));
        s3.expect_put_bucket_notification_configuration()
            .times(1)
            .returning(move |bucket, config| {
                assert_eq!(bucket, "physical-bucket");
                assert_eq!(config, &expected);
                Ok(())
            });
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .previous_resource(worker(&storage, "created"))
            .controller(AwsWorkerController {
                state: if deleting {
                    AwsWorkerState::DeletingEventSourceMappings
                } else {
                    AwsWorkerState::UpdateEventSourceMappings
                },
                // Empty checkpoint simulates a lost PUT response on initial registration.
                s3_permission_statement_ids: Vec::new(),
                ..AwsWorkerController::mock_ready("physical-worker")
            })
            .platform(Platform::Aws)
            .service_provider(provider(s3, MockLambdaApi::new()))
            .with_test_dependencies()
            .with_dependency(storage, storage_controller("physical-bucket"))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert!(state.s3_permission_statement_ids.is_empty());
        assert_eq!(
            state.state,
            if deleting {
                AwsWorkerState::DeletingScheduleTriggers
            } else {
                AwsWorkerState::UpdatingConcurrency
            }
        );
    }
}

#[tokio::test]
async fn storage_notification_cleanup_failure_retains_ownership() {
    for fail_read in [false, true] {
        let storage = Storage::new("objects".into()).build();
        let mut s3 = MockS3Api::new();
        s3.expect_get_bucket_notification_configuration()
            .times(1)
            .returning(move |_| {
                if fail_read {
                    Err(denied())
                } else {
                    Ok(notifications())
                }
            });
        s3.expect_put_bucket_notification_configuration()
            .times(usize::from(!fail_read))
            .returning(|_, _| Err(denied()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker(&storage, "created"))
            .controller(AwsWorkerController {
                state: AwsWorkerState::DeletingEventSourceMappings,
                s3_permission_statement_ids: vec!["physical-worker-s3-objects".into()],
                ..AwsWorkerController::mock_ready("physical-worker")
            })
            .platform(Platform::Aws)
            .service_provider(provider(s3, MockLambdaApi::new()))
            .with_test_dependencies()
            .with_dependency(storage, storage_controller("physical-bucket"))
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .expect_err("cleanup must fail on access denied");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::DeletingEventSourceMappings);
        assert_eq!(
            state.s3_permission_statement_ids,
            ["physical-worker-s3-objects"]
        );
    }
}

fn denied() -> AlienError<CloudError> {
    AlienError::new(CloudError::RemoteAccessDenied {
        resource_type: "Bucket".into(),
        resource_name: "physical-bucket".into(),
    })
}

#[tokio::test]
async fn storage_notification_cleanup_after_lost_response_does_not_write_again() {
    for bucket_missing in [false, true] {
        let storage = Storage::new("objects".into()).build();
        let mut s3 = MockS3Api::new();
        s3.expect_get_bucket_notification_configuration()
            .times(1)
            .returning(move |_| {
                if bucket_missing {
                    Err(AlienError::new(CloudError::RemoteResourceNotFound {
                        resource_type: "Bucket".into(),
                        resource_name: "physical-bucket".into(),
                    }))
                } else {
                    let mut config = notifications();
                    config.lambda_function_configurations.remove(0);
                    Ok(config)
                }
            });
        s3.expect_put_bucket_notification_configuration().times(0);
        let controller = AwsWorkerController {
            state: AwsWorkerState::DeletingEventSourceMappings,
            s3_permission_statement_ids: vec!["physical-worker-s3-objects".into()],
            ..AwsWorkerController::mock_ready("physical-worker")
        };
        let restored: AwsWorkerController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker(&storage, "created"))
            .controller(restored)
            .platform(Platform::Aws)
            .service_provider(provider(s3, MockLambdaApi::new()))
            .with_test_dependencies()
            .with_dependency(storage, storage_controller("physical-bucket"))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::DeletingScheduleTriggers);
        assert!(state.s3_permission_statement_ids.is_empty());
    }
}

/// The fixture owner provisions an isolated bucket with independent Lambda/SQS/SNS
/// entries plus EventBridge, and a disposable Worker function. It independently
/// lists the resulting configuration and cleans up every resource after this test.
#[tokio::test]
#[ignore = "requires ALIEN_S3_NOTIFICATIONS_LIVE_FIXTURE for explicitly owned test resources"]
async fn live_storage_trigger_update_and_worker_delete_preserve_independent_notifications() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        prefix: String,
        bucket: String,
        function_name: String,
        function_arn: String,
        config: AwsClientConfig,
    }
    let path =
        std::env::var("ALIEN_S3_NOTIFICATIONS_LIVE_FIXTURE").expect("explicit fixture required");
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(fixture.prefix.starts_with("n2081-"));
    assert!(fixture.bucket.starts_with(&fixture.prefix));
    assert!(fixture.function_name.starts_with(&fixture.prefix));
    assert!(fixture.config.service_overrides.is_none());
    let provider = Arc::new(DefaultPlatformServiceProvider::default());
    let s3 = provider.get_aws_s3_client(&fixture.config).await.unwrap();
    let original = s3
        .get_bucket_notification_configuration(&fixture.bucket)
        .await
        .unwrap();
    assert_eq!(original.lambda_function_configurations.len(), 2);
    assert_eq!(original.queue_configurations.len(), 1);
    assert_eq!(original.topic_configurations.len(), 1);
    assert!(original.event_bridge_configuration.is_some());
    let storage = Storage::new("objects".into()).build();
    let mut previous = worker(&storage, "created");
    previous.id = "worker".into();
    let mut current = worker(&storage, "deleted");
    current.id = "worker".into();
    let owned_id = format!("{}-s3-objects", fixture.function_name);
    let mut expected = original.clone();
    expected
        .lambda_function_configurations
        .retain(|entry| entry.id.as_deref() != Some(&owned_id));
    assert_eq!(expected.lambda_function_configurations.len(), 1);
    let independent = expected.clone();
    expected.lambda_function_configurations.push(
        alien_aws_clients::s3::LambdaFunctionConfiguration {
            id: Some(owned_id.clone()),
            lambda_function_arn: fixture.function_arn.clone(),
            events: vec!["s3:ObjectRemoved:*".into()],
            filter: None,
        },
    );
    let mut executor = SingleControllerExecutor::builder()
        .resource(current)
        .previous_resource(previous)
        .controller(AwsWorkerController {
            state: AwsWorkerState::UpdateEventSourceMappings,
            arn: Some(fixture.function_arn.clone()),
            s3_permission_statement_ids: vec![owned_id],
            ..AwsWorkerController::mock_ready(&fixture.function_name)
        })
        .platform(Platform::Aws)
        .resource_prefix(&fixture.prefix)
        .client_config(ClientConfig::Aws(Box::new(fixture.config)))
        .service_provider(provider)
        .with_test_dependencies()
        .with_dependency(storage, storage_controller(&fixture.bucket))
        .build()
        .await
        .unwrap();
    executor.step().await.expect("real trigger update succeeds");
    let mut actual = s3
        .get_bucket_notification_configuration(&fixture.bucket)
        .await
        .unwrap();
    actual
        .lambda_function_configurations
        .sort_by(|a, b| a.id.cmp(&b.id));
    expected
        .lambda_function_configurations
        .sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(actual, expected);
    executor
        .step()
        .await
        .expect("unchanged concurrency completes update");
    assert_eq!(executor.status(), ResourceStatus::Running);
    executor.delete().unwrap();
    executor
        .run_until_terminal()
        .await
        .expect("real Worker deletion succeeds");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    assert_eq!(
        s3.get_bucket_notification_configuration(&fixture.bucket)
            .await
            .unwrap(),
        independent
    );
}

fn storage_controller(bucket: &str) -> AwsStorageController {
    AwsStorageController {
        bucket_name: Some(bucket.into()),
        ..AwsStorageController::mock_ready(bucket)
    }
}

#[tokio::test]
async fn storage_trigger_cleanup_before_function_creation_needs_no_cloud_calls() {
    let storage = Storage::new("objects".into()).build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker(&storage, "created"))
        .controller(AwsWorkerController {
            state: AwsWorkerState::DeletingEventSourceMappings,
            ..Default::default()
        })
        .platform(Platform::Aws)
        .service_provider(provider(MockS3Api::new(), MockLambdaApi::new()))
        .with_test_dependencies()
        .with_dependency(storage, AwsStorageController::default())
        .build()
        .await
        .unwrap();
    executor.step().await.unwrap();
    assert_eq!(
        executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .state,
        AwsWorkerState::DeletingScheduleTriggers
    );
}
