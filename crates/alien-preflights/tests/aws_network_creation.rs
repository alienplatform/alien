//! Exercise the EC2 requests an automatic setup makes to create a new network against the
//! management policy produced by preflights. AWS authorizes each create against the new
//! resource and its parent: request tags apply only to the new resource, so the parent is
//! matched on the tags it already carries.
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

const ACCOUNT: &str = "fixture-account";
const PREFIX: &str = "acme";
const NETWORK_TAGS: &[(&str, &str)] = &[
    ("deployment", PREFIX),
    ("resource", "default-network"),
    ("managed-by", "runtime"),
];

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_aws_account_id(ACCOUNT)
        .with_managing_account_id("manager-account")
        .with_aws_region("us-west-2")
        .with_stack_prefix(PREFIX)
        .with_resource_name("default-network")
}

async fn automatic_management() -> Vec<AwsIamStatement> {
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
    prepared.management().profile().unwrap().0["*"]
        .iter()
        .flat_map(|grant| {
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    &grant
                        .resolve(|id| get_permission_set(id).cloned())
                        .expect("management grant"),
                    BindingTarget::Stack,
                    &context(),
                )
                .unwrap()
                .statement
        })
        // These requests are EC2-only; unrelated service conditions aren't part of this model.
        .filter(|statement| {
            statement
                .action
                .iter()
                .any(|action| action.starts_with("ec2:"))
        })
        .collect()
}

fn arn(kind: &str, id: &str) -> String {
    format!("arn:aws:ec2:us-west-2:{ACCOUNT}:{kind}/{id}")
}

/// Every resource AWS authorizes for one create call: the new resource with the tags the
/// request writes, then each parent with the tags it already has.
fn create_call(
    action: &'static str,
    new_kind: &str,
    parents: &[(&str, &str)],
    parent_tags: &[(&str, &str)],
) -> Vec<Request> {
    let mut requests = vec![Request::new(action, arn(new_kind, "new")).request_tags(NETWORK_TAGS)];
    for (kind, id) in parents {
        requests.push(Request::new(action, arn(kind, id)).resource_tags(parent_tags));
    }
    requests
}

fn network_creation_calls(parent_tags: &[(&str, &str)]) -> Vec<Vec<Request>> {
    vec![
        create_call("ec2:CreateVpc", "vpc", &[], parent_tags),
        create_call(
            "ec2:CreateInternetGateway",
            "internet-gateway",
            &[],
            parent_tags,
        ),
        create_call("ec2:AllocateAddress", "elastic-ip", &[], parent_tags),
        create_call(
            "ec2:CreateSubnet",
            "subnet",
            &[("vpc", "vpc-1")],
            parent_tags,
        ),
        create_call(
            "ec2:CreateRouteTable",
            "route-table",
            &[("vpc", "vpc-1")],
            parent_tags,
        ),
        create_call(
            "ec2:CreateSecurityGroup",
            "security-group",
            &[("vpc", "vpc-1")],
            parent_tags,
        ),
        create_call(
            "ec2:CreateNatGateway",
            "natgateway",
            &[("subnet", "subnet-1"), ("elastic-ip", "eipalloc-1")],
            parent_tags,
        ),
    ]
}

#[tokio::test]
async fn automatic_setup_can_create_a_new_network_inside_its_own_vpc() {
    let policy = automatic_management().await;
    for call in network_creation_calls(NETWORK_TAGS) {
        for request in &call {
            assert_eq!(
                evaluate(&policy, request),
                Decision::Allowed,
                "{} on {}",
                request.action,
                request.resource
            );
        }
    }
}

#[tokio::test]
async fn automatic_setup_cannot_create_children_in_a_vpc_it_does_not_own() {
    let policy = automatic_management().await;
    let other_deployment = [("deployment", "other"), ("managed-by", "runtime")];
    let setup_owned = [("deployment", PREFIX), ("managed-by", "setup")];
    for parent_tags in [&other_deployment[..], &setup_owned[..], &[][..]] {
        for call in network_creation_calls(parent_tags) {
            // postgres/provision grants CreateSecurityGroup on any VPC: the database group is
            // created before it can be tagged, and automatic setup includes every provision set.
            for request in call
                .iter()
                .skip(1)
                .filter(|request| request.action != "ec2:CreateSecurityGroup")
            {
                assert_ne!(
                    evaluate(&policy, request),
                    Decision::Allowed,
                    "{} on {} tagged {parent_tags:?}",
                    request.action,
                    request.resource
                );
            }
        }
    }
}
