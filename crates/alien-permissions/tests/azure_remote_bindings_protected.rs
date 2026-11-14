//! The remote-bindings identity is one the vendor's control plane can use. On a protected install
//! its roles are rendered with the protected filter, so no role it holds carries an action that
//! runs code, changes an identity, opens the install to others or changes the signing trust.

use alien_permissions::{
    generators::{azure_family, AzureRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};

fn context(protected: bool) -> PermissionContext {
    PermissionContext::new()
        .with_subscription_id("00000000-0000-0000-0000-000000000000")
        .with_resource_group("resource-group")
        .with_stack_prefix("prefix")
        .with_managing_subscription_id("00000000-0000-0000-0000-000000000000")
        .with_managing_resource_group("resource-group")
        .with_resource_name("resource")
        .with_resource_id("resource")
        .with_storage_account_name("storageaccount")
        .with_stack_name("stack")
        .with_signing_protected(protected)
}

/// Every set a remote binding publishes except the sandbox one, which protected Azure refuses.
const REMOTE_BINDING_SETS: &[&str] = &[
    "storage/remote-data-write",
    "queue/publish",
    "kv/remote-data-write",
    "key/remote-cryptography",
    "ai/invoke",
];

#[test]
fn a_protected_remote_bindings_role_holds_no_family_action() {
    let generator = AzureRuntimePermissionsGenerator::new();
    for id in REMOTE_BINDING_SETS {
        let set = get_permission_set(id).unwrap_or_else(|| panic!("{id} is not registered"));
        let plan = generator
            .generate_grant_plan(set, BindingTarget::Resource, &context(true))
            .unwrap_or_else(|error| panic!("{id} does not render protected: {error}"));
        for role in &plan.custom_roles {
            let definition = &role.role_definition;
            for action in definition.actions.iter().chain(&definition.data_actions) {
                assert_eq!(
                    azure_family(action),
                    None,
                    "{id}: role '{}' holds {action}",
                    definition.name
                );
            }
        }
    }
}

/// What a remote binding is for is data: the protected rendering keeps every data action the
/// unprotected one has.
#[test]
fn the_protected_filter_leaves_a_remote_bindings_data_grant_whole() {
    let generator = AzureRuntimePermissionsGenerator::new();
    for id in REMOTE_BINDING_SETS {
        let set = get_permission_set(id).expect("registered");
        let render = |protected| {
            generator
                .generate_grant_plan(set, BindingTarget::Resource, &context(protected))
                .expect("renders")
        };
        assert_eq!(render(true), render(false), "{id}");
    }
}

/// A protected install gives the remote-bindings identity the explicit sandbox list, not the
/// predefined Data Owner role: no predefined binding, one custom role holding exactly the list,
/// and none of the identity, reach or run operations. An unprotected install still gets the
/// predefined role.
#[test]
fn a_protected_remote_sandbox_role_is_the_explicit_list_and_not_data_owner() {
    use alien_permissions::generators::{
        azure_sandbox_data::{
            AZURE_SANDBOX_REMOTE_DATA_ACTIONS, AZURE_SANDBOX_REMOTE_DENIED_DATA_ACTIONS,
        },
        AzureRoleDefinitionRef,
    };
    let generator = AzureRuntimePermissionsGenerator::new();
    let set = get_permission_set("sandbox/remote-execute").expect("registered");

    let protected = generator
        .generate_grant_plan(set, BindingTarget::Resource, &context(true))
        .expect("renders protected");
    assert!(
        protected
            .bindings
            .iter()
            .all(|b| matches!(b.role_definition, AzureRoleDefinitionRef::Custom { .. })),
        "{:?}",
        protected.bindings
    );
    assert_eq!(protected.custom_roles.len(), 1);
    let definition = &protected.custom_roles[0].role_definition;
    let mut expected: Vec<String> = AZURE_SANDBOX_REMOTE_DATA_ACTIONS
        .iter()
        .map(|a| a.to_string())
        .collect();
    expected.sort();
    assert_eq!(definition.data_actions, expected);
    assert!(definition.actions.is_empty() && definition.not_data_actions.is_empty());
    for (denied, _) in AZURE_SANDBOX_REMOTE_DENIED_DATA_ACTIONS {
        assert!(
            !definition.data_actions.iter().any(|a| a == denied),
            "{denied}"
        );
    }

    let unprotected = generator
        .generate_grant_plan(set, BindingTarget::Resource, &context(false))
        .expect("renders unprotected");
    assert!(unprotected.custom_roles.is_empty());
    assert!(unprotected
        .bindings
        .iter()
        .all(|b| matches!(b.role_definition, AzureRoleDefinitionRef::Predefined { .. })));
}
