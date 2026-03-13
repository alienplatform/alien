use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, ResourceLifecycle, Stack};

/// Ensures all containers have Live lifecycle.
///
/// Containers run continuously and need to be updated for:
/// - Code changes
/// - Configuration changes  
/// - Certificate renewal (if public)
/// - Scaling
///
/// Unlike functions, containers cannot be "setup once and frozen" - they are
/// long-running processes that require ongoing management.
///
/// **Rule:** All containers must have `Live` lifecycle.
pub struct ContainerLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ContainerLifecycleCheck {
    fn description(&self) -> &'static str {
        "All containers must have Live lifecycle"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any containers
        stack
            .resources()
            .any(|(_, resource_entry)| resource_entry.config.downcast_ref::<Container>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.downcast_ref::<Container>().is_none() {
                continue;
            }

            if resource_entry.lifecycle != ResourceLifecycle::Live {
                errors.push(format!(
                    "Container '{}' has lifecycle {:?}, but containers must be Live. \
                    Containers are long-running processes that need ongoing updates (code, config, certificates, scaling). \
                    Change the lifecycle to Live.",
                    resource_id,
                    resource_entry.lifecycle
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
    use alien_core::{Container, Resource, ResourceEntry, ResourceLifecycle};
    use indexmap::IndexMap;

    fn create_container(id: &str) -> Container {
        use alien_core::{ContainerCode, ResourceSpec};

        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test".to_string())
            .build()
    }

    #[tokio::test]
    async fn test_container_live_succeeds() {
        let container = create_container("my-container");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-container".to_string(),
            ResourceEntry {
                config: Resource::new(container),
                lifecycle: ResourceLifecycle::Live, // Correct
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = ContainerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_container_frozen_fails() {
        let container = create_container("my-container");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-container".to_string(),
            ResourceEntry {
                config: Resource::new(container),
                lifecycle: ResourceLifecycle::Frozen, // Wrong
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = ContainerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("containers must be Live"));
    }

    #[tokio::test]
    async fn test_container_liveonsetup_fails() {
        let container = create_container("my-container");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-container".to_string(),
            ResourceEntry {
                config: Resource::new(container),
                lifecycle: ResourceLifecycle::LiveOnSetup, // Wrong
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = ContainerLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("containers must be Live"));
    }

    #[tokio::test]
    async fn test_should_run_returns_false_for_no_containers() {
        let resources = IndexMap::new();

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = ContainerLifecycleCheck;
        assert!(!check.should_run(&stack, Platform::Aws));
    }
}
