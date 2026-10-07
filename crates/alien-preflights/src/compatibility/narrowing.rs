//! Setup-owned permission changes an ordinary update may defer.
//!
//! A change that only removes grants leaves the installed identities broader
//! than the new stack needs, never narrower. The update can run with them, and
//! must: it still has to delete the resources and values the removed grants
//! covered. The next setup trims the identities to the new stack.

use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{Resource, ServiceAccount};
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

/// Whether a frozen service account only lost permission sets.
///
/// Legacy prepared stacks kept resource grants only in the permission profile,
/// so an empty resource capture can mean "not captured" rather than "none".
/// Only accounts that capture resource grants the same way are compared.
pub fn service_account_narrowed(old: &Resource, new: &Resource) -> bool {
    let (Some(old), Some(new)) = (
        old.downcast_ref::<ServiceAccount>(),
        new.downcast_ref::<ServiceAccount>(),
    ) else {
        return false;
    };
    old.id == new.id
        && old.resource_permission_sets.is_empty() == new.resource_permission_sets.is_empty()
        && new
            .stack_permission_sets
            .iter()
            .all(|set| old.stack_permission_sets.contains(set))
        && new.resource_permission_sets.iter().all(|(resource, sets)| {
            old.resource_permission_sets
                .get(resource)
                .is_some_and(|old_sets| sets.iter().all(|set| old_sets.contains(set)))
        })
}
