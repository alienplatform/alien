use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};
use std::collections::{HashMap, HashSet};

/// Ensures resources that must appear at most once don't have multiple instances.
///
/// Resources like `ArtifactRegistry` and `Build` must appear at most once in the stack.
pub struct UniqueResourcesCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for UniqueResourcesCheck {
    fn description(&self) -> &'static str {
        "Resources that must appear at most once shouldn't have multiple instances"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any of the unique resource types
        stack.resources().any(|(_, resource_entry)| {
            matches!(
                resource_entry.config.resource_type().0.as_ref(),
                "artifact-registry" | "build"
            )
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let unique_types = HashSet::from(["artifact-registry", "build"]);
        let mut type_counts: HashMap<String, Vec<String>> = HashMap::new();

        for (resource_id, resource_entry) in stack.resources() {
            let resource_type_value = resource_entry.config.resource_type();
            let resource_type = resource_type_value.0.as_ref();

            if unique_types.contains(resource_type) {
                type_counts
                    .entry(resource_type.to_string())
                    .or_default()
                    .push(resource_id.clone());
            }
        }

        let mut errors = Vec::new();

        for (resource_type, resource_ids) in type_counts {
            if resource_ids.len() > 1 {
                errors.push(format!(
                    "Resource type '{}' must appear at most once, but found {} instances: {}",
                    resource_type,
                    resource_ids.len(),
                    resource_ids.join(", ")
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
    use alien_core::{Build, ResourceEntry, ResourceLifecycle};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_unique_resources_success() {
        let build = Build::new("test-build".to_string())
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build),
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

        let check = UniqueResourcesCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_unique_resources_failure() {
        let build1 = Build::new("test-build-1".to_string())
            .permissions("test".to_string())
            .build();
        let build2 = Build::new("test-build-2".to_string())
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build-1".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build1),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-build-2".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build2),
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

        let check = UniqueResourcesCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("must appear at most once"));
    }
}
