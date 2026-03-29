use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Function, Ingress, Platform, Resource, ResourceLifecycle, Stack};

/// Ensures public functions are not Frozen.
///
/// Public functions require certificate renewal and DNS record updates, which means
/// we need to be able to update them. Frozen resources cannot be updated, creating
/// a fundamental conflict.
///
/// **Rule:** Public functions must have `Live` lifecycle.
///
/// Private functions can be any lifecycle since they don't need certificates.
pub struct PublicFunctionLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PublicFunctionLifecycleCheck {
    fn description(&self) -> &'static str {
        "Public functions cannot be Frozen (certificate renewal requires updates)"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any public functions
        stack
            .resources()
            .any(|(_, resource_entry)| is_public_function(&resource_entry.config))
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if !is_public_function(&resource_entry.config) {
                continue;
            }

            if resource_entry.lifecycle == ResourceLifecycle::Frozen {
                errors.push(format!(
                    "Function '{}' has public ingress but Frozen lifecycle. \
                    Public functions require certificate renewal every 90 days, which requires updating the function. \
                    Change the lifecycle to Live, or remove public ingress.",
                    resource_id
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

/// Check if a function has public ingress
fn is_public_function(resource: &Resource) -> bool {
    if let Some(function) = resource.downcast_ref::<Function>() {
        return function.ingress == Ingress::Public;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Function, FunctionCode, ResourceEntry, ResourceLifecycle};
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
    async fn test_public_function_frozen_fails() {
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
        };

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("public ingress but Frozen lifecycle"));
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
        };

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_private_function_frozen_succeeds() {
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
        };

        let check = PublicFunctionLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_should_run_returns_false_for_no_public_functions() {
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
        };

        let check = PublicFunctionLifecycleCheck;
        assert!(!check.should_run(&stack, Platform::Aws));
    }
}
