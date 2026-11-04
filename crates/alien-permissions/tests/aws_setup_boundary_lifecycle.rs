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
fn setup_manages_only_its_own_management_policies() {
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
                    // Setup reads the management policies it writes: the Terraform provider
                    // reads each one back after creating it.
                    if resource == BOUNDARY
                        || resource == "arn:aws:iam::123456789012:policy/my-stack-deployment-management-boundary-copy"
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

/// Direct setup creates the role the flow logs service assumes for a created VPC without the
/// management boundary, which does not allow log delivery. A bounded setup's role grants all
/// require that boundary, so these come from the setup-only `network/setup` set, and reach only
/// roles of this stack that carry its tags.
#[test]
fn setup_can_create_pass_and_delete_the_flow_log_role_of_its_own_stack() {
    let context = common::create_test_context().with_aws_permissions_boundary_arn(BOUNDARY);
    let setup = generate_aws_initial_setup_policy(&context)
        .unwrap()
        .statement;
    let role = "arn:aws:iam::123456789012:role/my-stack-default-network-flow-logs";
    let owned = |action: &'static str, resource: &str, tag_source: &str| {
        Request::new(action, resource)
            .with(format!("{tag_source}/deployment"), "my-stack")
            .with(format!("{tag_source}/managed-by"), "runtime")
    };

    for action in ["iam:CreateRole", "iam:TagRole"] {
        assert_eq!(
            evaluate(&setup, &owned(action, role, "aws:RequestTag")),
            Decision::Allowed,
            "{action}"
        );
        assert_eq!(
            evaluate(&setup, &Request::new(action, role)),
            Decision::NotAllowed,
            "{action} without this stack's tags"
        );
        assert_eq!(
            evaluate(
                &setup,
                &owned(
                    action,
                    "arn:aws:iam::123456789012:role/other-stack-default-network-flow-logs",
                    "aws:RequestTag"
                )
            ),
            Decision::NotAllowed,
            "{action} on another stack's role"
        );
    }
    for action in [
        "iam:PutRolePolicy",
        "iam:DeleteRolePolicy",
        "iam:DeleteRole",
    ] {
        assert_eq!(
            evaluate(&setup, &owned(action, role, "aws:ResourceTag")),
            Decision::Allowed,
            "{action}"
        );
        assert_eq!(
            evaluate(
                &setup,
                &Request::new(action, role).with("aws:ResourceTag/deployment", "other-stack")
            ),
            Decision::NotAllowed,
            "{action} on a role another stack tagged"
        );
    }
    // A CloudFormation or Terraform setup of this stack tags its role managed-by=setup and runs
    // with the same setup policy, so that role stays reachable. Another stack's role does not.
    assert_eq!(
        evaluate(
            &setup,
            &owned("iam:TagRole", role, "aws:RequestTag")
                .with("aws:ResourceTag/deployment", "my-stack")
                .with("aws:ResourceTag/managed-by", "setup")
        ),
        Decision::Allowed,
        "iam:TagRole on this stack's template setup role"
    );
    assert_eq!(
        evaluate(
            &setup,
            &owned("iam:TagRole", role, "aws:RequestTag")
                .with("aws:ResourceTag/deployment", "other-stack")
                .with("aws:ResourceTag/managed-by", "setup")
        ),
        Decision::NotAllowed,
        "iam:TagRole on a role another stack tagged"
    );
    assert_eq!(
        evaluate(
            &setup,
            &owned("iam:TagRole", role, "aws:RequestTag")
                .with("aws:ResourceTag/deployment", "my-stack")
                .with("aws:ResourceTag/managed-by", "foreign")
        ),
        Decision::NotAllowed,
        "iam:TagRole on a role with another managed-by value"
    );
    let pass = |role: &str| {
        Request::new("iam:PassRole", role)
            .with("iam:PassedToService", "vpc-flow-logs.amazonaws.com")
    };
    assert_eq!(
        evaluate(
            &setup,
            &owned("iam:PassRole", role, "iam:ResourceTag")
                .with("iam:PassedToService", "vpc-flow-logs.amazonaws.com")
        ),
        Decision::Allowed
    );
    for (request, case) in [
        (pass(role), "an untagged role"),
        (
            pass(role).with("iam:ResourceTag/deployment", "my-stack-prod"),
            "a role tagged for another stack",
        ),
    ] {
        assert_eq!(
            evaluate(&setup, &request),
            Decision::NotAllowed,
            "iam:PassRole to {case}"
        );
    }
}

/// A CloudFormation or Terraform setup creates a created network's flow log, its log group and
/// its delivery role with the setup policy, and tags all three `managed-by=setup`. Each call
/// below is one the templates make for them; every one must be allowed for this stack.
#[test]
fn template_setup_creates_and_deletes_the_flow_log_of_a_created_network() {
    let setup = generate_aws_initial_setup_policy(
        &common::create_test_context().with_aws_permissions_boundary_arn(BOUNDARY),
    )
    .unwrap()
    .statement;
    let tags: &[(&str, &str)] = &[
        ("deployment", "my-stack"),
        ("managed-by", "setup"),
        ("resource", "default-network"),
        ("resource-type", "network"),
    ];
    let group = "arn:aws:logs:us-east-1:123456789012:log-group:my-stack-default-network-flow-logs";
    let role = "arn:aws:iam::123456789012:role/my-stack-default-network-flow-logs";
    let vpc = "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-0123456789abcdef0";
    let flow_log = "arn:aws:ec2:us-east-1:123456789012:vpc-flow-log/fl-0123456789abcdef0";
    let mut calls = vec![
        Request::new("logs:CreateLogGroup", group).request_tags(tags),
        Request::new("logs:TagResource", group).request_tags(tags),
        Request::new("iam:CreateRole", role).request_tags(tags),
        Request::new("iam:TagRole", role).request_tags(tags),
        Request::new("ec2:CreateFlowLogs", vpc).resource_tags(tags),
        Request::new("ec2:CreateFlowLogs", flow_log).request_tags(tags),
        Request::new("ec2:CreateTags", flow_log)
            .request_tags(tags)
            .with("ec2:CreateAction", "CreateFlowLogs"),
        Request::new("ec2:DeleteFlowLogs", flow_log).resource_tags(tags),
        Request::new("iam:PassRole", role)
            .with("iam:PassedToService", "vpc-flow-logs.amazonaws.com")
            .with("iam:ResourceTag/deployment", "my-stack")
            .with("iam:ResourceTag/managed-by", "setup"),
    ];
    for action in ["logs:PutRetentionPolicy", "logs:DeleteLogGroup"] {
        calls.push(Request::new(action, group).resource_tags(tags));
    }
    for action in [
        "iam:GetRole",
        "iam:PutRolePolicy",
        "iam:DeleteRolePolicy",
        "iam:DeleteRole",
    ] {
        calls.push(Request::new(action, role).resource_tags(tags));
    }
    let denied = calls
        .iter()
        .filter(|call| evaluate(&setup, call) != Decision::Allowed)
        .map(|call| format!("{} on {}", call.action, call.resource))
        .collect::<Vec<_>>();
    assert!(
        denied.is_empty(),
        "the setup policy denies a template setup's flow log calls: {denied:#?}"
    );
}

/// Creating the flow log group with its tags authorizes the tagging actions. A CloudFormation or
/// Terraform setup of this stack tags its group managed-by=setup and runs with the same setup
/// policy, so it can tag and delete that group; a group another stack tagged stays out of reach.
#[test]
fn setup_tags_only_the_flow_log_group_it_creates() {
    let setup = generate_aws_initial_setup_policy(&common::create_test_context())
        .unwrap()
        .statement;
    let group = "arn:aws:logs:us-east-1:123456789012:log-group:my-stack-default-network-flow-logs";
    let tagging = |action: &'static str| {
        Request::new(action, group)
            .with("aws:RequestTag/deployment", "my-stack")
            .with("aws:RequestTag/managed-by", "runtime")
    };
    for action in [
        "logs:CreateLogGroup",
        "logs:TagResource",
        "logs:TagLogGroup",
    ] {
        assert_eq!(
            evaluate(&setup, &tagging(action)),
            Decision::Allowed,
            "{action} on the group being created"
        );
    }
    for action in ["logs:TagResource", "logs:TagLogGroup"] {
        assert_eq!(
            evaluate(
                &setup,
                &tagging(action)
                    .with("aws:ResourceTag/deployment", "my-stack-prod")
                    .with("aws:ResourceTag/managed-by", "setup")
            ),
            Decision::NotAllowed,
            "{action} on a group another stack tagged"
        );
    }
    let delete = |tags: &[(&str, &str)]| {
        tags.iter().fold(
            Request::new("logs:DeleteLogGroup", group),
            |request, (key, value)| request.with(format!("aws:ResourceTag/{key}"), *value),
        )
    };
    for (tags, decision, case) in [
        (
            &[("deployment", "my-stack"), ("managed-by", "setup")][..],
            Decision::Allowed,
            "this stack's template setup group",
        ),
        (
            &[("deployment", "my-stack-prod"), ("managed-by", "setup")][..],
            Decision::NotAllowed,
            "a group another stack tagged",
        ),
        (&[][..], Decision::NotAllowed, "an untagged group"),
        (
            &[("deployment", "my-stack")][..],
            Decision::NotAllowed,
            "a group without a managed-by tag",
        ),
        (
            &[("deployment", "my-stack"), ("managed-by", "foreign")][..],
            Decision::NotAllowed,
            "a group with another managed-by value",
        ),
    ] {
        assert_eq!(
            evaluate(&setup, &delete(tags)),
            decision,
            "logs:DeleteLogGroup on {case}"
        );
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
