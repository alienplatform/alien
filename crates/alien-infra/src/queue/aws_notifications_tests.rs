use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use alien_aws_clients::sqs::{Attribute, GetQueueAttributesResult, MockSqsApi};
use alien_core::{Resource, Storage, StorageNotification};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn queue_controller() -> AwsQueueController {
    AwsQueueController {
        state: AwsQueueState::Ready,
        queue_name: Some("test-work".into()),
        queue_url: Some("https://sqs.us-east-1.amazonaws.com/123456789012/test-work".into()),
        ..Default::default()
    }
}

fn notifying(queue: &Queue) -> Storage {
    Storage::new("uploads".into())
        .notifications(vec![StorageNotification::builder()
            .queue(queue.into())
            .build()])
        .build()
}

#[tokio::test]
async fn ingress_reconciles_only_on_change_and_preserves_foreign_policy() {
    for lifecycle in [ResourceLifecycle::Live, ResourceLifecycle::Frozen] {
        let queue = Queue::new("work".into()).build();
        let original = json!({"Version":"2012-10-17", "Id":"external", "Statement":[{
            "Sid":"External", "Effect":"Deny", "Principal":"*", "Action":"sqs:*",
            "Resource":"*", "Condition":{"Bool":{"aws:SecureTransport":"false"}}
        }]});
        let policy = Arc::new(Mutex::new(original.clone()));
        let mut sqs = MockSqsApi::new();
        let reads = policy.clone();
        sqs.expect_get_queue_attributes()
            .times(3)
            .returning(move |_, request| {
                let names = request.attribute_names.unwrap();
                assert!(names == ["Policy"] || names == ["All"]);
                Ok(GetQueueAttributesResponse {
                    get_queue_attributes_result: GetQueueAttributesResult {
                        attributes: vec![Attribute {
                            name: "Policy".into(),
                            value: reads.lock().unwrap().to_string(),
                        }],
                    },
                })
            });
        let writes = policy.clone();
        sqs.expect_set_queue_attributes()
            .times(2)
            .returning(move |_, request| {
                assert_eq!(request.attributes.len(), 1);
                *writes.lock().unwrap() =
                    serde_json::from_str(&request.attributes["Policy"]).unwrap();
                Ok(())
            });
        let sqs = Arc::new(sqs);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_sqs_client()
            .returning(move |_| Ok(sqs.clone()));
        let mut run = SingleControllerExecutor::builder()
            .resource(queue.clone())
            .controller(queue_controller())
            .platform(Platform::Aws)
            .resource_lifecycle(lifecycle)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(notifying(&queue), lifecycle)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        assert!(run.needs_update().unwrap());
        run.update(queue.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        assert!(!run.needs_update().unwrap());
        assert_eq!(
            run.internal_state::<AwsQueueController>()
                .unwrap()
                .storage_ingress_buckets,
            ["test-uploads"]
        );
        {
            let actual = policy.lock().unwrap();
            assert_eq!(actual["Id"], "external");
            assert_eq!(actual["Statement"][0], original["Statement"][0]);
            assert_eq!(actual["Statement"].as_array().unwrap().len(), 2);
            let grant = &actual["Statement"][1];
            assert_eq!(
                grant["Principal"],
                json!({"Service":"s3.amazonaws.com"})
            );
            assert_eq!(grant["Action"], "sqs:SendMessage");
            assert_eq!(
                grant["Resource"],
                "arn:aws:sqs:us-east-1:123456789012:test-work"
            );
            assert_eq!(
                grant["Condition"],
                json!({
                    "StringEquals":{"aws:SourceAccount":"123456789012"},
                    "ArnLike":{"aws:SourceArn":["arn:aws:s3:::test-uploads"]}
                })
            );
        }
        run.step().await.unwrap(); // Heartbeat only; no policy mutation.
        run.update(queue.clone()).unwrap();
        run.run_until_terminal().await.unwrap(); // Explicit no-op update has no policy read.
        *run.desired_resource_config_mut("uploads").unwrap() =
            Resource::new(Storage::new("uploads".into()).build());
        assert!(run.needs_update().unwrap());
        run.update(queue).unwrap();
        run.run_until_terminal().await.unwrap();
        assert!(!run.needs_update().unwrap());
        assert_eq!(*policy.lock().unwrap(), original);
    }
}

#[tokio::test]
async fn ordinary_and_imported_queues_never_request_ingress_updates() {
    for (notify, lifecycle) in [
        (false, ResourceLifecycle::Live),
        (true, ResourceLifecycle::Frozen),
    ] {
        let queue = Queue::new("work".into()).build();
        let storage = if notify {
            notifying(&queue)
        } else {
            Storage::new("uploads".into()).build()
        };
        let mut run = SingleControllerExecutor::builder()
            .resource(queue.clone())
            .controller(queue_controller())
            .platform(Platform::Aws)
            .resource_lifecycle(lifecycle)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .with_stack_resource(storage, lifecycle)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .await
            .unwrap();
        assert!(!run.needs_update().unwrap());
        run.update(queue).unwrap();
        run.run_until_terminal().await.unwrap();
        assert!(!run.needs_update().unwrap());
    }
}

#[tokio::test]
async fn lost_policy_response_is_recovered_without_duplicate_or_second_write() {
    let queue = Queue::new("work".into()).build();
    let policy = Arc::new(Mutex::new(json!({"Version":"2012-10-17", "Statement":[]})));
    let mut sqs = MockSqsApi::new();
    let reads = policy.clone();
    sqs.expect_get_queue_attributes()
        .times(2)
        .returning(move |_, _| {
            Ok(GetQueueAttributesResponse {
                get_queue_attributes_result: GetQueueAttributesResult {
                    attributes: vec![Attribute {
                        name: "Policy".into(),
                        value: reads.lock().unwrap().to_string(),
                    }],
                },
            })
        });
    let writes = policy.clone();
    sqs.expect_set_queue_attributes()
        .times(1)
        .returning(move |_, request| {
            *writes.lock().unwrap() =
                serde_json::from_str::<Value>(&request.attributes["Policy"]).unwrap();
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteServiceUnavailable {
                    message: "lost response".into(),
                },
            ))
        });
    let sqs = Arc::new(sqs);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    let mut run = SingleControllerExecutor::builder()
        .resource(queue.clone())
        .controller(queue_controller())
        .platform(Platform::Aws)
        .with_stack_resource(notifying(&queue), ResourceLifecycle::Live)
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap();
    run.update(queue).unwrap();
    run.step().await.unwrap(); // Checkpoint policy intent before the mutation.
    run.step()
        .await
        .expect_err("lost policy write response must fail");
    assert!(run
        .internal_state::<AwsQueueController>()
        .unwrap()
        .storage_ingress_buckets
        .is_empty());
    run.step().await.unwrap();
    assert!(!run.needs_update().unwrap());
    assert_eq!(
        policy.lock().unwrap()["Statement"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn pending_ingress_is_removed_when_declaration_changes_after_lost_write() {
    let queue = Queue::new("work".into()).build();
    let controller = AwsQueueController {
        state: AwsQueueState::UpdatingStorageIngress,
        storage_ingress_pending: Some(vec!["test-uploads".into()]),
        ..queue_controller()
    };
    let checkpoint = serde_json::to_value(controller).unwrap();
    let mut sqs = MockSqsApi::new();
    sqs.expect_get_queue_attributes().times(1).returning(|_,_| Ok(GetQueueAttributesResponse {
        get_queue_attributes_result:GetQueueAttributesResult {attributes:vec![Attribute {
            name:"Policy".into(),value:json!({"Version":"2012-10-17","Statement":[{"Sid":"External"},{"Sid":"StorageNotificationIngress"}]}).to_string(),
        }]},
    }));
    sqs.expect_set_queue_attributes()
        .times(1)
        .returning(|_, request| {
            let policy: Value = serde_json::from_str(&request.attributes["Policy"]).unwrap();
            assert_eq!(
                policy,
                json!({"Version":"2012-10-17","Statement":[{"Sid":"External"}]})
            );
            Ok(())
        });
    let sqs = Arc::new(sqs);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    let mut run = SingleControllerExecutor::builder()
        .resource(queue)
        .controller(serde_json::from_value::<AwsQueueController>(checkpoint).unwrap())
        .platform(Platform::Aws)
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap();
    assert!(
        run.needs_update().unwrap(),
        "unfinished policy intent survives a checkpoint"
    );
    run.run_until_terminal().await.unwrap();
    assert!(!run.needs_update().unwrap());
    let controller = run.internal_state::<AwsQueueController>().unwrap();
    assert!(controller.storage_ingress_buckets.is_empty());
    assert_eq!(controller.storage_ingress_pending, None);
}
