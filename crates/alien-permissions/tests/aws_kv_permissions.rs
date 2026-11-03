mod common;
mod iam_policy;

use alien_core::ALIEN_STACK_TAG_KEY;
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
// Stack "my-stack-prod" shares this stack's name prefix.
const PREFIX_SHARING_TABLE: &str =
    "arn:aws:dynamodb:us-east-1:123456789012:table/my-stack-prod-cache";

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

fn tagged(action: &'static str, table: &str, stack: Option<&str>) -> Request {
    let request = Request::new(action, table);
    match stack {
        Some(stack) => request.resource_tags(&[(ALIEN_STACK_TAG_KEY, stack)]),
        None => request,
    }
}

fn assert_point_in_time_recovery_reach(statements: &[AwsIamStatement], own_table: &str) {
    for action in POINT_IN_TIME_RECOVERY_ACTIONS {
        assert_eq!(
            evaluate(statements, &tagged(action, own_table, Some("my-stack"))),
            Decision::Allowed,
            "{action} on {own_table}"
        );
        let denied = [
            // Another stack whose name starts with this stack's prefix.
            (PREFIX_SHARING_TABLE, Some("my-stack-prod")),
            // This stack's name, but tagged for another stack or not tagged.
            (own_table, Some("other-stack")),
            (own_table, None),
            // Another stack's prefix, another region, another account (the managing account
            // included), each tagged for this stack.
            (
                "arn:aws:dynamodb:us-east-1:123456789012:table/other-stack-cache",
                Some("my-stack"),
            ),
            (
                "arn:aws:dynamodb:us-west-2:123456789012:table/my-stack-cache",
                Some("my-stack"),
            ),
            (
                "arn:aws:dynamodb:us-east-1:210987654321:table/my-stack-cache",
                Some("my-stack"),
            ),
            (
                "arn:aws:dynamodb:us-east-1:999999999999:table/my-stack-cache",
                Some("my-stack"),
            ),
        ];
        for (table, stack) in denied {
            assert_eq!(
                evaluate(statements, &tagged(action, table, stack)),
                Decision::NotAllowed,
                "{action} on {table} tagged {stack:?}"
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
            &tagged(
                "dynamodb:UpdateContinuousBackups",
                STACK_TABLE,
                Some("my-stack")
            )
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

#[test]
fn kv_provision_tags_new_tables_but_cannot_retag_another_stacks_table() {
    let statements = kv_provision(BindingTarget::Stack);
    let tag = |table: &str, current: Option<&str>, requested: &str| {
        evaluate(
            &statements,
            &tagged("dynamodb:TagResource", table, current)
                .request_tags(&[(ALIEN_STACK_TAG_KEY, requested)]),
        )
    };

    // CreateTable with tags: the table has no tags yet.
    assert_eq!(tag(STACK_TABLE, None, "my-stack"), Decision::Allowed);
    assert_eq!(
        tag(STACK_TABLE, Some("my-stack"), "my-stack"),
        Decision::Allowed
    );

    assert_eq!(
        tag(PREFIX_SHARING_TABLE, Some("my-stack-prod"), "my-stack"),
        Decision::NotAllowed
    );
    assert_eq!(tag(STACK_TABLE, None, "other-stack"), Decision::NotAllowed);
    assert_eq!(
        evaluate(
            &statements,
            &Request::new("dynamodb:TagResource", STACK_TABLE)
                .request_tags(&[("resource", "cache")])
        ),
        Decision::NotAllowed
    );
}
