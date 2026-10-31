use super::*;
use crate::{
    core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider},
    queue::aws::{AwsQueueController, AwsQueueState},
    storage::{AwsStorageController, AwsStorageState},
};
use alien_aws_clients::s3::{
    EventBridgeConfiguration, LambdaFunctionConfiguration, MockS3Api, TopicConfiguration,
};
use alien_core::{Platform, Queue, ResourceStatus, StorageNotification};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

const QUEUE_ARN: &str = "arn:aws:sqs:us-east-1:123456789012:test-work";

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

fn filter(rules: &[(&str, &str)]) -> Option<NotificationFilter> {
    Some(NotificationFilter {
        key: S3KeyFilter {
            filter_rules: rules
                .iter()
                .map(|(name, value)| FilterRule {
                    name: (*name).into(),
                    value: (*value).into(),
                })
                .collect(),
        },
    })
}

/// Entries other tools own, including another Storage of the same stack.
fn foreign() -> NotificationConfiguration {
    NotificationConfiguration {
        lambda_function_configurations: vec![LambdaFunctionConfiguration {
            id: Some("worker-trigger".into()),
            lambda_function_arn: "arn:aws:lambda:us-east-1:123456789012:function:other".into(),
            events: vec!["s3:ObjectRemoved:*".into()],
            filter: filter(&[("Prefix", "logs/")]),
        }],
        queue_configurations: vec![
            QueueConfiguration {
                id: Some("manual".into()),
                queue_arn: "arn:aws:sqs:us-east-1:123456789012:manual".into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: filter(&[("Prefix", "manual/")]),
            },
            QueueConfiguration {
                id: Some(aws_notification_id("test", "second", 0)),
                queue_arn: QUEUE_ARN.into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: filter(&[("Prefix", "shared/")]),
            },
        ],
        topic_configurations: vec![TopicConfiguration {
            id: Some("topic".into()),
            topic_arn: "arn:aws:sns:us-east-1:123456789012:topic".into(),
            events: vec!["s3:ObjectCreated:Copy".into()],
            filter: None,
        }],
        event_bridge_configuration: Some(EventBridgeConfiguration {}),
    }
}

/// An S3 bucket's notification document, counting writes. The first write can be made to
/// land while its response is lost.
struct Bucket {
    document: Arc<Mutex<NotificationConfiguration>>,
    writes: Arc<AtomicUsize>,
    s3: Arc<MockS3Api>,
    provider: Arc<MockPlatformServiceProvider>,
}

fn bucket(lose_first_response: bool) -> Bucket {
    let document = Arc::new(Mutex::new(foreign()));
    let writes = Arc::new(AtomicUsize::new(0));
    let mut s3 = MockS3Api::new();
    let read = document.clone();
    s3.expect_get_bucket_notification_configuration()
        .returning(move |bucket| {
            assert_eq!(bucket, "test-uploads");
            Ok(read.lock().unwrap().clone())
        });
    let (stored, count) = (document.clone(), writes.clone());
    s3.expect_put_bucket_notification_configuration()
        .returning(move |bucket, configuration| {
            assert_eq!(bucket, "test-uploads");
            *stored.lock().unwrap() = configuration.clone();
            if count.fetch_add(1, Ordering::SeqCst) == 0 && lose_first_response {
                return Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "response lost after the write".into(),
                }));
            }
            Ok(())
        });
    let s3 = Arc::new(s3);
    let client = s3.clone();
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_s3_client()
        .returning(move |_| Ok(client.clone()));
    Bucket {
        document,
        writes,
        s3,
        provider: Arc::new(provider),
    }
}

async fn run(
    bucket: &Bucket,
    resource: Storage,
    state: AwsStorageState,
) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(resource)
        .controller(AwsStorageController {
            state,
            bucket_name: Some("test-uploads".into()),
            notifications_managed: true,
            ..Default::default()
        })
        .platform(Platform::Aws)
        .service_provider(bucket.provider.clone())
        .with_dependency(Queue::new("work".into()).build(), queue_controller())
        .build()
        .await
        .unwrap()
}

fn owned(document: &NotificationConfiguration) -> Vec<QueueConfiguration> {
    let owner = aws_notification_id_prefix("test", "uploads");
    document
        .queue_configurations
        .iter()
        .filter(|configuration| {
            configuration
                .id
                .as_deref()
                .is_some_and(|id| id.starts_with(&owner))
        })
        .cloned()
        .collect()
}

fn assert_foreign_untouched(document: &NotificationConfiguration) {
    let expected = foreign();
    assert_eq!(
        document.lambda_function_configurations,
        expected.lambda_function_configurations
    );
    assert_eq!(document.topic_configurations, expected.topic_configurations);
    assert_eq!(
        document.event_bridge_configuration,
        expected.event_bridge_configuration
    );
    for entry in &expected.queue_configurations {
        assert!(
            document.queue_configurations.contains(entry),
            "{entry:?} must survive"
        );
    }
}

#[tokio::test]
async fn owned_entries_are_written_once_and_a_lost_response_recovers_by_observation() {
    let bucket = bucket(true);
    let mut run = run(&bucket, storage("in/"), AwsStorageState::CreatingNotifications).await;
    run.step()
        .await
        .expect_err("the lost response must propagate");
    run.run_until_terminal().await.unwrap();
    assert_eq!(run.status(), ResourceStatus::Running);
    assert_eq!(
        bucket.writes.load(Ordering::SeqCst),
        1,
        "the retry observes the landed write and does not write again"
    );
    let document = bucket.document.lock().unwrap().clone();
    assert_foreign_untouched(&document);
    assert_eq!(
        owned(&document),
        vec![
            QueueConfiguration {
                id: Some(aws_notification_id("test", "uploads", 0)),
                queue_arn: QUEUE_ARN.into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: filter(&[("prefix", "in/"), ("suffix", ".json")]),
            },
            QueueConfiguration {
                id: Some(aws_notification_id("test", "uploads", 1)),
                queue_arn: QUEUE_ARN.into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: filter(&[("prefix", "other/")]),
            },
        ]
    );
}

#[tokio::test]
async fn s3_normalization_is_not_a_change_but_an_edited_rule_is() {
    let bucket = bucket(false);
    run(&bucket, storage("in/"), AwsStorageState::CreatingNotifications)
        .await
        .run_until_terminal()
        .await
        .unwrap();
    assert_eq!(bucket.writes.load(Ordering::SeqCst), 1);
    // S3 returns `Prefix`/`Suffix` and may reorder entries and filter rules.
    {
        let mut document = bucket.document.lock().unwrap();
        document.queue_configurations.reverse();
        for configuration in &mut document.queue_configurations {
            if let Some(filter) = &mut configuration.filter {
                filter.key.filter_rules.reverse();
                for rule in &mut filter.key.filter_rules {
                    rule.name = format!("{}{}", rule.name[..1].to_uppercase(), &rule.name[1..]);
                }
            }
        }
    }
    run(&bucket, storage("in/"), AwsStorageState::UpdatingNotifications)
        .await
        .run_until_terminal()
        .await
        .unwrap();
    assert_eq!(
        bucket.writes.load(Ordering::SeqCst),
        1,
        "an unchanged declaration never rewrites the document"
    );

    run(&bucket, storage("changed/"), AwsStorageState::UpdatingNotifications)
        .await
        .run_until_terminal()
        .await
        .unwrap();
    assert_eq!(bucket.writes.load(Ordering::SeqCst), 2);
    let document = bucket.document.lock().unwrap().clone();
    assert_foreign_untouched(&document);
    let prefixes: Vec<_> = owned(&document)
        .iter()
        .map(|configuration| configuration.filter.clone().unwrap().key.filter_rules[0].value.clone())
        .collect();
    assert_eq!(prefixes, ["changed/", "other/"]);
}

#[tokio::test]
async fn deletion_and_retention_remove_only_owned_entries() {
    let bucket = bucket(false);
    run(&bucket, storage("in/"), AwsStorageState::CreatingNotifications)
        .await
        .run_until_terminal()
        .await
        .unwrap();
    let mut deleting = run(&bucket, storage("in/"), AwsStorageState::DeletingNotifications).await;
    deleting.step().await.unwrap();
    let document = bucket.document.lock().unwrap().clone();
    assert!(owned(&document).is_empty());
    assert_foreign_untouched(&document);
    assert_eq!(document.queue_configurations, foreign().queue_configurations);

    // Retention cleanup of a bucket without owned entries leaves it unwritten.
    let writes = bucket.writes.load(Ordering::SeqCst);
    remove_owned(bucket.s3.as_ref(), "test", "test-uploads", "uploads", &[])
        .await
        .unwrap();
    assert_eq!(bucket.writes.load(Ordering::SeqCst), writes);
}
