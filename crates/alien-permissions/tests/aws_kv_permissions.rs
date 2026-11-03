mod common;
mod iam_policy;

use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set,
    initial_setup::generate_aws_initial_setup_policy,
    BindingTarget,
};
use iam_policy::{evaluate, Decision, Request};

const POINT_IN_TIME_RECOVERY_ACTIONS: [&str; 2] = [
    "dynamodb:DescribeContinuousBackups",
    "dynamodb:UpdateContinuousBackups",
];

const STACK_TABLE: &str = "arn:aws:dynamodb:us-east-1:123456789012:table/my-stack-cache";
const RESOURCE_TABLE: &str = "arn:aws:dynamodb:us-east-1:123456789012:table/my-stack-payments-data";

fn kv_provision(target: BindingTarget) -> Vec<AwsIamStatement> {
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("kv/provision").unwrap(),
            target,
            &common::create_test_context(),
        )
        .unwrap()
        .statement
}

// Bound by table name like the other table actions: DynamoDB tag conditions are not relied on.
fn assert_point_in_time_recovery_reach(statements: &[AwsIamStatement], own_table: &str) {
    for action in POINT_IN_TIME_RECOVERY_ACTIONS {
        assert_eq!(
            evaluate(statements, &Request::new(action, own_table)),
            Decision::Allowed,
            "{action} on {own_table}"
        );
        let denied = [
            // Another stack's prefix, another region, another account (the managing account
            // included).
            "arn:aws:dynamodb:us-east-1:123456789012:table/other-stack-cache",
            "arn:aws:dynamodb:us-west-2:123456789012:table/my-stack-cache",
            "arn:aws:dynamodb:us-east-1:210987654321:table/my-stack-cache",
            "arn:aws:dynamodb:us-east-1:999999999999:table/my-stack-cache",
        ];
        for table in denied {
            assert_eq!(
                evaluate(statements, &Request::new(action, table)),
                Decision::NotAllowed,
                "{action} on {table}"
            );
        }
    }
}

#[test]
fn kv_provision_manages_point_in_time_recovery_only_on_the_stacks_tables() {
    assert_point_in_time_recovery_reach(&kv_provision(BindingTarget::Stack), STACK_TABLE);
}

#[test]
fn kv_provision_resource_binding_reaches_only_its_table() {
    let statements = kv_provision(BindingTarget::Resource);
    assert_point_in_time_recovery_reach(&statements, RESOURCE_TABLE);
    assert_eq!(
        evaluate(
            &statements,
            &Request::new("dynamodb:UpdateContinuousBackups", STACK_TABLE)
        ),
        Decision::NotAllowed
    );
}

// CloudFormation and Terraform create setup-owned tables with point-in-time recovery on, and
// a direct setup creates them through the runtime controller, so the setup policy needs it too.
#[test]
fn setup_policy_turns_on_point_in_time_recovery_only_on_the_stacks_tables() {
    let setup = generate_aws_initial_setup_policy(&common::create_test_context()).unwrap();
    assert_point_in_time_recovery_reach(&setup.statement, STACK_TABLE);
}
