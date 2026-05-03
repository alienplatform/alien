use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::{ResourceLifecycle, Stack};
use std::collections::{HashMap, HashSet};

/// Validates that frozen resources haven't been added or modified during stack updates.
///
/// Frozen resources are created once during initial deployment and should remain unchanged.
/// This is critical because:
/// 1. Updates only deploy live resources (frozen resources are skipped)
/// 2. Adding frozen resources during update creates inconsistent state
/// 3. Modifying frozen resources risks breaking security/permission models
pub struct FrozenResourcesUnchangedCheck;

#[async_trait::async_trait]
impl StackCompatibilityCheck for FrozenResourcesUnchangedCheck {
    fn description(&self) -> &'static str {
        "Frozen resources shouldn't be added or modified during updates"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        let mut errors = Vec::new();

        // Collect frozen resources from old stack
        let old_frozen: HashMap<_, _> = old_stack
            .resources()
            .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
            .map(|(id, entry)| (id.as_str(), entry))
            .collect();

        // Collect frozen resources from new stack
        let new_frozen: HashMap<_, _> = new_stack
            .resources()
            .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
            .map(|(id, entry)| (id.as_str(), entry))
            .collect();

        // Check for added frozen resources
        let old_frozen_ids: HashSet<_> = old_frozen.keys().copied().collect();
        let added_frozen: Vec<_> = new_frozen
            .keys()
            .filter(|id| !old_frozen_ids.contains(*id))
            .collect();

        if !added_frozen.is_empty() {
            errors.push(format!(
                "Cannot add frozen resources during update: {}. \
                 Frozen resources can only be added during initial deployment. \
                 \n\n💡 To proceed with frozen infrastructure changes:\n   \
                 1. Run with --allow-frozen-changes (requires elevated permissions)\n   \
                 2. Or perform a full redeployment",
                added_frozen
                    .iter()
                    .map(|s| format!("'{}'", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Check frozen resources from old stack
        for (id, old_entry) in &old_frozen {
            // Check if the resource still exists in new stack (by ID, regardless of lifecycle)
            if let Some(new_entry) = new_stack.resources.get(*id) {
                // Check if lifecycle changed (from Frozen to something else)
                if new_entry.lifecycle != ResourceLifecycle::Frozen {
                    errors.push(format!(
                        "Resource '{}' changed from Frozen to {:?} lifecycle. \
                         Frozen resources must remain frozen.",
                        id, new_entry.lifecycle
                    ));
                    continue;
                }

                // Check if configuration changed (only check if still frozen)
                if old_entry.config != new_entry.config {
                    errors.push(format!(
                        "Frozen resource '{}' was modified. \
                         Frozen resources cannot be changed after initial deployment.",
                        id
                    ));
                }
            }
            // Note: Removal of frozen resources is allowed (deletion scenario)
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
    use alien_core::permissions::PermissionsConfig;
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, Stack, Storage};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_unchanged_frozen_resources_success() {
        let storage = Storage::new("test-storage".to_string()).build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage.clone()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let mut new_resources = IndexMap::new();
        new_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_added_frozen_resource_failure() {
        let storage1 = Storage::new("storage-1".to_string()).build();
        let storage2 = Storage::new("storage-2".to_string()).build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "storage-1".to_string(),
            ResourceEntry {
                config: Resource::new(storage1.clone()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let mut new_resources = IndexMap::new();
        new_resources.insert(
            "storage-1".to_string(),
            ResourceEntry {
                config: Resource::new(storage1),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );
        new_resources.insert(
            "storage-2".to_string(),
            ResourceEntry {
                config: Resource::new(storage2),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("storage-2"));
        assert!(result.errors[0].contains("Cannot add frozen resources during update"));
    }

    #[tokio::test]
    async fn test_modified_frozen_resource_failure() {
        let storage_old = Storage::new("test-storage".to_string())
            .public_read(false)
            .build();
        let storage_new = Storage::new("test-storage".to_string())
            .public_read(true)
            .build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage_old),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let mut new_resources = IndexMap::new();
        new_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage_new),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("test-storage"));
        assert!(result.errors[0].contains("was modified"));
    }

    #[tokio::test]
    async fn test_lifecycle_change_failure() {
        let storage = Storage::new("test-storage".to_string()).build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage.clone()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let mut new_resources = IndexMap::new();
        new_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Live, // Changed from Frozen to Live
                dependencies: vec![],
                remote_access: false,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("test-storage"));
        assert!(result.errors[0].contains("changed from Frozen"));
    }

    #[tokio::test]
    async fn test_removed_frozen_resource_allowed() {
        let storage = Storage::new("test-storage".to_string()).build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
            },
        );

        let new_resources = IndexMap::new(); // Empty - resource removed

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        // Should succeed - removing frozen resources is allowed (deletion scenario)
        assert!(result.success);
        assert!(result.errors.is_empty());
    }
}
