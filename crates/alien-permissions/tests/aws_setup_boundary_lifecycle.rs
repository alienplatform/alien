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

#[test]
fn setup_can_manage_its_boundary_and_read_only_the_exact_boundary() {
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
                "arn:aws:iam::123456789012:policy/my-stack-other-deployment-management-boundary",
                "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary-copy",
                "arn:aws:iam::210987654321:policy/my-stack-deployment-management-boundary",
                "arn:aws:iam::123456789012:policy/other-deployment-management-boundary",
            ] {
                assert_eq!(
                    evaluate(&policy.statement, &Request::new(action, resource)),
                    if resource == BOUNDARY
                        || (resource == "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary-copy"
                            && !matches!(action, "iam:GetPolicy" | "iam:GetPolicyVersion"))
                    {
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

#[test]
fn management_boundary_deny_overrides_allows_without_affecting_setup_or_siblings() {
    let broad_allow = AwsIamStatement {
        sid: "ExplicitManagementExtension".into(),
        effect: "Allow".into(),
        action: vec!["iam:*".into()],
        resource: vec!["*".into()],
        not_resource: Vec::new(),
        condition: None,
    };
    let generator = AwsRuntimePermissionsGenerator::new();
    for bounded in [false, true] {
        let mut context = common::create_test_context();
        if bounded {
            context = context.with_aws_permissions_boundary_arn(BOUNDARY);
        }
        let mut management = generator
            .generate_policy(
                get_permission_set(alien_permissions::MANAGEMENT_ROLE_GUARD).unwrap(),
                BindingTarget::Stack,
                &context,
            )
            .unwrap()
            .statement;
        management.push(broad_allow.clone());
        for action in [
            "iam:CreatePolicyVersion",
            "iam:SetDefaultPolicyVersion",
            "iam:DeletePolicyVersion",
            "iam:DeletePolicy",
            "iam:TagPolicy",
            "iam:UntagPolicy",
        ] {
            for resource in [
                BOUNDARY,
                "arn:aws:iam::123456789012:policy/my-stack-deployment-management-1",
                "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary-copy",
                "arn:aws:iam::210987654321:policy/my-stack-deployment-management-boundary",
            ] {
                assert_eq!(
                    evaluate(&management, &Request::new(action, resource)),
                    if bounded && resource == BOUNDARY {
                        Decision::ExplicitlyDenied
                    } else {
                        Decision::Allowed
                    },
                    "bounded={bounded}, {action}, {resource}"
                );
            }
        }
        for role in [
            "arn:aws:iam::123456789012:role/my-stack-jobs",
            "arn:aws:iam::123456789012:role/other-jobs",
            "arn:aws:iam::210987654321:role/my-stack-jobs",
        ] {
            assert_eq!(
                evaluate(
                    &management,
                    &Request::new("iam:DeleteRolePermissionsBoundary", role)
                ),
                if bounded && role == "arn:aws:iam::123456789012:role/my-stack-jobs" {
                    Decision::ExplicitlyDenied
                } else {
                    Decision::Allowed
                }
            );
        }
        let installer = generate_aws_initial_setup_policy(&context).unwrap();
        for action in [
            "iam:CreatePolicyVersion",
            "iam:DeletePolicyVersion",
            "iam:DeletePolicy",
        ] {
            assert_eq!(
                evaluate(&installer.statement, &Request::new(action, BOUNDARY)),
                Decision::Allowed
            );
        }
    }
}
