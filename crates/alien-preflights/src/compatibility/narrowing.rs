//! Whether a permission change only removes grants.
//!
//! Permissions may narrow without setup: the installed identity stays broader than
//! the new stack needs, and the update runs with it. For management this is also what
//! lets the update delete the resources the removed grants covered. The next setup
//! trims the leftover. This holds for management permissions, workload profiles and
//! the frozen service accounts derived from them. Anything gained, or a permission
//! set whose definition changed under the same id, still needs setup.

use alien_core::permissions::{
    ManagementPermissions, PermissionProfile, PermissionSet, PermissionSetReference,
};
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
                .is_some_and(|old_grants| grants.iter().all(|grant| grant_kept(old_grants, grant)))
        })
}

/// Whether `grant` is one of `old_grants`, or an inline set that only lost resources.
fn grant_kept(old_grants: &[PermissionSetReference], grant: &PermissionSetReference) -> bool {
    old_grants.iter().any(|old| match (old, grant) {
        (PermissionSetReference::Inline(old), PermissionSetReference::Inline(new)) => {
            permission_set_narrowed(old, new)
        }
        _ => old == grant,
    })
}

/// Whether an inline permission set only lost resources. Some sets keep their id while their
/// body follows the stack, such as the deployer secrets read set, which lists one entry per
/// secret the workload reads. The new set must have the same id, effect, actions and roles; for
/// an `Allow` its resource list (and a GCP condition's alternatives) may only shrink. A `Deny`
/// that changes at all is not narrowing: dropping a denied resource widens access.
pub fn permission_set_narrowed(old: &PermissionSet, new: &PermissionSet) -> bool {
    if old == new {
        return true;
    }
    if old.id != new.id {
        return false;
    }
    let (Ok(mut old), Ok(mut new)) = (serde_json::to_value(old), serde_json::to_value(new)) else {
        return false;
    };
    // The description is documentation, not access.
    for set in [&mut old, &mut new] {
        if let Some(object) = set.as_object_mut() {
            object.remove("description");
        }
    }
    let (Some(old_platforms), Some(new_platforms)) = (
        old.get_mut("platforms").and_then(|v| v.as_object_mut()),
        new.get_mut("platforms").and_then(|v| v.as_object_mut()),
    ) else {
        return false;
    };
    if old_platforms.len() != new_platforms.len() {
        return false;
    }
    for (platform, old_entries) in old_platforms.iter_mut() {
        let Some(new_entries) = new_platforms.get_mut(platform) else {
            return false;
        };
        let (Some(old_entries), Some(new_entries)) =
            (old_entries.as_array_mut(), new_entries.as_array_mut())
        else {
            return false;
        };
        if old_entries.len() != new_entries.len() {
            return false;
        }
        for (old_entry, new_entry) in old_entries.iter_mut().zip(new_entries.iter_mut()) {
            if !entry_narrowed(old_entry, new_entry) {
                return false;
            }
        }
    }
    old == new
}

/// Whether one platform entry only lost resources. Strips the parts that may shrink from both
/// sides once checked, so the caller compares everything else for equality.
fn entry_narrowed(old: &mut serde_json::Value, new: &mut serde_json::Value) -> bool {
    use serde_json::Value;
    if old == new {
        return true;
    }
    let allows = |entry: &Value| entry.get("effect").map_or(true, |e| e == "Allow");
    if !allows(old) || !allows(new) {
        return false;
    }
    for target in ["stack", "resource"] {
        let (Some(old_spec), Some(new_spec)) = (
            old.pointer_mut(&format!("/binding/{target}")),
            new.pointer_mut(&format!("/binding/{target}")),
        ) else {
            continue;
        };
        if !spec_narrowed(old_spec, new_spec) {
            return false;
        }
    }
    true
}

fn spec_narrowed(old: &mut serde_json::Value, new: &mut serde_json::Value) -> bool {
    use serde_json::Value;
    let strings = |value: Option<&Value>| -> Option<Vec<String>> {
        match value {
            None => Some(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect(),
            _ => None,
        }
    };
    let (Some(old_resources), Some(new_resources)) =
        (strings(old.get("resources")), strings(new.get("resources")))
    else {
        return false;
    };
    if !new_resources
        .iter()
        .all(|item| old_resources.contains(item))
    {
        return false;
    }
    if let (Some(o), Some(n)) = (old.as_object_mut(), new.as_object_mut()) {
        o.remove("resources");
        n.remove("resources");
    }
    // A GCP condition lists one alternative per resource, joined with `||`.
    let terms = |spec: &Value| {
        spec.pointer("/condition/expression")
            .and_then(Value::as_str)
            .map(|expression| {
                expression
                    .split(" || ")
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
    };
    if let (Some(old_terms), Some(new_terms)) = (terms(old), terms(new)) {
        if !new_terms.iter().all(|term| old_terms.contains(term)) {
            return false;
        }
        for spec in [old, new] {
            if let Some(condition) = spec
                .pointer_mut("/condition")
                .and_then(Value::as_object_mut)
            {
                condition.remove("expression");
            }
        }
    }
    true
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
    let kept = |old_sets: &[PermissionSet], set: &PermissionSet| {
        old_sets.iter().any(|old| permission_set_narrowed(old, set))
    };
    new.stack_permission_sets
        .iter()
        .all(|set| kept(&old.stack_permission_sets, set))
        && new.resource_permission_sets.iter().all(|(resource, sets)| {
            old.resource_permission_sets
                .get(resource)
                .is_some_and(|old_sets| sets.iter().all(|set| kept(old_sets, set)))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(effect: &str, actions: &[&str], resources: &[&str]) -> PermissionSet {
        serde_json::from_value(serde_json::json!({
            "id": "vault/example",
            "description": "d",
            "platforms": { "aws": [{
                "effect": effect,
                "grant": { "actions": actions },
                "binding": { "resource": { "resources": resources } }
            }] }
        }))
        .unwrap()
    }

    /// An inline set keeps its id while its resource list follows the stack. Fewer resources is
    /// narrowing; anything else, or any change to a `Deny`, is not.
    #[test]
    fn an_inline_set_narrows_only_by_losing_resources() {
        let old = set("Allow", &["ssm:GetParameter"], &["a", "b", "c"]);
        assert!(permission_set_narrowed(&old, &old));
        assert!(permission_set_narrowed(
            &old,
            &set("Allow", &["ssm:GetParameter"], &["a", "c"])
        ));
        assert!(!permission_set_narrowed(
            &old,
            &set("Allow", &["ssm:GetParameter"], &["a", "d"])
        ));
        assert!(!permission_set_narrowed(
            &old,
            &set("Allow", &["ssm:GetParameter", "ssm:PutParameter"], &["a"])
        ));
        assert!(!permission_set_narrowed(
            &old,
            &set("Deny", &["ssm:GetParameter"], &["a", "b", "c"])
        ));
        let deny = set("Deny", &["ssm:GetParameter"], &["a", "b"]);
        assert!(!permission_set_narrowed(
            &deny,
            &set("Deny", &["ssm:GetParameter"], &["a"])
        ));
    }
}
