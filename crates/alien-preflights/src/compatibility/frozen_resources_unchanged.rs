use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::{ComputeCluster, Resource, ResourceLifecycle, Stack};
use std::collections::{HashMap, HashSet};

/// Validates that frozen resources haven't been added or modified during stack updates.
///
/// Frozen resources are created once during initial deployment and should remain unchanged.
/// This is critical because:
/// 1. Updates only deploy live resources (frozen resources are skipped)
/// 2. Adding frozen resources during update creates inconsistent state
/// 3. Modifying frozen resources risks breaking security/permission models
pub struct FrozenResourcesUnchangedCheck;

/// Setup owns the ComputeCluster identity and network boundary, but its
/// registered runtime controller deliberately owns fleet capacity. Keep this
/// exception structural and narrow: changing groups, profiles, placement, or
/// networking still requires setup.
fn runtime_managed_frozen_change(old: &Resource, new: &Resource) -> bool {
    let (Some(old_cluster), Some(new_cluster)) = (
        old.downcast_ref::<ComputeCluster>(),
        new.downcast_ref::<ComputeCluster>(),
    ) else {
        return false;
    };
    if old_cluster.capacity_groups.len() != new_cluster.capacity_groups.len() {
        return false;
    }

    let mut normalized = old_cluster.clone();
    for (old_group, new_group) in normalized
        .capacity_groups
        .iter_mut()
        .zip(&new_cluster.capacity_groups)
    {
        if old_group.group_id != new_group.group_id {
            return false;
        }
        old_group.min_size = new_group.min_size;
        old_group.max_size = new_group.max_size;
        old_group.scale_policy = new_group.scale_policy.clone();
    }
    normalized == *new_cluster
}

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
                 Frozen resources are setup-owned and can only be added by rerunning setup with the updated stack.",
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
                if old_entry.config != new_entry.config
                    && !runtime_managed_frozen_change(&old_entry.config, &new_entry.config)
                {
                    errors.push(format!(
                        "Frozen resource '{}' was modified. \
                         Frozen resources are setup-owned. Rerun setup with the updated stack.",
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
    use alien_core::{
        CapacityGroup, ComputeCluster, Resource, ResourceEntry, ResourceLifecycle, Stack, Storage,
    };
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
                enabled_when: None,
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
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
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
                enabled_when: None,
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
                enabled_when: None,
            },
        );
        new_resources.insert(
            "storage-2".to_string(),
            ResourceEntry {
                config: Resource::new(storage2),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
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
                enabled_when: None,
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
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
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
                enabled_when: None,
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
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
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
                enabled_when: None,
            },
        );

        let new_resources = IndexMap::new(); // Empty - resource removed

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        // Should succeed - removing frozen resources is allowed (deletion scenario)
        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    fn compute_stack(cluster: ComputeCluster) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    fn compute_cluster(size: u32) -> ComputeCluster {
        ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "workers".to_string(),
                instance_type: Some("m8i.2xlarge".to_string()),
                profile: None,
                min_size: size,
                max_size: size,
                scale_policy: None,
                nested_virtualization: Some(true),
            })
            .build()
    }

    #[tokio::test]
    async fn compute_capacity_is_runtime_manageable() {
        let result = FrozenResourcesUnchangedCheck
            .check(
                &compute_stack(compute_cluster(2)),
                &compute_stack(compute_cluster(3)),
            )
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn compute_boundary_change_remains_frozen() {
        let old = compute_cluster(2);
        let mut changed = compute_cluster(2);
        changed.capacity_groups[0].instance_type = Some("m8i.4xlarge".to_string());
        let result = FrozenResourcesUnchangedCheck
            .check(&compute_stack(old), &compute_stack(changed))
            .await
            .unwrap();
        assert!(!result.success);
    }
}
