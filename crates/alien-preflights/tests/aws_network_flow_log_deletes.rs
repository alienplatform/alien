//! Exercise the calls that delete a created network's VPC flow log objects against the whole
//! management policy an automatic setup produces, Deny statements included.
//!
//! With automatic setup management, runtime cleanup deletes a created network under the
//! management identity, and the network's flow log, delivery role and log group go first. Only
//! setup creates those objects, so this policy may delete them but never create or write them.
#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;

use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Network, NetworkSettings,
    Platform, ResourceLifecycle, SetupManagement, Stack, StackSettings, StackState,
};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use iam_policy::{evaluate, Decision, Request};

const ACCOUNT: &str = "123456789012";
const PREFIX: &str = "acme";
const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/acme-deployment-management-boundary";
const FLOW_LOG: &str = "arn:aws:ec2:us-west-2:123456789012:vpc-flow-log/fl-0123456789abcdef0";
const VPC: &str = "arn:aws:ec2:us-west-2:123456789012:vpc/vpc-0123456789abcdef0";
const ROLE: &str = "arn:aws:iam::123456789012:role/acme-default-network-flow-logs";
const GROUP: &str = "arn:aws:logs:us-west-2:123456789012:log-group:acme-default-network-flow-logs";

const RUNTIME: &[(&str, &str)] = &[("deployment", PREFIX), ("managed-by", "runtime")];
const TEMPLATE_SETUP: &[(&str, &str)] = &[("deployment", PREFIX), ("managed-by", "setup")];
const OTHER_STACK: &[(&str, &str)] = &[("deployment", "other"), ("managed-by", "runtime")];

fn context(bounded: bool) -> PermissionContext {
    let context = PermissionContext::new()
        .with_aws_account_id(ACCOUNT)
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
        .with_aws_region("us-west-2")
        .with_stack_prefix(PREFIX);
    if bounded {
        context.with_aws_permissions_boundary_arn(BOUNDARY)
    } else {
        context
    }
}

/// Every statement of the management identity's policy for a stack with a created network.
async fn automatic_management(bounded: bool) -> Vec<AwsIamStatement> {
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings {
            setup_management: SetupManagement::Automatic,
            ..Default::default()
        })
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let stack = Stack::new("fixture".into())
        .add(
            Network::new("default-network".into())
                .settings(NetworkSettings::Create {
                    cidr: Some("10.42.0.0/16".into()),
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let prepared = ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .unwrap();
    let profile = prepared.management().profile().unwrap();
    assert!(profile.0.keys().all(|scope| scope == "*"));
    profile.0["*"]
        .iter()
        .flat_map(|grant| {
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    &grant
                        .resolve(|id| get_permission_set(id).cloned())
                        .expect("management grant"),
                    BindingTarget::Stack,
                    &context(bounded),
                )
                .unwrap()
                .statement
        })
        .collect()
}

/// Every call the network controller's flow log delete makes, in order, for objects that carry
/// `tags`. The role and the log group are read before anything is changed. CloudWatch Logs
/// authorizes a log group under either form of its ARN, so the delete is checked under both.
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

fn is_read(request: &Request) -> bool {
    matches!(
        request.action,
        "ec2:DescribeFlowLogs" | "iam:GetRole" | "logs:ListTagsForResource"
    )
}

#[tokio::test]
async fn automatic_management_deletes_the_flow_log_objects_of_its_network() {
    for bounded in [false, true] {
        let policy = automatic_management(bounded).await;
        for request in delete_calls(RUNTIME) {
            assert_eq!(
                evaluate(&policy, &request),
                Decision::Allowed,
                "bounded={bounded}: {} on {}",
                request.action,
                request.resource
            );
        }
        // A repeated delete reads objects that are gone and so have no tags: each read must
        // reach them so they answer NotFound. No write may, except that a setup without a
        // boundary keeps the legacy grant that deletes any role under the stack prefix.
        for request in delete_calls(&[]) {
            let legacy_role_delete = !bounded && request.action == "iam:DeleteRole";
            assert_eq!(
                evaluate(&policy, &request),
                if is_read(&request) || legacy_role_delete {
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

#[tokio::test]
async fn automatic_management_cannot_delete_flow_log_objects_it_does_not_own() {
    for bounded in [false, true] {
        let policy = automatic_management(bounded).await;
        for tags in [TEMPLATE_SETUP, OTHER_STACK] {
            // A setup without a boundary keeps the legacy grant that deletes any role under the
            // stack prefix; the bounded profile requires this stack's runtime tags.
            for request in delete_calls(tags)
                .into_iter()
                .filter(|r| !is_read(r) && (bounded || r.action != "iam:DeleteRole"))
            {
                assert_ne!(
                    evaluate(&policy, &request),
                    Decision::Allowed,
                    "bounded={bounded}: {} on {} with tags {tags:?}",
                    request.action,
                    request.resource
                );
            }
        }
        for request in [
            Request::new(
                "iam:DeleteRole",
                "arn:aws:iam::123456789012:role/other-default-network-flow-logs",
            )
            .resource_tags(RUNTIME),
            Request::new(
                "logs:DeleteLogGroup",
                "arn:aws:logs:us-west-2:123456789012:log-group:other-default-network-flow-logs",
            )
            .resource_tags(RUNTIME),
        ] {
            assert_ne!(
                evaluate(&policy, &request),
                Decision::Allowed,
                "bounded={bounded}: {} on {}",
                request.action,
                request.resource
            );
        }
    }
}

/// Creating the role, writing its policy and passing it to the service cannot be limited to
/// what the role may do, so the management identity creates none of the flow log objects.
#[tokio::test]
async fn automatic_management_creates_no_flow_log_objects() {
    let creates = [
        Request::new("ec2:CreateFlowLogs", FLOW_LOG).request_tags(RUNTIME),
        Request::new("ec2:CreateFlowLogs", VPC).resource_tags(RUNTIME),
        Request::new("iam:CreateRole", ROLE).request_tags(RUNTIME),
        Request::new("iam:PutRolePolicy", ROLE).resource_tags(RUNTIME),
        Request::new("iam:PassRole", ROLE)
            .with("iam:PassedToService", "vpc-flow-logs.amazonaws.com")
            .with("iam:ResourceTag/deployment", PREFIX)
            .with("iam:ResourceTag/managed-by", "runtime"),
        Request::new("logs:CreateLogGroup", GROUP).request_tags(RUNTIME),
        Request::new("logs:PutRetentionPolicy", GROUP).resource_tags(RUNTIME),
    ];
    for bounded in [false, true] {
        let policy = automatic_management(bounded).await;
        for request in &creates {
            assert_ne!(
                evaluate(&policy, request),
                Decision::Allowed,
                "bounded={bounded}: {} on {}",
                request.action,
                request.resource
            );
        }
    }
}
