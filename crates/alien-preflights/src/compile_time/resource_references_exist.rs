use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};
use std::collections::HashSet;

/// Ensures all resource references point to resources that exist within the stack.
pub struct ResourceReferencesExistCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ResourceReferencesExistCheck {
    fn description(&self) -> &'static str {
        "All resource references should point to resources that exist within the stack"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true // Always run this check
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        // Collect all resource IDs in the stack
        let resource_ids: HashSet<String> = stack.resources().map(|(id, _)| id.clone()).collect();

        for (resource_id, resource_entry) in stack.resources() {
            // Check explicit dependencies
            for dep in &resource_entry.dependencies {
                if !resource_ids.contains(dep.id()) {
                    errors.push(format!(
                        "Resource '{}' depends on '{}' which does not exist in the stack",
                        resource_id,
                        dep.id()
                    ));
                }
            }

            // Check implicit dependencies from resource configuration
            for dep_ref in resource_entry.config.get_dependencies() {
                if !resource_ids.contains(dep_ref.id()) {
                    errors.push(format!(
                        "Resource '{}' references '{}' which does not exist in the stack",
                        resource_id,
                        dep_ref.id()
                    ));
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
        Function, FunctionCode, ResourceEntry, ResourceLifecycle, ResourceRef, Storage,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_valid_references_success() {
        let storage = Storage::new("storage".to_string()).build();
        let function = Function::new("function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .link(&storage) // Valid reference to existing storage
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "function".to_string(),
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

        let check = ResourceReferencesExistCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_invalid_reference_failure() {
        let function = Function::new("function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: vec![ResourceRef::new(
                    alien_core::ResourceType::from("storage"),
                    "nonexistent-storage".to_string(), // Reference to non-existent resource
                )],
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = ResourceReferencesExistCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("does not exist in the stack"));
    }
}
