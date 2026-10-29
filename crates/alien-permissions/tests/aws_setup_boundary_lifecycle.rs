mod common;
mod iam_policy;

use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set,
    initial_setup::generate_aws_initial_setup_policy, BindingTarget,
};
use iam_policy::{evaluate, Decision, Request};

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/my-stack-permissions-boundary";

#[test]
fn setup_can_manage_only_its_exact_boundary_policy() {
    let context = common::create_test_context();
    let mut policies = vec![generate_aws_initial_setup_policy(&context).unwrap()];
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        policies.push(
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    get_permission_set("remote-stack-management/provision").unwrap(),
                    target,
                    &context,
                )
                .unwrap(),
        );
    }
    for policy in policies {
        for action in [
            "iam:CreatePolicy",
            "iam:CreatePolicyVersion",
            "iam:DeletePolicy",
            "iam:DeletePolicyVersion",
            "iam:ListPolicyVersions",
            "iam:GetPolicy",
            "iam:GetPolicyVersion",
        ] {
            for resource in [
                BOUNDARY,
                "arn:aws:iam::123456789012:policy/my-stack-other-permissions-boundary",
                "arn:aws:iam::123456789012:policy/my-stack-permissions-boundary-copy",
                "arn:aws:iam::210987654321:policy/my-stack-permissions-boundary",
                "arn:aws:iam::123456789012:policy/other-permissions-boundary",
            ] {
                assert_eq!(
                    evaluate(&policy.statement, &Request::new(action, resource)),
                    if resource == BOUNDARY {
                        Decision::Allowed
                    } else {
                        Decision::NotAllowed
                    },
                    "{action}: {resource}"
                );
            }
        }
    }
}

#[test]
fn node_cleanup_only_detaches_the_two_obsolete_policies_within_its_scope() {
    for (target, allowed_roles, denied_roles) in [
        (
            BindingTarget::Stack,
            vec![
                "my-stack-compute-role",
                "my-stack-compute-isolation-v1-role",
            ],
            vec![
                "other-compute-role",
                "my-stack-management",
                "my-stack-dlm-123",
            ],
        ),
        (
            BindingTarget::Resource,
            vec![
                "my-stack-compute-role",
                "my-stack-compute-isolation-v1-role",
            ],
            vec![
                "my-stack-other-role",
                "my-stack-management",
                "other-compute-role",
            ],
        ),
    ] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                get_permission_set("compute-cluster/provision").unwrap(),
                target,
                &common::create_test_context()
                    .with_resource_name("compute")
                    .with_aws_permissions_boundary_arn(BOUNDARY),
            )
            .unwrap();
        for name in allowed_roles.iter().chain(&denied_roles) {
            let role = format!("arn:aws:iam::123456789012:role/{name}");
            for managed_policy in [
                "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore",
                "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy",
                "arn:aws:iam::aws:policy/AdministratorAccess",
                "arn:aws:iam::123456789012:policy/AmazonSSMManagedInstanceCore",
                "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy-copy",
            ] {
                for action in [
                    "iam:DetachRolePolicy",
                    "iam:AttachRolePolicy",
                    "iam:DeleteRolePolicy",
                ] {
                    let expected = if allowed_roles.contains(name)
                        && action == "iam:DetachRolePolicy"
                        && matches!(
                            managed_policy,
                            "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
                                | "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy"
                        ) {
                        Decision::Allowed
                    } else {
                        Decision::NotAllowed
                    };
                    assert_eq!(
                        evaluate(
                            &policy.statement,
                            &Request::new(action, &role)
                                .with("iam:PolicyARN", managed_policy)
                                .with("iam:PermissionsBoundary", BOUNDARY)
                                .resource_tags(&[
                                    ("managed-by", "runtime"),
                                    ("deployment", "my-stack")
                                ])
                        ),
                        expected,
                        "{action}: {role}: {managed_policy}"
                    );
                }
            }
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new("iam:DetachRolePolicy", &role)
                ),
                Decision::NotAllowed
            );
        }
    }
}
