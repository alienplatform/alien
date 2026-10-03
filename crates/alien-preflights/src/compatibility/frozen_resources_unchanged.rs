use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::instance_catalog::is_same_architecture_aws_machine;
use alien_core::{
    CapacityGroup, CapacityGroupScalePolicy, ComputeCluster, ComputePoolSelection, Platform,
    Resource, ResourceLifecycle, Sandbox, SandboxCode, Stack,
};
use std::collections::{HashMap, HashSet};

/// Validates that frozen resources haven't been added or modified during stack updates.
///
/// Frozen resources are created once during initial deployment and should remain unchanged.
/// This is critical because:
/// 1. Updates only deploy live resources (frozen resources are skipped)
/// 2. Adding frozen resources during update creates inconsistent state
/// 3. Modifying frozen resources risks breaking security/permission models
///
/// The platform is the installed stack's, because some runtime-owned fields exist on some
/// platforms only.
pub struct FrozenResourcesUnchangedCheck {
    pub platform: Platform,
}

/// Setup owns the ComputeCluster identity and network boundary, but its
/// registered runtime controller deliberately owns the fleet: capacity and, on
/// AWS, the machine type within one CPU architecture. The compute mutation has
/// already checked the new machine against the workloads and derived its
/// profile, so the profile follows the machine. Changing groups, placement, or
/// networking still requires setup.
fn runtime_managed_frozen_change(platform: Platform, old: &Resource, new: &Resource) -> bool {
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
        if let Some((old_machine, new_machine)) = machine_change(old_group, new_group) {
            if runtime_machine_change(platform, old_machine, new_machine)
                && validated_machine_profile(platform, new_group)
            {
                old_group.instance_type = new_group.instance_type.clone();
                old_group.profile = new_group.profile.clone();
            }
        }
    }
    normalized == *new_cluster
}

fn validated_machine_profile(platform: Platform, group: &CapacityGroup) -> bool {
    let scale = group.scale_policy.clone().unwrap_or_else(|| {
        CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
    });
    let selection = match scale {
        CapacityGroupScalePolicy::Fixed { .. } => ComputePoolSelection::Fixed {
            machines: group.min_size,
            machine: group.instance_type.clone(),
            failure_domains: None,
        },
        CapacityGroupScalePolicy::Autoscale { .. } => ComputePoolSelection::Autoscale {
            min: group.min_size,
            max: group.max_size,
            machine: group.instance_type.clone(),
            failure_domains: None,
        },
    };
    let mut materialized = group.clone();
    crate::mutations::compute_cluster::materialize_selected_group(
        &mut materialized,
        platform,
        &selection,
    )
    .is_ok()
        && materialized.profile == group.profile
}

fn machine_change<'a>(
    old: &'a CapacityGroup,
    new: &'a CapacityGroup,
) -> Option<(&'a str, &'a str)> {
    let (Some(old), Some(new)) = (old.instance_type.as_deref(), new.instance_type.as_deref())
    else {
        return None;
    };
    (old != new).then_some((old, new))
}

fn runtime_machine_change(platform: Platform, old: &str, new: &str) -> bool {
    platform == Platform::Aws && is_same_architecture_aws_machine(old, new)
}

/// Explains machine changes that need setup, so the deployment error names them.
fn machine_changes_needing_setup(
    platform: Platform,
    old: &Resource,
    new: &Resource,
) -> Vec<String> {
    let (Some(old_cluster), Some(new_cluster)) = (
        old.downcast_ref::<ComputeCluster>(),
        new.downcast_ref::<ComputeCluster>(),
    ) else {
        return Vec::new();
    };
    new_cluster
        .capacity_groups
        .iter()
        .filter_map(|new_group| {
            let old_group = old_cluster
                .capacity_groups
                .iter()
                .find(|group| group.group_id == new_group.group_id)?;
            let (old_machine, new_machine) = machine_change(old_group, new_group)?;
            (!runtime_machine_change(platform, old_machine, new_machine)).then(|| {
                format!(
                    "capacity group '{}' changes machine from '{old_machine}' to '{new_machine}', but without setup a machine can change only to an AWS machine of the same CPU architecture",
                    new_group.group_id
                )
            })
        })
        .collect()
}

/// On Azure and GCP only the runtime controller reads the image and setup renders no grant from
/// it, so only `code.image` may differ; AWS setup renders the build role from it. A GCP direct
/// setup is still refused at update by alien-infra's `changes_requiring_setup`.
fn runtime_managed_sandbox_image(platform: Platform, old: &Resource, new: &Resource) -> bool {
    if !matches!(platform, Platform::Azure | Platform::Gcp) {
        return false;
    }
    let (Some(old_sandbox), Some(new_sandbox)) =
        (old.downcast_ref::<Sandbox>(), new.downcast_ref::<Sandbox>())
    else {
        return false;
    };
    let (SandboxCode::Image { .. }, SandboxCode::Image { .. }) =
        (&old_sandbox.code, &new_sandbox.code)
    else {
        return false;
    };
    let mut normalized = old_sandbox.clone();
    normalized.code = new_sandbox.code.clone();
    normalized == *new_sandbox
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
                    && !runtime_managed_frozen_change(
                        self.platform,
                        &old_entry.config,
                        &new_entry.config,
                    )
                    && !runtime_managed_sandbox_image(
                        self.platform,
                        &old_entry.config,
                        &new_entry.config,
                    )
                {
                    let details = machine_changes_needing_setup(
                        self.platform,
                        &old_entry.config,
                        &new_entry.config,
                    );
                    let details = if details.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", details.join("; "))
                    };
                    errors.push(format!(
                        "Frozen resource '{}' was modified{}. \
                         Frozen resources are setup-owned. Rerun setup with the updated stack.",
                        id, details
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: new_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
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
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        }
        .check(
            &compute_stack(compute_cluster(2)),
            &compute_stack(compute_cluster(3)),
        )
        .await
        .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    async fn machine_change(platform: Platform, machine: &str) -> CheckResult {
        let old = compute_cluster(2);
        let mut changed = compute_cluster(3);
        let group = &mut changed.capacity_groups[0];
        group.instance_type = Some(machine.to_string());
        group.profile = alien_core::instance_catalog::find_instance_type(platform, machine)
            .map(|spec| spec.to_machine_profile());
        FrozenResourcesUnchangedCheck { platform }
            .check(&compute_stack(old), &compute_stack(changed))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn aws_machine_change_within_one_architecture_is_runtime_manageable() {
        let result = machine_change(Platform::Aws, "m8i.4xlarge").await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn declared_machine_change_rejects_inconsistent_profile_and_nested_virtualization() {
        let old = compute_cluster(2);
        let mut changed = compute_cluster(2);
        changed.capacity_groups[0].instance_type = Some("m7i.4xlarge".into());
        changed.capacity_groups[0].profile =
            alien_core::instance_catalog::find_instance_type(Platform::Aws, "m8i.8xlarge")
                .map(|spec| spec.to_machine_profile());
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
        assert!(
            !check
                .check(&compute_stack(old.clone()), &compute_stack(changed.clone()))
                .await
                .unwrap()
                .success
        );
        changed.capacity_groups[0].profile =
            alien_core::instance_catalog::find_instance_type(Platform::Aws, "m7i.4xlarge")
                .map(|spec| spec.to_machine_profile());
        changed.capacity_groups[0].nested_virtualization = Some(true);
        let mut old_nested = old;
        old_nested.capacity_groups[0].nested_virtualization = Some(true);
        assert!(
            !check
                .check(&compute_stack(old_nested), &compute_stack(changed))
                .await
                .unwrap()
                .success
        );
    }

    #[tokio::test]
    async fn other_machine_changes_need_setup() {
        let result = machine_change(Platform::Aws, "c7g.xlarge").await;
        assert!(!result.success);
        assert!(
            result.errors[0].contains(
                "capacity group 'workers' changes machine from 'm8i.2xlarge' to 'c7g.xlarge'"
            ),
            "{:?}",
            result.errors
        );
        assert!(!machine_change(Platform::Aws, "m8i.unknown").await.success);
        assert!(!machine_change(Platform::Gcp, "m8i.4xlarge").await.success);
    }

    #[tokio::test]
    async fn compute_boundary_change_remains_frozen() {
        let old = compute_cluster(2);
        let mut changed = compute_cluster(2);
        changed.capacity_groups[0].nested_virtualization = Some(false);
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        }
        .check(&compute_stack(old), &compute_stack(changed))
        .await
        .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("Rerun setup"));
    }

    fn sandbox_stack(image: &str, idle_pause_seconds: Option<u32>) -> Stack {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
            })
            .egress(alien_core::SandboxEgress::Deny)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds,
            })
            .build();
        Stack::new("stack".to_string())
            .add(sandbox, ResourceLifecycle::Frozen)
            .build()
    }

    #[tokio::test]
    async fn an_azure_frozen_sandbox_image_is_runtime_manageable() {
        let check = |platform| FrozenResourcesUnchangedCheck { platform };
        let old = sandbox_stack("ubuntu", None);

        let image_only = check(Platform::Azure)
            .check(&old, &sandbox_stack("debian", None))
            .await
            .unwrap();
        assert!(image_only.success, "{:?}", image_only.errors);

        let on_aws = check(Platform::Aws)
            .check(&old, &sandbox_stack("debian", None))
            .await
            .unwrap();
        assert!(!on_aws.success, "AWS Frozen sandboxes stay setup-owned");

        let with_other_field = check(Platform::Azure)
            .check(&old, &sandbox_stack("debian", Some(60)))
            .await
            .unwrap();
        assert!(
            !with_other_field.success,
            "an image change must not carry another field past setup"
        );
    }

    #[tokio::test]
    async fn a_gcp_frozen_sandbox_image_is_runtime_manageable() {
        let check = |platform| FrozenResourcesUnchangedCheck { platform };
        let old = sandbox_stack(
            "us-central1-docker.pkg.dev/proj/agents/sandbox@sha256:aaaa",
            None,
        );
        // Another host entirely: GCP setup grants no repository, so none can be left uncovered.
        let moved = || sandbox_stack("ghcr.io/org/sandbox:v2", None);

        let image_only = check(Platform::Gcp).check(&old, &moved()).await.unwrap();
        assert!(image_only.success, "{:?}", image_only.errors);

        let on_aws = check(Platform::Aws).check(&old, &moved()).await.unwrap();
        assert!(!on_aws.success, "AWS Frozen sandboxes stay setup-owned");

        let with_other_field = check(Platform::Gcp)
            .check(&old, &sandbox_stack("ghcr.io/org/sandbox:v2", Some(60)))
            .await
            .unwrap();
        assert!(
            !with_other_field.success,
            "an image change must not carry another field past setup"
        );
        assert!(with_other_field.errors[0].contains("Rerun setup"));
    }
}
