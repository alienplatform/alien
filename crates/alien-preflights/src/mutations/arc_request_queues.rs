use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Function, FunctionTrigger, Platform, Queue, Resource, ResourceEntry,
    ResourceLifecycle, Stack, StackState,
};

/// Adds request queues for functions with ARC (Alien Remote Call) enabled to support
/// the ARC protocol for remote function invocation.
///
/// For each ARC-enabled function on GCP or Azure, this creates a corresponding Queue resource
/// with the naming pattern `{functionName}-rq` and adds it as a trigger to the function.
/// AWS Lambda has built-in async invocation queues, and Kubernetes uses HTTP polling,
/// so no additional queues are needed for those platforms.
pub struct ArcRequestQueuesMutation;

#[async_trait::async_trait]
impl StackMutation for ArcRequestQueuesMutation {
    fn description(&self) -> &'static str {
        "Add request queues for ARC-enabled functions"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only process ARC queues for GCP and Azure platforms
        // AWS Lambda has built-in async invocation queue, Kubernetes uses HTTP polling
        if !matches!(stack_state.platform, Platform::Gcp | Platform::Azure) {
            return false;
        }

        // Check if there are any ARC-enabled functions
        stack.resources().any(|(_, resource_entry)| {
            if let Some(function) = resource_entry.config.downcast_ref::<Function>() {
                function.arc_enabled
            } else {
                false
            }
        })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        // Only process ARC queues for GCP and Azure platforms
        // AWS Lambda has built-in async invocation queue, Kubernetes uses HTTP polling
        if !matches!(stack_state.platform, Platform::Gcp | Platform::Azure) {
            return Ok(stack);
        }

        // Collect functions that need ARC request queues
        let mut functions_needing_queues: Vec<(String, Function)> = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if let Some(function) = resource_entry.config.downcast_ref::<Function>() {
                if function.arc_enabled {
                    functions_needing_queues.push((resource_id.clone(), function.clone()));
                }
            }
        }

        // Process each ARC-enabled function
        for (function_id, mut function) in functions_needing_queues {
            let queue_id = format!("{}-rq", function_id);

            // Check if the request queue already exists
            if stack.resources().any(|(id, _)| id == &queue_id) {
                continue; // Skip if already exists
            }

            // Get the lifecycle of the function to match it for the queue
            let function_lifecycle = stack
                .resources()
                .find(|(id, _)| id.as_str() == function_id.as_str())
                .map(|(_, entry)| entry.lifecycle)
                .unwrap_or(ResourceLifecycle::Live); // Default fallback

            // Create the request queue for this function
            let request_queue = Queue::new(queue_id.clone()).build();

            // Add the queue with the same lifecycle as the function
            let queue_entry = ResourceEntry {
                config: Resource::new(request_queue.clone()),
                lifecycle: function_lifecycle,
                dependencies: Vec::new(),
                remote_access: false,
            };
            stack.resources.insert(queue_id.clone(), queue_entry);

            // Add queue trigger to the function
            let queue_trigger = FunctionTrigger::queue(&request_queue);
            function.triggers.push(queue_trigger);

            // Update the function in the stack with the new trigger
            if let Some(function_entry) = stack.resources.get_mut(&function_id) {
                function_entry.config = Resource::new(function);
            }
        }

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, Function, FunctionCode, ResourceEntry,
        ResourceLifecycle, StackSettings,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_arc_queues_added_for_gcp() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .arc_enabled(true)
            .build();

        let mut resources = IndexMap::new();
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
        };

        let stack_state = alien_core::StackState::new(Platform::Gcp);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ArcRequestQueuesMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that the request queue was created
        assert!(result_stack
            .resources()
            .any(|(id, _)| id == "test-function-rq"));

        // Check that the function has a queue trigger
        let function_entry = result_stack
            .resources()
            .find(|(id, _)| id.as_str() == "test-function")
            .unwrap()
            .1;

        if let Some(function) = function_entry.config.downcast_ref::<Function>() {
            assert_eq!(function.triggers.len(), 1);
            assert!(matches!(
                function.triggers[0],
                FunctionTrigger::Queue { .. }
            ));
        } else {
            panic!("Expected Function resource");
        }
    }

    #[tokio::test]
    async fn test_arc_queues_not_added_for_aws() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .arc_enabled(true)
            .build();

        let mut resources = IndexMap::new();
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
        };

        let mutation = ArcRequestQueuesMutation;

        let stack_state_aws = alien_core::StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        // Check that should_run returns false for AWS
        assert!(!mutation.should_run(&stack, &stack_state_aws, &config));

        // Check that mutate() also respects the platform and doesn't add queues for AWS
        let result_stack = mutation
            .mutate(stack, &stack_state_aws, &config)
            .await
            .unwrap();

        // Check that no request queue was created for AWS
        assert!(!result_stack
            .resources()
            .any(|(id, _)| id == "test-function-rq"));

        // Check that the function has no additional triggers
        let function_entry = result_stack
            .resources()
            .find(|(id, _)| id.as_str() == "test-function")
            .unwrap()
            .1;

        if let Some(function) = function_entry.config.downcast_ref::<Function>() {
            assert_eq!(function.triggers.len(), 0);
        }
    }

    #[tokio::test]
    async fn test_existing_queue_not_duplicated() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .arc_enabled(true)
            .build();

        let existing_queue = Queue::new("test-function-rq".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-function-rq".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(existing_queue),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let stack_state = alien_core::StackState::new(Platform::Gcp);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ArcRequestQueuesMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that there's still only one queue with this ID
        let queue_count = result_stack
            .resources()
            .filter(|(id, _)| id.as_str() == "test-function-rq")
            .count();
        assert_eq!(queue_count, 1);
    }
}
