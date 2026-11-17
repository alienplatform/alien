//! A Queue's dead-letter setting reaches the generated Terraform of every cloud.

use super::helpers::{assert_terraform_valid, normalized, render};
use alien_core::{
    Queue, QueueDeadLetter, ResourceLifecycle, ResourceRef, Stack, StackSettings,
};
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
    assert!(text.contains("redrive_policy"), "{text}");
    assert!(text.contains("maxReceiveCount"), "{text}");
    assert!(text.contains("deadLetterTargetArn"), "{text}");
    assert_eq!(text.matches("message_retention_seconds = 1209600").count(), 1);
    assert_eq!(text.matches("message_retention_seconds = 345600").count(), 1);
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
    assert_eq!(text.matches("message_retention_seconds = 345600").count(), 2);
}
