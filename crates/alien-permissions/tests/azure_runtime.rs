mod common;

use alien_core::PermissionGrant;
use alien_permissions::generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator};
use alien_permissions::{get_permission_set, BindingTarget};
use common::*;
use insta::assert_json_snapshot;
use rstest::rstest;

#[rstest]
#[case::stack_binding(
    BindingTarget::Stack,
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod"
)]
#[case::resource_binding(
    BindingTarget::Resource,
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.Storage/storageAccounts/stcxpaymentsprod"
)]
fn test_azure_predefined_grant_plan(
    #[case] binding_target: BindingTarget,
    #[case] expected_scope: &str,
) {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_grant_plan(&permission_set, binding_target, &context)
        .expect("Should generate Azure grant plan successfully");

    assert!(result.custom_roles.is_empty());
    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].permission_set_id, "storage/data-read");
    assert_eq!(result.bindings[0].role_name, "Storage Blob Data Reader");
    assert_eq!(result.bindings[0].scope, expected_scope);
    assert_eq!(
        result.bindings[0].role_definition,
        AzureRoleDefinitionRef::Predefined {
            role_definition_id: "/subscriptions/00000000-0000-0000-0000-000000000000/providers/Microsoft.Authorization/roleDefinitions/2a2b9908-6ea1-4ae2-8e65-a410df84e7d1".to_string(),
        }
    );
}

#[test]
fn test_azure_custom_grant_plan() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_custom_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure grant plan successfully");

    assert_eq!(result.custom_roles.len(), 1);
    assert_eq!(result.bindings.len(), 1);
    assert_eq!(
        result.custom_roles[0].key,
        "storage/metadata-read:microsoft_storage_storage_accounts_read"
    );
    assert_eq!(
        result.custom_roles[0].role_definition.actions,
        vec!["Microsoft.Storage/storageAccounts/read"]
    );
    assert_eq!(
        result.bindings[0].role_definition,
        AzureRoleDefinitionRef::Custom {
            key: "storage/metadata-read:microsoft_storage_storage_accounts_read".to_string(),
        }
    );
}

#[test]
fn test_azure_observe_generates_subscription_scoped_read_grant_plan() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("observe/observe").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_grant_plan(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure observe grant plan successfully");

    assert_json_snapshot!("azure_observe_subscription_scoped_read_grant_plan", result);
}

#[test]
fn compute_cluster_execute_does_not_read_workload_secrets() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/execute").expect("permission set exists");
    let context = create_test_context()
        .with_managing_subscription_id("00000000-0000-0000-0000-000000000000")
        .with_managing_resource_group("rg-observability-prod");

    let result = generator
        .generate_grant_plan(permission_set, BindingTarget::Stack, &context)
        .expect("compute cluster execute grant plan should generate");

    assert!(result
        .bindings
        .iter()
        .all(|binding| binding.role_name != "Key Vault Secrets User"));
    assert!(result.custom_roles.iter().all(|role| !role
        .role_definition
        .data_actions
        .iter()
        .any(|action| action == "Microsoft.KeyVault/vaults/secrets/read")));
}

#[test]
fn compute_cluster_lifecycle_can_discover_subscription_sku_availability() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let context = create_test_context();

    for permission_set_id in [
        "compute-cluster/provision",
        "compute-cluster/management",
        "compute-cluster/heartbeat",
    ] {
        let permission_set = get_permission_set(permission_set_id).expect("permission set exists");
        let result = generator
            .generate_grant_plan(permission_set, BindingTarget::Stack, &context)
            .expect("compute cluster grant plan should generate");

        let sku_role = result
            .custom_roles
            .iter()
            .find(|role| {
                role.role_definition
                    .actions
                    .iter()
                    .any(|action| action == "Microsoft.Compute/skus/read")
            })
            .expect("SKU discovery must have a custom role");
        assert_eq!(
            sku_role.role_definition.assignable_scopes,
            ["/subscriptions/00000000-0000-0000-0000-000000000000"]
        );
        let sku_binding = result
            .bindings
            .iter()
            .find(|binding| {
                binding.role_definition
                    == AzureRoleDefinitionRef::Custom {
                        key: sku_role.key.clone(),
                    }
            })
            .expect("SKU discovery role must be assigned");
        assert_eq!(
            sku_binding.scope,
            "/subscriptions/00000000-0000-0000-0000-000000000000"
        );
    }
}

#[test]
fn test_azure_hybrid_grant_plan() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_hybrid_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure grant plan successfully");

    assert_eq!(result.custom_roles.len(), 1);
    assert_eq!(result.bindings.len(), 2);
    assert!(matches!(
        result.bindings[0].role_definition,
        AzureRoleDefinitionRef::Predefined { .. }
    ));
    assert_eq!(
        result.bindings[1].role_definition,
        AzureRoleDefinitionRef::Custom {
            key: "artifact-registry/provision:microsoft_container_registry_registries_write_permissions"
                .to_string(),
        }
    );
}

#[test]
fn test_azure_role_definition_generation_for_residual_custom_role() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_custom_permission_set();
    permission_set.id = "complex/permission-set/with-dashes".to_string();
    let context = create_test_context();

    let result = generator
        .generate_role_definition(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure role definition successfully");

    assert_eq!(result.name, "Complex Permission Set With Dashes (my-stack)");
    assert!(result.is_custom);
    assert_eq!(
        result.actions,
        vec!["Microsoft.Storage/storageAccounts/read"]
    );
    assert!(result.data_actions.is_empty());
}

#[test]
fn test_azure_missing_platform_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_aws_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator.generate_grant_plan(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Platform 'azure' is not supported"));
}

#[test]
fn test_azure_missing_binding_target_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    if let Some(azure_permissions) = &mut permission_set.platforms.azure {
        azure_permissions[0].binding.resource = None;
    }
    let context = create_test_context();

    let result = generator.generate_grant_plan(&permission_set, BindingTarget::Resource, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Binding target 'resource' is not supported"));
}

#[test]
fn test_azure_empty_grant_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    if let Some(azure_permissions) = &mut permission_set.platforms.azure {
        azure_permissions[0].grant = PermissionGrant {
            actions: None,
            permissions: None,
            predefined_roles: None,
            residual_permissions: None,
            data_actions: None,
        };
    }
    let context = create_test_context();

    let result = generator.generate_grant_plan(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("has no predefined role or residual actions"));
}

#[test]
fn test_azure_empty_list_yields_empty_plan() {
    // postgres/data-access ships an empty `azure` list by design; the runtime generator must contribute
    // an empty plan, not fail-fast — otherwise an Azure deployment linking Postgres errors even
    // though the Terraform emitter skips it.
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    permission_set.id = "postgres/data-access".to_string();
    permission_set.platforms.azure = Some(Vec::new());
    let context = create_test_context();

    let result = generator
        .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
        .expect("empty azure list should yield an empty plan, not an error");

    assert!(result.custom_roles.is_empty());
    assert!(result.bindings.is_empty());
}

#[test]
fn test_azure_role_definition_rejects_empty_azure_list() {
    // Parity with generate_grant_plan: an empty `azure` list has no role to emit here, so the
    // single-role path must fail with a specific "grants nothing" error, not the generic "no
    // residual actions".
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    permission_set.id = "postgres/data-access".to_string();
    permission_set.platforms.azure = Some(Vec::new());
    let context = create_test_context();

    let result =
        generator.generate_role_definition(&permission_set, BindingTarget::Stack, &context);

    let err = result.expect_err("an empty azure list has no custom role to generate");
    assert!(
        err.to_string().contains("grants nothing"),
        "error should explain the empty-azure case: {err}"
    );
}

#[test]
fn test_azure_unknown_predefined_role_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    if let Some(azure_permissions) = &mut permission_set.platforms.azure {
        azure_permissions[0].grant.predefined_roles = Some(vec!["Not A Real Role".to_string()]);
    }
    let context = create_test_context();

    let result = generator.generate_grant_plan(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("references unknown predefined role"));
}

#[test]
fn test_azure_wildcard_scope_error() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let mut permission_set = create_azure_storage_data_read_permission_set();
    if let Some(azure_permissions) = &mut permission_set.platforms.azure {
        azure_permissions[0]
            .binding
            .stack
            .as_mut()
            .expect("stack binding")
            .scope = "/subscriptions/${subscriptionId}/resourceGroups/${stackPrefix}-*".to_string();
    }
    let context = create_test_context();

    let result = generator.generate_grant_plan(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("uses wildcard scope"));
}
