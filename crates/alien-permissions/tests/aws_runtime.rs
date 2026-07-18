mod common;

use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
};
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
fn test_aws_observe_generates_account_wide_read_policy() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("observe/observe").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS observe policy successfully");

    assert_json_snapshot!("aws_observe_account_wide_read_policy", result);
}

#[test]
fn compute_cluster_execute_reads_only_stack_secret_parameters() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/execute").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("compute cluster execute policy should generate");
    let statement = result
        .statement
        .iter()
        .find(|statement| statement.action == ["ssm:GetParameter"])
        .expect("scoped SSM read statement");

    assert_eq!(
        statement.resource,
        vec!["arn:aws:ssm:us-east-1:123456789012:parameter/my-stack-secrets-*".to_string()]
    );
    assert!(!statement.resource.iter().any(|resource| resource == "*"));
}

#[test]
fn test_compute_cluster_management_can_tag_otlp_secrets() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/management").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy successfully");

    let secret_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .resource
                .iter()
                .any(|resource| resource.contains(":secretsmanager:"))
        })
        .expect("compute-cluster management should grant OTLP secret access");

    assert!(secret_statement
        .action
        .contains(&"secretsmanager:CreateSecret".to_string()));
    assert!(secret_statement
        .action
        .contains(&"secretsmanager:TagResource".to_string()));
}

#[test]
fn test_compute_cluster_management_can_use_tagged_launch_templates() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/management").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy successfully");

    let launch_template_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .resource
                .iter()
                .any(|resource| resource.contains(":launch-template/"))
                && statement.action.contains(&"ec2:RunInstances".to_string())
                && statement
                    .condition
                    .as_ref()
                    .is_some_and(|condition| condition.contains_key("StringEquals"))
        })
        .expect("compute-cluster management should grant tagged launch-template access");

    assert!(launch_template_statement
        .action
        .contains(&"ec2:RunInstances".to_string()));
    assert!(condition_equals(
        launch_template_statement,
        "aws:ResourceTag/managed-by",
        "runtime"
    ));

    let setup_security_group_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .resource
                .iter()
                .any(|resource| resource.contains(":security-group/"))
                && statement.action.contains(&"ec2:RunInstances".to_string())
        })
        .expect("compute-cluster management should allow setup compute security group use");

    assert!(condition_equals(
        setup_security_group_statement,
        "aws:ResourceTag/deployment",
        "my-stack"
    ));
    assert!(condition_equals(
        setup_security_group_statement,
        "aws:ResourceTag/managed-by",
        "setup"
    ));
    assert!(condition_equals(
        setup_security_group_statement,
        "aws:ResourceTag/resource-type",
        "compute-cluster"
    ));

    let instance_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .resource
                .iter()
                .any(|resource| resource.contains(":instance/"))
                && statement.action.contains(&"ec2:RunInstances".to_string())
                && statement
                    .condition
                    .as_ref()
                    .is_some_and(|condition| condition.contains_key("StringEquals"))
        })
        .expect("compute-cluster management should require worker instance request tags");

    assert!(instance_statement
        .condition
        .as_ref()
        .and_then(|condition| condition.get("StringEquals"))
        .is_some_and(|values| values.contains_key("aws:RequestTag/deployment")));

    let create_tags_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement.action.contains(&"ec2:CreateTags".to_string())
                && statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(":instance/"))
                && statement
                    .condition
                    .as_ref()
                    .is_some_and(|condition| condition.contains_key("StringEquals"))
        })
        .expect("compute-cluster management should allow RunInstances tag specifications");

    assert!(create_tags_statement
        .condition
        .as_ref()
        .and_then(|condition| condition.get("StringEquals"))
        .is_some_and(|values| {
            values.contains_key("ec2:CreateAction")
                && values.contains_key("aws:RequestTag/deployment")
        }));

    for expected_resource in [":image/", ":subnet/", ":network-interface/", ":volume/"] {
        assert!(
            result.statement.iter().any(|statement| {
                statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(expected_resource))
                    && statement.action.contains(&"ec2:RunInstances".to_string())
            }),
            "compute-cluster management should include RunInstances companion resource {expected_resource}"
        );
    }
}

#[test]
fn test_compute_cluster_provision_can_create_asg_workers() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/provision").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy successfully");

    let runtime_boundary_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement.action.contains(&"ec2:RunInstances".to_string())
                && statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(":launch-template/"))
                && statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(":security-group/"))
        })
        .expect(
            "compute-cluster provision should allow runtime launch template and security group use",
        );

    assert!(condition_equals(
        runtime_boundary_statement,
        "aws:ResourceTag/deployment",
        "my-stack"
    ));
    assert!(condition_equals(
        runtime_boundary_statement,
        "aws:ResourceTag/managed-by",
        "runtime"
    ));

    let instance_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement.action.contains(&"ec2:RunInstances".to_string())
                && statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(":instance/"))
        })
        .expect("compute-cluster provision should allow runtime worker instance creation");

    assert!(condition_equals(
        instance_statement,
        "aws:RequestTag/deployment",
        "my-stack"
    ));
    assert!(condition_equals(
        instance_statement,
        "aws:RequestTag/managed-by",
        "runtime"
    ));

    for expected_resource in [":image/", ":subnet/", ":network-interface/", ":volume/"] {
        assert!(
            result.statement.iter().any(|statement| {
                statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(expected_resource))
                    && statement.action.contains(&"ec2:RunInstances".to_string())
            }),
            "compute-cluster provision should include RunInstances companion resource {expected_resource}"
        );
    }

    let create_tags_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement.action.contains(&"ec2:CreateTags".to_string())
                && statement
                    .resource
                    .iter()
                    .any(|resource| resource.contains(":instance/"))
                && condition_equals(statement, "ec2:CreateAction", "RunInstances")
        })
        .expect("compute-cluster provision should allow RunInstances tag specifications");

    assert!(condition_equals(
        create_tags_statement,
        "aws:RequestTag/deployment",
        "my-stack"
    ));
    assert!(condition_equals(
        create_tags_statement,
        "aws:RequestTag/managed-by",
        "runtime"
    ));
}

#[test]
fn test_container_provision_can_manage_setup_compute_security_group_ingress() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("container/provision").expect("permission set exists");
    let context = create_test_context().with_resource_name("alien-manager");

    let result = generator
        .generate_policy(permission_set, BindingTarget::Resource, &context)
        .expect("Should generate AWS policy successfully");

    let setup_compute_ingress_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .action
                .contains(&"ec2:AuthorizeSecurityGroupIngress".to_string())
                && statement
                    .action
                    .contains(&"ec2:RevokeSecurityGroupIngress".to_string())
                && condition_equals(
                    statement,
                    "aws:ResourceTag/resource-type",
                    "compute-cluster",
                )
                && condition_equals(statement, "aws:ResourceTag/managed-by", "setup")
        })
        .expect("container provision should manage ingress on setup compute security groups");

    assert_eq!(
        setup_compute_ingress_statement.resource,
        vec!["arn:aws:ec2:us-east-1:123456789012:security-group/*"]
    );
    assert!(!setup_compute_ingress_statement
        .action
        .contains(&"ec2:DeleteSecurityGroup".to_string()));
    assert!(condition_equals(
        setup_compute_ingress_statement,
        "aws:ResourceTag/deployment",
        "my-stack"
    ));
}

#[test]
fn test_compute_cluster_management_can_pass_stack_prefixed_instance_roles() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("compute-cluster/management").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_policy(permission_set, BindingTarget::Stack, &context)
        .expect("Should generate AWS policy successfully");

    let pass_role_statement = result
        .statement
        .iter()
        .find(|statement| statement.action.contains(&"iam:PassRole".to_string()))
        .expect("compute-cluster management should grant PassRole");

    assert!(
        pass_role_statement
            .resource
            .contains(&"arn:aws:iam::123456789012:role/my-stack-*".to_string()),
        "PassRole must cover CloudFormation-generated compute instance role names"
    );
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
fn test_worker_provision_uses_resource_name_for_arn_and_resource_id_for_tags() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("worker/provision").expect("permission set exists");
    let context = create_test_context()
        .with_resource_id("job")
        .with_resource_name("my-stack-job");

    let result = generator
        .generate_policy(permission_set, BindingTarget::Resource, &context)
        .expect("Should generate AWS policy successfully");

    let create_function_statement = result
        .statement
        .iter()
        .find(|statement| {
            statement
                .action
                .contains(&"lambda:CreateFunction".to_string())
                && statement.resource.contains(
                    &"arn:aws:lambda:us-east-1:123456789012:function:my-stack-job".to_string(),
                )
        })
        .expect("worker provision should allow creating the physical Lambda function");

    assert!(condition_equals(
        create_function_statement,
        "aws:RequestTag/resource",
        "job"
    ));
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

fn condition_equals(
    statement: &alien_permissions::generators::AwsIamStatement,
    key: &str,
    expected: &str,
) -> bool {
    statement
        .condition
        .as_ref()
        .and_then(|condition| condition.get("StringEquals"))
        .and_then(|values| values.get(key))
        .is_some_and(|value| value == expected)
}
