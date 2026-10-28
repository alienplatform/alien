mod common;
mod iam_policy;

use alien_core::ALIEN_STACK_TAG_KEY;
use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsIamStatement, AwsRuntimePermissionsGenerator,
    },
    get_permission_set, list_permission_set_ids, BindingTarget,
};
use iam_policy::{evaluate, wildcard_match, Decision, Request};
use serde_json::json;

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/my-stack-permissions-boundary";
const ROLE: &str = "arn:aws:iam::123456789012:role/my-stack-jobs";
const SETS: &[&str] = &[
    "service-account/provision",
    "artifact-registry/provision",
    "artifact-registry/management",
    "worker/provision",
    "ai/provision",
];
const WRITES: &[&str] = &[
    "iam:CreateRole",
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:DeleteRolePolicy",
    "iam:DetachRolePolicy",
    "iam:UpdateAssumeRolePolicy",
    "iam:PutRolePermissionsBoundary",
];

fn render(bounded: bool) -> Vec<AwsIamStatement> {
    let mut context = common::create_test_context();
    if bounded {
        context = context.with_aws_permissions_boundary_arn(BOUNDARY);
    }
    SETS.iter()
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
        .collect()
}

#[test]
fn role_writes_require_our_boundary_but_reads_tags_and_deletion_do_not() {
    let policies = render(true);
    for action in WRITES {
        // Not every writer needs every operation, but none may allow it unbounded.
        assert_eq!(
            evaluate(&policies, &Request::new(action, ROLE)),
            Decision::NotAllowed,
            "{action}"
        );
        assert_eq!(
            evaluate(
                &policies,
                &Request::new(action, ROLE).with("iam:PermissionsBoundary", "another-boundary")
            ),
            Decision::NotAllowed,
            "{action}"
        );
    }
    for action in [
        "iam:CreateRole",
        "iam:PutRolePolicy",
        "iam:DeleteRolePolicy",
        "iam:DetachRolePolicy",
        "iam:UpdateAssumeRolePolicy",
        "iam:PutRolePermissionsBoundary",
    ] {
        assert_eq!(
            evaluate(
                &policies,
                &Request::new(action, ROLE).with("iam:PermissionsBoundary", BOUNDARY)
            ),
            Decision::Allowed,
            "{action}"
        );
    }
    for action in [
        "iam:GetRole",
        "iam:ListAttachedRolePolicies",
        "iam:TagRole",
        "iam:DeleteRole",
    ] {
        assert_eq!(
            evaluate(&policies, &Request::new(action, ROLE)),
            Decision::Allowed,
            "{action}"
        );
    }
    for action in [
        "iam:DeleteRolePermissionsBoundary",
        "iam:CreatePolicyVersion",
        "iam:SetDefaultPolicyVersion",
        "iam:DeletePolicy",
        "iam:DeletePolicyVersion",
    ] {
        for resource in [ROLE, BOUNDARY] {
            assert_eq!(
                evaluate(
                    &policies,
                    &Request::new(action, resource).with("iam:PermissionsBoundary", BOUNDARY)
                ),
                Decision::NotAllowed,
                "{action}"
            );
        }
    }
}

#[test]
fn legacy_setup_keeps_role_writes_without_granting_boundary_adoption() {
    let policies = render(false);
    for action in [
        "iam:CreateRole",
        "iam:PutRolePolicy",
        "iam:DeleteRolePolicy",
        "iam:DetachRolePolicy",
        "iam:UpdateAssumeRolePolicy",
    ] {
        assert_eq!(
            evaluate(&policies, &Request::new(action, ROLE)),
            Decision::Allowed,
            "{action}"
        );
    }
    assert_eq!(
        evaluate(
            &policies,
            &Request::new("iam:PutRolePermissionsBoundary", ROLE)
                .with("iam:PermissionsBoundary", BOUNDARY)
        ),
        Decision::NotAllowed
    );
}

#[test]
fn catalog_management_role_writes_are_conditioned_except_removal_only_dlm_cleanup() {
    let context = common::create_test_context().with_aws_permissions_boundary_arn(BOUNDARY);
    let generator = AwsRuntimePermissionsGenerator::new();
    for id in list_permission_set_ids() {
        if id == "remote-stack-management/provision" {
            continue;
        } // setup credentials only
        let set = get_permission_set(id).unwrap();
        let Some(entries) = &set.platforms.aws else {
            continue;
        };
        for (target, stack_binding) in [
            (BindingTarget::Stack, true),
            (BindingTarget::Resource, false),
        ] {
            let mut set = set.clone();
            set.platforms.aws = Some(
                entries
                    .iter()
                    .filter(|entry| {
                        if stack_binding {
                            entry.binding.stack.is_some()
                        } else {
                            entry.binding.resource.is_some()
                        }
                    })
                    .cloned()
                    .collect(),
            );
            for statement in generator
                .generate_policy(&set, target, &context)
                .unwrap()
                .statement
            {
                if statement.effect != "Allow" {
                    continue;
                }
                for action in &statement.action {
                    for policy_write in [
                        "iam:CreatePolicyVersion",
                        "iam:SetDefaultPolicyVersion",
                        "iam:DeletePolicyVersion",
                        "iam:DeletePolicy",
                    ] {
                        assert!(
                            !wildcard_match(action, policy_write)
                                || !statement
                                    .resource
                                    .iter()
                                    .any(|resource| wildcard_match(resource, BOUNDARY)),
                            "{id} permits {policy_write} on the boundary"
                        );
                    }
                    assert!(
                        !wildcard_match(action, "iam:DeleteRolePermissionsBoundary"),
                        "{id}"
                    );
                    if !WRITES.iter().any(|write| wildcard_match(action, write)) {
                        continue;
                    }
                    if id == "container/provision" && action == "iam:DetachRolePolicy" {
                        assert_eq!(statement.action, ["iam:DetachRolePolicy"]);
                        assert_eq!(
                            statement.resource,
                            ["arn:aws:iam::123456789012:role/my-stack-dlm-*"]
                        );
                        assert_eq!(
                            json!(statement.condition),
                            json!({"StringEquals": {"iam:PolicyARN": "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole"}})
                        );
                    } else {
                        assert_eq!(
                            statement
                                .condition
                                .as_ref()
                                .and_then(|c| c.get("StringEquals"))
                                .and_then(|v| v.get("iam:PermissionsBoundary"))
                                .map(String::as_str),
                            Some(BOUNDARY),
                            "{id}: {action}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cloudformation_resolves_boundary_arn_and_preserves_tag_conditions() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = common::create_test_context().with_aws_permissions_boundary_arn(
        "arn:aws:iam::${awsAccountId}:policy/${stackPrefix}-permissions-boundary",
    );
    let policy = generator
        .generate_policy(
            get_permission_set("worker/provision").unwrap(),
            BindingTarget::Stack,
            &context,
        )
        .unwrap();
    let write = policy
        .statement
        .iter()
        .find(|s| s.action.contains(&json!("iam:PutRolePolicy")))
        .unwrap();
    let conditions = &write.condition.as_ref().unwrap()["StringEquals"];
    assert_eq!(
        conditions["iam:PermissionsBoundary"],
        json!({"Fn::Sub": "arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/${AWS::StackName}-my-stack-permissions-boundary"})
    );
    assert_eq!(
        conditions[&format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}")],
        json!({"Fn::Sub": "${AWS::StackName}-my-stack"})
    );
    let legacy = generator
        .generate_policy(
            get_permission_set("worker/provision").unwrap(),
            BindingTarget::Stack,
            &common::create_test_context(),
        )
        .unwrap();
    for statement in legacy.statement {
        assert!(!statement
            .action
            .contains(&json!("iam:PutRolePermissionsBoundary")));
        if let Some(conditions) = statement.condition {
            assert!(conditions
                .values()
                .all(|values| !values.contains_key("iam:PermissionsBoundary")));
        }
    }
}

#[test]
fn explicit_custom_boundary_conditions_are_not_removed_for_legacy_setups() {
    let mut set = get_permission_set("service-account/provision")
        .unwrap()
        .clone();
    let entry = &mut set.platforms.aws.as_mut().unwrap()[0];
    for binding in [&mut entry.binding.stack, &mut entry.binding.resource]
        .into_iter()
        .flatten()
    {
        binding.condition.as_mut().unwrap()["StringEquals"]
            .insert("iam:PermissionsBoundary".to_string(), BOUNDARY.to_string());
    }
    let policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Stack, &common::create_test_context())
        .unwrap();
    assert_eq!(
        evaluate(&policy.statement, &Request::new("iam:CreateRole", ROLE)),
        Decision::NotAllowed
    );
    assert_eq!(
        evaluate(
            &policy.statement,
            &Request::new("iam:CreateRole", ROLE).with("iam:PermissionsBoundary", BOUNDARY)
        ),
        Decision::Allowed
    );
}
