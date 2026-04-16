mod common;

use alien_permissions::{generators::AwsRuntimePermissionsGenerator, BindingTarget};
use common::*;
use insta::assert_json_snapshot;
use rstest::rstest;

#[rstest]
#[case::stack_binding(BindingTarget::Stack)]
#[case::resource_binding(BindingTarget::Resource)]
fn test_aws_storage_data_read_policy_generation(#[case] binding_target: BindingTarget) {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = create_aws_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_policy(&permission_set, binding_target, &context)
        .expect("Should generate AWS policy successfully");

    let snapshot_name = format!("aws_storage_data_read_{}_binding", binding_target);
    assert_json_snapshot!(snapshot_name, result);
}

#[test]
fn test_aws_policy_with_conditions() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = create_aws_storage_data_read_permission_set_with_condition();
    let context = create_test_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy with conditions successfully");

    assert_json_snapshot!("aws_policy_with_conditions", result);
}

#[test]
fn test_aws_statement_id_generation() {
    let generator = AwsRuntimePermissionsGenerator::new();

    // Create a permission set with a complex ID
    let mut permission_set = create_aws_storage_data_read_permission_set();
    permission_set.id = "complex/permission-set/with-dashes".to_string();

    let context = create_test_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy successfully");

    // Verify that the statement ID is properly formatted
    assert_eq!(result.statement[0].sid, "ComplexPermissionSetWithDashes");
    assert_eq!(result.version, "2012-10-17");
    assert_eq!(result.statement[0].effect, "Allow");
}

#[test]
fn test_aws_missing_actions_error() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = create_permission_set_missing_actions();
    let context = create_test_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("AWS permission grant must have 'actions' field"));
}

#[test]
fn test_aws_variable_interpolation() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = create_aws_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Resource, &context)
        .expect("Should generate AWS policy successfully");

    // Verify that variables were interpolated correctly
    let expected_resources = vec![
        "arn:aws:s3:::my-stack-payments-data".to_string(),
        "arn:aws:s3:::my-stack-payments-data/*".to_string(),
    ];
    assert_eq!(result.statement[0].resource, expected_resources);
}

#[test]
fn test_aws_missing_variable_error() {
    let generator = AwsRuntimePermissionsGenerator::new();

    // Create a permission set with a variable that won't be found
    let mut permission_set = create_aws_storage_data_read_permission_set();
    if let Some(aws_permissions) = &mut permission_set.platforms.aws {
        if let Some(resource_binding) = &mut aws_permissions[0].binding.resource {
            resource_binding.resources = vec!["arn:aws:s3:::${missingVariable}".to_string()];
        }
    }

    let context = create_empty_context(); // Empty context

    let result = generator.generate_policy(&permission_set, BindingTarget::Resource, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Variable 'missingVariable' is not found"));
}

#[test]
fn test_aws_missing_platform_error() {
    let generator = AwsRuntimePermissionsGenerator::new();

    // Create a permission set without AWS platform
    let permission_set = create_gcp_storage_data_read_permission_set(); // This only has GCP
    let context = create_test_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Platform 'aws' is not supported"));
}

#[test]
fn test_aws_missing_binding_target_error() {
    let generator = AwsRuntimePermissionsGenerator::new();

    // Create a permission set with only stack binding
    let mut permission_set = create_aws_storage_data_read_permission_set();
    if let Some(aws_permissions) = &mut permission_set.platforms.aws {
        aws_permissions[0].binding.resource = None; // Remove resource binding
    }

    let context = create_test_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Resource, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Binding target 'resource' is not supported"));
}
