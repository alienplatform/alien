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
        // Tagged for this stack, or with tags not evaluated (DynamoDB ABAC off).
        for stack in [Some("my-stack"), None] {
            assert_eq!(
                evaluate(statements, &tagged(action, own_table, stack)),
                Decision::Allowed,
                "{action} on {own_table} tagged {stack:?}"
            );
        }
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
                evaluate(statements, &tagged(action, table, Some("my-stack"))),
                Decision::NotAllowed,
                "{action} on {table}"
            );
        }
    }
    // A stack whose name starts with this stack's prefix keeps its backups.
    assert_eq!(
        evaluate(
            statements,
            &tagged(
                "dynamodb:UpdateContinuousBackups",
                PREFIX_SHARING_TABLE,
                Some("my-stack-prod")
            )
        ),
        Decision::NotAllowed
    );
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
    let tag = |table: &str, current: Option<&str>, requested: &[(&str, &str)]| {
        evaluate(
            &statements,
            &tagged("dynamodb:TagResource", table, current).request_tags(requested),
        )
    };
    let own = [(ALIEN_STACK_TAG_KEY, "my-stack")];

    // CreateTable with tags checks TagResource before the table has tags.
    assert_eq!(tag(STACK_TABLE, None, &own), Decision::Allowed);
    assert_eq!(tag(STACK_TABLE, Some("my-stack"), &own), Decision::Allowed);

    // Moving another stack's table into this one, with or without the stack tag.
    assert_eq!(
        tag(PREFIX_SHARING_TABLE, Some("my-stack-prod"), &own),
        Decision::NotAllowed
    );
    assert_eq!(
        tag(
            PREFIX_SHARING_TABLE,
            Some("my-stack-prod"),
            &[("resource", "cache")]
        ),
        Decision::NotAllowed
    );
    // Giving this stack's table another stack's tag.
    assert_eq!(
        tag(STACK_TABLE, None, &[(ALIEN_STACK_TAG_KEY, "other-stack")]),
        Decision::NotAllowed
    );
}
