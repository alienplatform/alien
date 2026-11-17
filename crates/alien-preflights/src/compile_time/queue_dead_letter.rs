use crate::{error::Result, CheckResult, CompileTimeCheck};
use alien_core::{
    Platform, Queue, Stack, MAX_DEAD_LETTER_MAX_ATTEMPTS, MIN_DEAD_LETTER_MAX_ATTEMPTS,
};

/// Checks a Queue's dead-letter setting: attempt range, target ownership and platform support.
pub struct QueueDeadLetterCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for QueueDeadLetterCheck {
    fn code(&self) -> Option<&'static str> {
        Some("QUEUE_DEAD_LETTER_INVALID")
    }

    fn description(&self) -> &'static str {
        "Queue dead-letter targets are Queues of the same stack and lifecycle, on a supported platform"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Queue>()
                .is_some_and(|queue| queue.dead_letter.is_some())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        for (id, entry) in &stack.resources {
            let Some(queue) = entry.config.downcast_ref::<Queue>() else {
                continue;
            };
            let Some(dead_letter) = &queue.dead_letter else {
                continue;
            };
            let label = format!("Queue '{id}': deadLetter");

            if !matches!(
                platform,
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Local
            ) {
                result.add_error(format!(
                    "{label} is not supported on {platform}: Alien creates no queue there, so configure the dead-letter target on the external queue"
                ));
            }
            if !(MIN_DEAD_LETTER_MAX_ATTEMPTS..=MAX_DEAD_LETTER_MAX_ATTEMPTS)
                .contains(&dead_letter.max_attempts)
            {
                result.add_error(format!(
                    "{label}.maxAttempts must be between {MIN_DEAD_LETTER_MAX_ATTEMPTS} and {MAX_DEAD_LETTER_MAX_ATTEMPTS}; got {}",
                    dead_letter.max_attempts
                ));
            }
            if dead_letter.queue.resource_type != Queue::RESOURCE_TYPE {
                result.add_error(format!("{label}.queue must reference a Queue"));
                continue;
            }
            if dead_letter.queue.id == *id {
                result.add_error(format!(
                    "{label}.queue must be a different Queue than '{id}' itself"
                ));
                continue;
            }
            let Some(target) = stack.resources.get(&dead_letter.queue.id) else {
                result.add_error(format!(
                    "{label}.queue '{}' does not exist in the stack",
                    dead_letter.queue.id
                ));
                continue;
            };
            let Some(target_queue) = target.config.downcast_ref::<Queue>() else {
                result.add_error(format!(
                    "{label}.queue '{}' is not a Queue",
                    dead_letter.queue.id
                ));
                continue;
            };
            if entry.lifecycle != target.lifecycle {
                result.add_error(format!(
                    "{label}.queue '{}': both Queues must have the same lifecycle (both Frozen or both Live)",
                    dead_letter.queue.id
                ));
            }
            if target_queue.dead_letter.is_some() {
                result.add_error(format!(
                    "{label}.queue '{}' has a deadLetter of its own; chains of dead-letter queues are not supported",
                    dead_letter.queue.id
                ));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{QueueDeadLetter, ResourceLifecycle, ResourceRef};

    fn dead_letter(target: &str, max_attempts: u32) -> QueueDeadLetter {
        QueueDeadLetter::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, target))
            .max_attempts(max_attempts)
            .build()
    }

    fn stack(
        source: Option<QueueDeadLetter>,
        source_lifecycle: ResourceLifecycle,
        target: Queue,
        target_lifecycle: ResourceLifecycle,
    ) -> Stack {
        let mut tasks = Queue::new("tasks".to_string()).build();
        tasks.dead_letter = source;
        Stack::new("dead-letter".to_string())
            .add(tasks, source_lifecycle)
            .add(target, target_lifecycle)
            .build()
    }

    fn simple(source: Option<QueueDeadLetter>, lifecycle: ResourceLifecycle) -> Stack {
        stack(
            source,
            lifecycle,
            Queue::new("failed".to_string()).build(),
            lifecycle,
        )
    }

    async fn errors(stack: &Stack, platform: Platform) -> Vec<String> {
        QueueDeadLetterCheck
            .check(stack, platform)
            .await
            .expect("check runs")
            .errors
    }

    #[tokio::test]
    async fn accepts_frozen_and_live_pairs_on_every_cloud_and_local() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let stack = simple(Some(dead_letter("failed", 5)), lifecycle);
            for platform in [
                Platform::Aws,
                Platform::Gcp,
                Platform::Azure,
                Platform::Local,
            ] {
                assert!(QueueDeadLetterCheck.should_run(&stack, platform));
                assert_eq!(errors(&stack, platform).await, Vec::<String>::new());
            }
        }
    }

    #[tokio::test]
    async fn does_not_run_without_a_dead_letter_setting() {
        let stack = simple(None, ResourceLifecycle::Frozen);
        assert!(!QueueDeadLetterCheck.should_run(&stack, Platform::Aws));
    }

    #[tokio::test]
    async fn max_attempts_must_be_between_five_and_one_hundred() {
        for (attempts, valid) in [(4, false), (5, true), (100, true), (101, false), (0, false)] {
            let stack = simple(
                Some(dead_letter("failed", attempts)),
                ResourceLifecycle::Frozen,
            );
            let errors = errors(&stack, Platform::Aws).await;
            assert_eq!(errors.is_empty(), valid, "{attempts}: {errors:?}");
            if !valid {
                assert!(errors[0].contains("between 5 and 100"), "{errors:?}");
            }
        }
    }

    #[tokio::test]
    async fn target_must_exist_and_not_be_the_source() {
        let missing = simple(Some(dead_letter("nope", 5)), ResourceLifecycle::Frozen);
        assert!(errors(&missing, Platform::Aws).await[0].contains("does not exist"));
        let itself = simple(Some(dead_letter("tasks", 5)), ResourceLifecycle::Frozen);
        assert!(errors(&itself, Platform::Aws).await[0].contains("different Queue"));
    }

    #[tokio::test]
    async fn target_must_be_a_queue() {
        let mut source = dead_letter("failed", 5);
        source.queue = ResourceRef::new(alien_core::Storage::RESOURCE_TYPE, "failed");
        let stack = simple(Some(source), ResourceLifecycle::Frozen);
        assert!(errors(&stack, Platform::Aws).await[0].contains("must reference a Queue"));
    }

    #[tokio::test]
    async fn source_and_target_must_share_a_lifecycle() {
        let stack = stack(
            Some(dead_letter("failed", 5)),
            ResourceLifecycle::Frozen,
            Queue::new("failed".to_string()).build(),
            ResourceLifecycle::Live,
        );
        assert!(errors(&stack, Platform::Gcp).await[0].contains("same lifecycle"));
    }

    #[tokio::test]
    async fn target_cannot_have_its_own_dead_letter() {
        let mut target = Queue::new("failed".to_string()).build();
        target.dead_letter = Some(dead_letter("tasks", 5));
        let stack = stack(
            Some(dead_letter("failed", 5)),
            ResourceLifecycle::Frozen,
            target,
            ResourceLifecycle::Frozen,
        );
        assert!(errors(&stack, Platform::Azure)
            .await
            .iter()
            .any(|error| error.contains("chains")));
    }

    #[tokio::test]
    async fn kubernetes_and_machines_reject_the_setting() {
        let stack = simple(Some(dead_letter("failed", 5)), ResourceLifecycle::Frozen);
        for platform in [Platform::Kubernetes, Platform::Machines] {
            let errors = errors(&stack, platform).await;
            assert!(
                errors[0].contains("external queue"),
                "{platform}: {errors:?}"
            );
        }
    }
}
