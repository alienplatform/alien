use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Function, FunctionTrigger, Platform, Stack};

/// Ensures Functions have at most one queue trigger each.
///
/// Multiple queue triggers per function are not currently supported due to
/// implementation complexity around partial failure scenarios in event source mapping creation.
pub struct SingleQueueTriggerCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SingleQueueTriggerCheck {
    fn description(&self) -> &'static str {
        "Functions should have at most one queue trigger each"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any Function resources
        stack.resources().any(|(_, resource_entry)| {
            resource_entry.config.resource_type().0.as_ref() == "function"
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.resource_type().0.as_ref() == "function" {
                if let Some(function) = resource_entry.config.downcast_ref::<Function>() {
                    let queue_trigger_count = function
                        .triggers
                        .iter()
                        .filter(|trigger| matches!(trigger, FunctionTrigger::Queue { .. }))
                        .count();

                    if queue_trigger_count > 1 {
                        errors.push(format!(
                            "Function '{}' has {} queue triggers, but only one queue trigger per function is supported",
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
    use alien_core::{
        Function, FunctionCode, FunctionTrigger, Queue, ResourceEntry, ResourceLifecycle,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_single_queue_trigger_success() {
        let queue = Queue::new("test-queue".to_string()).build();
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(FunctionTrigger::queue(&queue))
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
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
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
        };

        let check = SingleQueueTriggerCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_multiple_queue_triggers_failure() {
        let queue1 = Queue::new("test-queue-1".to_string()).build();
        let queue2 = Queue::new("test-queue-2".to_string()).build();

        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(FunctionTrigger::queue(&queue1))
            .trigger(FunctionTrigger::queue(&queue2)) // Multiple queue triggers
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
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
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
        };

        let check = SingleQueueTriggerCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("queue triggers"));
    }
}
