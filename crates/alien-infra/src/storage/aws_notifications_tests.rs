use super::*;
use crate::{
    core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider},
    queue::aws::{AwsQueueController, AwsQueueState},
    storage::{AwsStorageController, AwsStorageState},
};
use alien_aws_clients::{
    eventbridge::{MockEventBridgeApi, PutRuleResponse},
    s3::{MockS3Api, NotificationConfiguration, QueueConfiguration},
};
use alien_core::{
    InitialSetupAuthority, Platform, Queue, ResourceLifecycle, ResourceStatus, StorageNotification,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn storage(prefix: &str) -> Storage {
    let queue = Queue::new("work".into()).build();
    Storage::new("uploads".into())
        .notifications(vec![
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix(prefix.into())
                .suffix(".json".into())
                .build(),
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix("other/".into())
                .suffix(".data".into())
                .build(),
        ])
        .build()
}

fn queue_controller() -> AwsQueueController {
    AwsQueueController {
        state: AwsQueueState::Ready,
        queue_name: Some("test-work".into()),
        queue_url: Some("https://sqs.us-east-1.amazonaws.com/123456789012/test-work".into()),
        ..Default::default()
    }
}

#[derive(Default)]
struct Remote {
    rules: BTreeMap<String, EventBridgeRule>,
    targets: BTreeMap<String, Vec<EventBridgeTarget>>,
    mutations: usize,
}

#[tokio::test]
async fn lost_rule_response_recovers_updates_and_selectively_cleans_up() {
    let remote = Arc::new(Mutex::new(Remote::default()));
    remote.lock().unwrap().rules.insert(
        "independent".into(),
        EventBridgeRule {
            name: "independent".into(),
            ..Default::default()
        },
    );
    let mut events = MockEventBridgeApi::new();
    let read = remote.clone();
    events.expect_describe_rule().returning(move |name| {
        read.lock()
            .unwrap()
            .rules
            .get(name)
            .cloned()
            .ok_or_else(|| {
                AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "rule".into(),
                    resource_name: name.into(),
                })
            })
    });
    let write = remote.clone();
    let mut lost = false;
    events.expect_put_rule().times(3).returning(move |request| {
        let mut remote = write.lock().unwrap();
        remote.mutations += 1;
        remote.rules.insert(
            request.name.clone(),
            EventBridgeRule {
                name: request.name,
                event_pattern: request.event_pattern,
                description: request.description,
                state: request.state,
                schedule_expression: request.schedule_expression,
                ..Default::default()
            },
        );
        if !lost {
            lost = true;
            return Err(AlienError::new(ClientError::Timeout {
                message: "PUT accepted, response lost".into(),
            }));
        }
        Ok(PutRuleResponse {
            rule_arn: Some("arn".into()),
        })
    });
    let read = remote.clone();
    events.expect_list_targets_by_rule().returning(move |name| {
        Ok(read
            .lock()
            .unwrap()
            .targets
            .get(name)
            .cloned()
            .unwrap_or_default())
    });
    let write = remote.clone();
    events
        .expect_put_targets()
        .times(2)
        .returning(move |request| {
            let mut remote = write.lock().unwrap();
            remote.mutations += 1;
            remote.targets.insert(request.rule, request.targets);
            Ok(())
        });
    let remove = remote.clone();
    events
        .expect_remove_targets()
        .times(2)
        .returning(move |name, ids| {
            assert_eq!(ids, ["queue"]);
            let mut remote = remove.lock().unwrap();
            remote.mutations += 1;
            assert!(remote.targets.remove(name).is_some());
            Ok(())
        });
    let remove = remote.clone();
    events.expect_delete_rule().times(2).returning(move |name| {
        assert_ne!(name, "independent");
        let mut remote = remove.lock().unwrap();
        remote.mutations += 1;
        assert!(!remote.targets.contains_key(name));
        assert!(remote.rules.remove(name).is_some());
        Ok(())
    });
    let original = NotificationConfiguration {
        queue_configurations: vec![QueueConfiguration {
            id: Some("manual".into()),
            queue_arn: "external".into(),
            events: vec!["s3:ObjectCreated:*".into()],
            filter: None,
        }],
        ..Default::default()
    };
    let document = Arc::new(Mutex::new(original.clone()));
    let mut s3 = MockS3Api::new();
    let read = document.clone();
    s3.expect_get_bucket_notification_configuration()
        .returning(move |bucket| {
            assert_eq!(bucket, "test-uploads");
            Ok(read.lock().unwrap().clone())
        });
    let write = document.clone();
    let mutations = remote.clone();
    s3.expect_put_bucket_notification_configuration()
        .times(1)
        .returning(move |_, configuration| {
            *write.lock().unwrap() = configuration.clone();
            mutations.lock().unwrap().mutations += 1;
            Ok(())
        });
    let events = Arc::new(events);
    let s3 = Arc::new(s3);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_eventbridge_client()
        .returning(move |_| Ok(events.clone()));
    provider
        .expect_get_aws_s3_client()
        .returning(move |_| Ok(s3.clone()));
    let provider = Arc::new(provider);
    let checkpoint = AwsStorageController {
        state: AwsStorageState::CreatingNotifications,
        bucket_name: Some("test-uploads".into()),
        notifications_managed: true,
        ..Default::default()
    };
    let mut run = SingleControllerExecutor::builder()
        .resource(storage("in/"))
        .controller(checkpoint.clone())
        .platform(Platform::Aws)
        .service_provider(provider.clone())
        .with_dependency(Queue::new("work".into()).build(), queue_controller())
        .build()
        .await
        .unwrap();
    run.step().await.unwrap(); // Enable the shared flag without dropping manual SQS.
    assert_eq!(
        document.lock().unwrap().queue_configurations,
        original.queue_configurations
    );
    run.step()
        .await
        .expect_err("lost rule response must propagate");
    let restored: AwsStorageController =
        serde_json::from_value(serde_json::to_value(checkpoint).unwrap()).unwrap();
    let mut run = SingleControllerExecutor::builder()
        .resource(storage("in/"))
        .controller(restored)
        .platform(Platform::Aws)
        .service_provider(provider.clone())
        .with_dependency(Queue::new("work".into()).build(), queue_controller())
        .build()
        .await
        .unwrap();
    while run.status() != ResourceStatus::Running {
        let before = remote.lock().unwrap().mutations;
        run.step().await.unwrap();
        assert!(remote.lock().unwrap().mutations - before <= 1);
    }
    assert!(!run.needs_update().unwrap());
    assert_eq!(remote.lock().unwrap().rules.len(), 3);
    assert_eq!(remote.lock().unwrap().targets.len(), 2);
    let mut checkpoint = run
        .internal_state::<AwsStorageController>()
        .unwrap()
        .clone();
    checkpoint.state = AwsStorageState::UpdatingNotifications;
    let mut run = SingleControllerExecutor::builder()
        .resource(storage("changed/"))
        .controller(checkpoint)
        .platform(Platform::Aws)
        .service_provider(provider.clone())
        .with_dependency(Queue::new("work".into()).build(), queue_controller())
        .build()
        .await
        .unwrap();
    run.run_until_terminal().await.unwrap();
    let name = aws_notification_rule_name("test", "test-uploads", 0);
    let actual: serde_json::Value = serde_json::from_str(
        remote.lock().unwrap().rules[&name]
            .event_pattern
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        aws_eventbridge_pattern(&storage("changed/").notifications[0], "test-uploads")
    );
    run.delete().unwrap();
    // Stop at the bucket-delete boundary: retained buckets use the same rule cleanup.
    while run.internal_state::<AwsStorageController>().unwrap().state
        != AwsStorageState::DeleteStart
    {
        let before = remote.lock().unwrap().mutations;
        run.step().await.unwrap();
        assert!(remote.lock().unwrap().mutations - before <= 1);
    }
    assert_eq!(
        remote
            .lock()
            .unwrap()
            .rules
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["independent"]
    );
    assert!(remote.lock().unwrap().targets.is_empty());
    assert_eq!(
        document.lock().unwrap().queue_configurations,
        original.queue_configurations
    );
    assert!(document
        .lock()
        .unwrap()
        .event_bridge_configuration
        .is_some());
}

#[tokio::test]
async fn frozen_handoff_never_writes_notifications_even_from_saved_mutation_states() {
    for state in [
        AwsStorageState::CreatingNotifications,
        AwsStorageState::UpdatingNotifications,
        AwsStorageState::DeletingNotifications,
    ] {
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(AwsStorageController {
                state,
                bucket_name: Some("test-uploads".into()),
                notifications_managed: true,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_dependency(Queue::new("work".into()).build(), queue_controller())
            .build()
            .await
            .unwrap();
        assert!(!run.needs_update().unwrap());
        run.step().await.unwrap();
    }
}

#[tokio::test]
async fn cleanup_refuses_foreign_rule_and_foreign_targets() {
    let name = aws_notification_rule_name("test", "test-uploads", 0);
    let mut events = MockEventBridgeApi::new();
    events.expect_describe_rule().times(1).returning(|name| {
        Ok(EventBridgeRule {
            name: name.into(),
            description: Some("created independently".into()),
            ..Default::default()
        })
    });
    let error = remove_owned_step(&events, "test", "test-uploads", "uploads")
        .await
        .unwrap_err();
    assert_eq!(error.code, "EXISTING_CLOUD_RESOURCE_NOT_OWNED");
    let mut events = MockEventBridgeApi::new();
    events
        .expect_list_targets_by_rule()
        .times(1)
        .returning(|_| {
            Ok(vec![EventBridgeTarget {
                id: "manual".into(),
                arn: "external".into(),
                input_transformer: None,
            }])
        });
    let error = remove_rule_step(&events, &name, "uploads")
        .await
        .unwrap_err();
    assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    assert!(error.message.contains("independently managed target"));
}
