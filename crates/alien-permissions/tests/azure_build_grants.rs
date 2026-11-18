//! The Azure build permission sets: where their grants land, and that they carry every action
//! the build flow calls.

mod common;

use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
};
use common::create_test_context;

const RESOURCE_GROUP: &str =
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod";

const IDENTITY: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.ManagedIdentity/userAssignedIdentities/my-stack-build-sa";
const ENVIRONMENT: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.App/managedEnvironments/my-stack-env";

fn build_context() -> alien_permissions::PermissionContext {
    create_test_context()
        .with_build_identity_id(IDENTITY)
        .with_build_environment_id(ENVIRONMENT)
}

/// The scope each Azure entry of a build set binds at, with the actions it grants there.
fn scoped_actions(
    id: &str,
    target: BindingTarget,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set(id).expect("permission set exists");
    let plan = generator
        .generate_grant_plan(set, target, &build_context())
        .unwrap_or_else(|e| panic!("{id} {target:?}: {e}"));
    // A role is assignable only at a resource group or above (Azure refuses a resource); the
    // assignment is what binds it to one resource.
    for role in &plan.custom_roles {
        for scope in &role.role_definition.assignable_scopes {
            assert_eq!(
                alien_permissions::resource_group_scope_of(scope).as_deref(),
                Some(scope.as_str()),
                "{id} {target:?}: assignable scope {scope} is not a resource group"
            );
        }
    }
    let mut by_scope = std::collections::BTreeMap::new();
    for binding in &plan.bindings {
        let alien_permissions::generators::AzureRoleDefinitionRef::Custom { key } =
            &binding.role_definition
        else {
            continue;
        };
        let role = plan.custom_roles.iter().find(|r| &r.key == key).unwrap();
        let mut actions = role.role_definition.actions.clone();
        actions.sort();
        by_scope.insert(binding.scope.clone(), actions);
    }
    // Every binding names a scope that has a role, and nothing else.
    let mut bound: Vec<String> = plan.bindings.iter().map(|b| b.scope.clone()).collect();
    bound.sort();
    let mut roles: Vec<String> = by_scope.keys().cloned().collect();
    roles.sort();
    assert_eq!(bound, roles, "{id} {target:?}");
    by_scope
}

fn sorted(actions: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = actions.iter().map(|a| a.to_string()).collect();
    v.sort();
    v
}

/// The exact actions of each build set at the exact scope string, for both targets. An extra
/// action, a job scope, or a wider scope fails here and not only a mock.
#[test]
fn build_sets_grant_exactly_these_actions_at_exactly_these_scopes() {
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let expect = |id: &str, entries: Vec<(&str, Vec<String>)>| {
            let want: std::collections::BTreeMap<String, Vec<String>> = entries
                .into_iter()
                .map(|(s, a)| (s.to_string(), a))
                .collect();
            assert_eq!(scoped_actions(id, target), want, "{id} {target:?}");
        };
        expect(
            "build/execute",
            vec![
                (
                    RESOURCE_GROUP,
                    sorted(&[
                        "Microsoft.App/jobs/read",
                        "Microsoft.App/jobs/write",
                        "Microsoft.App/jobs/delete",
                        "Microsoft.App/jobs/start/action",
                        "Microsoft.App/jobs/executions/read",
                    ]),
                ),
                (
                    ENVIRONMENT,
                    sorted(&["Microsoft.App/managedEnvironments/join/action"]),
                ),
                (
                    IDENTITY,
                    sorted(&["Microsoft.ManagedIdentity/userAssignedIdentities/assign/action"]),
                ),
            ],
        );
        expect(
            "build/management",
            vec![(
                RESOURCE_GROUP,
                sorted(&[
                    "Microsoft.App/jobs/read",
                    "Microsoft.App/jobs/write",
                    "Microsoft.App/jobs/executions/read",
                    "Microsoft.App/managedEnvironments/read",
                ]),
            )],
        );
        expect(
            "build/provision",
            vec![(
                RESOURCE_GROUP,
                sorted(&[
                    "Microsoft.App/jobs/write",
                    "Microsoft.App/jobs/delete",
                    "Microsoft.App/managedEnvironments/read",
                    "Microsoft.App/managedEnvironments/write",
                    "Microsoft.App/jobs/read",
                    "Microsoft.App/jobs/executions/read",
                ]),
            )],
        );
    }
}

/// build/heartbeat grants the built-in Reader role at the resource group, nothing custom.
#[test]
fn build_heartbeat_binds_reader_at_the_resource_group() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("build/heartbeat").unwrap();
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let plan = generator
            .generate_grant_plan(set, target, &build_context())
            .unwrap();
        assert!(plan.custom_roles.is_empty(), "{target:?}");
        assert_eq!(plan.bindings.len(), 1, "{target:?}");
        assert_eq!(plan.bindings[0].scope, RESOURCE_GROUP);
        assert_eq!(plan.bindings[0].role_name, "Reader");
    }
}

fn azure_actions(id: &str) -> Vec<String> {
    get_permission_set(id)
        .expect("permission set exists")
        .platforms
        .azure
        .as_ref()
        .expect("azure section")
        .iter()
        .flat_map(|permission| {
            let grant = &permission.grant;
            let all: Vec<String> = grant
                .actions
                .iter()
                .flatten()
                .chain(grant.data_actions.iter().flatten())
                .cloned()
                .collect();
            all
        })
        .collect()
}

/// The actions Azure authorizes for each Container Apps client call the build binding makes.
/// A call missing here fails the test below, so a new call cannot ship without a grant.
fn actions_for_call(method: &str) -> &'static [&'static str] {
    match method {
        // Creating the per-run job under the build's identity, in the environment.
        "create_or_update_job" => &[
            "Microsoft.App/jobs/write",
            "Microsoft.App/managedEnvironments/join/action",
            "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
        ],
        "start_job" => &["Microsoft.App/jobs/start/action"],
        "get_job" => &["Microsoft.App/jobs/read"],
        "get_job_execution" | "list_job_executions" => &["Microsoft.App/jobs/executions/read"],
        "delete_job" => &["Microsoft.App/jobs/delete"],
        other => panic!("aca.rs calls `{other}`: map it to its Azure actions in this test"),
    }
}

#[test]
fn every_azure_call_of_the_build_flow_has_its_action_in_build_execute() {
    let source = include_str!("../../alien-bindings/src/providers/build/aca.rs");
    let mut calls = Vec::new();
    for part in source.split(".client").skip(1) {
        let method: String = part
            .trim_start()
            .trim_start_matches('.')
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        calls.push(method);
    }
    assert!(
        calls.len() >= 4,
        "found no client calls in aca.rs: {calls:?}"
    );

    let granted = azure_actions("build/execute");
    for call in calls {
        for action in actions_for_call(&call) {
            assert!(
                granted.iter().any(|g| g == action),
                "`{call}` needs {action}, absent from build/execute"
            );
        }
    }
}

/// No wildcard and no data or secret action in any build set.
#[test]
fn build_sets_have_no_wildcards_or_data_actions() {
    for id in ["build/execute", "build/management", "build/provision"] {
        for action in azure_actions(id) {
            assert!(!action.contains('*'), "{id}: {action}");
            assert!(
                !action.to_lowercase().contains("listsecrets"),
                "{id}: {action}"
            );
        }
        let set = get_permission_set(id).unwrap();
        for permission in set.platforms.azure.as_ref().unwrap() {
            assert!(
                permission
                    .grant
                    .data_actions
                    .iter()
                    .flatten()
                    .next()
                    .is_none(),
                "{id} grants data actions"
            );
        }
    }
}

/// A stack-wide grant binds the identity assignment once for each Build identity, each with its own
/// role, and a stack with no Build binds none: the resource group and environment entries remain.
#[test]
fn a_stack_wide_grant_binds_assign_once_per_build_identity() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("build/execute").expect("permission set exists");
    let other = IDENTITY.replace("my-stack-build-sa", "my-stack-other-sa");
    let with_two = create_test_context()
        .with_build_environment_id(ENVIRONMENT)
        .with_build_identity_id(IDENTITY)
        .with_build_identity_id(other.clone());
    let plan = generator
        .generate_grant_plan(set, BindingTarget::Stack, &with_two)
        .unwrap();
    let mut scopes: Vec<&str> = plan.bindings.iter().map(|b| b.scope.as_str()).collect();
    scopes.sort();
    let mut want = vec![RESOURCE_GROUP, ENVIRONMENT, IDENTITY, other.as_str()];
    want.sort();
    assert_eq!(scopes, want);
    let mut keys: Vec<&str> = plan.custom_roles.iter().map(|r| r.key.as_str()).collect();
    let count = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), count, "role keys are unique per scope");
    // Azure refuses a second custom role with the same name in a directory, so the names
    // must be as unique as the keys (and so the ids derived from them).
    let mut names: Vec<&str> = plan
        .custom_roles
        .iter()
        .map(|r| r.role_definition.name.as_str())
        .collect();
    names.sort();
    let name_count = names.len();
    names.dedup();
    assert_eq!(names.len(), name_count, "role names are unique per scope");

    // Adding a Build later does not rename the roles of the existing one.
    let only_first = create_test_context()
        .with_build_environment_id(ENVIRONMENT)
        .with_build_identity_id(IDENTITY);
    let first_plan = generator
        .generate_grant_plan(set, BindingTarget::Stack, &only_first)
        .unwrap();
    let first_keys: Vec<&str> = first_plan
        .custom_roles
        .iter()
        .map(|r| r.key.as_str())
        .collect();
    let reversed = create_test_context()
        .with_build_environment_id(ENVIRONMENT)
        .with_build_identity_id(other.clone())
        .with_build_identity_id(IDENTITY);
    let plan = generator
        .generate_grant_plan(set, BindingTarget::Stack, &reversed)
        .unwrap();
    for key in first_keys {
        assert!(
            plan.custom_roles.iter().any(|r| r.key == key),
            "role key {key} survives adding a Build"
        );
    }
}

/// A grant that names the build identity fails when the context has none, instead of dropping the
/// identity assignment.
#[test]
fn a_missing_build_identity_is_an_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("build/execute").expect("permission set exists");
    let context = create_test_context().with_build_environment_id(ENVIRONMENT);
    let err = generator
        .generate_grant_plan(set, BindingTarget::Stack, &context)
        .unwrap_err();
    assert!(err.to_string().contains("buildIdentityId"), "{err}");
}

/// A grant that names the build environment fails when the context has none.
#[test]
fn a_missing_build_environment_is_an_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("build/execute").expect("permission set exists");
    let context = create_test_context().with_build_identity_id(IDENTITY);
    let err = generator
        .generate_grant_plan(set, BindingTarget::Stack, &context)
        .unwrap_err();
    assert!(err.to_string().contains("buildEnvironmentId"), "{err}");
}

const SHARED_GROUP: &str =
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-shared-environments";
const SHARED_ENVIRONMENT: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-shared-environments/providers/Microsoft.App/managedEnvironments/shared-env";

/// A shared Container Apps environment in another resource group: the join grant is assigned on
/// that environment resource and nowhere else in its group, the jobs and the identity assignment
/// stay in the stack's group, and the only extra assignable scope is the environment's group.
#[test]
fn an_environment_in_another_group_is_joined_on_the_environment_only() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("build/execute").unwrap();
    let context = create_test_context()
        .with_build_identity_id(IDENTITY)
        .with_build_environment_id(SHARED_ENVIRONMENT);
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let plan = generator
            .generate_grant_plan(set, target, &context)
            .unwrap();
        let mut bound: Vec<&str> = plan.bindings.iter().map(|b| b.scope.as_str()).collect();
        bound.sort();
        let mut want = vec![RESOURCE_GROUP, SHARED_ENVIRONMENT, IDENTITY];
        want.sort();
        assert_eq!(
            bound, want,
            "{target:?}: nothing is assigned on the shared group"
        );
        for binding in &plan.bindings {
            let alien_permissions::generators::AzureRoleDefinitionRef::Custom { key } =
                &binding.role_definition
            else {
                panic!("custom roles only");
            };
            let role = plan.custom_roles.iter().find(|r| &r.key == key).unwrap();
            let expected_group = if binding.scope == SHARED_ENVIRONMENT {
                SHARED_GROUP
            } else {
                RESOURCE_GROUP
            };
            assert_eq!(
                role.role_definition.assignable_scopes,
                vec![expected_group.to_string()],
                "{target:?} {}",
                binding.scope
            );
            if binding.scope != SHARED_ENVIRONMENT {
                // No job right outside the stack's group.
                assert!(!role
                    .role_definition
                    .assignable_scopes
                    .iter()
                    .any(|s| s == SHARED_GROUP));
            } else {
                assert_eq!(
                    role.role_definition.actions,
                    vec!["Microsoft.App/managedEnvironments/join/action".to_string()]
                );
            }
        }
    }
}

/// In the group a role is written in, it is assignable there first and in the shared group too.
#[test]
fn a_role_is_written_in_the_stack_group_and_assignable_in_the_shared_one() {
    let scopes = alien_permissions::generators::role_definition_assignable_scopes(
        &[SHARED_GROUP.to_string()],
        RESOURCE_GROUP,
    );
    assert_eq!(
        scopes,
        vec![RESOURCE_GROUP.to_string(), SHARED_GROUP.to_string()]
    );
    // The same group is not listed twice.
    let scopes = alien_permissions::generators::role_definition_assignable_scopes(
        &[RESOURCE_GROUP.to_lowercase()],
        RESOURCE_GROUP,
    );
    assert_eq!(scopes, vec![RESOURCE_GROUP.to_string()]);
}
