mod common;

use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};
use common::*;
use insta::assert_json_snapshot;
use rstest::rstest;
use serde_json::json;

#[rstest]
#[case::stack_binding(BindingTarget::Stack)]
#[case::resource_binding(BindingTarget::Resource)]
fn test_aws_cloudformation_storage_data_read_policy_generation(
    #[case] binding_target: BindingTarget,
) {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_cloudformation_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, binding_target, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    let snapshot_name = format!(
        "aws_cloudformation_storage_data_read_{}_binding",
        binding_target
    );
    assert_json_snapshot!(snapshot_name, result);
}

#[test]
fn test_aws_cloudformation_policy_with_conditions() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_lambda_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy with conditions successfully");

    assert_json_snapshot!("aws_cloudformation_policy_with_conditions", result);
}

#[test]
fn test_aws_cloudformation_intrinsic_functions() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_cloudformation_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // Verify that CloudFormation intrinsic functions are used
    let first_statement = &result.statement[0];

    // Check that resources contain Fn::Sub for CloudFormation variables
    let expected_resources = vec![
        json!({"Fn::Sub": "arn:aws:s3:::${AWS::StackName}-*"}),
        json!({"Fn::Sub": "arn:aws:s3:::${AWS::StackName}-*/*"}),
    ];
    assert_eq!(first_statement.resource, expected_resources);
}

#[test]
fn test_aws_cloudformation_resource_references() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_cloudformation_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Resource, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // Verify that resource-level binding substitutes resourceName in ARN context
    let first_statement = &result.statement[0];
    let expected_resources = vec![
        json!("arn:aws:s3:::PaymentsDataBucket"),
        json!("arn:aws:s3:::PaymentsDataBucket/*"),
    ];
    assert_eq!(first_statement.resource, expected_resources);
}

#[test]
fn test_aws_cloudformation_statement_id_generation() {
    let generator = AwsCloudFormationPermissionsGenerator::new();

    // Create a permission set with a complex ID
    let mut permission_set = create_aws_cloudformation_permission_set();
    permission_set.id = "complex/permission-set/with-dashes".to_string();

    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // Verify that the statement ID is properly formatted
    assert_eq!(result.statement[0].sid, "ComplexPermissionSetWithDashes");
    assert_eq!(result.version, "2012-10-17");
    assert_eq!(result.statement[0].effect, "Allow");
}

#[test]
fn test_aws_cloudformation_multiple_statements() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_lambda_permission_set(); // Has multiple AWS platform permissions
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // Should have two statements (Lambda + ECR permissions)
    assert_eq!(result.statement.len(), 2);

    // First statement should be for Lambda
    assert_eq!(result.statement[0].sid, "FunctionExecute1");
    assert_eq!(
        result.statement[0].action,
        vec![json!("lambda:InvokeFunction")]
    );

    // Second statement should be for ECR
    assert_eq!(result.statement[1].sid, "FunctionExecute2");
    assert_eq!(
        result.statement[1].action,
        vec![
            json!("ecr:BatchGetImage"),
            json!("ecr:GetDownloadUrlForLayer")
        ]
    );
}

#[test]
fn test_aws_cloudformation_condition_interpolation() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_lambda_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // The ECR statement (second statement) should have conditions
    let ecr_statement = &result.statement[1];
    assert!(ecr_statement.condition.is_some());

    let conditions = ecr_statement.condition.as_ref().unwrap();
    let string_equals = conditions.get("StringEquals").unwrap();
    // Variables in conditions should be interpolated with actual values
    assert_eq!(
        string_equals.get("sts:ExternalId").unwrap(),
        &json!("my-external-id")
    );
}

#[test]
fn test_aws_cloudformation_missing_actions_error() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_permission_set_missing_actions();
    let context = create_cloudformation_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("AWS permission grant must have 'actions' field"));
}

#[test]
fn test_aws_cloudformation_missing_platform_error() {
    let generator = AwsCloudFormationPermissionsGenerator::new();

    // Create a permission set without AWS platform
    let permission_set = create_gcp_storage_data_read_permission_set(); // This only has GCP
    let context = create_cloudformation_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Platform 'aws' is not supported"));
}

#[test]
fn test_aws_cloudformation_missing_binding_target_error() {
    let generator = AwsCloudFormationPermissionsGenerator::new();

    // Create a permission set with only stack binding
    let mut permission_set = create_aws_cloudformation_permission_set();
    if let Some(aws_permissions) = &mut permission_set.platforms.aws {
        aws_permissions[0].binding.resource = None; // Remove resource binding
    }

    let context = create_cloudformation_context();

    let result = generator.generate_policy(&permission_set, BindingTarget::Resource, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(error_string.contains("Binding target 'resource' is not supported"));
}

#[test]
fn test_aws_cloudformation_managing_account_id_substitution() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let permission_set = create_aws_lambda_permission_set();
    let context = create_cloudformation_context();

    let result = generator
        .generate_policy(&permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS CloudFormation policy successfully");

    // The ECR statement should have the managing account ID as a CloudFormation parameter reference
    let ecr_statement = &result.statement[1];
    let expected_resource = json!({"Fn::Sub": "arn:aws:ecr:*:${ManagingAccountId}:repository/*"});
    assert_eq!(ecr_statement.resource[0], expected_resource);
}
