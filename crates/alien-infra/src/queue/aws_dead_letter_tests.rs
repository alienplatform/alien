use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use alien_aws_clients::sqs::MockSqsApi;
use alien_core::{QueueDeadLetter, Resource, ResourceRef};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn controller(name: &str) -> AwsQueueController {
    AwsQueueController {
        state: AwsQueueState::Ready,
        queue_name: Some(format!("test-{name}")),
        queue_url: Some(format!(
            "https://sqs.us-east-1.amazonaws.com/123456789012/test-{name}"
        )),
        ..Default::default()
    }
}

fn with_dead_letter(max_attempts: u32) -> Queue {
    let mut queue = Queue::new("tasks".into()).build();
    queue.dead_letter = Some(
        QueueDeadLetter::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "failed"))
            .max_attempts(max_attempts)
            .build(),
    );
    queue
}

type Writes = Arc<Mutex<Vec<HashMap<String, String>>>>;

fn provider(writes: &Writes) -> Arc<MockPlatformServiceProvider> {
    let mut sqs = MockSqsApi::new();
    let writes = writes.clone();
    sqs.expect_set_queue_attributes()
        .returning(move |_, request| {
            writes.lock().unwrap().push(request.attributes.clone());
            Ok(())
        });
    let sqs = Arc::new(sqs);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    Arc::new(provider)
}

async fn source(
    current: Queue,
    desired: Queue,
    lifecycle: ResourceLifecycle,
    authority: InitialSetupAuthority,
    writes: &Writes,
) -> crate::core::controller_test::SingleControllerExecutor {
    let mut run = SingleControllerExecutor::builder()
        .resource(current)
        .controller(controller("tasks"))
        .platform(Platform::Aws)
        .resource_lifecycle(lifecycle)
        .initial_setup_authority(authority)
        .with_stack_resource(Queue::new("failed".into()).build(), lifecycle)
        .service_provider(provider(writes))
        .build()
        .await
        .unwrap();
    *run.desired_resource_config_mut("tasks").unwrap() = Resource::new(desired);
    run
}

#[tokio::test]
async fn adding_then_removing_a_dead_letter_writes_and_clears_the_redrive_policy() {
    let writes: Writes = Default::default();
    let plain = Queue::new("tasks".into()).build();
    let mut run = source(
        plain.clone(),
        with_dead_letter(7),
        ResourceLifecycle::Live,
        InitialSetupAuthority::DirectSetup,
        &writes,
    )
    .await;
    assert!(run.needs_update().unwrap());
    run.update(with_dead_letter(7)).unwrap();
    run.run_until_terminal().await.unwrap();
    assert!(!run.needs_update().unwrap());
    {
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            writes[0]["MessageRetentionPeriod"],
            AWS_DEFAULT_RETENTION_SECONDS.to_string(),
            "both attributes are written; a source queue keeps the default retention"
        );
        let policy: Value = serde_json::from_str(&writes[0]["RedrivePolicy"]).unwrap();
        assert_eq!(
            policy,
            json!({
                "deadLetterTargetArn": "arn:aws:sqs:us-east-1:123456789012:test-failed",
                "maxReceiveCount": 7
            })
        );
    }

    *run.desired_resource_config_mut("tasks").unwrap() = Resource::new(plain.clone());
    assert!(run.needs_update().unwrap());
    run.update(plain).unwrap();
    run.run_until_terminal().await.unwrap();
    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        writes[1]["RedrivePolicy"], "",
        "an empty value clears the policy"
    );
}

#[tokio::test]
async fn a_target_keeps_messages_fourteen_days_and_returns_to_the_default() {
    let writes: Writes = Default::default();
    let failed = Queue::new("failed".into()).build();
    let mut run = SingleControllerExecutor::builder()
        .resource(failed.clone())
        .controller(controller("failed"))
        .platform(Platform::Aws)
        .resource_lifecycle(ResourceLifecycle::Live)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .with_stack_resource(with_dead_letter(5), ResourceLifecycle::Live)
        .service_provider(provider(&writes))
        .build()
        .await
        .unwrap();
    assert!(run.needs_update().unwrap());
    run.update(failed.clone()).unwrap();
    run.run_until_terminal().await.unwrap();
    assert!(!run.needs_update().unwrap());
    {
        let writes = writes.lock().unwrap();
        assert_eq!(writes[0]["MessageRetentionPeriod"], "1209600");
        assert_eq!(writes[0]["RedrivePolicy"], "", "a target has no policy");
    }

    *run.desired_resource_config_mut("tasks").unwrap() =
        Resource::new(Queue::new("tasks".into()).build());
    assert!(run.needs_update().unwrap());
    run.update(failed).unwrap();
    run.run_until_terminal().await.unwrap();
    assert_eq!(
        writes.lock().unwrap()[1]["MessageRetentionPeriod"],
        "345600"
    );
}

#[tokio::test]
async fn a_setup_owned_queue_is_not_changed_by_the_runtime() {
    let writes: Writes = Default::default();
    let run = source(
        Queue::new("tasks".into()).build(),
        with_dead_letter(5),
        ResourceLifecycle::Frozen,
        InitialSetupAuthority::ImportedHandoff,
        &writes,
    )
    .await;
    assert!(!run.needs_update().unwrap());
    assert!(writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn queues_without_a_dead_letter_setting_are_never_written() {
    let writes: Writes = Default::default();
    let plain = Queue::new("tasks".into()).build();
    let run = source(
        plain.clone(),
        plain,
        ResourceLifecycle::Live,
        InitialSetupAuthority::DirectSetup,
        &writes,
    )
    .await;
    assert!(!run.needs_update().unwrap());
}

#[test]
fn the_target_arn_uses_the_partition_of_the_region() {
    for (region, partition) in [
        ("us-east-1", "aws"),
        ("us-gov-west-1", "aws-us-gov"),
        ("cn-north-1", "aws-cn"),
    ] {
        let policy: Value = serde_json::from_str(&redrive_policy_json(
            region,
            "123456789012",
            "test-failed",
            5,
        ))
        .unwrap();
        assert_eq!(
            policy["deadLetterTargetArn"],
            format!("arn:{partition}:sqs:{region}:123456789012:test-failed")
        );
        assert_eq!(policy["maxReceiveCount"], 5);
    }
}

/// SQS applies the attributes but the response is lost; the setting is removed before the
/// retry. Recorded state still says "no policy", yet the applied policy must be cleared.
#[tokio::test]
async fn a_redrive_write_with_a_lost_response_is_cleared_when_the_setting_is_removed() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cloud: Writes = Default::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut sqs = MockSqsApi::new();
    let (stored, counted) = (cloud.clone(), calls.clone());
    sqs.expect_set_queue_attributes()
        .returning(move |_, request| {
            stored.lock().unwrap().push(request.attributes.clone());
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteServiceUnavailable {
                        message: "response lost after the write".into(),
                    },
                ));
            }
            Ok(())
        });
    let sqs = Arc::new(sqs);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    let plain = Queue::new("tasks".into()).build();
    let mut run = SingleControllerExecutor::builder()
        .resource(plain.clone())
        .controller(controller("tasks"))
        .platform(Platform::Aws)
        .resource_lifecycle(ResourceLifecycle::Live)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .with_stack_resource(Queue::new("failed".into()).build(), ResourceLifecycle::Live)
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap();
    *run.desired_resource_config_mut("tasks").unwrap() = Resource::new(with_dead_letter(7));
    run.update(with_dead_letter(7)).unwrap();
    while calls.load(Ordering::SeqCst) == 0 {
        let _ = run.step().await;
    }
    assert_eq!(
        cloud.lock().unwrap()[0]["RedrivePolicy"].is_empty(),
        false,
        "the lost write carried the policy"
    );

    *run.desired_resource_config_mut("tasks").unwrap() = Resource::new(plain.clone());
    run.run_until_terminal().await.unwrap();
    if run.needs_update().unwrap() {
        run.update(plain).unwrap();
        run.run_until_terminal().await.unwrap();
    }
    let writes = cloud.lock().unwrap();
    assert_eq!(
        writes.last().unwrap()["RedrivePolicy"],
        "",
        "no policy may outlive the setting"
    );
    assert!(!run.needs_update().unwrap());
}

/// The manager stopped after SQS applied the write: the saved state has the intent recorded
/// but none of the written values. Removing the setting must still clear both attributes.
#[tokio::test]
async fn a_saved_write_intent_with_nothing_recorded_clears_the_policy_and_retention() {
    let writes: Writes = Default::default();
    let plain = Queue::new("tasks".into()).build();
    let mut saved = controller("tasks");
    saved.dead_letter_write_started = true;
    let mut run = SingleControllerExecutor::builder()
        .resource(plain.clone())
        .controller(saved)
        .platform(Platform::Aws)
        .resource_lifecycle(ResourceLifecycle::Live)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .with_stack_resource(Queue::new("failed".into()).build(), ResourceLifecycle::Live)
        .service_provider(provider(&writes))
        .build()
        .await
        .unwrap();
    assert!(
        run.needs_update().unwrap(),
        "a saved intent asks for an update"
    );
    run.update(plain).unwrap();
    run.run_until_terminal().await.unwrap();
    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0]["RedrivePolicy"], "");
    assert_eq!(
        writes[0]["MessageRetentionPeriod"],
        AWS_DEFAULT_RETENTION_SECONDS.to_string()
    );
    drop(writes);
    assert!(!run.needs_update().unwrap());
}

/// The write is not sent in the step that records the intent: the intent is saved first.
#[tokio::test]
async fn the_intent_is_saved_before_the_write_is_sent() {
    let writes: Writes = Default::default();
    let plain = Queue::new("tasks".into()).build();
    let mut run = source(
        plain.clone(),
        with_dead_letter(7),
        ResourceLifecycle::Live,
        InitialSetupAuthority::DirectSetup,
        &writes,
    )
    .await;
    run.update(with_dead_letter(7)).unwrap();
    // UpdateStart, UpdatingStorageIngress, then the step that records the intent.
    for _ in 0..3 {
        let _ = run.step().await;
    }
    assert!(writes.lock().unwrap().is_empty(), "nothing sent yet");
    run.run_until_terminal().await.unwrap();
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// The manager stopped inside the applying step after SQS applied a write: the saved state is
/// that step with the intent set and nothing recorded. A release without the setting lands
/// before the resume, which starts in the applying step directly. Both attributes are cleared.
async fn resumed_in(state: AwsQueueState) -> Vec<HashMap<String, String>> {
    let writes: Writes = Default::default();
    let plain = Queue::new("tasks".into()).build();
    let mut saved = controller("tasks");
    saved.state = state;
    saved.dead_letter_write_started = true;
    let mut run = SingleControllerExecutor::builder()
        .resource(plain)
        .controller(saved)
        .platform(Platform::Aws)
        .resource_lifecycle(ResourceLifecycle::Live)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .with_stack_resource(Queue::new("failed".into()).build(), ResourceLifecycle::Live)
        .service_provider(provider(&writes))
        .build()
        .await
        .unwrap();
    run.run_until_terminal().await.unwrap();
    let writes = writes.lock().unwrap().clone();
    writes
}

#[tokio::test]
async fn a_resume_in_the_update_applying_step_clears_the_policy_and_retention() {
    let writes = resumed_in(AwsQueueState::ApplyingDeadLetterUpdate).await;
    assert_eq!(writes.len(), 1, "the clearing write is sent: {writes:?}");
    assert_eq!(writes[0]["RedrivePolicy"], "");
    assert_eq!(
        writes[0]["MessageRetentionPeriod"],
        AWS_DEFAULT_RETENTION_SECONDS.to_string()
    );
}

#[tokio::test]
async fn a_resume_in_the_create_applying_step_clears_the_policy_and_retention() {
    let writes = resumed_in(AwsQueueState::ApplyingDeadLetter).await;
    assert_eq!(writes.len(), 1, "the clearing write is sent: {writes:?}");
    assert_eq!(writes[0]["RedrivePolicy"], "");
    assert_eq!(
        writes[0]["MessageRetentionPeriod"],
        AWS_DEFAULT_RETENTION_SECONDS.to_string()
    );
}

/// An update on an imported setup-owned queue whose desired stack declares a dead letter, with
/// nothing recorded and no intent saved (the state of a queue that predates the setting), writes
/// nothing at runtime.
#[tokio::test]
async fn an_update_on_a_setup_owned_queue_never_writes_the_dead_letter_attributes() {
    let writes: Writes = Default::default();
    let mut run = SingleControllerExecutor::builder()
        .resource(with_dead_letter(5))
        .controller(controller("tasks"))
        .platform(Platform::Aws)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
        .with_stack_resource(
            Queue::new("failed".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .service_provider(provider(&writes))
        .build()
        .await
        .unwrap();
    run.update(with_dead_letter(5)).unwrap();
    run.run_until_terminal().await.unwrap();
    assert!(writes.lock().unwrap().is_empty());
}
