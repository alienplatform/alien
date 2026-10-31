mod iam_policy;

use alien_core::PermissionSet;
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::{json, Value};

// Compile fixtures through the real permission compiler, so these checks exercise
// scalar/list serialization and interpolation as well as the test evaluator.
fn policy(condition: Value) -> Vec<AwsIamStatement> {
    let set: PermissionSet = serde_json::from_value(json!({
        "id": "custom/tag-check",
        "description": "Test tag conditions",
        "platforms": {"aws": [{
            "grant": {"actions": ["iam:TagRole", "iam:UntagRole"]},
            "binding": {"stack": {
                "resources": ["arn:aws:iam::123456789012:role/${stackPrefix}-*"],
                "condition": condition
            }}
        }]}
    }))
    .unwrap();
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            &set,
            BindingTarget::Stack,
            &PermissionContext::new().with_stack_prefix("app"),
        )
        .unwrap()
        .statement
}

fn request() -> Request {
    Request::new("iam:TagRole", "arn:aws:iam::123456789012:role/app-worker")
}

// AWS's documented ForAllValues and ForAnyValue examples, including the missing
// key behavior that makes an unguarded ForAllValues Allow overly permissive:
// https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_condition-single-vs-multi-valued-context-keys.html
#[test]
fn set_operator_truth_tables_include_missing_empty_and_mixed_requests() {
    for (operator, expected) in [
        (
            "ForAllValues:StringEquals",
            [true, true, true, false, false, true, true],
        ),
        (
            "ForAnyValue:StringEquals",
            [true, true, true, true, false, false, false],
        ),
        (
            "ForAllValues:StringNotEquals",
            [false, false, false, false, true, true, true],
        ),
        (
            "ForAnyValue:StringNotEquals",
            [false, false, false, true, true, false, false],
        ),
    ] {
        let policy = policy(json!({operator: {"aws:TagKeys": ["environment", "cost-center"]}}));
        let cases: [Option<&[&str]>; 7] = [
            Some(&["environment"]),
            Some(&["cost-center"]),
            Some(&["environment", "cost-center"]),
            Some(&["environment", "dept"]),
            Some(&["dept"]),
            None,
            Some(&[]),
        ];
        for (keys, allowed) in cases.into_iter().zip(expected) {
            let request =
                keys.map_or_else(request, |keys| request().with_values("aws:TagKeys", keys));
            assert_eq!(
                evaluate(&policy, &request) == Decision::Allowed,
                allowed,
                "{operator}: {keys:?}"
            );
        }
    }
}

#[test]
fn null_guard_closes_the_empty_set_allow() {
    let policy = policy(json!({
        "ForAllValues:StringEquals": {"aws:TagKeys": ["resource", "environment"]},
        "Null": {"aws:TagKeys": "false"}
    }));
    for request in [
        request(),
        request().with_values("aws:TagKeys", &[]),
        request().with("aws:TagKeys", ""),
    ] {
        assert_eq!(evaluate(&policy, &request), Decision::NotAllowed);
    }
    assert_eq!(
        evaluate(
            &policy,
            &request().with_values("aws:TagKeys", &["resource"])
        ),
        Decision::Allowed
    );
}

#[test]
fn policy_lists_use_or_for_positive_and_nor_for_negative_comparisons() {
    for (operator, expected) in [
        ("StringEquals", [true, true, false, false]),
        ("StringNotEquals", [false, false, true, true]),
        ("StringLike", [true, true, false, false]),
        ("StringNotLike", [false, false, true, true]),
    ] {
        let patterns = if operator.ends_with("Like") {
            json!(["run*", "set?p"])
        } else {
            json!(["runtime", "setup"])
        };
        let policy = policy(json!({operator: {"aws:ResourceTag/managed-by": patterns}}));
        for (owner, allowed) in [Some("runtime"), Some("setup"), Some("foreign"), None]
            .into_iter()
            .zip(expected)
        {
            let request = owner.map_or_else(request, |owner| {
                request().with("AWS:ResourceTag/MANAGED-BY", owner)
            });
            assert_eq!(
                evaluate(&policy, &request) == Decision::Allowed,
                allowed,
                "{operator}: {owner:?}"
            );
        }
    }
}

// This is a condition fixture, not the catalog's management policy. It checks
// that the evaluator can distinguish the ownership cases required by role grants.
#[test]
fn tag_condition_fixture_requires_existing_and_requested_ownership() {
    let policy = policy(json!({
        "StringEquals": {
            "aws:ResourceTag/managed-by": "runtime",
            "aws:ResourceTag/deployment": "${stackPrefix}",
            "aws:RequestTag/managed-by": "runtime",
            "aws:RequestTag/deployment": "${stackPrefix}"
        },
        "ForAllValues:StringEquals": {"aws:TagKeys": ["managed-by", "deployment", "resource"]}
    }));
    for existing_owner in ["runtime", "setup", "Runtime", ""] {
        for existing_deployment in ["app", "other", ""] {
            for requested_owner in ["runtime", "setup", "Runtime", ""] {
                for requested_deployment in ["app", "other", ""] {
                    let existing: Vec<_> = [
                        ("managed-by", existing_owner),
                        ("deployment", existing_deployment),
                    ]
                    .into_iter()
                    .filter(|(_, value)| !value.is_empty())
                    .collect();
                    let requested: Vec<_> = [
                        ("managed-by", requested_owner),
                        ("deployment", requested_deployment),
                        ("resource", "worker"),
                    ]
                    .into_iter()
                    .filter(|(_, value)| !value.is_empty())
                    .collect();
                    let request = request().resource_tags(&existing).request_tags(&requested);
                    assert_eq!(
                        evaluate(&policy, &request) == Decision::Allowed,
                        existing_owner == "runtime"
                            && existing_deployment == "app"
                            && requested_owner == "runtime"
                            && requested_deployment == "app",
                        "existing={existing:?}, requested={requested:?}"
                    );
                }
            }
        }
    }
    for owner_key in ["MANAGED-BY", "Managed-By"] {
        let request = request()
            .resource_tags(&[("managed-by", "runtime"), ("deployment", "app")])
            .request_tags(&[(owner_key, "runtime"), ("deployment", "app")]);
        assert_eq!(evaluate(&policy, &request), Decision::NotAllowed);
    }
    // A context-key lookup alone is case insensitive; TagKeys supplies the
    // separate case-sensitive check. Removing it must expose the difference.
    let mut without_key_check = vec![policy[0].clone()];
    without_key_check[0]
        .condition
        .as_mut()
        .unwrap()
        .shift_remove("ForAllValues:StringEquals");
    assert_eq!(
        evaluate(
            &without_key_check,
            &request()
                .resource_tags(&[("managed-by", "runtime"), ("deployment", "app")])
                .request_tags(&[("MANAGED-BY", "runtime"), ("deployment", "app")])
        ),
        Decision::Allowed
    );
}

#[test]
fn untag_allowlist_rejects_ownership_removal_and_mixed_requests() {
    let policy = policy(json!({
        "ForAllValues:StringEquals": {"aws:TagKeys": ["resource"]},
        "Null": {"aws:TagKeys": "false"}
    }));
    for (keys, allowed) in [
        (vec!["resource"], true),
        (vec!["managed-by"], false),
        (vec!["deployment"], false),
        (vec!["resource", "deployment"], false),
        (vec!["Resource"], false),
        (vec![], false),
    ] {
        let request = Request::new("iam:UntagRole", "arn:aws:iam::123456789012:role/app-worker")
            .with_values("aws:TagKeys", &keys);
        assert_eq!(
            evaluate(&policy, &request) == Decision::Allowed,
            allowed,
            "{keys:?}"
        );
    }
}

#[test]
fn list_condition_deny_overrides_a_matching_allow() {
    let mut policy =
        policy(json!({"ForAnyValue:StringEquals": {"aws:TagKeys": ["managed-by", "deployment"]}}));
    policy[0].effect = "Deny".into();
    let mut allow = policy[0].clone();
    allow.effect = "Allow".into();
    allow.condition = None;
    policy.push(allow);
    assert_eq!(
        evaluate(
            &policy,
            &request().with_values("aws:TagKeys", &["resource", "deployment"])
        ),
        Decision::ExplicitlyDenied
    );
    assert_eq!(
        evaluate(
            &policy,
            &request().with_values("aws:TagKeys", &["resource"])
        ),
        Decision::Allowed
    );
}

#[test]
#[should_panic(expected = "multivalued request keys require a set operator")]
fn scalar_operator_does_not_silently_evaluate_a_multivalued_request() {
    evaluate(
        &policy(json!({"StringEquals": {"aws:TagKeys": ["resource"]}})),
        &request().with_values("aws:TagKeys", &["resource"]),
    );
}

#[test]
#[should_panic(expected = "doesn't model condition operator")]
fn unknown_operators_fail_even_for_missing_request_keys() {
    evaluate(
        &policy(json!({"ForAllValues:Unknown": {"aws:TagKeys": ["resource"]}})),
        &request(),
    );
}

#[test]
#[should_panic(expected = "duplicate tag keys")]
fn ambiguous_case_variant_tag_contexts_are_rejected() {
    request().request_tags(&[("managed-by", "runtime"), ("Managed-By", "setup")]);
}
