use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::instance_catalog::{
    find_instance_type, is_same_architecture_aws_machine, max_configurable_ephemeral_storage_bytes,
};
use alien_core::{
    CapacityGroup, ComputeCluster, Platform, Resource, ResourceLifecycle, Sandbox, SandboxCode,
    Stack,
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

/// Setup owns the ComputeCluster identity and network boundary; the runtime controller owns fleet
/// capacity and, on AWS, a group's machine within one catalog architecture, which the stack's
/// images target. Any other group, profile, placement or networking change needs setup.
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
        if platform == Platform::Aws
            && old_group.instance_type != new_group.instance_type
            && same_aws_architecture(
                old_group.instance_type.as_deref(),
                new_group.instance_type.as_deref(),
            )
            && profile_matches_catalog(new_group, installed_disk(old_group))
        {
            old_group.instance_type = new_group.instance_type.clone();
            old_group.profile = new_group.profile.clone();
        }
    }
    normalized == *new_cluster
}

fn same_aws_architecture(old: Option<&str>, new: Option<&str>) -> bool {
    old.zip(new)
        .is_some_and(|(old, new)| is_same_architecture_aws_machine(old, new))
}

/// The installed group's disk, which bounds what an unchanged workload request can produce.
enum InstalledDisk {
    /// A configurable disk already sized to the request: the new machine keeps this size.
    Configurable(u64),
    /// A fixed disk that met the request: the new disk fits between its catalog size and this.
    Fixed(u64),
    /// No catalog machine or profile recorded: only the new machine's catalog disk is known safe.
    Unknown,
}

fn installed_disk(group: &CapacityGroup) -> InstalledDisk {
    let spec = group
        .instance_type
        .as_deref()
        .and_then(|name| find_instance_type(Platform::Aws, name));
    match (spec, group.profile.as_ref()) {
        (Some(spec), Some(profile)) if spec.has_configurable_ephemeral_storage() => {
            InstalledDisk::Configurable(profile.ephemeral_storage_bytes)
        }
        (Some(spec), Some(_)) => {
            InstalledDisk::Fixed(spec.to_machine_profile().ephemeral_storage_bytes)
        }
        _ => InstalledDisk::Unknown,
    }
}

/// The profile must be one the compute mutation could produce for this machine with the
/// workload's disk request unchanged (see `InstalledDisk`), a fixed disk at the catalog's, and
/// nested virtualization only on a machine that supports it. A disk change needs setup.
fn profile_matches_catalog(group: &CapacityGroup, installed: InstalledDisk) -> bool {
    let (Some(spec), Some(profile)) = (
        group
            .instance_type
            .as_deref()
            .and_then(|name| find_instance_type(Platform::Aws, name)),
        group.profile.as_ref(),
    ) else {
        return false;
    };
    let catalog = spec.to_machine_profile();
    let disk = profile.ephemeral_storage_bytes;
    let storage_matches = if spec.has_configurable_ephemeral_storage() {
        let within_limits = disk >= catalog.ephemeral_storage_bytes
            && max_configurable_ephemeral_storage_bytes(Platform::Aws)
                .is_some_and(|max| disk <= max);
        within_limits
            && match installed {
                InstalledDisk::Configurable(installed) => disk == installed,
                InstalledDisk::Fixed(installed) => disk <= installed,
                InstalledDisk::Unknown => disk == catalog.ephemeral_storage_bytes,
            }
    } else {
        profile.ephemeral_storage_bytes == catalog.ephemeral_storage_bytes
    };
    profile
        .architecture
        .is_none_or(|arch| arch == spec.architecture)
        && profile.cpu == catalog.cpu
        && profile.memory_bytes == catalog.memory_bytes
        && profile.gpu == catalog.gpu
        && storage_matches
        && (group.nested_virtualization != Some(true) || spec.is_nested_virt_capable())
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
    use alien_core::MachineProfile;
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
        machine_cluster("m8i.2xlarge", size)
    }

    fn machine_cluster(instance_type: &str, size: u32) -> ComputeCluster {
        ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "workers".to_string(),
                instance_type: Some(instance_type.to_string()),
                profile: None,
                min_size: size,
                max_size: size,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build()
    }

    /// Sets a group's machine and its unadjusted catalog profile.
    fn with_machine(mut cluster: ComputeCluster, instance_type: &str) -> ComputeCluster {
        let group = &mut cluster.capacity_groups[0];
        group.instance_type = Some(instance_type.to_string());
        group.profile =
            find_instance_type(Platform::Aws, instance_type).map(|spec| spec.to_machine_profile());
        cluster
    }

    async fn frozen_check(
        platform: Platform,
        old: ComputeCluster,
        new: ComputeCluster,
    ) -> CheckResult {
        FrozenResourcesUnchangedCheck { platform }
            .check(&compute_stack(old), &compute_stack(new))
            .await
            .unwrap()
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

    #[tokio::test]
    async fn compute_boundary_change_remains_frozen() {
        let old = compute_cluster(2);
        let mut changed = compute_cluster(2);
        changed.capacity_groups[0].nested_virtualization = Some(false);
        let result = frozen_check(Platform::Aws, old, changed).await;
        assert!(!result.success);
        assert!(result.errors[0].contains("Rerun setup"));
    }

    #[tokio::test]
    async fn an_aws_machine_change_within_one_architecture_is_runtime_manageable() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let new = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
        let result = frozen_check(Platform::Aws, old, new).await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn an_aws_machine_change_with_new_bounds_is_runtime_manageable() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let new = with_machine(machine_cluster("t4g.micro", 4), "c7g.medium");
        let result = frozen_check(Platform::Aws, old, new).await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn an_aws_machine_change_across_architectures_needs_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let new = with_machine(machine_cluster("t4g.micro", 2), "m7i.large");
        let result = frozen_check(Platform::Aws, old, new).await;
        assert!(!result.success, "arm64 to x86_64 must rerun setup");
    }

    #[tokio::test]
    async fn an_unknown_machine_needs_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let to_unknown = with_machine(machine_cluster("t4g.micro", 2), "t4g.unknown");
        assert!(
            !frozen_check(Platform::Aws, old.clone(), to_unknown.clone())
                .await
                .success
        );
        assert!(
            !frozen_check(Platform::Aws, to_unknown, old.clone())
                .await
                .success
        );

        let mut to_none = old.clone();
        to_none.capacity_groups[0].instance_type = None;
        assert!(!frozen_check(Platform::Aws, old, to_none).await.success);
    }

    #[tokio::test]
    async fn a_machine_change_outside_aws_needs_setup() {
        for platform in [Platform::Gcp, Platform::Azure] {
            let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
            let new = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
            let result = frozen_check(platform, old, new).await;
            assert!(
                !result.success,
                "{platform} machine changes stay setup-owned"
            );
        }
    }

    #[tokio::test]
    async fn a_profile_change_without_a_machine_change_needs_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let mut new = old.clone();
        new.capacity_groups[0]
            .profile
            .as_mut()
            .expect("catalog profile")
            .memory_bytes *= 2;
        let result = frozen_check(Platform::Aws, old, new).await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn a_machine_change_cannot_carry_a_forged_profile_past_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let forgeries: [fn(&mut MachineProfile); 5] = [
            |profile| profile.ephemeral_storage_bytes = 1,
            |profile| {
                profile.architecture = Some(alien_core::instance_catalog::Architecture::X86_64)
            },
            |profile| profile.memory_bytes *= 64,
            |profile| profile.cpu = "64.0".to_string(),
            |profile| {
                profile.gpu = Some(alien_core::GpuSpec {
                    gpu_type: "nvidia-h100".to_string(),
                    count: 8,
                })
            },
        ];
        for forge in forgeries {
            let mut new = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
            forge(
                new.capacity_groups[0]
                    .profile
                    .as_mut()
                    .expect("catalog profile"),
            );
            let result = frozen_check(Platform::Aws, old.clone(), new).await;
            assert!(
                !result.success,
                "a profile the catalog does not back needs setup"
            );
        }

        let mut without_profile = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
        without_profile.capacity_groups[0].profile = None;
        assert!(
            !frozen_check(Platform::Aws, old, without_profile)
                .await
                .success
        );
    }

    #[tokio::test]
    async fn a_fixed_disk_machine_change_keeps_the_catalog_disk() {
        let old = with_machine(machine_cluster("i4i.xlarge", 2), "i4i.xlarge");
        let new = with_machine(machine_cluster("i4i.xlarge", 2), "i4i.2xlarge");
        let result = frozen_check(Platform::Aws, old.clone(), new.clone()).await;
        assert!(result.success, "{:?}", result.errors);

        let mut grown = new;
        grown.capacity_groups[0]
            .profile
            .as_mut()
            .expect("catalog profile")
            .ephemeral_storage_bytes *= 2;
        assert!(!frozen_check(Platform::Aws, old, grown).await.success);
    }

    #[tokio::test]
    async fn a_nested_virtualization_group_moves_only_to_a_capable_machine() {
        let nested = |machine| {
            let mut cluster = with_machine(machine_cluster(machine, 2), machine);
            cluster.capacity_groups[0].nested_virtualization = Some(true);
            cluster
        };
        let result = frozen_check(Platform::Aws, nested("m8i.large"), nested("c8i.large")).await;
        assert!(result.success, "{:?}", result.errors);
        assert!(
            !frozen_check(Platform::Aws, nested("m8i.large"), nested("m7i.xlarge"))
                .await
                .success
        );
        let plain = |machine| with_machine(machine_cluster(machine, 2), machine);
        let result = frozen_check(Platform::Aws, plain("m8i.large"), plain("m7i.xlarge")).await;
        assert!(result.success, "{:?}", result.errors);
    }

    /// A cluster with no workload skips the compute mutation, so its profile is whatever the
    /// stack declares; a machine change must not carry a disk change past setup.
    #[tokio::test]
    async fn a_machine_change_cannot_grow_the_disk_past_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");
        let mut new = with_machine(machine_cluster("t4g.micro", 2), "t4g.small");
        new.capacity_groups[0]
            .profile
            .as_mut()
            .expect("catalog profile")
            .ephemeral_storage_bytes = 1024 * 1024 * 1024 * 1024;
        assert!(!frozen_check(Platform::Aws, old, new).await.success);
    }

    #[tokio::test]
    async fn a_machine_change_cannot_shrink_the_disk_past_setup() {
        let mut old = machine_cluster("t4g.micro", 2);
        old.capacity_groups[0].profile = Some(
            find_instance_type(Platform::Aws, "t4g.micro")
                .expect("catalog machine")
                .to_machine_profile_for_storage(100 * 1024 * 1024 * 1024),
        );
        let new = with_machine(machine_cluster("t4g.micro", 2), "t4g.small");
        assert!(!frozen_check(Platform::Aws, old, new).await.success);
    }

    #[tokio::test]
    async fn a_fixed_disk_machine_moves_to_a_disk_its_request_fits() {
        let old = with_machine(machine_cluster("i4i.xlarge", 2), "i4i.xlarge");
        let with_disk = |bytes: u64| {
            let mut cluster = machine_cluster("i4i.xlarge", 2);
            cluster.capacity_groups[0].instance_type = Some("m7i.large".to_string());
            cluster.capacity_groups[0].profile = Some(
                find_instance_type(Platform::Aws, "m7i.large")
                    .expect("catalog machine")
                    .to_machine_profile_for_storage(bytes),
            );
            cluster
        };
        const GIB: u64 = 1024 * 1024 * 1024;
        for requested in [0, 40 * GIB] {
            let result = frozen_check(Platform::Aws, old.clone(), with_disk(requested)).await;
            assert!(result.success, "{requested}: {:?}", result.errors);
        }
        assert!(
            !frozen_check(Platform::Aws, old, with_disk(2048 * GIB))
                .await
                .success,
            "a disk beyond the installed fixed disk is a disk change"
        );
    }

    #[tokio::test]
    async fn a_machine_change_keeps_the_storage_the_workload_requested() {
        let mut old = machine_cluster("t4g.micro", 2);
        old.capacity_groups[0].profile = Some(
            find_instance_type(Platform::Aws, "t4g.micro")
                .expect("catalog machine")
                .to_machine_profile_for_storage(200 * 1024 * 1024 * 1024),
        );
        let mut new = machine_cluster("c7g.medium", 2);
        new.capacity_groups[0].profile = Some(
            find_instance_type(Platform::Aws, "c7g.medium")
                .expect("catalog machine")
                .to_machine_profile_for_storage(200 * 1024 * 1024 * 1024),
        );
        let mut without_architecture = new.clone();
        if let Some(profile) = without_architecture.capacity_groups[0].profile.as_mut() {
            profile.architecture = None;
        }

        let result = frozen_check(Platform::Aws, old.clone(), new).await;
        assert!(result.success, "{:?}", result.errors);
        let result = frozen_check(Platform::Aws, old, without_architecture).await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn a_machine_change_cannot_carry_a_group_change_past_setup() {
        let old = with_machine(machine_cluster("t4g.micro", 2), "t4g.micro");

        let mut renamed = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
        renamed.capacity_groups[0].group_id = "others".to_string();
        assert!(
            !frozen_check(Platform::Aws, old.clone(), renamed)
                .await
                .success
        );

        let mut added = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
        let mut extra = added.capacity_groups[0].clone();
        extra.group_id = "others".to_string();
        added.capacity_groups.push(extra);
        assert!(
            !frozen_check(Platform::Aws, old.clone(), added)
                .await
                .success
        );

        let mut nested = with_machine(machine_cluster("t4g.micro", 2), "c7g.medium");
        nested.capacity_groups[0].nested_virtualization = Some(false);
        assert!(!frozen_check(Platform::Aws, old, nested).await.success);
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
