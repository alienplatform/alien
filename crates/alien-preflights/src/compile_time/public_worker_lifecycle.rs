use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, ResourceLifecycle, Stack, Worker};

/// Ensures workers have Live lifecycle.
///
/// Kept as a focused error message for worker users. The canonical rule
/// lives in `alien-core` ownership policy and is also checked by
/// `FrozenResourceLifecycleCheck`.
pub struct PublicWorkerLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PublicWorkerLifecycleCheck {
    fn description(&self) -> &'static str {
        "Workers must have Live lifecycle"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack
            .resources()
            .any(|(_, resource_entry)| resource_entry.config.downcast_ref::<Worker>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.downcast_ref::<Worker>().is_none() {
                continue;
            }

            if resource_entry.lifecycle != ResourceLifecycle::Live {
                errors.push(format!(
                    "Worker '{}' has lifecycle {:?}, but workers must be Live. \
                    Workers are Alien-owned resources and require provision permissions.",
                    resource_id, resource_entry.lifecycle
                ));
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
        Resource, ResourceEntry, ResourceLifecycle, Worker, WorkerCode, WorkerPublicEndpoint,
    };
    use indexmap::IndexMap;

    fn create_public_function(id: &str) -> Worker {
        Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .build()
    }

    fn create_private_function(id: &str) -> Worker {
        Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build()
    }

    #[tokio::test]
    async fn test_function_frozen_fails() {
        let worker = create_public_function("my-worker");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-worker".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Frozen, // Wrong!
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

        let check = PublicWorkerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("workers must be Live"));
    }

    #[tokio::test]
    async fn test_public_function_live_succeeds() {
        let worker = create_public_function("my-worker");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-worker".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Live, // Correct
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

        let check = PublicWorkerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_private_function_frozen_fails() {
        let worker = create_private_function("my-worker");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-worker".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Frozen, // OK for private
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

        let check = PublicWorkerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("workers must be Live"));
    }

    #[tokio::test]
    async fn test_should_run_returns_true_for_private_functions() {
        let worker = create_private_function("my-worker");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-worker".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Frozen,
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

        let check = PublicWorkerLifecycleCheck;
        assert!(check.should_run(&stack, Platform::Aws));
    }
}
