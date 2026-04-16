use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, ResourceLifecycle, Stack};
use std::collections::HashSet;

/// Ensures resources that must be Frozen are marked with Frozen lifecycle only.
///
/// Resources like `ArtifactRegistry` and `Build` must have Frozen lifecycle.
pub struct FrozenResourceLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for FrozenResourceLifecycleCheck {
    fn description(&self) -> &'static str {
        "Resources that must be Frozen should be marked with Frozen lifecycle only"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Check if stack contains any of the must-be-frozen resource types
        stack.resources().any(|(_, resource_entry)| {
            matches!(
                resource_entry.config.resource_type().0.as_ref(),
                "artifact-registry" | "build"
            )
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let must_be_frozen_types = HashSet::from(["artifact-registry", "build"]);
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            let resource_type_value = resource_entry.config.resource_type();
            let resource_type = resource_type_value.0.as_ref();

            if must_be_frozen_types.contains(resource_type)
                && resource_entry.lifecycle != ResourceLifecycle::Frozen
            {
                errors.push(format!(
                    "Resource '{}' of type '{}' must have Frozen lifecycle, but has {:?}",
                    resource_id, resource_type, resource_entry.lifecycle
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
    use alien_core::{ArtifactRegistry, Build, ResourceEntry, ResourceLifecycle};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_frozen_lifecycle_success() {
        let build = Build::new("test-build".to_string())
            .permissions("test".to_string())
            .build();
        let registry = ArtifactRegistry::new("test-registry".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build),
                lifecycle: ResourceLifecycle::Frozen, // Correct lifecycle
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
                lifecycle: ResourceLifecycle::Frozen, // Correct lifecycle
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_frozen_lifecycle_failure() {
        let build = Build::new("test-build".to_string())
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build),
                lifecycle: ResourceLifecycle::Live, // Wrong lifecycle!
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("must have Frozen lifecycle"));
    }
}
