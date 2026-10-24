use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, Worker, WorkerTrigger};

/// Validate the supported single queue trigger and its batch settings.
pub struct SingleQueueTriggerCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SingleQueueTriggerCheck {
    fn description(&self) -> &'static str {
        "Workers support one queue trigger with valid batch settings"
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
                    let count = worker
                        .triggers
                        .iter()
                        .filter(|trigger| matches!(trigger, WorkerTrigger::Queue { .. }))
                        .count();
                    if count > 1 {
                        errors.push(format!("Worker '{}' has {} queue triggers, but only one queue trigger per worker is supported", resource_id, count));
                    }
                    let mut sources = std::collections::HashSet::new();
                    for trigger in &worker.triggers {
                        if let WorkerTrigger::Queue { queue, batch } = trigger {
                            if !sources.insert(&queue.id) {
                                errors.push(format!(
                                    "Worker '{}' repeats queue trigger '{}'",
                                    resource_id, queue.id
                                ));
                            }
                            if let Some(batch) = batch {
                                if let Err(error) = batch.validate() {
                                    errors.push(error.to_string());
                                }
                            }
                        }
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
    async fn test_multiple_queue_triggers_rejected() {
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "test-queue-2".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(queue2),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
        assert_eq!(result.errors.len(), 1);
    }
}
