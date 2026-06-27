use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, Worker, WorkerTrigger};

/// Ensures Workers have at most one queue trigger each.
///
/// Multiple queue triggers per worker are not currently supported due to
/// implementation complexity around partial failure scenarios in event source mapping creation.
pub struct SingleQueueTriggerCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SingleQueueTriggerCheck {
    fn description(&self) -> &'static str {
        "Workers should have at most one queue trigger each"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any Worker resources
        stack
            .resources()
            .any(|(_, resource_entry)| resource_entry.config.resource_type().0.as_ref() == "worker")
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.resource_type().0.as_ref() == "worker" {
                if let Some(worker) = resource_entry.config.downcast_ref::<Worker>() {
                    let queue_trigger_count = worker
                        .triggers
                        .iter()
                        .filter(|trigger| matches!(trigger, WorkerTrigger::Queue { .. }))
                        .count();

                    if queue_trigger_count > 1 {
                        errors.push(format!(
                            "Worker '{}' has {} queue triggers, but only one queue trigger per worker is supported",
                            resource_id,
                            queue_trigger_count
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Queue, ResourceEntry, ResourceLifecycle, Worker, WorkerCode, WorkerTrigger};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_single_queue_trigger_success() {
        let queue = Queue::new("test-queue".to_string()).build();
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::queue(&queue))
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-queue".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(queue),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = SingleQueueTriggerCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_multiple_queue_triggers_failure() {
        let queue1 = Queue::new("test-queue-1".to_string()).build();
        let queue2 = Queue::new("test-queue-2".to_string()).build();

        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::queue(&queue1))
            .trigger(WorkerTrigger::queue(&queue2)) // Multiple queue triggers
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-queue-1".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(queue1),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-queue-2".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(queue2),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = SingleQueueTriggerCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("queue triggers"));
    }
}
