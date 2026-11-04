//! A created network's VPC flow log objects, checked against the rendered setup policy.
//!
//! Only setup creates the flow log, its delivery role and its log group (`network/setup`).
//! Deleting them is in `network/provision`, which the setup policy also includes, so a setup
//! teardown still deletes them. The management identity's side is checked against the whole
//! automatic management policy in `alien-preflights/tests/aws_network_flow_log_deletes.rs`.

mod common;
mod iam_policy;

use alien_permissions::{
    generators::AwsIamStatement, initial_setup::generate_aws_initial_setup_policy,
};
use iam_policy::{evaluate, Decision, Request};

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary";
const FLOW_LOG: &str = "arn:aws:ec2:us-east-1:123456789012:vpc-flow-log/fl-0123456789abcdef0";
const ROLE: &str = "arn:aws:iam::123456789012:role/my-stack-default-network-flow-logs";
const GROUP: &str =
    "arn:aws:logs:us-east-1:123456789012:log-group:my-stack-default-network-flow-logs";
const VPC: &str = "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-0123456789abcdef0";

const RUNTIME: &[(&str, &str)] = &[("deployment", "my-stack"), ("managed-by", "runtime")];

fn setup_policies() -> Vec<(bool, Vec<AwsIamStatement>)> {
    [false, true]
        .into_iter()
        .map(|bounded| {
            let mut context = common::create_test_context();
            if bounded {
                context = context.with_aws_permissions_boundary_arn(BOUNDARY);
            }
            (
                bounded,
                generate_aws_initial_setup_policy(&context)
                    .unwrap()
                    .statement,
            )
        })
        .collect()
}

/// Every call the network controller's flow log delete makes, in order, for objects that carry
/// `tags`. CloudWatch Logs authorizes a log group under either form of its ARN, so the delete
/// is checked under both.
fn delete_calls(tags: &[(&str, &str)]) -> Vec<Request> {
    vec![
        Request::new("ec2:DescribeFlowLogs", "*"),
        Request::new("ec2:DeleteFlowLogs", FLOW_LOG).resource_tags(tags),
        Request::new("iam:GetRole", ROLE).resource_tags(tags),
        Request::new("iam:DeleteRolePolicy", ROLE).resource_tags(tags),
        Request::new("iam:DeleteRole", ROLE).resource_tags(tags),
        Request::new("logs:ListTagsForResource", GROUP).resource_tags(tags),
        Request::new("logs:DeleteLogGroup", GROUP).resource_tags(tags),
        Request::new("logs:DeleteLogGroup", format!("{GROUP}:*")).resource_tags(tags),
    ]
}

fn create_calls() -> Vec<Request> {
    vec![
        Request::new("ec2:CreateFlowLogs", FLOW_LOG).request_tags(RUNTIME),
        Request::new("ec2:CreateFlowLogs", VPC).resource_tags(RUNTIME),
        Request::new("iam:CreateRole", ROLE).request_tags(RUNTIME),
        Request::new("iam:PutRolePolicy", ROLE).resource_tags(RUNTIME),
        Request::new("iam:PassRole", ROLE)
            .with("iam:PassedToService", "vpc-flow-logs.amazonaws.com")
            .with("iam:ResourceTag/deployment", "my-stack")
            .with("iam:ResourceTag/managed-by", "runtime"),
        Request::new("logs:CreateLogGroup", GROUP).request_tags(RUNTIME),
        Request::new("logs:PutRetentionPolicy", GROUP).resource_tags(RUNTIME),
        Request::new("logs:PutRetentionPolicy", format!("{GROUP}:*")).resource_tags(RUNTIME),
    ]
}

#[test]
fn setup_creates_and_deletes_the_flow_log_objects() {
    for (bounded, setup) in setup_policies() {
        let denied: Vec<_> = create_calls()
            .into_iter()
            .chain(delete_calls(RUNTIME))
            .filter(|request| evaluate(&setup, request) != Decision::Allowed)
            .map(|request| format!("{} on {}", request.action, request.resource))
            .collect();
        assert!(
            denied.is_empty(),
            "bounded={bounded}: setup is denied a flow log call: {denied:#?}"
        );
    }
}

/// A setup teardown repeated after an object was deleted reads it again. A gone object has no
/// tags, so these reads must not depend on them, or AWS answers AccessDenied instead of
/// NotFound and the teardown fails. No write reaches an untagged object, except the role
/// deletes that a setup without a boundary already grants on every role under the stack
/// prefix: `legacy-role-*` statements are kept only when unbounded, with the
/// `iam:PermissionsBoundary` condition removed. DeleteRolePolicy comes from `ai/provision`
/// (legacy-role-1) and `artifact-registry/provision` (legacy-role-3), DeleteRole from
/// `artifact-registry/provision` (legacy-role-4).
#[test]
fn setup_reads_flow_log_objects_that_are_gone() {
    for (bounded, setup) in setup_policies() {
        for request in delete_calls(&[]) {
            let read = matches!(
                request.action,
                "ec2:DescribeFlowLogs" | "iam:GetRole" | "logs:ListTagsForResource"
            );
            let legacy_role_delete =
                !bounded && matches!(request.action, "iam:DeleteRolePolicy" | "iam:DeleteRole");
            assert_eq!(
                evaluate(&setup, &request),
                if read || legacy_role_delete {
                    Decision::Allowed
                } else {
                    Decision::NotAllowed
                },
                "bounded={bounded}: {} on an untagged {}",
                request.action,
                request.resource
            );
        }
    }
}
