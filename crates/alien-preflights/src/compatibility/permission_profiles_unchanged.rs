use crate::compatibility::narrowing::management_narrowed;
use crate::error::Result;
use crate::mutations::management_permission_profile::{
    GATE_DERIVED_GLOBAL_SUFFIXES, KEY_AVAILABILITY_PERMISSION_SET_ID,
    replica_secret_existence, secrets_guard, NATIVE_VOLUME_MANAGEMENT_PERMISSION_SET_ID,
};
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{Key, Stack};
use alien_permissions::{MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD};
use std::collections::HashSet;

/// Validates that permission profiles in the stack haven't been modified.
///
/// A workload profile change, including one that only removes grants or the whole profile,
/// requires setup: setup owns the identities and rewrites their grants. Management permissions
/// may still narrow without setup: the update needs the broader installed grants to delete what
/// the removed ones covered (see [`crate::compatibility::narrowing`]).
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

/// `new_management` must already be without the migration grants
/// ([`without_added_migration_grants`]).
fn management_differs_outside_gates(
    old_stack: &Stack,
    new_stack: &Stack,
    old_management: &ManagementPermissions,
    new_management: &ManagementPermissions,
    gated: &GatedContributions,
    allow_email_heartbeat_migration: bool,
) -> bool {
    match (old_management, new_management) {
        (ManagementPermissions::Auto, ManagementPermissions::Auto) => false,
        (
            ManagementPermissions::Extend(old_profile),
            ManagementPermissions::Extend(new_profile),
        ) => {
            profiles_differ_outside_gates(old_profile, new_profile, gated)
                && !(allow_email_heartbeat_migration
                    && is_email_heartbeat_registration_migration(
                        old_stack,
                        new_stack,
                        old_profile,
                        new_profile,
                        gated,
                    ))
        }
        (
            ManagementPermissions::Override(old_profile),
            ManagementPermissions::Override(new_profile),
        ) => profiles_differ_outside_gates(old_profile, new_profile, gated),
        _ => true,
    }
}

/// The new management permissions without the grants preparation now adds to every existing
/// deployment, so the first update after they were introduced runs without setup:
///
/// - the role guards: each is a Deny that only narrows the identity. Only the canonical
///   references at stack scope.
/// - `key/availability` on a Key the installed stack already has. The Key controller uses it only
///   when setup reports that it installed it (`availabilityProbe` in the key's import data), so a
///   setup that predates it keeps its metadata-only health check until setup is rerun. Only the
///   canonical reference in that key's own scope.
/// - the canonical AWS sandbox secrets Deny (a Deny only narrows) and the replica secret's
///   existence check, which the sandbox controller treats as "no replica secret yet" while
///   the installed role lacks it. Stack scope only.
/// - `key/native-volume-management` on a Key whose installed scope holds `key/native-volume`:
///   the same EC2 key use without `kms:Decrypt`, so the installed role already covers it.
///
/// Anything else still reads as drift, and a grant the old profile held may not leave.
fn without_added_migration_grants(
    old_stack: &Stack,
    old: &ManagementPermissions,
    new: &ManagementPermissions,
) -> ManagementPermissions {
    let old_profile = match old {
        ManagementPermissions::Auto => None,
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            Some(profile)
        }
    };
    let old_scope = |scope: &str| old_profile.and_then(|profile| profile.0.get(scope));
    let added_guards: Vec<PermissionSetReference> =
        [MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD]
            .into_iter()
            .map(PermissionSetReference::from_name)
            .filter(|guard| !old_scope("*").is_some_and(|grants| grants.contains(guard)))
            .collect();
    let key_availability = PermissionSetReference::from_name(KEY_AVAILABILITY_PERMISSION_SET_ID);
    let keys_gaining_availability: Vec<&str> = old_stack
        .resources()
        .filter(|(_, entry)| entry.config.resource_type() == Key::RESOURCE_TYPE)
        .map(|(id, _)| id.as_str())
        .filter(|id| !old_scope(id).is_some_and(|grants| grants.contains(&key_availability)))
        .collect();
    let strip = |profile: &mut PermissionProfile, scope: &str, added: &[PermissionSetReference]| {
        if let Some(grants) = profile.0.get_mut(scope) {
            grants.retain(|grant| !added.contains(grant));
            if grants.is_empty() && old_scope(scope).is_none() {
                profile.0.shift_remove(scope);
            }
        }
    };
    // Only the canonical grants preparation authors are exempt: an author's grant reusing one of
    // these ids may ask for anything, so it is compared like any other grant.
    let added_canonical = |scope: &str, grant: Option<PermissionSetReference>| {
        grant
            .filter(|grant| !old_scope(scope).is_some_and(|grants| grants.contains(grant)))
            .into_iter()
            .collect::<Vec<_>>()
    };
    let sandbox_secrets = [secrets_guard().ok(), replica_secret_existence().ok()]
        .into_iter()
        .flat_map(|grant| added_canonical("*", grant))
        .collect::<Vec<_>>();
    let installed_volume_use = PermissionSetReference::from_name("key/native-volume");
    let volume_management =
        PermissionSetReference::from_name(NATIVE_VOLUME_MANAGEMENT_PERMISSION_SET_ID);
    let keys_with_installed_volume_use: Vec<&String> = old_profile
        .map(|profile| {
            profile
                .0
                .iter()
                .filter(|(_, grants)| grants.contains(&installed_volume_use))
                .map(|(scope, _)| scope)
                .collect()
        })
        .unwrap_or_default();
    let without = |profile: &PermissionProfile| {
        let mut profile = profile.clone();
        strip(&mut profile, "*", &added_guards);
        strip(&mut profile, "*", &sandbox_secrets);
        for scope in &keys_with_installed_volume_use {
            strip(&mut profile, scope, std::slice::from_ref(&volume_management));
        }
        for key in &keys_gaining_availability {
            strip(&mut profile, key, std::slice::from_ref(&key_availability));
        }
        profile
    };
    match new {
        ManagementPermissions::Auto => ManagementPermissions::Auto,
        // A profile the mutation left `Auto` becomes `Extend` only to hold these grants.
        ManagementPermissions::Extend(profile) => {
            let profile = without(profile);
            if profile.0.is_empty() && matches!(old, ManagementPermissions::Auto) {
                ManagementPermissions::Auto
            } else {
                ManagementPermissions::Extend(profile)
            }
        }
        ManagementPermissions::Override(profile) => {
            ManagementPermissions::Override(without(profile))
        }
    }
}

/// Accepts the one-way profile migration caused by registering the formerly
/// unresolved built-in Email heartbeat permission. Existing prepared stacks
/// omitted this reference because unresolved built-ins are skipped. The first
/// update after registration therefore adds it even when application code did
/// not change.
///
/// This is deliberately narrow: both stack versions must contain Email, the
/// old profile must lack the reference, the new profile must contain the
/// canonical named reference, and removing only that reference must make the
/// profiles otherwise identical. Removal and inline substitutions still fail.
fn is_email_heartbeat_registration_migration(
    old_stack: &Stack,
    new_stack: &Stack,
    old_profile: &PermissionProfile,
    new_profile: &PermissionProfile,
    gated: &GatedContributions,
) -> bool {
    const PERMISSION_ID: &str = "email/heartbeat";
    let contains_email = |stack: &Stack| {
        stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type().as_ref() == "email")
    };
    if !contains_email(old_stack) || !contains_email(new_stack) {
        return false;
    }

    let canonical = PermissionSetReference::from_name(PERMISSION_ID);
    let old_global = old_profile.0.get("*");
    let new_global = new_profile.0.get("*");
    if old_global.is_some_and(|grants| grants.iter().any(|grant| grant.id() == PERMISSION_ID))
        || !new_global.is_some_and(|grants| grants.iter().any(|grant| grant == &canonical))
    {
        return false;
    }

    let mut migrated_profile = new_profile.clone();
    if let Some(grants) = migrated_profile.0.get_mut("*") {
        grants.retain(|grant| grant != &canonical);
    }
    !profiles_differ_outside_gates(old_profile, &migrated_profile, gated)
}

#[async_trait::async_trait]
impl StackCompatibilityCheck for PermissionProfilesUnchangedCheck {
    fn description(&self) -> &'static str {
        "Permission profiles in the stack shouldn't be modified"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        check_permission_profiles(old_stack, new_stack, false, true)
    }

    async fn check_with_config(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: &alien_core::DeploymentConfig,
    ) -> Result<CheckResult> {
        check_permission_profiles(
            old_stack,
            new_stack,
            config.stack_settings.heartbeats.is_enabled(),
            true,
        )
    }
}

impl PermissionProfilesUnchangedCheck {
    /// Compares without the narrowing allowance: any change, including one
    /// that only removes grants, fails. For callers deciding what setup
    /// installs rather than whether an update may run with it.
    pub fn check_exact(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        check_permission_profiles(old_stack, new_stack, false, false)
    }
}

fn check_permission_profiles(
    old_stack: &Stack,
    new_stack: &Stack,
    allow_email_heartbeat_migration: bool,
    allow_narrowing: bool,
) -> Result<CheckResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let gated = gated_contributions(old_stack, new_stack);

    // Check if permission profiles have been added, removed, or modified
    let old_profiles = &old_stack.permissions.profiles;
    let new_profiles = &new_stack.permissions.profiles;

    // A removed profile's identity is setup-owned: setup removes it.
    for (profile_name, _) in old_profiles {
        if !new_profiles.contains_key(profile_name) {
            errors.push(format!(
                "Permission profile '{}' was removed from the stack. Rerun setup with the \
                 updated stack to remove its identity",
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

    // Check management permissions. Both the comparison and the narrowing allowance see the new
    // profile without the migration grants, so an update that adds one and removes another
    // grant still only narrows.
    let new_management =
        without_added_migration_grants(old_stack, old_stack.management(), new_stack.management());
    if management_differs_outside_gates(
        old_stack,
        new_stack,
        old_stack.management(),
        &new_management,
        &gated,
        allow_email_heartbeat_migration,
    ) && !(allow_narrowing && management_narrowed(old_stack.management(), &new_management))
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::SECRETS_VAULT_ID;
    use crate::mutations::management_permission_profile::ManagementPermissionProfileMutation;
    use crate::StackMutation;
    use alien_core::permissions::PermissionsConfig;
    use alien_core::{Email, Kv, ResourceLifecycle};
    use alien_core::{Platform, StackState};
    use indexmap::IndexMap;

    /// The management profile an AWS internet-sandbox stack was prepared with before management
    /// stopped reading the secrets vault, and the one it is prepared with now.
    fn secrets_vault_management(installed: bool) -> ManagementPermissions {
        let global = vec![PermissionSetReference::from_name("sandbox/management")];
        let profile = if installed {
            PermissionProfile::new()
                .global(global)
                .resource(SECRETS_VAULT_ID, ["vault/data-read", "vault/data-write"])
        } else {
            let mut global = global;
            global.push(secrets_guard().unwrap());
            global.push(replica_secret_existence().unwrap());
            PermissionProfile::new()
                .global(global)
                .resource(SECRETS_VAULT_ID, ["vault/data-write"])
        };
        ManagementPermissions::Extend(profile)
    }

    /// An installed setup granted management `vault/data-read` on the whole secrets vault. The
    /// new preparation drops it, adds a Deny on the vault and the replica secret, and an
    /// existence check the controller tolerates missing: the update runs before setup is rerun.
    #[tokio::test]
    async fn narrowing_the_secrets_vault_read_does_not_need_setup() {
        assert!(
            update_passes(
                secrets_vault_management(true),
                secrets_vault_management(false)
            )
            .await
        );
    }

    /// An author's grant that reuses the existence check's id but asks for more is not the
    /// canonical grant, so it still needs setup.
    #[tokio::test]
    async fn a_grant_reusing_the_existence_check_id_still_needs_setup() {
        let mut broader = secrets_vault_management(false);
        if let ManagementPermissions::Extend(profile) = &mut broader {
            let set: alien_core::permissions::PermissionSet = serde_json::from_value(
                serde_json::json!({
                    "id": "sandbox/egress-replica-secret-exists",
                    "description": "not the canonical check",
                    "platforms": { "aws": [{
                        "grant": { "actions": ["secretsmanager:DescribeSecret", "secretsmanager:GetSecretValue"] },
                        "binding": { "stack": { "resources": ["*"] } }
                    }] }
                }),
            )
            .unwrap();
            let global = profile.0.get_mut("*").unwrap();
            global.retain(|grant| grant.id() != "sandbox/egress-replica-secret-exists");
            global.push(PermissionSetReference::from_inline(set));
        }
        assert!(!update_passes(secrets_vault_management(true), broader).await);
    }

    /// Management's volume-key grant moves from `key/native-volume` to the same EC2 use
    /// without `kms:Decrypt`. The installed role covers it, so the update runs before setup;
    /// adding it where no volume key was installed still needs setup.
    #[tokio::test]
    async fn the_volume_key_grant_narrows_without_setup() {
        let profile = |sets: &[&str]| {
            ManagementPermissions::Extend(
                PermissionProfile::new()
                    .resource("cell-key", sets.iter().map(|set| set.to_string())),
            )
        };
        assert!(
            update_passes(
                profile(&["key/management", "key/native-volume"]),
                profile(&["key/management", NATIVE_VOLUME_MANAGEMENT_PERMISSION_SET_ID]),
            )
            .await
        );
        assert!(
            !update_passes(
                profile(&["key/management"]),
                profile(&["key/management", NATIVE_VOLUME_MANAGEMENT_PERMISSION_SET_ID]),
            )
            .await
        );
    }

    /// The gateway's replica-secret read is a grant its installed identity lacks: the update
    /// waits for setup, which also creates the secret the read names.
    #[tokio::test]
    async fn the_gateway_gaining_the_replica_secret_read_needs_setup() {
        let gateway = |grants: Vec<PermissionSetReference>| {
            let mut stack = guarded_sandbox_stack(ManagementPermissions::Auto);
            let mut profile = PermissionProfile::new();
            profile.0.insert("*".to_string(), grants);
            stack.permissions.profiles.insert(
                alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.to_string(),
                profile,
            );
            stack
        };
        let config_read = PermissionSetReference::from_name("vault/data-read");
        let read = PermissionSetReference::from_inline(
            crate::mutations::sandbox_gateway_permissions::replica_secret_read().unwrap(),
        );
        let result = PermissionProfilesUnchangedCheck
            .check(
                &gateway(vec![config_read.clone()]),
                &gateway(vec![config_read, read]),
            )
            .await
            .expect("check should run");
        assert!(!result.success);
    }

    fn guarded_sandbox_stack(management: ManagementPermissions) -> Stack {
        Stack::new("s".to_string())
            .management(management)
            .add(
                alien_core::Sandbox::new("agents".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: "s3://acme/sandbox-bundle/f00d/bundle.zip".to_string(),
                    })
                    .egress(alien_core::SandboxEgress::Internet)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    async fn update_passes(old: ManagementPermissions, new: ManagementPermissions) -> bool {
        let result = PermissionProfilesUnchangedCheck
            .check(&guarded_sandbox_stack(old), &guarded_sandbox_stack(new))
            .await
            .expect("check should run");
        result.success
    }

    /// A stack as the preparing mutation leaves it.
    async fn prepared(stack: Stack) -> Stack {
        let config = alien_core::DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        ManagementPermissionProfileMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("the mutation runs")
    }

    /// `prepared` as it read before the preparing mutation added `guards`.
    fn without_guards(
        prepared: &Stack,
        original: &ManagementPermissions,
        guards: &[&str],
    ) -> Stack {
        let guards: Vec<_> = guards
            .iter()
            .map(|guard| PermissionSetReference::from_name(*guard))
            .collect();
        let mut stack = prepared.clone();
        if let ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) =
            &mut stack.permissions.management
        {
            if let Some(grants) = profile.0.get_mut("*") {
                grants.retain(|grant| !guards.contains(grant));
                if grants.is_empty() {
                    profile.0.shift_remove("*");
                }
            }
        }
        let emptied = matches!(
            &stack.permissions.management,
            ManagementPermissions::Extend(profile) if profile.0.is_empty()
        );
        if emptied && matches!(original, ManagementPermissions::Auto) {
            stack.permissions.management = ManagementPermissions::Auto;
        }
        stack
    }

    /// A sandbox deployment prepared before either guard existed gains both on its next update.
    #[tokio::test]
    async fn an_update_may_add_both_role_guards_to_a_sandbox_stack() {
        for management in [
            ManagementPermissions::Auto,
            ManagementPermissions::Extend(PermissionProfile::new().global(["kv/management"])),
            ManagementPermissions::Override(PermissionProfile::new().global(["kv/management"])),
        ] {
            let now = prepared(guarded_sandbox_stack(management.clone())).await;
            let before = without_guards(
                &now,
                &management,
                &[MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD],
            );
            let result = PermissionProfilesUnchangedCheck
                .check(&before, &now)
                .await
                .expect("check should run");
            assert!(result.success, "{management:?}: {:?}", result.errors);
        }
    }

    /// An AWS deployment prepared before the guard existed gains it on its next update, whatever
    /// the profile's mode and the stack's resources.
    #[tokio::test]
    async fn an_update_may_add_the_management_role_guard() {
        let mut stacks = Vec::new();
        for management in [
            ManagementPermissions::Auto,
            ManagementPermissions::Extend(PermissionProfile::new().global(["kv/management"])),
            ManagementPermissions::Override(PermissionProfile::new().global(["kv/management"])),
        ] {
            stacks.push((
                management.clone(),
                guarded_sandbox_stack(management.clone()),
            ));
            let kv_only = Stack::new("s".to_string())
                .management(management.clone())
                .add(
                    Kv::new("cache".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            stacks.push((management.clone(), kv_only));
            let empty = Stack::new("s".to_string())
                .management(management.clone())
                .build();
            stacks.push((management, empty));
        }

        for (management, stack) in stacks {
            let now = prepared(stack).await;
            let result = PermissionProfilesUnchangedCheck
                .check(
                    &without_guards(&now, &management, &[MANAGEMENT_ROLE_GUARD]),
                    &now,
                )
                .await
                .expect("check should run");
            assert!(
                result.success,
                "{management:?} gaining the guard in {:?}: {:?}",
                now.management(),
                result.errors
            );
        }
    }

    #[tokio::test]
    async fn an_update_may_not_remove_a_role_guard() {
        for guard in [MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD] {
            assert!(
                !update_passes(
                    ManagementPermissions::Extend(PermissionProfile::new().global([guard])),
                    ManagementPermissions::Extend(PermissionProfile::new()),
                )
                .await,
                "{guard}"
            );
        }
    }

    #[tokio::test]
    async fn the_guard_does_not_carry_another_grant_in_with_it() {
        assert!(
            !update_passes(
                ManagementPermissions::Extend(PermissionProfile::new()),
                ManagementPermissions::Extend(
                    PermissionProfile::new().global([MANAGEMENT_ROLE_GUARD, "sandbox/execute"])
                ),
            )
            .await
        );
    }

    #[tokio::test]
    async fn the_guard_is_exempt_only_at_stack_scope() {
        assert!(
            !update_passes(
                ManagementPermissions::Extend(PermissionProfile::new()),
                ManagementPermissions::Extend(
                    PermissionProfile::new().resource("agents", [MANAGEMENT_ROLE_GUARD])
                ),
            )
            .await
        );
    }

    /// The deployer said no to a gated live resource: its scoped management
    /// grant leaves with it, and the update must not read that as drift.
    fn key_stack(keys: &[&str], management: ManagementPermissions) -> Stack {
        keys.iter()
            .fold(
                Stack::new("s".to_string()).management(management),
                |stack, key| {
                    stack.add(Key::new(key.to_string()).build(), ResourceLifecycle::Frozen)
                },
            )
            .build()
    }

    async fn key_update_passes(
        old_keys: &[&str],
        old: PermissionProfile,
        new_keys: &[&str],
        new: PermissionProfile,
    ) -> bool {
        PermissionProfilesUnchangedCheck
            .check(
                &key_stack(old_keys, ManagementPermissions::Extend(old)),
                &key_stack(new_keys, ManagementPermissions::Extend(new)),
            )
            .await
            .expect("check should run")
            .success
    }

    /// Setups from before the availability probe have only the key's metadata grant.
    #[tokio::test]
    async fn an_installed_key_may_gain_its_availability_probe() {
        assert!(
            key_update_passes(
                &["customer-key"],
                PermissionProfile::new().resource("customer-key", ["key/management"]),
                &["customer-key"],
                PermissionProfile::new()
                    .resource("customer-key", ["key/availability", "key/management"]),
            )
            .await
        );
    }

    /// The first update after the probe existed may also drop another grant, such as a removed
    /// Live table's `kv/provision`.
    #[tokio::test]
    async fn gaining_the_probe_while_dropping_another_grant_still_only_narrows() {
        assert!(
            key_update_passes(
                &["customer-key"],
                PermissionProfile::new()
                    .global(["kv/provision"])
                    .resource("customer-key", ["key/management"]),
                &["customer-key"],
                PermissionProfile::new()
                    .resource("customer-key", ["key/availability", "key/management"]),
            )
            .await
        );
    }

    #[tokio::test]
    async fn the_availability_probe_is_exempt_only_on_an_installed_keys_scope() {
        let installed = PermissionProfile::new().resource("customer-key", ["key/management"]);
        for new in [
            installed.clone().global(["key/availability"]),
            installed
                .clone()
                .resource("other-key", ["key/availability"]),
        ] {
            assert!(
                !key_update_passes(
                    &["customer-key"],
                    installed.clone(),
                    &["customer-key"],
                    new.clone()
                )
                .await,
                "{new:?}"
            );
        }
        // A Key the installed stack doesn't have is new setup-owned IAM.
        assert!(
            !key_update_passes(
                &["customer-key"],
                installed.clone(),
                &["customer-key", "other-key"],
                installed.resource("other-key", ["key/availability"]),
            )
            .await
        );
    }

    #[tokio::test]
    async fn an_installed_availability_probe_may_not_be_swapped_for_another_grant() {
        assert!(
            !key_update_passes(
                &["customer-key"],
                PermissionProfile::new()
                    .resource("customer-key", ["key/availability", "key/management"]),
                &["customer-key"],
                PermissionProfile::new()
                    .resource("customer-key", ["key/management", "key/native-volume"]),
            )
            .await
        );
    }

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
            .add(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
            )
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
            .add(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().resource("cache", ["kv/provision", "kv/data-write"]),
            ))
            .add(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");
        assert!(!result.success);
        assert!(result.errors[0].contains("Management permissions"));
    }

    /// Management grants may narrow before setup: the update still needs the installed ones to
    /// delete what they covered. A workload profile that is removed or loses grants needs setup.
    #[tokio::test]
    async fn only_management_narrows_without_setup() {
        let kv_stack = |grants: &[&str]| {
            Stack::new("s".to_string())
                .management(ManagementPermissions::Extend(
                    PermissionProfile::new().resource("cache", grants.iter().copied()),
                ))
                .add(
                    Kv::new("cache".to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .build()
        };
        let narrowed = PermissionProfilesUnchangedCheck
            .check(
                &kv_stack(&["kv/provision", "kv/data-write"]),
                &kv_stack(&["kv/provision"]),
            )
            .await
            .expect("check should run");
        assert!(narrowed.success, "{:?}", narrowed.errors);

        let mut old_profiles = IndexMap::new();
        old_profiles.insert(
            "worker".to_string(),
            PermissionProfile::new().global(["worker/execute"]),
        );
        let mut with_profile = Stack::new("s".to_string()).build();
        with_profile.permissions.profiles = old_profiles;
        let without_profile = Stack::new("s".to_string()).build();
        let removed = PermissionProfilesUnchangedCheck
            .check(&with_profile, &without_profile)
            .await
            .expect("check should run");
        assert!(!removed.success);
        assert!(removed.errors[0].contains("'worker' was removed"), "{:?}", removed.errors);

        let mut narrowed_profiles = IndexMap::new();
        narrowed_profiles.insert("worker".to_string(), PermissionProfile::new());
        let mut narrowed_profile = Stack::new("s".to_string()).build();
        narrowed_profile.permissions.profiles = narrowed_profiles;
        let narrowed = PermissionProfilesUnchangedCheck
            .check(&with_profile, &narrowed_profile)
            .await
            .expect("check should run");
        assert!(!narrowed.success);
        assert!(narrowed.errors[0].contains("'worker' was modified"), "{:?}", narrowed.errors);
    }

    #[tokio::test]
    async fn newly_registered_email_heartbeat_permission_migrates_existing_stacks() {
        let stack_with_management = |grants: &[&str]| {
            Stack::new("s".to_string())
                .management(ManagementPermissions::Extend(
                    PermissionProfile::new().global(grants.iter().copied()),
                ))
                .add(
                    Email::new("mail".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
        };
        let old_stack = stack_with_management(&["storage/heartbeat"]);
        let new_stack = stack_with_management(&["email/heartbeat", "storage/heartbeat"]);

        let migration = check_permission_profiles(&old_stack, &new_stack, true, true)
            .expect("check should run");
        assert!(migration.success, "{:?}", migration.errors);

        // Dropping the grant only narrows, which never needs setup.
        let removal = check_permission_profiles(&new_stack, &old_stack, true, true)
            .expect("check should run");
        assert!(removal.success, "{:?}", removal.errors);

        let explicit_grant_with_heartbeats_disabled =
            check_permission_profiles(&old_stack, &new_stack, false, true)
                .expect("check should run");
        assert!(!explicit_grant_with_heartbeats_disabled.success);
    }

    #[tokio::test]
    async fn an_observe_grant_cannot_enter_management() {
        let old_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["worker/provision"]),
            ))
            .build();
        let new_stack = Stack::new("s".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["observe/observe", "worker/provision"]),
            ))
            .build();

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert_eq!(
            result.errors,
            vec!["Management permissions configuration was modified"]
        );
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn observe_addition_to_a_named_profile_is_rejected() {
        let old_profile = PermissionProfile::new().global(["worker/execute"]);
        let new_profile = PermissionProfile::new().global(["observe/observe", "worker/execute"]);

        let mut old_profiles = IndexMap::new();
        old_profiles.insert("application".to_string(), old_profile);
        let mut new_profiles = IndexMap::new();
        new_profiles.insert("application".to_string(), new_profile);

        let old_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "s".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: old_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };
        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "s".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: new_profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let result = PermissionProfilesUnchangedCheck
            .check(&old_stack, &new_stack)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert_eq!(
            result.errors,
            vec!["Permission profile 'application' was modified"]
        );
        assert!(result.warnings.is_empty());
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: permissions_config.clone(),
            supported_platforms: None,
            inputs: vec![],
        };

        let new_stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
