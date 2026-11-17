//! Whether a permission change only removes grants.
//!
//! Permissions may narrow without setup: the installed identity stays broader than
//! the new stack needs, and the update runs with it. For management this is also what
//! lets the update delete the resources the removed grants covered. The next setup
//! trims the leftover. This holds for management permissions, workload profiles and
//! the frozen service accounts derived from them. Anything gained, or a permission
//! set whose definition changed under the same id, still needs setup.

use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{Resource, ServiceAccount, Stack};
use alien_permissions::{MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD};

/// Whether `new` grants nothing `old` does not: every scope it keeps holds a
/// subset of that scope's old grants. A role guard is a Deny, so dropping one
/// widens access and is never narrowing.
pub fn profile_narrowed(old: &PermissionProfile, new: &PermissionProfile) -> bool {
    let keeps_guards = [MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD]
        .into_iter()
        .map(PermissionSetReference::from_name)
        .all(|guard| {
            let held = |profile: &PermissionProfile| {
                profile.0.values().any(|grants| grants.contains(&guard))
            };
            !held(old) || held(new)
        });
    keeps_guards
        && new.0.iter().all(|(scope, grants)| {
            old.0
                .get(scope)
                .is_some_and(|old_grants| grants.iter().all(|grant| old_grants.contains(grant)))
        })
}

/// Whether the management permissions only lost grants. `Auto` derives its
/// grants later, so only a change between explicit profiles can be judged.
pub fn management_narrowed(old: &ManagementPermissions, new: &ManagementPermissions) -> bool {
    match (old, new) {
        (ManagementPermissions::Extend(old), ManagementPermissions::Extend(new))
        | (ManagementPermissions::Override(old), ManagementPermissions::Override(new)) => {
            profile_narrowed(old, new)
        }
        _ => false,
    }
}

/// Whether a frozen service account in `new_stack` only lost permission sets.
///
/// Legacy prepared stacks kept resource grants only in the permission profile,
/// so an account with no captured resource grants may simply not capture them.
/// An empty capture counts as "none left" only when the account's profile in
/// `new_stack` has no resource-scoped grants either.
pub fn service_account_narrowed(new_stack: &Stack, old: &Resource, new: &Resource) -> bool {
    let (Some(old), Some(new)) = (
        old.downcast_ref::<ServiceAccount>(),
        new.downcast_ref::<ServiceAccount>(),
    ) else {
        return false;
    };
    if old.id != new.id {
        return false;
    }
    if old.resource_permission_sets.is_empty() && !new.resource_permission_sets.is_empty() {
        return false;
    }
    if !old.resource_permission_sets.is_empty()
        && new.resource_permission_sets.is_empty()
        && profile_has_resource_grants(new_stack, &new.id)
    {
        return false;
    }
    new.stack_permission_sets
        .iter()
        .all(|set| old.stack_permission_sets.contains(set))
        && new.resource_permission_sets.iter().all(|(resource, sets)| {
            old.resource_permission_sets
                .get(resource)
                .is_some_and(|old_sets| sets.iter().all(|set| old_sets.contains(set)))
        })
}

/// Whether the profile behind service account `account_id` grants anything on
/// a specific resource. Profile accounts are named `<profile>-sa`.
fn profile_has_resource_grants(stack: &Stack, account_id: &str) -> bool {
    account_id
        .strip_suffix("-sa")
        .and_then(|profile| stack.permissions.profiles.get(profile))
        .is_some_and(|profile| profile.0.keys().any(|scope| scope != "*"))
}

/// The permission profiles and frozen service accounts of `new` that grant less than `old`
/// does, named for a "setup update recommended" notice. The installed identities keep what
/// the release no longer asks for until the next setup update removes it.
pub fn leftover_grants(old: &Stack, new: &Stack) -> Vec<String> {
    let mut leftovers = Vec::new();
    for (name, new_profile) in &new.permissions.profiles {
        if let Some(old_profile) = old.permissions.profiles.get(name) {
            if old_profile != new_profile && profile_narrowed(old_profile, new_profile) {
                leftovers.push(format!("permission profile '{name}'"));
            }
        }
    }
    for (id, new_entry) in new.resources() {
        if let Some(old_entry) = old.resources.get(id) {
            if old_entry.config != new_entry.config
                && service_account_narrowed(new, &old_entry.config, &new_entry.config)
            {
                leftovers.push(format!("service account '{id}'"));
            }
        }
    }
    leftovers
}
