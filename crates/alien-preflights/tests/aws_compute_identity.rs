//! Exercise the IAM requests needed to create, retry and remove a compute identity
//! against the management policy produced by preflights, including explicit denies.
#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;

use alien_core::{
    ComputeCluster, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
    ResourceLifecycle, SetupManagement, Stack, StackSettings, StackState,
};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use iam_policy::{evaluate, Decision, Request};

const ACCOUNT: &str = "fixture-account";
const PREFIX: &str = "acme";
const PROFILE_ACTIONS: &[&str] = &[
    "iam:CreateInstanceProfile",
    "iam:TagInstanceProfile",
    "iam:ListInstanceProfileTags",
    "iam:GetInstanceProfile",
    "iam:AddRoleToInstanceProfile",
    "iam:RemoveRoleFromInstanceProfile",
    "iam:DeleteInstanceProfile",
];

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_aws_account_id(ACCOUNT)
        .with_managing_account_id("manager-account")
        .with_aws_region("us-west-2")
        .with_stack_prefix(PREFIX)
        .with_resource_name("compute")
}

async fn management(mode: SetupManagement) -> Vec<AwsIamStatement> {
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings {
            setup_management: mode,
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
            ComputeCluster::new("compute".into()).build(),
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
        // These requests are IAM-only; unrelated service conditions aren't part of this model.
        .filter(|statement| {
            statement
                .action
                .iter()
                .any(|action| action.starts_with("iam:"))
        })
        .collect()
}

fn arn(kind: &str, name: &str) -> String {
    format!("arn:aws:iam::{ACCOUNT}:{kind}/{name}")
}

#[tokio::test]
async fn automatic_compute_identity_create_retry_and_teardown_are_authorized() {
    let policy = management(SetupManagement::Automatic).await;
    let tags = [
        ("deployment", PREFIX),
        ("resource", "compute"),
        ("managed-by", "runtime"),
    ];
    for generation in ["compute", "compute-isolation-v1"] {
        let profile = arn(
            "instance-profile",
            &format!("{PREFIX}-{generation}-profile"),
        );
        for action in PROFILE_ACTIONS {
            assert_eq!(
                evaluate(&policy, &Request::new(action, &profile).request_tags(&tags)),
                Decision::Allowed,
                "{action} on {profile}"
            );
        }
        let role = arn("role", &format!("{PREFIX}-{generation}-role"));
        for action in [
            "iam:CreateRole",
            "iam:TagRole",
            "iam:GetRole",
            "iam:PutRolePolicy",
            "iam:DeleteRolePolicy",
            "iam:ListRolePolicies",
            "iam:DeleteRole",
            "iam:PassRole",
        ] {
            assert_eq!(
                evaluate(
                    &policy,
                    &Request::new(action, &role)
                        .request_tags(&tags)
                        .with("iam:PassedToService", "ec2.amazonaws.com")
                ),
                Decision::Allowed,
                "{action} on {role}"
            );
        }
        assert_eq!(
            evaluate(&policy, &Request::new("iam:PassRole", &role)),
            Decision::Allowed,
            "PassRole on {role} without iam:PassedToService"
        );
        for name in [
            "AmazonSSMManagedInstanceCore",
            "CloudWatchAgentServerPolicy",
        ] {
            assert_eq!(
                evaluate(
                    &policy,
                    &Request::new("iam:DetachRolePolicy", &role)
                        .with("iam:PolicyARN", format!("arn:aws:iam::aws:policy/{name}"))
                ),
                Decision::Allowed
            );
        }
    }
}

#[tokio::test]
async fn automatic_profile_writes_follow_stack_prefix_and_account() {
    let policy = management(SetupManagement::Automatic).await;
    for action in PROFILE_ACTIONS
        .iter()
        .filter(|action| **action != "iam:GetInstanceProfile")
    {
        // The stack wildcard also matches longer prefixes such as acme-prod.
        let nested_prefix = arn("instance-profile", "acme-prod-compute-profile");
        assert_eq!(
            evaluate(&policy, &Request::new(action, &nested_prefix)),
            Decision::Allowed,
            "{action} on {nested_prefix} is included by the acme-* grant"
        );
        for target in [
            arn("instance-profile", "other-compute-profile"),
            format!("arn:aws:iam::other-account:instance-profile/{PREFIX}-compute-profile"),
        ] {
            assert_ne!(
                evaluate(&policy, &Request::new(action, target.clone())),
                Decision::Allowed,
                "{action} on {target}"
            );
        }
    }
}

#[tokio::test]
async fn manual_compute_still_cannot_mutate_setup_profiles() {
    let policy = management(SetupManagement::Manual).await;
    let profile = arn("instance-profile", "acme-compute-profile");
    assert_eq!(
        evaluate(&policy, &Request::new("iam:GetInstanceProfile", &profile)),
        Decision::Allowed
    );
    for action in PROFILE_ACTIONS
        .iter()
        .filter(|action| **action != "iam:GetInstanceProfile")
    {
        assert_ne!(
            evaluate(&policy, &Request::new(action, &profile)),
            Decision::Allowed,
            "{action}"
        );
    }
}

#[test]
fn resource_scoped_compute_provision_cannot_mutate_a_sibling_profile() {
    let policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("compute-cluster/provision").unwrap(),
            BindingTarget::Resource,
            &context(),
        )
        .unwrap();
    for action in PROFILE_ACTIONS
        .iter()
        .filter(|action| **action != "iam:GetInstanceProfile")
    {
        for name in ["acme-compute-profile", "acme-compute-isolation-v1-profile"] {
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new(action, arn("instance-profile", name))
                ),
                Decision::Allowed,
                "{action} on {name}"
            );
        }
        for name in [
            "acme-compute-2-profile",
            "acme-other-profile",
            "other-compute-profile",
        ] {
            assert_ne!(
                evaluate(
                    &policy.statement,
                    &Request::new(action, arn("instance-profile", name))
                ),
                Decision::Allowed,
                "{action} on {name}"
            );
        }
    }
}
