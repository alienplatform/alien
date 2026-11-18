//! A Queue's dead-letter setting reaches the generated Terraform of every cloud.

use super::helpers::{assert_terraform_valid, normalized, render};
use alien_core::{Queue, QueueDeadLetter, ResourceLifecycle, ResourceRef, Stack, StackSettings};
use alien_terraform::TerraformTarget;

fn stack(dead_letter: bool, lifecycle: ResourceLifecycle) -> Stack {
    let mut tasks = Queue::new("tasks".into()).build();
    if dead_letter {
        tasks.dead_letter = Some(
            QueueDeadLetter::builder()
                .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "tasks-failed"))
                .max_attempts(7)
                .build(),
        );
    }
    Stack::new("dead-letter".into())
        .add(tasks, lifecycle)
        .add(Queue::new("tasks-failed".into()).build(), lifecycle)
        .build()
}

#[test]
fn aws_redrive_policy_points_at_the_target_and_the_target_keeps_messages_longer() {
    let module = render(
        &stack(true, ResourceLifecycle::Frozen),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let text = normalized(&module);
    // A separate resource: an inline `redrive_policy` is kept by the provider when removed,
    // so removing the setting would leave it on the queue.
    assert!(
        text.contains("resource \"aws_sqs_queue_redrive_policy\""),
        "{text}"
    );
    assert!(text.contains("maxReceiveCount"), "{text}");
    assert!(text.contains("deadLetterTargetArn"), "{text}");
    assert_eq!(
        text.matches("message_retention_seconds = 1209600").count(),
        1
    );
    assert_eq!(
        text.matches("message_retention_seconds = 345600").count(),
        1
    );
    assert_terraform_valid(&module, "AWS queue with a dead-letter target");
}

#[test]
fn aws_queues_without_a_dead_letter_render_exactly_as_before() {
    let module = render(
        &stack(false, ResourceLifecycle::Frozen),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let text = normalized(&module);
    assert!(!text.contains("redrive_policy"));
    assert!(!text.contains("1209600"));
    assert_eq!(
        text.matches("message_retention_seconds = 345600").count(),
        2
    );
}

fn azure_stack(dead_letter: bool) -> Stack {
    use alien_core::{AzureResourceGroup, AzureServiceBusNamespace, AzureStorageAccount};
    let lifecycle = ResourceLifecycle::Frozen;
    let mut tasks = Queue::new("tasks".into()).build();
    if dead_letter {
        tasks.dead_letter = Some(
            QueueDeadLetter::builder()
                .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "tasks-failed"))
                .max_attempts(7)
                .build(),
        );
    }
    Stack::new("dead-letter".into())
        .add(
            AzureResourceGroup::new("default-resource-group".into()).build(),
            lifecycle,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".into()).build(),
            lifecycle,
        )
        .add(
            AzureServiceBusNamespace::new("default-service-bus-namespace".into()).build(),
            lifecycle,
        )
        .add(tasks, lifecycle)
        .add(Queue::new("tasks-failed".into()).build(), lifecycle)
        .build()
}

#[test]
fn azure_queue_forwards_dead_lettered_messages_to_the_target() {
    let module = render(
        &azure_stack(true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let text = normalized(&module);
    assert!(text.contains("forward_dead_lettered_messages_to"), "{text}");
    assert!(text.contains("max_delivery_count = 7"), "{text}");
    assert_terraform_valid(&module, "Azure queue with a dead-letter target");
}

#[test]
fn azure_queues_without_a_dead_letter_keep_todays_settings() {
    let module = render(
        &azure_stack(false),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let text = normalized(&module);
    assert!(!text.contains("forward_dead_lettered_messages_to"));
    assert_eq!(text.matches("max_delivery_count = 10").count(), 2);
}

#[test]
fn gcp_subscription_gets_a_dead_letter_policy_and_minimal_service_agent_grants() {
    let module = render(
        &stack(true, ResourceLifecycle::Frozen),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let text = normalized(&module);
    assert!(text.contains("dead_letter_policy"), "{text}");
    assert!(text.contains("max_delivery_attempts = 7"), "{text}");
    // Publisher on the target topic and subscriber on the source subscription, nothing wider.
    assert_eq!(text.matches("roles/pubsub.publisher").count(), 1, "{text}");
    assert_eq!(text.matches("roles/pubsub.subscriber").count(), 1, "{text}");
    assert!(!text.contains("google_project_iam"), "{text}");
    assert!(
        text.contains("gcp-sa-pubsub.iam.gserviceaccount.com"),
        "{text}"
    );
    assert_terraform_valid(&module, "GCP queue with a dead-letter target");
}

#[test]
fn gcp_queues_without_a_dead_letter_render_no_policy_or_service_agent_grant() {
    let module = render(
        &stack(false, ResourceLifecycle::Frozen),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    let text = normalized(&module);
    assert!(!text.contains("dead_letter_policy"));
    assert!(!text.contains("gcp-sa-pubsub"));
    assert!(!text.contains("roles/pubsub.subscriber"));
}

/// Two dead-lettering queues, each triggering a Live Worker, with the queues' lifecycle given.
fn worker_stack(lifecycle: ResourceLifecycle, queues: &[&str]) -> Stack {
    use alien_core::{Worker, WorkerCode, WorkerTrigger};
    let mut builder = Stack::new("dead-letter".into());
    builder = builder.add(Queue::new("tasks-failed".into()).build(), lifecycle);
    for id in queues {
        let mut queue = Queue::new((*id).into()).build();
        queue.dead_letter = Some(
            QueueDeadLetter::builder()
                .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "tasks-failed"))
                .build(),
        );
        let worker = Worker::new(format!("{id}-consumer"))
            .code(WorkerCode::Image {
                image: "example.com/app:1".into(),
            })
            .permissions("execution".into())
            .trigger(WorkerTrigger::queue(&queue))
            .build();
        builder = builder
            .add(queue, lifecycle)
            .add(worker, ResourceLifecycle::Live);
    }
    builder.build()
}

#[test]
fn gcp_frozen_queues_with_a_live_worker_get_one_project_subscriber_grant() {
    for queues in [&["tasks"][..], &["tasks", "other"][..]] {
        let module = render(
            &worker_stack(ResourceLifecycle::Frozen, queues),
            TerraformTarget::Gcp,
            StackSettings::default(),
        );
        let text = normalized(&module);
        // One project-level member for the Pub/Sub service agent, however many queues need it.
        assert_eq!(
            text.matches("resource \"google_project_iam_member\" \"dead_letter_push_subscriber\"")
                .count(),
            1,
            "{queues:?}: {text}"
        );
        assert!(text.contains("roles/pubsub.subscriber"), "{text}");
        assert_terraform_valid(&module, "GCP Frozen queues with a Live Worker trigger");
    }
}

#[test]
fn gcp_live_queues_and_workerless_queues_get_no_project_subscriber_grant() {
    let live = render(
        &worker_stack(ResourceLifecycle::Live, &["tasks"]),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    assert!(!normalized(&live).contains("dead_letter_push_subscriber"));
    let workerless = render(
        &stack(true, ResourceLifecycle::Frozen),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );
    assert!(!normalized(&workerless).contains("dead_letter_push_subscriber"));
}
