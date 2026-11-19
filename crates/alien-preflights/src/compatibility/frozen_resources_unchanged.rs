use crate::error::{ErrorData, Result};
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::instance_catalog::{find_instance_type, is_same_architecture_machine};
use alien_core::{
    permissions::PermissionSet, AzureStorageAccount, CapacityGroup, CapacityGroupScalePolicy,
    ComputeCluster, ComputePoolSelection, DeploymentConfig, InputAnswers, Platform, Resource,
    ResourceEntry, ResourceLifecycle, Sandbox, SandboxCode, ServiceAccount, Stack,
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
/// registered runtime controller deliberately owns the fleet: capacity, disk
/// sizes and, on AWS, GCP and Azure, the machine type within one CPU
/// architecture. The compute mutation has already checked the new machine
/// against the workloads and derived its profile, so the profile follows the
/// machine. Disk sizes are applied by replacing machines on AWS, GCP and Azure
/// and checked when a host joins on Machines. Changing groups, placement, or
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
        // Like the scale policy, the data disk sizes offered are a bound on a
        // runtime choice, not setup-owned structure.
        old_group.data_disk_gib_range = new_group.data_disk_gib_range.clone();
        if let Some((old_machine, new_machine)) = machine_change(old_group, new_group) {
            if runtime_machine_change(platform, old_machine, new_machine)
                && validated_machine_profile(platform, new_group)
            {
                old_group.instance_type = new_group.instance_type.clone();
                old_group.profile = new_group.profile.clone();
            }
        }
        if recorded_profile_is_stale(platform, old_group, new_group) {
            old_group.profile = new_group.profile.clone();
        }
        if runtime_disk_change(platform, old_group, new_group) {
            old_group.profile = new_group.profile.clone();
        }
    }
    normalized == *new_cluster
}

/// Whether the old group recorded a profile that does not belong to its own machine, and
/// the new group carries the machine's profile. First setup of an imported Azure install
/// used to record the machine it chose with the profile of the registration default, so
/// those installs hold a profile their machine does not have. The machine is unchanged;
/// only the profile is brought in line with it. Azure only: elsewhere no install holds such
/// a record, and the excuse would quietly adopt a later correction of the catalog.
fn recorded_profile_is_stale(platform: Platform, old: &CapacityGroup, new: &CapacityGroup) -> bool {
    platform == Platform::Azure
        && old.instance_type.is_some()
        && old.instance_type == new.instance_type
        && old.profile.is_some()
        && old.profile != new.profile
        && profile_is_the_machines(platform, new)
}

/// Whether the group's profile is what the planner derives for its machine: the catalog
/// profile, with the disk the workloads asked for (the larger of the catalog's and the
/// request, which the group's own ephemeral storage records) and the group's data disk.
fn profile_is_the_machines(platform: Platform, group: &CapacityGroup) -> bool {
    let (Some(machine), Some(profile)) = (group.instance_type.as_deref(), group.profile.as_ref())
    else {
        return false;
    };
    let Some(spec) = alien_core::instance_catalog::find_instance_type(platform, machine) else {
        return false;
    };
    let mut derived = spec.to_machine_profile_for_storage(profile.ephemeral_storage_bytes);
    derived.data_disk = profile.data_disk.clone();
    derived == *profile
}

/// Whether the profiles differ only in disk sizes that the platform applies
/// without setup.
fn runtime_disk_change(platform: Platform, old: &CapacityGroup, new: &CapacityGroup) -> bool {
    let (Some(old_profile), Some(new_profile)) = (&old.profile, &new.profile) else {
        return false;
    };
    let mut resized = old_profile.clone();
    resized.ephemeral_storage_bytes = new_profile.ephemeral_storage_bytes;
    resized.data_disk = new_profile.data_disk.clone();
    if resized != *new_profile || old.instance_type != new.instance_type {
        return false;
    }
    match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => validated_machine_profile(platform, new),
        Platform::Machines => true,
        Platform::Kubernetes | Platform::Local | Platform::Test => false,
    }
}

fn validated_machine_profile(platform: Platform, group: &CapacityGroup) -> bool {
    let scale = group.scale_policy.clone().unwrap_or_else(|| {
        CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
    });
    // The group already carries the deployment's data disk choice; keep it so
    // only the machine's hardware is compared.
    let data_disk_gib = group.data_disk_gib_range.as_ref().map(|_| {
        group
            .data_disk_size_gib()
            .map_or(0, |size_gib| u32::try_from(size_gib).unwrap_or(u32::MAX))
    });
    let selection = match scale {
        CapacityGroupScalePolicy::Fixed { .. } => ComputePoolSelection::Fixed {
            machines: group.min_size,
            machine: group.instance_type.clone(),
            failure_domains: None,
            data_disk_gib,
        },
        CapacityGroupScalePolicy::Autoscale { .. } => ComputePoolSelection::Autoscale {
            min: group.min_size,
            max: group.max_size,
            machine: group.instance_type.clone(),
            failure_domains: None,
            data_disk_gib,
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

/// A Live Kubernetes sandbox's image, limits and lifecycle are applied by the Operator at runtime,
/// but its egress mode is the NetworkPolicy the chart installed. Changing the mode, or adding a
/// sandbox, without a chart upgrade would run it under the old policy (or none), so it waits
/// for setup.
fn kubernetes_live_sandbox_chart_changes(old_stack: &Stack, new_stack: &Stack) -> Vec<String> {
    new_stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Live)
        .filter_map(|(id, entry)| {
            let new = entry.config.downcast_ref::<Sandbox>()?;
            let Some(old_entry) = old_stack.resources.get(id) else {
                // A new sandbox has no NetworkPolicy yet: the chart installs it, so on a
                // cluster that allows traffic by default it would run unrestricted.
                return Some(format!(
                    "Live sandbox '{id}' is new, and its NetworkPolicy is installed by the \
                     chart. Rerun setup with the updated stack (helm upgrade)."
                ));
            };
            if old_entry.lifecycle != ResourceLifecycle::Live {
                return None;
            }
            let old = old_entry.config.downcast_ref::<Sandbox>()?;
            if old.egress != new.egress {
                return Some(format!(
                    "Live sandbox '{id}' changes its egress mode, which is the NetworkPolicy \
                     the chart installs. Rerun setup with the updated stack (helm upgrade)."
                ));
            }
            // The chart's NetworkPolicy admits only the workloads that link the sandbox. A
            // workload added to (or dropped from) that set is refused (or kept) by the old
            // policy until the chart is upgraded.
            (linkers(old_stack, id) != linkers(new_stack, id)).then(|| {
                format!(
                    "The workloads that link Live sandbox '{id}' change, and the chart's \
                     NetworkPolicy lists them. Rerun setup with the updated stack (helm upgrade)."
                )
            })
        })
        .collect()
}

/// Sorted `type/id` of every resource that links the sandbox `id`.
fn linkers(stack: &Stack, id: &str) -> Vec<String> {
    let mut linkers: Vec<String> = stack
        .linkers(&alien_core::ResourceRef::new(Sandbox::RESOURCE_TYPE, id))
        .iter()
        .map(|linker| format!("{}/{}", linker.resource_type, linker.id))
        .collect();
    linkers.sort();
    linkers
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

/// Whether a Frozen resource that is new in this release is not an addition because its gate is
/// off: a gate declared with default false that resolves false. The identity of a profile
/// (`<profile>-sa`) is excused when it has users and every one of them carries the same off
/// gate. An answer that turns true, a gate that cannot be resolved, another declared default, a
/// profile with an ungated user, and a resource the installed stack already has are not excused.
fn frozen_addition_excused_while_gated_off(
    new_stack: &Stack,
    config: Option<&DeploymentConfig>,
    platform: Platform,
    id: &str,
    entry: &ResourceEntry,
) -> bool {
    let gate_is_off = |gate: &str, resource_id: &str| {
        config.is_some_and(|config| {
            new_stack.inputs.iter().any(|input| {
                input.id == gate
                    && input.default == Some(alien_core::StackInputDefaultValue::Boolean(false))
            }) && alien_core::gate_resolves_true(
                &new_stack.inputs,
                gate,
                &config.input_values,
                resource_id,
                platform,
            ) == Ok(false)
        })
    };
    if let Some(gate) = entry.enabled_when.as_deref() {
        return gate_is_off(gate, id);
    }
    let Some(profile) = id.strip_suffix("-sa") else {
        return false;
    };
    if entry.config.downcast_ref::<ServiceAccount>().is_none() {
        return false;
    }
    let mut users = new_stack
        .resources()
        .filter(|(_, user)| user.config.get_permissions() == Some(profile))
        .map(|(_, user)| user.enabled_when.as_deref());
    let Some(Some(gate)) = users.next() else {
        return false;
    };
    // A retained resource may link the identity without running under its profile (a worker of
    // another profile that links `app-sa`). It would start without the identity, so the
    // identity is in use unless every linker carries the same off gate.
    let linked_by_other = new_stack
        .linkers(&alien_core::ResourceRef::new(
            ServiceAccount::RESOURCE_TYPE,
            id,
        ))
        .iter()
        .any(|linker| {
            new_stack
                .resources
                .get(linker.id())
                .is_none_or(|user| user.enabled_when.as_deref() != Some(gate))
        });
    !linked_by_other && users.all(|other| other == Some(gate)) && gate_is_off(gate, id)
}

/// Frozen resources of `new_stack` that the installed stack lacks and that the update leaves
/// out because their gate is off (see [`FrozenResourcesUnchangedCheck`]). Setup never created
/// them and nothing uses them, so the update must not plan them: it would try to create them
/// with runtime credentials, which fails. Identities setup already installed are not listed.
pub fn unused_frozen_additions(
    old_stack: &Stack,
    new_stack: &Stack,
    config: &DeploymentConfig,
    platform: Platform,
) -> Vec<String> {
    new_stack
        .resources()
        .filter(|(id, entry)| {
            entry.lifecycle == ResourceLifecycle::Frozen
                && !old_stack.resources.contains_key(id.as_str())
                && frozen_addition_excused_while_gated_off(
                    new_stack,
                    Some(config),
                    platform,
                    id,
                    entry,
                )
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Writes into the frozen resources of an update target what setup installed for an input
/// reference that resolves to no value: the reference is dropped. The update that accepted the
/// reference (see [`FrozenResourcesUnchangedCheck`]) installed nothing for it, so the stack
/// recorded afterwards must say so. Were the reference kept, a later answer that lists values
/// would compare equal to the recorded reference, skip setup, and never be installed.
///
/// Only a reference the installed stack did not record is dropped. One setup recorded (it
/// installed values for it) stays, so the baseline keeps what setup installed.
pub fn resolve_empty_frozen_input_references(
    installed: &Stack,
    stack: &mut Stack,
    config: &DeploymentConfig,
) {
    let resolved: Vec<_> = {
        let answers = InputAnswers::new(&stack.inputs, &config.input_values);
        stack
            .resources()
            .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
            .filter(|(id, _)| {
                installed
                    .resources
                    .get(id.as_str())
                    .is_some_and(|old| old.config.input_references().is_empty())
            })
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .with_empty_input_references_removed(&answers)
                    .map(|resource| (id.clone(), resource))
            })
            .collect()
    };
    for (id, resource) in resolved {
        if let Some(entry) = stack.resources.get_mut(&id) {
            entry.config = resource;
        }
    }
}

#[async_trait::async_trait]
impl StackCompatibilityCheck for FrozenResourcesUnchangedCheck {
    fn description(&self) -> &'static str {
        "Frozen resources shouldn't be added or modified during updates"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        self.compare(old_stack, new_stack, None)
    }

    async fn check_with_config(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: &DeploymentConfig,
        _platform: Platform,
    ) -> Result<CheckResult> {
        self.compare(old_stack, new_stack, Some(config))
    }
}

impl FrozenResourcesUnchangedCheck {
    /// `config` carries the deployment's input answers; without it input references compare as
    /// written.
    fn compare(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: Option<&DeploymentConfig>,
    ) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

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
        // A resource that is new in this release and gated by an input declared with default
        // false stays out of an installed stack until setup answers the gate, as the setup
        // templates leave it out, so it is not an addition while the gate resolves false. So is
        // the identity of a permission profile whose every user is such a resource: it carries
        // nothing of its own and nothing would use it. An answer that turns true, a gate that
        // cannot be resolved, another declared default, a profile with an ungated user, and a
        // resource the installed stack already has are compared as before.
        let added_frozen: Vec<_> = new_frozen
            .iter()
            .filter(|(id, _)| !old_frozen_ids.contains(*id))
            .filter(|(id, entry)| {
                !frozen_addition_excused_while_gated_off(
                    new_stack,
                    config,
                    self.platform,
                    id,
                    entry,
                )
            })
            .map(|(id, _)| id)
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
                // A reference to an input nobody filled in is not a change to what setup
                // recorded without the reference (a release that adds an optional input).
                let answers =
                    config.map(|config| InputAnswers::new(&new_stack.inputs, &config.input_values));
                let no_value_reference = answers.is_some_and(|answers| {
                    new_config
                        .with_empty_input_references_removed(&answers)
                        .is_some_and(|resolved| {
                            *old_config == *on_platform(self.platform, &resolved)
                        })
                });
                // A service account that only lost permission sets keeps its installed role
                // until the next setup, which is broader than this release needs. Anything it
                // gains, or a set whose definition changed under the same id, is not narrowing.
                let only_narrowed = old_config != new_config
                    && crate::compatibility::narrowing::service_account_narrowed(
                        new_stack,
                        &old_config,
                        &new_config,
                    );
                if only_narrowed {
                    warnings.push(format!(
                        "Service account '{id}' grants less than the installed setup. The installed grants stay until the next setup update removes them."
                    ));
                }
                if old_config != new_config
                    && !no_value_reference
                    && !only_narrowed
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
                    let mut details = details;
                    details.extend(new_config.input_references().into_iter().filter_map(
                        |(field, input)| {
                            // Only a reference the recorded config lacks is the new part.
                            (!old_config.input_references().iter().any(|old| old.0 == field))
                                .then(|| {
                                    format!(
                                        "'{field}' reads deployment input '{input}', which lists values the installed setup does not have"
                                    )
                                })
                        },
                    ));
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

        if self.platform == Platform::Kubernetes {
            errors.extend(kubernetes_live_sandbox_chart_changes(old_stack, new_stack));
        }

        if errors.is_empty() {
            if warnings.is_empty() {
                Ok(CheckResult::success())
            } else {
                Ok(CheckResult::with_warnings(warnings))
            }
        } else {
            Ok(CheckResult::failed_with_warnings(errors, warnings))
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

    /// Dropping an account's last resource-scoped grant only narrows it. Dropping only the
    /// capture of grants its profile still holds hides what the account grants, so it needs setup.
    #[tokio::test]
    async fn losing_the_last_resource_grant_narrows_but_losing_only_the_capture_needs_setup() {
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
        assert!(removed.success, "{:?}", removed.errors);
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: old_resources,
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            extensions: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
                data_disk_gib_range: None,
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

    /// Setup owns a frozen service account's role. Gaining permission sets needs setup; losing
    /// them does not, the installed role simply stays broader until the next setup.
    #[tokio::test]
    async fn a_service_account_that_only_loses_permission_sets_needs_no_setup() {
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
        assert!(narrowed.success, "{:?}", narrowed.errors);
        assert_eq!(narrowed.warnings.len(), 1);

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

    /// First setup used to record the chosen machine with the registration default's profile.
    /// The next release brings the profile in line with the recorded machine, without setup.
    #[tokio::test]
    async fn recorded_profile_of_another_machine_is_adopted_for_the_same_machine() {
        let platform = Platform::Azure;
        let profile_of = |machine: &str| {
            alien_core::instance_catalog::find_instance_type(platform, machine)
                .map(|spec| spec.to_machine_profile())
        };
        let mut old = compute_cluster(2);
        old.capacity_groups[0].nested_virtualization = None;
        old.capacity_groups[0].instance_type = Some("Standard_D2s_v5".into());
        old.capacity_groups[0].profile = profile_of("Standard_B2s");
        let mut new = old.clone();
        new.capacity_groups[0].profile = profile_of("Standard_D2s_v5");
        assert_ne!(
            old.capacity_groups[0].profile,
            new.capacity_groups[0].profile
        );
        let check = FrozenResourcesUnchangedCheck { platform };
        let result = check
            .check(&compute_stack(old.clone()), &compute_stack(new.clone()))
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);

        // A profile that is not the machine's own is still a modification.
        new.capacity_groups[0].profile = profile_of("Standard_B4ms");
        let result = check
            .check(&compute_stack(old), &compute_stack(new))
            .await
            .unwrap();
        assert!(!result.success);
    }

    fn stale_pair(platform: Platform, machine: &str) -> (ComputeCluster, ComputeCluster) {
        let profile_of = |machine: &str| {
            find_instance_type(platform, machine).map(|spec| spec.to_machine_profile())
        };
        let mut old = compute_cluster(2);
        old.capacity_groups[0].nested_virtualization = None;
        old.capacity_groups[0].instance_type = Some(machine.into());
        old.capacity_groups[0].profile = profile_of("Standard_B2s");
        let mut new = old.clone();
        new.capacity_groups[0].profile = profile_of(machine);
        (old, new)
    }

    async fn frozen_ok(platform: Platform, old: ComputeCluster, new: ComputeCluster) -> bool {
        FrozenResourcesUnchangedCheck { platform }
            .check(&compute_stack(old), &compute_stack(new))
            .await
            .unwrap()
            .success
    }

    #[tokio::test]
    async fn stale_profile_excuse_keeps_the_workloads_larger_disk() {
        let (mut old, mut new) = stale_pair(Platform::Azure, "Standard_D2s_v5");
        // The planner records the larger of the catalog's disk and the workload's request.
        old.capacity_groups[0]
            .profile
            .as_mut()
            .unwrap()
            .ephemeral_storage_bytes = 50 * GIB;
        new.capacity_groups[0]
            .profile
            .as_mut()
            .unwrap()
            .ephemeral_storage_bytes = 50 * GIB;
        assert!(frozen_ok(Platform::Azure, old.clone(), new.clone()).await);
        // A disk smaller than the catalog's is not something the planner records.
        new.capacity_groups[0]
            .profile
            .as_mut()
            .unwrap()
            .ephemeral_storage_bytes = GIB;
        assert!(!frozen_ok(Platform::Azure, old, new).await);
    }

    #[tokio::test]
    async fn stale_profile_excuse_refuses_a_machine_not_in_the_catalog() {
        let (mut old, mut new) = stale_pair(Platform::Azure, "Standard_D2s_v5");
        old.capacity_groups[0].instance_type = Some("Standard_Nope".into());
        new.capacity_groups[0].instance_type = Some("Standard_Nope".into());
        assert!(!frozen_ok(Platform::Azure, old, new).await);
    }

    #[tokio::test]
    async fn stale_profile_excuse_refuses_a_profile_of_a_different_machine() {
        // The new group names D2s_v5 but carries the profile of D4s_v5.
        let (old, mut new) = stale_pair(Platform::Azure, "Standard_D2s_v5");
        new.capacity_groups[0].profile =
            find_instance_type(Platform::Azure, "Standard_D4s_v5").map(|s| s.to_machine_profile());
        assert!(!frozen_ok(Platform::Azure, old, new).await);
    }

    #[tokio::test]
    async fn stale_profile_excuse_with_a_fixed_scale_policy() {
        let (mut old, mut new) = stale_pair(Platform::Azure, "Standard_D2s_v5");
        for cluster in [&mut old, &mut new] {
            cluster.capacity_groups[0].scale_policy =
                Some(CapacityGroupScalePolicy::from_selected_bounds(2, 2));
        }
        assert!(frozen_ok(Platform::Azure, old.clone(), new.clone()).await);
        // Fixed or not, a profile that is not the machine's stays a modification.
        new.capacity_groups[0]
            .profile
            .as_mut()
            .unwrap()
            .memory_bytes += 1;
        assert!(!frozen_ok(Platform::Azure, old, new).await);
    }

    #[tokio::test]
    async fn stale_profile_excuse_is_azure_only() {
        let profile_of = |platform, machine: &str| {
            find_instance_type(platform, machine).map(|spec| spec.to_machine_profile())
        };
        for (platform, other, machine) in [
            (Platform::Aws, "m7i.large", "m7i.xlarge"),
            (Platform::Gcp, "e2-medium", "n2-standard-4"),
        ] {
            let mut old = compute_cluster(2);
            old.capacity_groups[0].nested_virtualization = None;
            old.capacity_groups[0].instance_type = Some(machine.into());
            old.capacity_groups[0].profile = profile_of(platform, other);
            let mut new = old.clone();
            new.capacity_groups[0].profile = profile_of(platform, machine);
            assert!(!frozen_ok(platform, old, new).await, "{platform}");
        }
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

    const GIB: u64 = 1024 * 1024 * 1024;

    /// A pool as the compute mutation prepares it: the machine's profile with
    /// the declared storage and data disk.
    fn disk_cluster(
        platform: Platform,
        machine: Option<&str>,
        ephemeral_gib: u64,
        data_disk_gib: Option<u64>,
    ) -> ComputeCluster {
        let mut profile = match machine {
            Some(machine) => find_instance_type(platform, machine)
                .expect("catalog machine")
                .to_machine_profile_for_storage(ephemeral_gib * GIB),
            None => alien_core::MachineProfile {
                cpu: "4".to_string(),
                memory_bytes: 16 * GIB,
                ephemeral_storage_bytes: ephemeral_gib * GIB,
                architecture: None,
                gpu: None,
                data_disk: None,
            },
        };
        profile.data_disk = data_disk_gib.map(|gib| alien_core::DataDiskRequirement {
            size_bytes: gib * GIB,
        });
        ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "workers".to_string(),
                instance_type: machine.map(str::to_string),
                profile: Some(profile),
                min_size: 2,
                max_size: 2,
                scale_policy: None,
                nested_virtualization: Some(platform == Platform::Aws),
                data_disk_gib_range: None,
            })
            .build()
    }

    async fn disk_change(
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
    async fn disk_size_changes_do_not_need_setup_where_alien_applies_them() {
        for (platform, machine) in [
            (Platform::Aws, Some("m8i.2xlarge")),
            (Platform::Gcp, Some("n2-standard-4")),
            (Platform::Azure, Some("Standard_D4s_v5")),
            (Platform::Machines, None),
        ] {
            let cluster = |ephemeral_gib, data_disk_gib| {
                disk_cluster(platform, machine, ephemeral_gib, data_disk_gib)
            };
            for (old, new) in [
                (cluster(20, None), cluster(20, Some(30))),
                (cluster(20, Some(30)), cluster(20, Some(50))),
                (cluster(20, Some(50)), cluster(20, None)),
                (cluster(20, None), cluster(40, None)),
                (cluster(40, Some(30)), cluster(20, Some(50))),
            ] {
                let result = disk_change(platform, old, new).await;
                assert!(result.success, "{platform}: {:?}", result.errors);
            }
        }
    }

    #[tokio::test]
    async fn deployment_data_disk_choices_do_not_need_setup() {
        for (platform, machine) in [
            (Platform::Aws, Some("m8i.2xlarge")),
            (Platform::Machines, None),
        ] {
            // A pool offering 0 to `max` GiB, prepared with the deployment's choice.
            let chosen = |data_disk_gib: Option<u64>, max: u32| {
                let mut cluster = disk_cluster(platform, machine, 20, data_disk_gib);
                cluster.capacity_groups[0].data_disk_gib_range =
                    Some(alien_core::ComputeChoiceRange {
                        min: 0,
                        max,
                        default: 200,
                    });
                cluster
            };
            for (case, old, new) in [
                (
                    "another size",
                    chosen(Some(40), 4096),
                    chosen(Some(60), 4096),
                ),
                ("no data disk", chosen(Some(60), 4096), chosen(None, 4096)),
                (
                    "a wider range",
                    chosen(Some(40), 1024),
                    chosen(Some(40), 4096),
                ),
                (
                    "a range replacing a fixed size",
                    disk_cluster(platform, machine, 20, Some(40)),
                    chosen(Some(40), 4096),
                ),
            ] {
                let result = disk_change(platform, old, new).await;
                assert!(result.success, "{platform}, {case}: {:?}", result.errors);
            }
        }
    }

    #[tokio::test]
    async fn disk_size_changes_still_need_setup_on_kubernetes() {
        let cluster = |data_disk_gib| disk_cluster(Platform::Kubernetes, None, 20, data_disk_gib);
        let result = disk_change(Platform::Kubernetes, cluster(None), cluster(Some(30))).await;
        assert!(!result.success);
        assert!(
            result.errors[0].contains("Rerun setup"),
            "{:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn disk_size_change_does_not_carry_other_changes_past_setup() {
        let old = disk_cluster(Platform::Aws, Some("m8i.2xlarge"), 20, None);

        // A machine in another CPU architecture still needs setup with a disk change.
        let mut cross_arch = disk_cluster(Platform::Aws, Some("c7g.xlarge"), 20, Some(30));
        cross_arch.capacity_groups[0].nested_virtualization = Some(true);
        let result = disk_change(Platform::Aws, old.clone(), cross_arch).await;
        assert!(!result.success);
        assert!(
            result.errors[0].contains("changes machine from 'm8i.2xlarge' to 'c7g.xlarge'"),
            "{:?}",
            result.errors
        );

        // A machine in the same architecture is replaced with the new disks anyway.
        let same_arch = disk_cluster(Platform::Aws, Some("m8i.4xlarge"), 40, Some(30));
        let result = disk_change(Platform::Aws, old.clone(), same_arch).await;
        assert!(result.success, "{:?}", result.errors);

        // Hardware that does not follow from the machine is not a disk change.
        let mut other_cpu = disk_cluster(Platform::Aws, Some("m8i.2xlarge"), 20, Some(30));
        other_cpu.capacity_groups[0].profile.as_mut().unwrap().cpu = "16".to_string();
        assert!(
            !disk_change(Platform::Aws, old.clone(), other_cpu)
                .await
                .success
        );

        // Root storage the provider cannot create is not applied by the runtime.
        let too_large = disk_cluster(Platform::Aws, Some("m8i.2xlarge"), 60 * 1024, None);
        assert!(!disk_change(Platform::Aws, old, too_large).await.success);
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
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds,
                ..Default::default()
            })
            .build();
        Stack::new("stack".to_string())
            .add(sandbox, ResourceLifecycle::Frozen)
            .build()
    }

    fn live_sandbox_stack(image: &str, memory: &str, egress: alien_core::SandboxEgress) -> Stack {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
                bundled_from: None,
            })
            .limits(alien_core::SandboxLimits {
                cpu: "1".to_string(),
                memory: memory.to_string(),
                disk: "10Gi".to_string(),
                max_processes: None,
            })
            .egress(egress)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        Stack::new("stack".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    /// A release may change a Live Kubernetes sandbox's image and limits without a chart
    /// upgrade, but not its egress mode, which is the chart's NetworkPolicy.
    #[tokio::test]
    async fn a_live_kubernetes_sandbox_changes_image_and_limits_but_not_egress_without_setup() {
        use alien_core::SandboxEgress;
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Kubernetes,
        };
        let old = live_sandbox_stack("app:1", "2Gi", SandboxEgress::None);

        let image_and_limits = check
            .check(
                &old,
                &live_sandbox_stack("app:2", "4Gi", SandboxEgress::None),
            )
            .await
            .unwrap();
        assert!(image_and_limits.success, "{:?}", image_and_limits.errors);

        let egress = check
            .check(
                &old,
                &live_sandbox_stack("app:1", "2Gi", SandboxEgress::Internet),
            )
            .await
            .unwrap();
        assert!(!egress.success);
        assert!(
            egress.errors[0].contains("egress mode"),
            "{:?}",
            egress.errors
        );
    }

    /// The chart's NetworkPolicy lists the workloads that link a sandbox, so a workload that
    /// starts linking an existing Live sandbox is refused by the old policy until setup runs.
    #[tokio::test]
    async fn a_new_workload_linking_a_live_kubernetes_sandbox_needs_setup() {
        use alien_core::SandboxEgress;
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Kubernetes,
        };
        let old = live_sandbox_stack("app:1", "2Gi", SandboxEgress::None);
        let sandbox = old
            .resources
            .get("agents")
            .unwrap()
            .config
            .downcast_ref::<Sandbox>()
            .unwrap()
            .clone();
        let container = alien_core::Container::new("web".into())
            .code(alien_core::ContainerCode::Image {
                image: "nginx:alpine".into(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(alien_core::ResourceSpec {
                min: "1Gi".into(),
                desired: "1Gi".into(),
            })
            .permissions("app".into())
            .link(&sandbox)
            .build();
        let new = Stack::new("stack".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .add(container, ResourceLifecycle::Live)
            .build();

        let result = check.check(&old, &new).await.unwrap();
        assert!(!result.success);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("workloads that link Live sandbox 'agents'")),
            "{:?}",
            result.errors
        );
        let unchanged = check.check(&new, &new).await.unwrap();
        assert!(unchanged.success, "{:?}", unchanged.errors);
    }

    /// A sandbox added by a release has no chart NetworkPolicy yet, so it waits for setup.
    #[tokio::test]
    async fn a_new_live_kubernetes_sandbox_needs_setup() {
        use alien_core::SandboxEgress;
        let check = FrozenResourcesUnchangedCheck {
            platform: Platform::Kubernetes,
        };
        let old = Stack::new("stack".to_string()).build();
        let result = check
            .check(
                &old,
                &live_sandbox_stack("app:1", "2Gi", SandboxEgress::None),
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(
            result.errors.iter().any(|e| e.contains("is new")),
            "{:?}",
            result.errors
        );
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
        assert!(crate::compatibility::azure_setup::authorization_matches(
            &baseline, &target, &reloaded
        )
        .unwrap());
        let changed = stack(
            Storage::new("files".to_string())
                .versioning(true)
                .public_read(true)
                .build(),
        );
        assert!(!crate::compatibility::azure_setup::authorization_matches(
            &baseline, &changed, &reloaded
        )
        .unwrap());
        let mut gated = target.clone();
        gated.resources.get_mut("files").unwrap().enabled_when = Some("includeFiles".to_string());
        assert!(!crate::compatibility::azure_setup::authorization_matches(
            &baseline, &gated, &reloaded
        )
        .unwrap());
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

    mod gated_additions {
        use super::*;
        use alien_core::{
            AwsManagementConfig, EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig,
            StackInputDefaultValue, StackInputDefinition, StackSettings,
        };

        fn base() -> Stack {
            Stack::new("gated".to_string())
                .add(
                    Storage::new("kept".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
        }

        fn with_gated(gate: Option<&str>, inputs: Vec<StackInputDefinition>) -> Stack {
            let mut builder = Stack::new("gated".to_string()).inputs(inputs).add(
                Storage::new("kept".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
            let added = Storage::new("added".to_string()).build();
            builder = match gate {
                Some(gate) => builder.add_enabled_when(added, ResourceLifecycle::Frozen, gate),
                None => builder.add(added, ResourceLifecycle::Frozen),
            };
            builder.build()
        }

        fn gate(default: Option<bool>) -> StackInputDefinition {
            let mut input =
                StackInputDefinition::deployer_boolean("feature", "Feature", "A feature.", default);
            input.required = false;
            input.default = default.map(StackInputDefaultValue::Boolean);
            input
        }

        fn config(answer: Option<bool>) -> DeploymentConfig {
            let mut config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .management_config(ManagementConfig::Aws(AwsManagementConfig {
                    managing_role_arn: "arn:aws:iam::111122223333:role/management".to_string(),
                }))
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: Vec::new(),
                    hash: String::new(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                })
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build();
            if let Some(answer) = answer {
                config
                    .input_values
                    .insert("feature".to_string(), serde_json::json!(answer));
            }
            config
        }

        async fn errors(new: Stack, config: &DeploymentConfig) -> Vec<String> {
            FrozenResourcesUnchangedCheck {
                platform: Platform::Aws,
            }
            .check_with_config(&base(), &new, config, Platform::Aws)
            .await
            .unwrap()
            .errors
        }

        fn added(errors: &[String]) -> bool {
            errors
                .iter()
                .any(|e| e.starts_with("Cannot add frozen resources during update: 'added'"))
        }

        #[tokio::test]
        async fn a_new_frozen_resource_whose_gate_resolves_false_is_not_an_addition() {
            let new = with_gated(Some("feature"), vec![gate(Some(false))]);
            assert!(errors(new.clone(), &config(None)).await.is_empty());
            // An explicit no behaves the same.
            assert!(errors(new, &config(Some(false))).await.is_empty());
        }

        #[tokio::test]
        async fn an_answer_that_turns_true_without_setup_is_an_addition_again() {
            let new = with_gated(Some("feature"), vec![gate(Some(false))]);
            assert!(added(&errors(new, &config(Some(true))).await));
            // A later release whose declared default is true does the same.
            let later = with_gated(Some("feature"), vec![gate(Some(true))]);
            assert!(added(&errors(later, &config(None)).await));
        }

        #[tokio::test]
        async fn an_ungated_new_frozen_resource_is_still_an_addition() {
            let new = with_gated(None, vec![gate(Some(false))]);
            assert!(added(&errors(new, &config(None)).await));
        }

        #[tokio::test]
        async fn a_gate_that_cannot_be_resolved_is_still_an_addition() {
            // Undeclared input.
            let undeclared = with_gated(Some("feature"), vec![]);
            assert!(added(&errors(undeclared, &config(None)).await));
            // Declared with no default and no answer.
            let unanswered = with_gated(Some("feature"), vec![gate(None)]);
            assert!(added(&errors(unanswered, &config(None)).await));
        }

        #[tokio::test]
        async fn a_gate_declared_with_another_default_is_not_excused() {
            // Required (no default), and an explicit no: only a declared default of false
            // marks an input a later release added for installed stacks to leave off.
            let new = with_gated(Some("feature"), vec![gate(None)]);
            assert!(added(&errors(new, &config(Some(false))).await));
            let default_true = with_gated(Some("feature"), vec![gate(Some(true))]);
            assert!(added(&errors(default_true, &config(Some(false))).await));
        }

        #[tokio::test]
        async fn an_installed_frozen_resource_whose_gate_turns_off_keeps_its_behavior() {
            // The installed stack has `added`; the new release gates it off. Excusing it would
            // hide a removal, so the outcome must equal the ungated release's.
            let installed = with_gated(None, vec![]);
            let ungated = with_gated(None, vec![gate(Some(false))]);
            let gated_off = with_gated(Some("feature"), vec![gate(Some(false))]);
            let check = FrozenResourcesUnchangedCheck {
                platform: Platform::Aws,
            };
            let config = config(Some(false));
            let ungated_result = check
                .check_with_config(&installed, &ungated, &config, Platform::Aws)
                .await
                .unwrap();
            let gated_result = check
                .check_with_config(&installed, &gated_off, &config, Platform::Aws)
                .await
                .unwrap();
            assert_eq!(ungated_result.success, gated_result.success);
            assert_eq!(ungated_result.errors, gated_result.errors);
            // And the stack that lost the resource entirely is still a removal.
            let removed = check
                .check_with_config(&installed, &base(), &config, Platform::Aws)
                .await
                .unwrap();
            assert!(!removed.success, "{:?}", removed.errors);
        }

        /// A stack whose profile `app` has the identity `app-sa` and one worker per entry of
        /// `gates` running as it.
        fn with_profile_users(gates: &[Option<&str>], inputs: Vec<StackInputDefinition>) -> Stack {
            let mut builder = Stack::new("gated".to_string())
                .inputs(inputs)
                .add(
                    Storage::new("kept".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    ServiceAccount::new("app-sa".to_string()).build(),
                    ResourceLifecycle::Frozen,
                );
            for (index, gate) in gates.iter().enumerate() {
                let worker = alien_core::Worker::new(format!("w{index}"))
                    .permissions("app".to_string())
                    .code(alien_core::WorkerCode::Image {
                        image: "example.com/app:latest".to_string(),
                    })
                    .build();
                builder = match gate {
                    Some(gate) => builder.add_enabled_when(worker, ResourceLifecycle::Live, *gate),
                    None => builder.add(worker, ResourceLifecycle::Live),
                };
            }
            builder.build()
        }

        fn identity_added(errors: &[String]) -> bool {
            errors
                .iter()
                .any(|e| e.starts_with("Cannot add frozen resources during update: 'app-sa'"))
        }

        #[tokio::test]
        async fn the_identity_of_a_profile_whose_users_are_all_gated_off_is_not_an_addition() {
            let new =
                with_profile_users(&[Some("feature"), Some("feature")], vec![gate(Some(false))]);
            assert!(errors(new, &config(None)).await.is_empty());
        }

        #[tokio::test]
        async fn that_identity_is_an_addition_when_the_gate_turns_true_or_a_user_is_ungated() {
            let on = with_profile_users(&[Some("feature")], vec![gate(Some(false))]);
            assert!(identity_added(&errors(on, &config(Some(true))).await));
            let mixed = with_profile_users(&[Some("feature"), None], vec![gate(Some(false))]);
            assert!(identity_added(&errors(mixed, &config(None)).await));
            let none = with_profile_users(&[], vec![gate(Some(false))]);
            assert!(identity_added(&errors(none, &config(None)).await));
            let mut other = gate(Some(false));
            other.id = "other".to_string();
            let different = with_profile_users(
                &[Some("feature"), Some("other")],
                vec![gate(Some(false)), other],
            );
            assert!(identity_added(&errors(different, &config(None)).await));
            let default_true = with_profile_users(&[Some("feature")], vec![gate(Some(true))]);
            assert!(identity_added(
                &errors(default_true, &config(Some(false))).await
            ));
        }

        #[tokio::test]
        async fn an_identity_a_retained_worker_links_is_an_addition() {
            // An ungated worker of another profile links `app-sa` while every `app` worker is
            // gated off: dropping the identity would start it without the link.
            let linking = alien_core::Worker::new("linker".to_string())
                .permissions("base".to_string())
                .link(&ServiceAccount::new("app-sa".to_string()).build())
                .code(alien_core::WorkerCode::Image {
                    image: "example.com/app:latest".to_string(),
                })
                .build();
            let mut new = with_profile_users(&[Some("feature")], vec![gate(Some(false))]);
            new.resources.insert(
                "linker".to_string(),
                alien_core::ResourceEntry {
                    config: alien_core::Resource::new(linking),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: Vec::new(),
                    remote_access: false,
                    enabled_when: None,
                    description: None,
                    display_name: None,
                },
            );
            assert!(identity_added(&errors(new.clone(), &config(None)).await));
            assert!(
                unused_frozen_additions(&base(), &new, &config(None), Platform::Aws).is_empty()
            );
        }

        #[tokio::test]
        async fn without_the_deployment_config_a_gated_addition_is_an_addition() {
            let new = with_gated(Some("feature"), vec![gate(Some(false))]);
            let result = FrozenResourcesUnchangedCheck {
                platform: Platform::Aws,
            }
            .check(&base(), &new)
            .await
            .unwrap();
            assert!(added(&result.errors));
        }

        #[test]
        fn an_excused_identity_setup_never_installed_is_left_out_of_the_target() {
            let installed = base();
            let new = with_profile_users(&[Some("feature")], vec![gate(Some(false))]);
            let unused = unused_frozen_additions(&installed, &new, &config(None), Platform::Aws);
            assert_eq!(unused, vec!["app-sa".to_string()]);
            // A gated resource that is itself frozen is left out the same way.
            let gated = with_gated(Some("feature"), vec![gate(Some(false))]);
            assert_eq!(
                unused_frozen_additions(&installed, &gated, &config(None), Platform::Aws),
                vec!["added".to_string()]
            );
        }

        #[test]
        fn an_identity_setup_installed_stays_in_the_target() {
            // Setup already installed `app-sa`, so the update keeps it.
            let new = with_profile_users(&[Some("feature")], vec![gate(Some(false))]);
            let installed = with_profile_users(&[], vec![gate(Some(false))]);
            assert!(
                unused_frozen_additions(&installed, &new, &config(None), Platform::Aws).is_empty()
            );
        }

        #[test]
        fn an_identity_that_is_an_addition_is_not_left_out() {
            // Not excused (the answer turned true, or a user is ungated): the check refuses it
            // and nothing is dropped from the target.
            let installed = base();
            let on = with_profile_users(&[Some("feature")], vec![gate(Some(false))]);
            assert!(
                unused_frozen_additions(&installed, &on, &config(Some(true)), Platform::Aws)
                    .is_empty()
            );
            let mixed = with_profile_users(&[Some("feature"), None], vec![gate(Some(false))]);
            assert!(
                unused_frozen_additions(&installed, &mixed, &config(None), Platform::Aws)
                    .is_empty()
            );
        }
    }
}
