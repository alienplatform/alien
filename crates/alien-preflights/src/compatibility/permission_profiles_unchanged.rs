use crate::error::Result;
use crate::mutations::management_permission_profile::GATE_DERIVED_GLOBAL_SUFFIXES;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::Stack;
use std::collections::HashSet;

/// Validates that permission profiles in the stack haven't been modified.
///
/// Permission profiles define the security model of the stack and changing them
/// during updates could lead to security vulnerabilities or privilege escalation.
///
/// Grants that follow a resource gated with `.enabled(input)` are exempt from
/// the unchanged requirement, but only for **presence**: the deployer controls
/// that resource's existence, so a scope keyed by its id may appear or
/// disappear with the answer, and the gated types' `<type>/…` entries may
/// enter or leave the derived `'*'` scope. When a gated resource's scope is
/// present on both sides its contents are still compared, so a grant
/// escalation (say `kv/provision` to `kv/data-write`) cannot hide behind the
/// gate. The presence exemption is safe because the setup template renders
/// management grants for live resources unconditionally, so the installed
/// role already covers either answer.
pub struct PermissionProfilesUnchangedCheck;

/// The scopes and grant prefixes that follow a deployer's gate, from either
/// stack side: the gated resources' own scope keys, plus the `<type>/` grant
/// prefixes their types contribute to the derived `'*'` scope.
struct GatedContributions {
    resource_ids: HashSet<String>,
    type_prefixes: HashSet<String>,
}

fn gated_contributions(old_stack: &Stack, new_stack: &Stack) -> GatedContributions {
    let mut resource_ids = HashSet::new();
    let mut type_prefixes = HashSet::new();
    for (resource_id, entry) in old_stack.resources().chain(new_stack.resources()) {
        if entry.enabled_when.is_none() {
            continue;
        }
        resource_ids.insert(resource_id.clone());
        // Grant ids use the permission namespace, which is not always the raw
        // resource type; building the prefix any other way would silently
        // miss the very grants this exemption exists for.
        type_prefixes.insert(format!(
            "{}/",
            crate::mutations::management_permission_profile::permission_resource_type(
                entry.config.resource_type().as_ref(),
            )
        ));
    }
    GatedContributions {
        resource_ids,
        type_prefixes,
    }
}

/// A `'*'`-scope grant list without the entries that follow a gate.
///
/// Only the derived management suffixes actually follow a gate; exempting a
/// data-capable grant here would let it slip through as soon as any resource
/// of its type is gated. Full references are compared (not their ids), so an
/// inline set whose body changed under a stable id still reads as drift.
fn without_gate_derived<'a>(
    grants: Option<&'a Vec<PermissionSetReference>>,
    gated: &GatedContributions,
) -> Vec<&'a PermissionSetReference> {
    grants
        .map(|grants| {
            grants
                .iter()
                .filter(|grant| {
                    !gated.type_prefixes.iter().any(|prefix| {
                        grant
                            .id()
                            .strip_prefix(prefix.as_str())
                            .is_some_and(|suffix| GATE_DERIVED_GLOBAL_SUFFIXES.contains(&suffix))
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether two profiles differ once the gated presence exemption is applied.
///
/// A scope keyed by a gated resource's id is exempt only when it exists on
/// exactly one side (the gate toggled); present on both sides, its contents
/// must match. `'*'`-scope entries with a gated type's prefix are exempt in
/// both directions: they are derived per type, so a single gated resource
/// legitimately adds and removes them.
fn profiles_differ_outside_gates(
    old_profile: &PermissionProfile,
    new_profile: &PermissionProfile,
    gated: &GatedContributions,
) -> bool {
    let scopes: HashSet<&str> = old_profile
        .0
        .keys()
        .chain(new_profile.0.keys())
        .map(String::as_str)
        .collect();

    for scope in scopes {
        let old_grants = old_profile.0.get(scope);
        let new_grants = new_profile.0.get(scope);

        if scope == "*" {
            if without_gate_derived(old_grants, gated) != without_gate_derived(new_grants, gated) {
                return true;
            }
            continue;
        }

        if gated.resource_ids.contains(scope) && (old_grants.is_none() || new_grants.is_none()) {
            continue;
        }

        if old_grants != new_grants {
            return true;
        }
    }

    false
}

fn management_differs_outside_gates(
    old_management: &ManagementPermissions,
    new_management: &ManagementPermissions,
    gated: &GatedContributions,
) -> bool {
    match (old_management, new_management) {
        (ManagementPermissions::Auto, ManagementPermissions::Auto) => false,
        (ManagementPermissions::Extend(old_profile), ManagementPermissions::Extend(new_profile))
        | (
            ManagementPermissions::Override(old_profile),
            ManagementPermissions::Override(new_profile),
        ) => profiles_differ_outside_gates(old_profile, new_profile, gated),
        _ => true,
    }
}

#[async_trait::async_trait]
impl StackCompatibilityCheck for PermissionProfilesUnchangedCheck {
    fn description(&self) -> &'static str {
        "Permission profiles in the stack shouldn't be modified"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let gated = gated_contributions(old_stack, new_stack);

        // Check if permission profiles have been added, removed, or modified
        let old_profiles = &old_stack.permissions.profiles;
        let new_profiles = &new_stack.permissions.profiles;

        // Check for removed profiles
        for (profile_name, _) in old_profiles {
            if !new_profiles.contains_key(profile_name) {
                errors.push(format!(
                    "Permission profile '{}' was removed from the stack",
                    profile_name
                ));
            }
        }

        // Check for modified or added profiles
        for (profile_name, new_profile) in new_profiles {
            if let Some(old_profile) = old_profiles.get(profile_name) {
                // Profile exists in both - check if it was modified
                if profiles_differ_outside_gates(old_profile, new_profile, &gated) {
                    errors.push(format!(
                        "Permission profile '{}' was modified",
                        profile_name
                    ));
                }
            } else {
                // Profile is new
                warnings.push(format!(
                    "New permission profile '{}' was added",
                    profile_name
                ));
            }
        }

        // Check management permissions
        if management_differs_outside_gates(old_stack.management(), new_stack.management(), &gated)
        {
            errors.push("Management permissions configuration was modified".to_string());
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
    use alien_core::permissions::PermissionsConfig;
    use alien_core::{Kv, ResourceLifecycle};
    use indexmap::IndexMap;

    /// The deployer said no to a gated live resource: its scoped management
    /// grant leaves with it, and the update must not read that as drift.
    #[tokio::test]
    async fn a_gated_resources_management_grants_follow_its_gate() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().resource("cache", ["kv/provision"]),
            ))
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(PermissionProfile::new()))
            .build();

        let toggled_off = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(toggled_off.success, "{:?}", toggled_off.errors);

        let toggled_on = PermissionProfilesUnchangedCheck
            .check(&new_stack, &old_stack)
            .await
            .expect("check should run");
        assert!(toggled_on.success, "{:?}", toggled_on.errors);
    }

    /// The derived management profile carries per-type grants in the '*'
    /// scope; a gated kv leaving takes `kv/…` entries with it, and the check
    /// must read that as the gate's doing, not drift.
    #[tokio::test]
    async fn a_gated_types_global_grants_follow_its_gate() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["kv/provision", "worker/heartbeat"]),
            ))
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["worker/heartbeat"]),
            ))
            .build();

        let toggled_off = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(toggled_off.success, "{:?}", toggled_off.errors);

        let toggled_on = PermissionProfilesUnchangedCheck
            .check(&new_stack, &old_stack)
            .await
            .expect("check should run");
        assert!(toggled_on.success, "{:?}", toggled_on.errors);
    }

    /// An ungated type's global grant changing is real drift even while a
    /// gate exists on another type.
    #[tokio::test]
    async fn an_ungated_types_global_grant_change_still_fails() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["kv/provision", "worker/heartbeat"]),
            ))
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["kv/provision", "storage/provision"]),
            ))
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build();

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    /// A gated resource's scope present on BOTH sides is not a toggle; a
    /// grant escalation inside it must still fail the check, or the gate
    /// becomes a hole in the install-time permission approval.
    #[tokio::test]
    async fn a_grant_escalation_on_a_gated_scope_still_fails() {
        let gated_cache_with = |grants: [&str; 1]| {
            Stack::new("s".to_string())
                .management(ManagementPermissions::Extend(
                    PermissionProfile::new().resource("cache", grants),
                ))
                .add_enabled_when(
                    Kv::new("cache".to_string()).build(),
                    ResourceLifecycle::Live,
                    "cacheEnabled",
                )
                .build()
        };
        let old_stack = gated_cache_with(["kv/provision"]);
        let new_stack = gated_cache_with(["kv/data-write"]);

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    /// The same escalation hidden inside a named profile's gated scope fails
    /// too, while a pure presence toggle of that scope passes.
    #[tokio::test]
    async fn named_profile_gated_scopes_toggle_but_never_escalate() {
        let gated_stack_with_profile = |grants: Option<[&str; 1]>| {
            let profile = match grants {
                Some(grants) => PermissionProfile::new().resource("cache", grants),
                None => PermissionProfile::new(),
            };
            Stack::new("s".to_string())
                .permission("execution", profile)
                .add_enabled_when(
                    Kv::new("cache".to_string()).build(),
                    ResourceLifecycle::Live,
                    "cacheEnabled",
                )
                .build()
        };

        let with_read = gated_stack_with_profile(Some(["kv/data-read"]));
        let with_write = gated_stack_with_profile(Some(["kv/data-write"]));
        let without = gated_stack_with_profile(None);

        let toggled = PermissionProfilesUnchangedCheck
            .check(&with_read, &without)
            .await
            .expect("check should run");
        assert!(toggled.success, "{:?}", toggled.errors);

        let escalated = PermissionProfilesUnchangedCheck
            .check(&with_read, &with_write)
            .await
            .expect("check should run");
        assert!(!escalated.success);
        assert!(escalated.errors[0].contains("'execution'"));
    }

    /// A '*'-scoped DATA grant for a gated type is user-authored, not
    /// gate-derived; adding one must read as drift even while the type has a
    /// gate (defense in depth next to the compile-time wildcard net).
    #[tokio::test]
    async fn a_global_data_grant_never_hides_behind_a_gate() {
        let gated_kv_with_globals = |grants: &[&str]| {
            Stack::new("s".to_string())
                .management(ManagementPermissions::Extend(
                    PermissionProfile::new().global(grants.iter().copied()),
                ))
                .add_enabled_when(
                    Kv::new("cache".to_string()).build(),
                    ResourceLifecycle::Live,
                    "cacheEnabled",
                )
                .build()
        };
        let old_stack = gated_kv_with_globals(&["kv/provision"]);
        let new_stack = gated_kv_with_globals(&["kv/provision", "kv/data-write"]);

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    /// Removing a gate in the new release must not exempt that type's data
    /// grants: the gated-type union spans both sides, and only the derived
    /// management suffixes follow the gate.
    #[tokio::test]
    async fn a_removed_gate_does_not_exempt_new_global_data_grants() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["kv/provision"]),
            ))
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["kv/provision", "kv/data-write"]),
            ))
            .add(Kv::new("cache".to_string()).build(), ResourceLifecycle::Live)
            .build();

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    /// The same management change without a gate is real drift and still fails.
    #[tokio::test]
    async fn an_ungated_management_change_still_fails() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().resource("cache", ["kv/provision"]),
            ))
            .add(Kv::new("cache".to_string()).build(), ResourceLifecycle::Live)
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(PermissionProfile::new()))
            .build();

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    #[tokio::test]
    async fn test_unchanged_profiles_success() {
        let mut profile = PermissionProfile::new();
        profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("worker/execute")],
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), profile.clone());

        let permissions_config = PermissionsConfig {
            profiles: profiles.clone(),
            management: ManagementPermissions::Auto,
        };

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: permissions_config.clone(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: permissions_config,
            supported_platforms: None,
            inputs: vec![],
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_modified_profile_failure() {
        let mut old_profile = PermissionProfile::new();
        old_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("worker/execute")],
        );

        let mut new_profile = PermissionProfile::new();
        new_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("storage/data-read")],
        );

        let mut old_profiles = IndexMap::new();
        old_profiles.insert("test-profile".to_string(), old_profile);

        let mut new_profiles = IndexMap::new();
        new_profiles.insert("test-profile".to_string(), new_profile);

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: old_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: new_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("was modified"));
    }

    #[tokio::test]
    async fn test_added_profile_warning() {
        let old_profiles = IndexMap::new();

        let mut new_profile = PermissionProfile::new();
        new_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("worker/execute")],
        );

        let mut new_profiles = IndexMap::new();
        new_profiles.insert("new-profile".to_string(), new_profile);

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: old_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: new_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(result.success); // Success but with warnings
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("was added"));
    }
}
