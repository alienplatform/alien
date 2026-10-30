mod iam_policy;

use alien_core::{PermissionProfile, PermissionSet, PermissionSetReference, Stack};
use alien_permissions::{
    generators::{
        aws_permissions_boundary, aws_stack_permissions_boundary,
        AwsCloudFormationPermissionsGenerator, AwsRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::{json, Value};

fn context(bounded: bool) -> PermissionContext {
    let context = PermissionContext::new()
        .with_stack_prefix("example")
        .with_aws_account_id("123456789012")
        .with_managing_account_id("123456789012")
        .with_aws_region("us-west-2");
    if bounded {
        context.with_aws_permissions_boundary_arn(
            "arn:aws:iam::123456789012:policy/example-permissions-boundary",
        )
    } else {
        context
    }
}

fn custom(action: &str, condition: Value) -> PermissionSet {
    let binding = json!({"resources": ["arn:aws:iam::123456789012:role/example-worker"], "condition": condition});
    serde_json::from_value(json!({
        "id": "custom/roles", "description": "Custom role passing",
        "platforms": {"aws": [{
            "grant": {"actions": [action]},
            "binding": {"stack": binding, "resource": binding}
        }]}
    }))
    .unwrap()
}

#[test]
fn bounded_generators_reject_unrestricted_custom_role_passing() {
    let invalid = [
        Value::Null,
        json!({"StringEqualsIfExists": {"iam:PassedToService": "lambda.amazonaws.com"}}),
        json!({"ForAllValues:StringEquals": {"iam:PassedToService": "lambda.amazonaws.com"}}),
        json!({"StringNotEquals": {"iam:PassedToService": "ec2.amazonaws.com"}}),
        json!({"Null": {"iam:PassedToService": "false"}}),
        json!({"StringLike": {"iam:PassedToService": "*.amazonaws.com"}}),
        json!({"StringEquals": {"iam:PassedToService": "*.amazonaws.com"}}),
        json!({"StringEquals": {"iam:PassedToService": "${aws:PrincipalTag/service}"}}),
        json!({"StringEquals": {"iam:PassedToService": []}}),
        json!({"StringEquals": {"iam:PassedToService": ["lambda.amazonaws.com", "*"]}}),
        json!({"StringEquals": {"iam:PassedToService": ""}}),
    ];
    for action in ["iam:PassRole", "IAM:pAsSrOlE", "iam:*", "*", "i?m:*Role"] {
        for condition in &invalid {
            let set = custom(action, condition.clone());
            for target in [BindingTarget::Stack, BindingTarget::Resource] {
                let runtime = AwsRuntimePermissionsGenerator::new()
                    .generate_policy(&set, target, &context(true))
                    .unwrap_err();
                let cfn = AwsCloudFormationPermissionsGenerator::new()
                    .generate_policy(&set, target, &context(true))
                    .unwrap_err();
                assert_eq!(
                    runtime.code, "PASS_ROLE_SERVICE_REQUIRED",
                    "{action}: {condition}"
                );
                assert_eq!(cfn.code, runtime.code);
                assert!(runtime.message.contains("custom/roles"));
            }
        }
    }
}

#[test]
fn explicit_custom_services_preserve_conditions_and_limit_requests() {
    for services in [
        json!("lambda.amazonaws.com"),
        json!(["lambda.amazonaws.com", "ecs-tasks.amazonaws.com"]),
    ] {
        let condition = json!({"StringEquals": {"IAM:PassedToService": services}});
        let set = custom("iam:PassRole", condition.clone());
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let runtime = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, target, &context(true))
                .unwrap();
            let cfn = AwsCloudFormationPermissionsGenerator::new()
                .generate_policy(&set, target, &context(true))
                .unwrap();
            assert_eq!(
                serde_json::to_value(&runtime.statement[0].condition).unwrap(),
                condition
            );
            assert_eq!(
                serde_json::to_value(&cfn.statement[0].condition).unwrap(),
                condition
            );
            for service in ["lambda.amazonaws.com", "ec2.amazonaws.com", ""] {
                let request = Request::new(
                    "iam:PassRole",
                    "arn:aws:iam::123456789012:role/example-worker",
                );
                let request = if service.is_empty() {
                    request
                } else {
                    request.with("iam:PassedToService", service)
                };
                assert_eq!(
                    evaluate(&runtime.statement, &request) == Decision::Allowed,
                    service == "lambda.amazonaws.com"
                );
            }
            let boundary = aws_permissions_boundary(&context(true), &runtime.statement).unwrap();
            assert!(boundary["Statement"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["Condition"] == condition));
        }
    }
}

#[test]
fn legacy_custom_grants_remain_unchanged_but_cannot_enter_a_new_boundary() {
    let set = custom("iam:*", Value::Null);
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let runtime = AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, target, &context(false))
            .unwrap();
        let cfn = AwsCloudFormationPermissionsGenerator::new()
            .generate_policy(&set, target, &context(false))
            .unwrap();
        assert!(runtime.statement[0].condition.is_none());
        assert!(cfn.statement[0].condition.is_none());
        assert_eq!(runtime.statement[0].action, ["iam:*"]);
        assert_eq!(
            evaluate(
                &runtime.statement,
                &Request::new(
                    "iam:PassRole",
                    "arn:aws:iam::123456789012:role/example-worker"
                )
            ),
            Decision::Allowed
        );
        let error = aws_permissions_boundary(&context(false), &runtime.statement).unwrap_err();
        assert_eq!(error.code, "PASS_ROLE_SERVICE_REQUIRED");
    }
}

#[test]
fn unrelated_actions_and_explicit_denies_do_not_require_a_service() {
    let mut deny = custom("iam:*", Value::Null);
    deny.platforms.aws.as_mut().unwrap()[0].effect = alien_core::AwsPermissionEffect::Deny;
    for set in [custom("iam:GetRole", Value::Null), deny] {
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let runtime = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, target, &context(true))
                .unwrap();
            AwsCloudFormationPermissionsGenerator::new()
                .generate_policy(&set, target, &context(true))
                .unwrap();
            aws_permissions_boundary(&context(true), &runtime.statement).unwrap();
        }
    }
}

#[test]
fn setup_boundary_rejects_unrestricted_inline_profiles_before_installation() {
    for scope in ["*", "worker"] {
        let set = custom("iam:PassRole", Value::Null);
        let stack = Stack::new("example".to_string())
            .permission(
                "custom",
                PermissionProfile(indexmap::indexmap! {
                    scope.to_string() => vec![PermissionSetReference::Inline(set)]
                }),
            )
            .build();
        // A fresh setup has no installed boundary ARN yet.
        let error =
            aws_stack_permissions_boundary(&stack, &context(false), |_, _| Ok(context(false)))
                .unwrap_err();
        assert_eq!(error.code, "PASS_ROLE_SERVICE_REQUIRED");
    }
}
