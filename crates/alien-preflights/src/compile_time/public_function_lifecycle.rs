use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Function, Platform, ResourceLifecycle, Stack};

/// Ensures functions have Live lifecycle.
///
/// Kept as a focused error message for function users. The canonical rule
/// lives in `alien-core` ownership policy and is also checked by
/// `FrozenResourceLifecycleCheck`.
pub struct PublicFunctionLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PublicFunctionLifecycleCheck {
    fn description(&self) -> &'static str {
        "Functions must have Live lifecycle"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack
            .resources()
            .any(|(_, resource_entry)| resource_entry.config.downcast_ref::<Function>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.downcast_ref::<Function>().is_none() {
                continue;
            }

            if resource_entry.lifecycle != ResourceLifecycle::Live {
                errors.push(format!(
                    "Function '{}' has lifecycle {:?}, but functions must be Live. \
                    Functions are Alien-owned resources and require provision permissions.",
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
    use alien_core::{Function, FunctionCode, Ingress, Resource, ResourceEntry, ResourceLifecycle};
    use indexmap::IndexMap;

    fn create_public_function(id: &str) -> Function {
        Function::new(id.to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .ingress(Ingress::Public)
            .build()
    }

    fn create_private_function(id: &str) -> Function {
        Function::new(id.to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .ingress(Ingress::Private)
            .build()
    }

    #[tokio::test]
    async fn test_function_frozen_fails() {
        let function = create_public_function("my-function");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-function".to_string(),
            ResourceEntry {
                config: Resource::new(function),
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

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("functions must be Live"));
    }

    #[tokio::test]
    async fn test_public_function_live_succeeds() {
        let function = create_public_function("my-function");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-function".to_string(),
            ResourceEntry {
                config: Resource::new(function),
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

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_private_function_frozen_fails() {
        let function = create_private_function("my-function");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-function".to_string(),
            ResourceEntry {
                config: Resource::new(function),
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

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("functions must be Live"));
    }

    #[tokio::test]
    async fn test_should_run_returns_true_for_private_functions() {
        let function = create_private_function("my-function");

        let mut resources = IndexMap::new();
        resources.insert(
            "my-function".to_string(),
            ResourceEntry {
                config: Resource::new(function),
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

        let check = PublicFunctionLifecycleCheck;
        assert!(check.should_run(&stack, Platform::Aws));
    }
}
