//! Deleting a created network's VPC flow log objects, checked by evaluating requests against
//! the rendered policies.
//!
//! Only setup creates the flow log, its delivery role and its log group (`network/setup`). The
//! network's delete removes them first, under whichever identity deletes the network: setup in
//! a setup teardown, or the management identity when it deletes the network it was given. That
//! identity holds the network's `provision` set, so the deletes live there, with the same
//! ownership tags as the network's other children. Creating and writing them stays with setup.

mod common;
mod iam_policy;

use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set,
    initial_setup::generate_aws_initial_setup_policy,
    BindingTarget,
};
use iam_policy::{evaluate, Decision, Request};

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary";
const FLOW_LOG: &str = "arn:aws:ec2:us-east-1:123456789012:vpc-flow-log/fl-0123456789abcdef0";
const ROLE: &str = "arn:aws:iam::123456789012:role/my-stack-default-network-flow-logs";
const GROUP: &str =
    "arn:aws:logs:us-east-1:123456789012:log-group:my-stack-default-network-flow-logs";
const VPC: &str = "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-0123456789abcdef0";

const RUNTIME: &[(&str, &str)] = &[("deployment", "my-stack"), ("managed-by", "runtime")];
const TEMPLATE_SETUP: &[(&str, &str)] = &[("deployment", "my-stack"), ("managed-by", "setup")];
const OTHER_STACK: &[(&str, &str)] = &[("deployment", "other-stack"), ("managed-by", "runtime")];

/// The management identity's network grants, and the role reads every management identity
/// holds, with and without the setup's permissions boundary.
fn management_policies() -> Vec<(bool, Vec<AwsIamStatement>)> {
    [false, true]
        .into_iter()
        .map(|bounded| {
            let mut context = common::create_test_context();
            if bounded {
                context = context.with_aws_permissions_boundary_arn(BOUNDARY);
            }
            let statements = ["network/provision", "service-account/management"]
                .into_iter()
                .flat_map(|id| {
                    AwsRuntimePermissionsGenerator::new()
                        .generate_policy(
                            get_permission_set(id).unwrap(),
                            BindingTarget::Stack,
                            &context,
                        )
                        .unwrap()
                        .statement
                })
                .collect();
            (bounded, statements)
        })
        .collect()
}

/// Every call the network controller's flow log delete makes, in order, for objects that carry
/// `tags`. The role and the log group are read before anything is changed.
fn delete_calls(tags: &[(&str, &str)]) -> Vec<Request> {
    vec![
        Request::new("ec2:DescribeFlowLogs", "*"),
        Request::new("ec2:DeleteFlowLogs", FLOW_LOG).resource_tags(tags),
        Request::new("iam:GetRole", ROLE).resource_tags(tags),
        Request::new("iam:DeleteRolePolicy", ROLE).resource_tags(tags),
        Request::new("iam:DeleteRole", ROLE).resource_tags(tags),
        Request::new("logs:ListTagsForResource", GROUP).resource_tags(tags),
        Request::new("logs:DeleteLogGroup", GROUP).resource_tags(tags),
    ]
}

fn decisions(statements: &[AwsIamStatement], requests: &[Request]) -> Vec<(String, Decision)> {
    requests
        .iter()
        .map(|request| {
            (
                format!("{} on {}", request.action, request.resource),
                evaluate(statements, request),
            )
        })
        .collect()
}

#[test]
fn management_deletes_the_flow_log_objects_of_the_network_it_deletes() {
    for (bounded, policy) in management_policies() {
        let denied: Vec<_> = decisions(&policy, &delete_calls(RUNTIME))
            .into_iter()
            .filter(|(_, decision)| *decision != Decision::Allowed)
            .collect();
        assert!(
            denied.is_empty(),
            "bounded={bounded}: the management identity cannot delete its network's flow log objects: {denied:#?}"
        );
        // A repeated delete reads objects that are gone and so have no tags: each read must
        // reach them so they answer NotFound, and no write may.
        for (call, decision) in decisions(&policy, &delete_calls(&[])) {
            let read = call.starts_with("ec2:DescribeFlowLogs")
                || call.starts_with("iam:GetRole")
                || call.starts_with("logs:ListTagsForResource");
            assert_eq!(
                decision,
                if read {
                    Decision::Allowed
                } else {
                    Decision::NotAllowed
                },
                "bounded={bounded}: {call} on an untagged object"
            );
        }
    }
}

#[test]
fn management_cannot_delete_flow_log_objects_it_does_not_own() {
    for (bounded, policy) in management_policies() {
        for tags in [TEMPLATE_SETUP, OTHER_STACK] {
            for (call, decision) in decisions(&policy, &delete_calls(tags)) {
                if call.starts_with("ec2:DescribeFlowLogs")
                    || call.starts_with("iam:GetRole")
                    || call.starts_with("logs:ListTagsForResource")
                {
                    continue;
                }
                assert_eq!(
                    decision,
                    Decision::NotAllowed,
                    "bounded={bounded}: {call} with tags {tags:?}"
                );
            }
        }
        // Same names in another stack's prefix.
        for request in [
            Request::new(
                "iam:DeleteRole",
                "arn:aws:iam::123456789012:role/other-stack-default-network-flow-logs",
            )
            .resource_tags(RUNTIME),
            Request::new(
                "logs:DeleteLogGroup",
                "arn:aws:logs:us-east-1:123456789012:log-group:other-stack-default-network-flow-logs",
            )
            .resource_tags(RUNTIME),
            // A role of this stack that is not a flow log delivery role.
            Request::new("iam:DeleteRolePolicy", "arn:aws:iam::123456789012:role/my-stack-api")
                .resource_tags(RUNTIME),
        ] {
            assert_eq!(
                evaluate(&policy, &request),
                Decision::NotAllowed,
                "bounded={bounded}: {} on {}",
                request.action,
                request.resource
            );
        }
    }
}

/// Creating the role, writing its policy and passing it to the service cannot be limited to
/// what the role may do, so they stay with setup.
#[test]
fn only_setup_creates_the_flow_log_objects() {
    let creates = [
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
    ];
    for (bounded, policy) in management_policies() {
        for (call, decision) in decisions(&policy, &creates) {
            assert_eq!(decision, Decision::NotAllowed, "bounded={bounded}: {call}");
        }
    }
    // Setup still holds every create and every delete.
    let setup = generate_aws_initial_setup_policy(
        &common::create_test_context().with_aws_permissions_boundary_arn(BOUNDARY),
    )
    .unwrap()
    .statement;
    let denied: Vec<_> = decisions(
        &setup,
        &creates
            .into_iter()
            .chain(delete_calls(RUNTIME))
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .filter(|(_, decision)| *decision != Decision::Allowed)
    .collect();
    assert!(denied.is_empty(), "setup lost a flow log call: {denied:#?}");
}
