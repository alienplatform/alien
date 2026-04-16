mod common;

use alien_permissions::generators::GcpRuntimePermissionsGenerator;
use alien_permissions::BindingTarget;
use common::*;
use insta::assert_json_snapshot;
use rstest::rstest;

#[test]
fn test_gcp_custom_role_generation() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_custom_role(&permission_set, &context)
        .expect("Should generate GCP custom role successfully");

    assert_json_snapshot!("gcp_custom_role", result);
}

#[rstest]
#[case::stack_binding(BindingTarget::Stack)]
#[case::resource_binding(BindingTarget::Resource)]
fn test_gcp_iam_bindings_generation(#[case] binding_target: BindingTarget) {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_bindings(&permission_set, binding_target, &context)
        .expect("Should generate GCP IAM bindings successfully");

    let snapshot_name = format!("gcp_iam_bindings_{}_binding", binding_target);
    assert_json_snapshot!(snapshot_name, result);
}

#[test]
fn test_gcp_role_id_generation() {
    let generator = GcpRuntimePermissionsGenerator::new();

    // Create a permission set with a complex ID
    let mut permission_set = create_gcp_storage_data_read_permission_set();
    permission_set.id = "complex/permission-set/with-dashes".to_string();

    let context = create_test_context();

    let result = generator
        .generate_custom_role(&permission_set, &context)
        .expect("Should generate GCP custom role successfully");

    // Verify that the role ID is properly formatted (camelCase)
    assert_eq!(
        result.name,
        "projects/my-project/roles/complexPermissionSetWithDashes"
    );
    assert_eq!(result.title, "Complex Permission Set With Dashes");
}

#[test]
fn test_gcp_missing_permissions_error() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_permission_set_missing_gcp_permissions();
    let context = create_test_context();

    let result = generator.generate_custom_role(&permission_set, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("GCP permission grant must have 'permissions' field"));
}

#[test]
fn test_gcp_condition_interpolation() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_bindings(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate GCP IAM bindings successfully");

    // Verify that the condition was interpolated correctly
    let binding = &result.bindings[0];
    let condition = binding.condition.as_ref().unwrap();
    assert_eq!(condition.title, "Stack-prefixed only");
    assert_eq!(
        condition.expression,
        "resource.name.startsWith('projects/_/buckets/my-stack-')"
    );
}

#[test]
fn test_gcp_service_account_generation() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_bindings(&permission_set, BindingTarget::Resource, &context)
        .expect("Should generate GCP IAM bindings successfully");

    // Verify that the service account is properly formatted
    let binding = &result.bindings[0];
    assert_eq!(
        binding.members,
        vec!["serviceAccount:my-sa@my-project.iam.gserviceaccount.com"]
    );
    assert_eq!(binding.role, "projects/my-project/roles/storageDataRead");
}

#[test]
fn test_gcp_missing_platform_error() {
    let generator = GcpRuntimePermissionsGenerator::new();

    // Create a permission set without GCP platform
    let permission_set = create_aws_storage_data_read_permission_set(); // This only has AWS
    let context = create_test_context();

    let result = generator.generate_custom_role(&permission_set, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Platform 'gcp' is not supported"));
}

#[test]
fn test_gcp_missing_binding_target_graceful_skip() {
    let generator = GcpRuntimePermissionsGenerator::new();

    // Create a permission set with only stack binding
    let mut permission_set = create_gcp_storage_data_read_permission_set();
    if let Some(gcp_permissions) = &mut permission_set.platforms.gcp {
        gcp_permissions[0].binding.resource = None; // Remove resource binding
    }

    let context = create_test_context();

    // Should succeed with empty bindings (graceful skip), not error
    let result = generator.generate_bindings(&permission_set, BindingTarget::Resource, &context);
    assert!(result.is_ok());
    let bindings = result.unwrap();
    assert!(bindings.bindings.is_empty());
}
