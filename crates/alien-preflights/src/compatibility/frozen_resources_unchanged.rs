use crate::error::{ErrorData, Result};
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::instance_catalog::{find_instance_type, is_same_architecture_machine};
use alien_core::{
    AzureStorageAccount, CapacityGroup, CapacityGroupScalePolicy, ComputeCluster,
    ComputePoolSelection, Platform, Resource, ResourceLifecycle, Sandbox, SandboxCode,
    ServiceAccount, Stack, permissions::PermissionSet,
};
use alien_error::Context;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// Validates that frozen resources haven't been added, modified or removed during stack updates.
///
/// Frozen resources are setup-owned: only setup creates, changes and deletes them. This is
/// critical because:
/// 1. Updates only deploy live resources (frozen resources are skipped)
/// 2. Adding or removing frozen resources during an update leaves them out of sync with setup
/// 3. Modifying frozen resources, including removing a service account's grants, changes the
///    security model, which only setup may do
///
/// The platform is the installed stack's, because some runtime-owned fields exist on some
/// platforms only.
pub struct FrozenResourcesUnchangedCheck {
    pub platform: Platform,
}

/// Setup owns the ComputeCluster identity and network boundary, but its
/// registered runtime controller deliberately owns the fleet: capacity and, on
/// AWS, GCP and Azure, the machine type within one CPU architecture. The compute mutation has
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
        && (group.profile.is_none() || materialized.profile == group.profile)
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
    is_same_architecture_machine(platform, old, new)
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
                    "capacity group '{}' changes machine from '{old_machine}' to '{new_machine}', but without setup a machine can change only within the same cloud and with the same CPU architecture",
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

/// Keeps only `platform`'s section of each permission set.
fn permission_sets_on_platform(platform: Platform, sets: &mut [PermissionSet]) {
    for set in sets {
        let platforms = &mut set.platforms;
        match platform {
            Platform::Aws => {
                platforms.gcp = None;
                platforms.azure = None;
            }
            Platform::Gcp => {
                platforms.aws = None;
                platforms.azure = None;
            }
            Platform::Azure => {
                platforms.aws = None;
                platforms.gcp = None;
            }
            _ => {}
        }
    }
}

/// A service account as its identity on `platform` sees it.
///
/// Its permission sets keep every cloud's section, but setup renders only the installed cloud's,
/// so a change to another cloud's section (a permission set edited for GCP only) leaves an AWS
/// role as it was. Other platforms keep every section.
fn on_platform(platform: Platform, resource: &Resource) -> Cow<'_, Resource> {
    let Some(account) = resource.downcast_ref::<ServiceAccount>() else {
        return Cow::Borrowed(resource);
    };
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return Cow::Borrowed(resource);
    }
    let mut account = account.clone();
    permission_sets_on_platform(platform, &mut account.stack_permission_sets);
    for sets in account.resource_permission_sets.values_mut() {
        permission_sets_on_platform(platform, sets);
    }
    Cow::Owned(Resource::new(account))
}

/// Older prepared stacks stored resource grants only in the explicit permission profile.
/// Capturing those same grants is metadata migration, not a setup-owned IAM change.
fn unchanged_legacy_service_account_grants(
    platform: Platform,
    old_stack: &Stack,
    new_stack: &Stack,
    old: &Resource,
    new: &Resource,
) -> Result<bool> {
    // AWS treats these captured grants as comparison metadata. Other providers
    // consume them in identity bindings and still require setup for migration.
    if platform != Platform::Aws {
        return Ok(false);
    }
    let (Some(old_account), Some(new_account)) = (
        old.downcast_ref::<ServiceAccount>(),
        new.downcast_ref::<ServiceAccount>(),
    ) else {
        return Ok(false);
    };
    if !old_account.resource_permission_sets.is_empty()
        || new_account.resource_permission_sets.is_empty()
    {
        return Ok(false);
    }
    let Some(profile_name) = old_account.id.strip_suffix("-sa") else {
        return Ok(false);
    };
    let Some(profile) = old_stack.permissions.profiles.get(profile_name) else {
        return Ok(false);
    };
    if new_stack.permissions.profiles.get(profile_name) != Some(profile) {
        return Ok(false);
    }
    let captured = ServiceAccount::from_permission_profile(old_account.id.clone(), profile, |id| {
        alien_permissions::get_permission_set(id).cloned()
    })
    .context(ErrorData::StackCompatibilityCheckFailed {
        check_name: "Frozen resources shouldn't be added or modified during updates".to_string(),
        message: "Failed to resolve the installed service account's explicit grants".to_string(),
        old_resource_id: Some(old_account.id.clone()),
        new_resource_id: Some(new_account.id.clone()),
    })?;
    let mut normalized = old_account.clone();
    normalized.resource_permission_sets = captured.resource_permission_sets;
    Ok(*on_platform(platform, &Resource::new(normalized)) == *on_platform(platform, new))
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

        if self.platform == Platform::Azure
            && old_frozen
                .values()
                .any(|entry| entry.config.downcast_ref::<AzureStorageAccount>().is_some())
            && AzureStorageAccount::setup_settings_digest(old_stack)
                != AzureStorageAccount::setup_settings_digest(new_stack)
        {
            errors.push("Azure shared storage account settings changed through container declarations. Rerun setup to apply public capability, versioning, CORS or lifecycle changes before updating Live containers.".to_string());
        }

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
                // Queue config contains only its ID; setup also derives visibility from
                // Live consumers. A Worker-only change must not bypass setup approval.
                if self.platform == Platform::Aws
                    && old_entry
                        .config
                        .downcast_ref::<alien_core::Queue>()
                        .is_some()
                    && new_stack.resources.values().any(|entry| {
                        entry
                            .config
                            .downcast_ref::<alien_core::Worker>()
                            .is_some_and(|worker| worker.queue_batch_config(id).is_some())
                    })
                    && new_stack.queue_visibility_timeout_seconds(id)
                        > old_stack.queue_visibility_timeout_seconds(id)
                {
                    errors.push(format!(
                        "Frozen queue '{}' requires visibility of {} seconds (previously {}). Rerun setup with the updated stack before updating its consumers.",
                        id, new_stack.queue_visibility_timeout_seconds(id),
                        old_stack.queue_visibility_timeout_seconds(id),
                    ));
                }
                let old_config = on_platform(self.platform, &old_entry.config);
                let new_config = on_platform(self.platform, &new_entry.config);
                if old_config != new_config
                    && !runtime_managed_frozen_change(self.platform, &old_config, &new_config)
                    && !runtime_managed_sandbox_image(self.platform, &old_config, &new_config)
                    && !unchanged_legacy_service_account_grants(
                        self.platform,
                        old_stack,
                        new_stack,
                        &old_config,
                        &new_config,
                    )?
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
            } else {
                errors.push(format!(
                    "Frozen resource '{}' was removed. Frozen resources are setup-owned: rerun \
                     setup with the updated stack to delete it and its grants.",
                    id
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
    use alien_core::permissions::{PermissionProfile, PermissionsConfig, PlatformPermissions};
    use alien_core::{
        CapacityGroup, ComputeCluster, Resource, ResourceEntry, ResourceLifecycle, Stack, Storage,
    };
    use indexmap::IndexMap;

    fn account_stack(profile: PermissionProfile, captured: bool) -> Stack {
        let mut account =
            ServiceAccount::from_permission_profile("reader-sa".to_string(), &profile, |id| {
                alien_permissions::get_permission_set(id).cloned()
            })
            .unwrap();
        if !captured {
            account.resource_permission_sets.clear();
        }
        Stack::new("stack".to_string())
            .permissions(PermissionsConfig::new().with_profile("reader", profile))
            .add(account, ResourceLifecycle::Frozen)
            .build()
    }

    /// Edits every permission set the stack's service account embeds.
    fn edit_account_sets(stack: &mut Stack, edit: impl Fn(&mut PlatformPermissions)) {
        let account = stack
            .resources
            .get_mut("reader-sa")
            .unwrap()
            .config
            .downcast_mut::<ServiceAccount>()
            .unwrap();
        for set in account
            .stack_permission_sets
            .iter_mut()
            .chain(account.resource_permission_sets.values_mut().flatten())
        {
            edit(&mut set.platforms);
        }
    }

    /// A permission set edited for one cloud only, as when a release changes only the GCP
    /// section of `postgres/data-access`, changes the installed identities on that cloud only.
    #[tokio::test]
    async fn a_permission_set_change_for_one_cloud_needs_setup_only_on_that_cloud() {
        let profile = PermissionProfile::new()
            .global(["vault/data-read"])
            .resource("database", ["postgres/data-access"]);
        let installed = account_stack(profile.clone(), true);
        let mut gcp_edited = installed.clone();
        edit_account_sets(&mut gcp_edited, |platforms| {
            assert!(
                platforms.gcp.is_some(),
                "the fixture sets have a GCP section"
            );
            platforms.gcp = Some(Vec::new());
        });

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = FrozenResourcesUnchangedCheck { platform }
                .check(&installed, &gcp_edited)
                .await
                .unwrap();
            assert_eq!(
                result.success,
                platform != Platform::Gcp,
                "{platform:?}: {:?}",
                result.errors
            );
        }

        let mut aws_edited = installed.clone();
        edit_account_sets(&mut aws_edited, |platforms| {
            platforms.aws = Some(Vec::new())
        });
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        }
        .check(&installed, &aws_edited)
        .await
        .unwrap();
        assert!(!result.success);
        assert!(
            result.errors[0].contains("'reader-sa' was modified"),
            "{:?}",
            result.errors
        );
    }

    /// Dropping an account's last resource-scoped grant changes its installed role, so it needs
    /// setup, as does dropping only the capture of grants its profile still holds.
    #[tokio::test]
    async fn losing_the_last_resource_grant_needs_setup() {
        let with_grant = PermissionProfile::new().resource("objects", ["storage/data-read"]);
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
        let removed = check
            .check(
                &account_stack(with_grant.clone(), true),
                &account_stack(PermissionProfile::new(), true),
            )
            .await
            .unwrap();
        assert!(!removed.success);
        assert!(
            removed.errors[0].contains("'reader-sa' was modified"),
            "{:?}",
            removed.errors
        );
        let uncaptured = check
            .check(
                &account_stack(with_grant.clone(), true),
                &account_stack(with_grant, false),
            )
            .await
            .unwrap();
        assert!(!uncaptured.success);
    }

    #[tokio::test]
    async fn legacy_resource_grants_can_be_captured_without_setup() {
        let profile = PermissionProfile::new()
            .resource("objects", ["storage/data-read", "storage/data-write"])
            .resource("database", ["postgres/data-access"]);
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = FrozenResourcesUnchangedCheck { platform }
                .check(
                    &account_stack(profile.clone(), false),
                    &account_stack(profile.clone(), true),
                )
                .await
                .unwrap();
            assert_eq!(
                result.success,
                platform == Platform::Aws,
                "{:?}",
                result.errors
            );
        }
    }

    #[tokio::test]
    async fn legacy_grant_capture_rejects_permission_changes_and_missing_profiles() {
        let profile = PermissionProfile::new().resource("objects", ["storage/data-read"]);
        let old = account_stack(profile.clone(), false);
        let target = account_stack(profile.clone(), true);
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
        for changed in [
            PermissionProfile::new()
                .resource("objects", ["storage/data-read", "storage/data-write"]),
            PermissionProfile::new().resource("other", ["storage/data-read"]),
            PermissionProfile::new().resource("objects", ["storage/data-write"]),
            profile.clone().resource("*", ["storage/data-read"]),
        ] {
            assert!(
                !check
                    .check(&old, &account_stack(changed, true))
                    .await
                    .unwrap()
                    .success
            );
        }
        let mut missing_profile = old.clone();
        missing_profile.permissions.profiles.clear();
        assert!(
            !check
                .check(&missing_profile, &target)
                .await
                .unwrap()
                .success
        );
        let mut changed_capture = target.clone();
        changed_capture
            .resources
            .get_mut("reader-sa")
            .unwrap()
            .config
            .downcast_mut::<ServiceAccount>()
            .unwrap()
            .resource_permission_sets
            .insert(
                "other".to_string(),
                target.resources["reader-sa"]
                    .config
                    .downcast_ref::<ServiceAccount>()
                    .unwrap()
                    .resource_permission_sets["objects"]
                    .clone(),
            );
        assert!(!check.check(&old, &changed_capture).await.unwrap().success);
        assert!(
            !check
                .check(&target, &changed_capture)
                .await
                .unwrap()
                .success
        );
        assert!(!check.check(&target, &old).await.unwrap().success);
    }

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
                display_name: None,
                description: None,
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
                display_name: None,
                description: None,
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
                display_name: None,
                description: None,
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
                display_name: None,
                description: None,
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
                display_name: None,
                description: None,
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
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
    async fn test_removed_frozen_resource_needs_setup() {
        let storage = Storage::new("test-storage".to_string()).build();

        let mut old_resources = IndexMap::new();
        old_resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let new_resources = IndexMap::new(); // Empty - resource removed

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
        // Only setup deletes a setup-owned resource and its grants.
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].contains("'test-storage' was removed"),
            "{:?}",
            result.errors
        );
    }

    fn compute_stack(cluster: ComputeCluster) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
    async fn resolved_signing_scope_changes_require_setup_with_unchanged_names() {
        let current = alien_permissions::get_permission_set("storage/data-read")
            .unwrap()
            .clone();
        let mut previous = current.clone();
        for entry in previous.platforms.gcp.as_mut().unwrap() {
            if entry.grant.permissions.as_ref().is_some_and(|permissions| {
                permissions
                    .iter()
                    .any(|permission| permission == "iam.serviceAccounts.signBlob")
            }) {
                entry.binding.resource.as_mut().unwrap().scope =
                    "projects/${projectName}".to_string();
            }
        }
        let profile =
            alien_core::PermissionProfile::new().resource("objects", ["storage/data-read"]);
        let old_account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| Some(previous.clone()),
        )
        .unwrap();
        let new_account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| Some(current.clone()),
        )
        .unwrap();
        let stack = |account| {
            Stack::new("example".to_string())
                .add(account, ResourceLifecycle::Frozen)
                .build()
        };
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Gcp,
        }
        .check(&stack(old_account), &stack(new_account))
        .await
        .unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
        let mut old_node = compute_cluster(2);
        old_node.node_permissions = Some(alien_core::PermissionProfile::new().resource(
            "objects",
            [alien_core::PermissionSetReference::Inline(previous)],
        ));
        let mut new_node = old_node.clone();
        new_node.node_permissions = Some(alien_core::PermissionProfile::new().resource(
            "objects",
            [alien_core::PermissionSetReference::Inline(current)],
        ));
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Gcp,
        }
        .check(&compute_stack(old_node), &compute_stack(new_node))
        .await
        .unwrap();
        assert!(!result.success);
        assert!(!result.errors.is_empty());
    }

    /// Setup owns a frozen service account's role: losing permission sets needs setup just as
    /// gaining them does.
    #[tokio::test]
    async fn a_service_account_change_in_either_direction_needs_setup() {
        let account = |sets: &[&str]| {
            let profile = alien_core::PermissionProfile::new().global(sets.iter().copied());
            alien_core::ServiceAccount::from_permission_profile(
                "worker-sa".to_string(),
                &profile,
                |name| alien_permissions::get_permission_set(name).cloned(),
            )
            .expect("service account")
        };
        let stack = |sets: &[&str]| {
            Stack::new("s".to_string())
                .add(account(sets), ResourceLifecycle::Frozen)
                .build()
        };
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };

        let narrowed = check
            .check(
                &stack(&["storage/data-read", "storage/data-write"]),
                &stack(&["storage/data-read"]),
            )
            .await
            .expect("check should run");
        assert!(!narrowed.success);

        let widened = check
            .check(
                &stack(&["storage/data-read"]),
                &stack(&["storage/data-read", "storage/data-write"]),
            )
            .await
            .expect("check should run");
        assert!(!widened.success);
    }

    #[tokio::test]
    async fn node_grant_content_changes_require_setup() {
        let mut old = compute_cluster(2);
        old.node_permissions =
            Some(alien_core::PermissionProfile::new().resource("objects", ["storage/data-read"]));
        let mut changed = old.clone();
        changed.node_permissions =
            Some(alien_core::PermissionProfile::new().resource("objects", ["storage/data-write"]));
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = FrozenResourcesUnchangedCheck { platform }
                .check(&compute_stack(old.clone()), &compute_stack(changed.clone()))
                .await
                .unwrap();
            assert!(!result.success);
            assert!(!result.errors.is_empty());
        }
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
    async fn managed_cloud_machine_changes_preserve_the_setup_boundary() {
        for (platform, old_machine, new_machine) in [
            (Platform::Aws, "m7i.large", "m7i.xlarge"),
            (Platform::Gcp, "e2-medium", "n2-standard-4"),
            (Platform::Azure, "Standard_D2s_v5", "Standard_D4s_v5"),
        ] {
            let mut old = compute_cluster(1);
            old.capacity_groups[0].nested_virtualization = None;
            old.capacity_groups[0].instance_type = Some(old_machine.into());
            old.capacity_groups[0].profile = Some(
                find_instance_type(platform, old_machine)
                    .expect("catalog machine")
                    .to_machine_profile(),
            );
            let mut new = old.clone();
            new.capacity_groups[0].instance_type = Some(new_machine.into());
            new.capacity_groups[0].profile = Some(
                find_instance_type(platform, new_machine)
                    .expect("catalog machine")
                    .to_machine_profile(),
            );
            let check = FrozenResourcesUnchangedCheck { platform };
            let result = check
                .check(&compute_stack(old.clone()), &compute_stack(new.clone()))
                .await
                .unwrap();
            assert!(result.success, "{platform}: {:?}", result.errors);
            new.capacity_groups[0].nested_virtualization = Some(true);
            assert!(
                !check
                    .check(&compute_stack(old), &compute_stack(new))
                    .await
                    .unwrap()
                    .success
            );
        }
    }

    #[tokio::test]
    async fn idle_declared_machine_change_validates_without_a_recorded_profile() {
        let old = compute_cluster(2);
        let mut changed = old.clone();
        changed.capacity_groups[0].instance_type = Some("m8i.4xlarge".into());
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        };
        assert!(
            check
                .check(&compute_stack(old.clone()), &compute_stack(changed.clone()))
                .await
                .unwrap()
                .success
        );
        changed.capacity_groups[0].instance_type = Some("m7i.4xlarge".into());
        assert!(
            !check
                .check(&compute_stack(old), &compute_stack(changed))
                .await
                .unwrap()
                .success
        );
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
            .egress(alien_core::SandboxEgress::None)
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

#[cfg(test)]
mod azure_storage_settings_tests {
    use super::*;
    #[cfg(feature = "runtime-checks")]
    use alien_core::SetupUpdateAuthorization;
    use alien_core::{LifecycleRule, Storage};
    #[cfg(feature = "runtime-checks")]
    use alien_permissions::generators::azure_setup_owned::azure_setup_update_digest;

    fn stack(storage: Storage) -> Stack {
        Stack::new("shared".to_string())
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(storage, ResourceLifecycle::Live)
            .build()
    }

    #[tokio::test]
    async fn live_shared_settings_changes_require_setup_but_unchanged_settings_do_not() {
        let baseline = stack(Storage::new("files".to_string()).build());
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Azure,
        };
        assert!(check.check(&baseline, &baseline).await.unwrap().success);
        for config in [
            Storage::new("files".to_string()).public_read(true).build(),
            Storage::new("files".to_string()).versioning(true).build(),
            Storage::new("files".to_string())
                .cors_allowed_origins(vec!["https://example.test".to_string()])
                .build(),
            Storage::new("files".to_string())
                .lifecycle_rules(vec![LifecycleRule {
                    days: 1,
                    prefix: None,
                }])
                .build(),
        ] {
            let target = stack(config);
            assert_ne!(baseline.setup_update_digest(), target.setup_update_digest());
            let result = check.check(&baseline, &target).await.unwrap();
            assert!(!result.success);
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].contains("Rerun setup"));
            assert!(!check.check(&target, &baseline).await.unwrap().success);
            assert!(check.check(&target, &target).await.unwrap().success);
            assert!(
                FrozenResourcesUnchangedCheck {
                    platform: Platform::Gcp
                }
                .check(&baseline, &target)
                .await
                .unwrap()
                .success
            );
        }
    }

    #[test]
    #[cfg(feature = "runtime-checks")]
    fn setup_authority_cannot_be_reused_after_live_account_settings_change() {
        let baseline = stack(Storage::new("files".to_string()).build());
        let target = stack(Storage::new("files".to_string()).versioning(true).build());
        let authorization = SetupUpdateAuthorization {
            nonce: "revision".to_string(),
            baseline_frozen_digest: azure_setup_update_digest(&baseline).unwrap(),
            target_frozen_digest: azure_setup_update_digest(&target).unwrap(),
            release_id: "release".to_string(),
            setup_target: "azure".to_string(),
            setup_fingerprint: "artifact".to_string(),
            setup_fingerprint_version: 1,
        };
        let saved = serde_json::to_vec(&authorization).unwrap();
        let reloaded = serde_json::from_slice(&saved).unwrap();
        assert!(
            crate::compatibility::azure_setup::authorization_matches(&baseline, &target, &reloaded)
                .unwrap()
        );
        let changed = stack(
            Storage::new("files".to_string())
                .versioning(true)
                .public_read(true)
                .build(),
        );
        assert!(
            !crate::compatibility::azure_setup::authorization_matches(
                &baseline, &changed, &reloaded
            )
            .unwrap()
        );
        let mut gated = target.clone();
        gated.resources.get_mut("files").unwrap().enabled_when = Some("includeFiles".to_string());
        assert!(
            !crate::compatibility::azure_setup::authorization_matches(&baseline, &gated, &reloaded)
                .unwrap()
        );
        assert!(AzureStorageAccount::setup_settings_digest(&baseline).is_none());
        let mut reordered = target.clone();
        reordered.resources.reverse();
        assert_eq!(
            target.setup_update_digest(),
            reordered.setup_update_digest()
        );
    }

    #[tokio::test]
    async fn redundant_live_capabilities_do_not_require_setup() {
        let storage = Storage::new("files".to_string())
            .public_read(true)
            .versioning(true)
            .cors_allowed_origins(vec!["https://example.test".to_string()])
            .build();
        let mut old = stack(storage.clone());
        let mut sibling = storage;
        sibling.id = "sibling".to_string();
        let mut entry = old.resources["files"].clone();
        entry.config = Resource::new(sibling.clone());
        old.resources.insert("sibling".to_string(), entry);
        let mut new = old.clone();
        sibling.public_read = false;
        sibling.versioning = false;
        sibling.cors_allowed_origins.clear();
        new.resources.get_mut("sibling").unwrap().config = Resource::new(sibling);
        assert!(
            FrozenResourcesUnchangedCheck {
                platform: Platform::Azure
            }
            .check(&old, &new)
            .await
            .unwrap()
            .success
        );
    }
}
