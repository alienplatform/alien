mod common;

use alien_permissions::generators::AzureRuntimePermissionsGenerator;
use alien_permissions::BindingTarget;
use common::*;
use insta::assert_json_snapshot;
use rstest::rstest;

#[rstest]
#[case::stack_binding(BindingTarget::Stack)]
#[case::resource_binding(BindingTarget::Resource)]
fn test_azure_role_definition_generation(#[case] binding_target: BindingTarget) {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_role_definition(&permission_set, binding_target, &context)
        .expect("Should generate Azure role definition successfully");

    let snapshot_name = format!("azure_role_definition_{}_binding", binding_target);
    assert_json_snapshot!(snapshot_name, result);
}

#[rstest]
#[case::stack_binding(BindingTarget::Stack)]
#[case::resource_binding(BindingTarget::Resource)]
fn test_azure_role_assignment_generation(#[case] binding_target: BindingTarget) {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_role_assignment(&permission_set, binding_target, &context)
        .expect("Should generate Azure role assignment successfully");

    let snapshot_name = format!("azure_role_assignment_{}_binding", binding_target);
    assert_json_snapshot!(snapshot_name, result);
}

#[test]
fn test_azure_role_name_generation() {
    let generator = AzureRuntimePermissionsGenerator::new();

    // Create a permission set with a complex ID
    let mut permission_set = create_azure_storage_data_read_permission_set();
    permission_set.id = "complex/permission-set/with-dashes".to_string();

    let context = create_test_context();

    let result = generator
        .generate_role_definition(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure role definition successfully");

    // Verify that the role name is properly formatted (includes stack prefix)
    assert_eq!(result.name, "Complex Permission Set With Dashes (my-stack)");
    assert_eq!(result.is_custom, true);
}

#[test]
fn test_azure_missing_platform_error() {
    let generator = AzureRuntimePermissionsGenerator::new();

    // Create a permission set without Azure platform
    let permission_set = create_aws_storage_data_read_permission_set(); // This only has AWS
    let context = create_test_context();

    let result =
        generator.generate_role_definition(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Platform 'azure' is not supported"));
}

#[test]
fn test_azure_missing_binding_target_error() {
    let generator = AzureRuntimePermissionsGenerator::new();

    // Create a permission set with only stack binding
    let mut permission_set = create_azure_storage_data_read_permission_set();
    if let Some(azure_permissions) = &mut permission_set.platforms.azure {
        azure_permissions[0].binding.resource = None; // Remove resource binding
    }

    let context = create_test_context();

    let result =
        generator.generate_role_definition(&permission_set, BindingTarget::Resource, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Binding target 'resource' is not supported"));
}

#[test]
fn test_azure_scope_interpolation() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_role_assignment(&permission_set, BindingTarget::Resource, &context)
        .expect("Should generate Azure role assignment successfully");

    // Verify that the scope was set correctly
    assert_eq!(
        result.properties.scope, 
        "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.Storage/storageAccounts/stcxpaymentsprod"
    );
}

#[test]
fn test_azure_principal_id_usage() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_role_assignment(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure role assignment successfully");

    // Verify that the principal ID was used correctly
    assert_eq!(
        result.properties.principal_id,
        "11111111-2222-3333-4444-555555555555"
    );
}

#[test]
fn test_azure_actions_and_data_actions_aggregation() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_set = create_azure_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_role_definition(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate Azure role definition successfully");

    // Verify that both actions and data actions are included
    assert_eq!(
        result.actions,
        vec!["Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"]
    );
    assert_eq!(
        result.data_actions,
        vec!["Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"]
    );
    assert_eq!(result.not_actions, Vec::<String>::new());
    assert_eq!(result.not_data_actions, Vec::<String>::new());
}
